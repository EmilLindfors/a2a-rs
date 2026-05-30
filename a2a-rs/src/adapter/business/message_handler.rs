//! Default message handler implementation.
//!
//! `ResponderMessageHandler` owns the *plumbing* of turning an incoming message
//! into a task — parse the id, create the task if absent, append the message to
//! history, broadcast each transition — and delegates the *business decision*
//! (what to reply, and what state the task should end in) to an injected
//! [`Responder`]. The built-in [`EchoResponder`] echoes the message back; a
//! caller that wants AI behaviour implements `Responder` and keeps all of the
//! lifecycle + streaming wiring for free.
//!
//! This split keeps the broadcasting in one place: because the handler holds
//! both the lifecycle and streaming ports it hosts the [`TaskStatusBroadcast`]
//! mixin (`REFACTORING_PLAN.md` §4.1), so every transition it drives — the
//! incoming-message append *and* the responder's reply — goes through
//! [`update_and_broadcast`], announcing to streaming subscribers. Storage
//! mutators no longer self-broadcast (§4.0.2), so a `Responder` author never has
//! to think about streaming at all.
//!
//! `Responder` is synchronous-shaped (`message + task → reply + state`); agents
//! that need "acknowledge now, finish later" semantics implement
//! [`AsyncMessageHandler`](crate::port::AsyncMessageHandler) directly and host
//! the mixin themselves (the reimbursement agent does this).
//!
//! [`update_and_broadcast`]: TaskStatusBroadcast::update_and_broadcast

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    application::{HasStreaming, HasTaskLifecycle, TaskStatusBroadcast},
    domain::{A2AError, ContextId, Message, Part, Role, Task, TaskId, TaskState},
    port::{AsyncMessageHandler, AsyncStreamingHandler, AsyncTaskLifecycle},
};

/// The business decision behind a message handler: given the incoming `message`
/// and the `task` as it now stands (already in `Working` with the message
/// appended to history), produce the agent's reply and the state the task
/// should transition to.
///
/// Implement this to plug custom logic (an LLM call, a rules engine, …) into
/// [`ResponderMessageHandler`] without re-implementing task lifecycle or
/// streaming. Implementations must be cheap to share (`Send + Sync`): the
/// handler holds the responder behind an `Arc`.
#[async_trait]
pub trait Responder: Send + Sync {
    /// Produce the reply message and the resulting task state.
    async fn respond(
        &self,
        message: &Message,
        task: &Task,
    ) -> Result<(Message, TaskState), A2AError>;
}

/// The reference [`Responder`]: echoes the incoming text back and leaves the
/// task in `Working`. Useful for smoke tests, examples, and as the default for
/// [`ResponderMessageHandler::echo`].
#[derive(Clone, Debug, Default)]
pub struct EchoResponder;

#[async_trait]
impl Responder for EchoResponder {
    async fn respond(
        &self,
        message: &Message,
        task: &Task,
    ) -> Result<(Message, TaskState), A2AError> {
        let echoed = message
            .parts
            .iter()
            .filter_map(|p| p.get_text())
            .collect::<Vec<_>>()
            .join(" ");

        let reply = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("Echo: {}", echoed))])
            .message_id(uuid::Uuid::new_v4().to_string())
            .task_id(task.id.clone())
            .context_id(message.context_id.clone())
            .build();

        // The reference handler keeps the task Working; real agents pick a
        // terminal state appropriate to their processing.
        Ok((reply, TaskState::Working))
    }
}

/// A message handler that owns task-lifecycle plumbing and streaming
/// announcements, delegating the reply to an injected [`Responder`].
///
/// Holds its ports as `Arc<dyn …>` trait objects (the composition edge — see
/// `REFACTORING_PLAN.md` Phase 2), so the handler carries no generic parameter.
/// Because it holds both the lifecycle and streaming ports it is a host for the
/// [`TaskStatusBroadcast`] capability mixin (§4.1).
#[derive(Clone)]
pub struct ResponderMessageHandler {
    /// Task lifecycle port for handling task operations
    task_lifecycle: Arc<dyn AsyncTaskLifecycle>,
    /// Streaming port for announcing status transitions to subscribers
    streaming: Arc<dyn AsyncStreamingHandler>,
    /// The business decision: what to reply and which state to end in
    responder: Arc<dyn Responder>,
}

impl ResponderMessageHandler {
    /// Create a handler with a custom [`Responder`].
    ///
    /// The lifecycle and streaming ports are commonly the same storage instance
    /// (which satisfies both), but they are accepted separately so the handler
    /// depends only on the capabilities it uses.
    pub fn new(
        task_lifecycle: impl AsyncTaskLifecycle + 'static,
        streaming: impl AsyncStreamingHandler + 'static,
        responder: impl Responder + 'static,
    ) -> Self {
        Self {
            task_lifecycle: Arc::new(task_lifecycle),
            streaming: Arc::new(streaming),
            responder: Arc::new(responder),
        }
    }

    /// Create the reference echo handler ([`EchoResponder`]).
    pub fn echo(
        task_lifecycle: impl AsyncTaskLifecycle + 'static,
        streaming: impl AsyncStreamingHandler + 'static,
    ) -> Self {
        Self::new(task_lifecycle, streaming, EchoResponder)
    }
}

impl HasTaskLifecycle for ResponderMessageHandler {
    fn lifecycle(&self) -> &dyn AsyncTaskLifecycle {
        self.task_lifecycle.as_ref()
    }
}

impl HasStreaming for ResponderMessageHandler {
    fn streaming(&self) -> &dyn AsyncStreamingHandler {
        self.streaming.as_ref()
    }
}

#[async_trait]
impl AsyncMessageHandler for ResponderMessageHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;

        // Create the task on first contact.
        if !self.task_lifecycle.exists(&id).await? {
            let context_id: ContextId = session_id.unwrap_or("default").parse()?;
            self.task_lifecycle.create(&id, &context_id).await?;
        }

        // Append the incoming message to history (Working), announcing the
        // transition to any streaming subscribers.
        let task = self
            .update_and_broadcast(&id, TaskState::Working, Some(message.clone()))
            .await?;

        // Delegate the business decision to the responder, then commit and
        // announce its reply.
        let (reply, state) = self.responder.respond(message, &task).await?;
        let final_task = self.update_and_broadcast(&id, state, Some(reply)).await?;

        Ok(final_task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::storage::InMemoryTaskStorage;

    /// A responder that ignores the input and drives the task to a terminal
    /// state with a fixed reply — proof that the injected responder, not the
    /// handler, owns the reply text and the final state.
    struct FixedResponder;

    #[async_trait]
    impl Responder for FixedResponder {
        async fn respond(
            &self,
            _message: &Message,
            task: &Task,
        ) -> Result<(Message, TaskState), A2AError> {
            let reply = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text("done".to_string())])
                .message_id("fixed-1".to_string())
                .task_id(task.id.clone())
                .build();
            Ok((reply, TaskState::Completed))
        }
    }

    #[tokio::test]
    async fn injected_responder_controls_reply_and_state() {
        let storage = InMemoryTaskStorage::new();
        let handler = ResponderMessageHandler::new(storage.clone(), storage, FixedResponder);

        let message = Message::user_text("anything".to_string(), "m1".to_string());
        let task = handler
            .process_message("t1", &message, None)
            .await
            .unwrap();

        // The responder chose the terminal state...
        assert_eq!(task.status.state, TaskState::Completed);
        // ...and its reply landed in history (after the appended user message).
        let replied = task.history.iter().any(|m| {
            m.parts
                .iter()
                .filter_map(|p| p.get_text())
                .any(|t| t == "done")
        });
        assert!(replied, "responder reply should be in task history");
    }
}
