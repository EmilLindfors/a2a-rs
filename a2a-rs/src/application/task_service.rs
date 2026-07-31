//! The task application service: use-case orchestration over the port traits.
//!
//! `TaskService` is the **inner** half of the service/transport split: it owns
//! the ports (`Arc<dyn …>`), orchestrates them, and speaks only the domain
//! vocabulary (`Task`, `Message`, `TaskId`, `A2AError`). It knows nothing about
//! ConnectRPC, `buffa` views, or wire error codes — that glue lives in the
//! transport adapter ([`ConnectRpcAdapter`](crate::adapter::ConnectRpcAdapter)),
//! which decodes wire requests into these domain calls and re-encodes the
//! results.
//!
//! Because the service holds both the lifecycle and streaming ports it exposes
//! them as mixin ingredients ([`HasTaskLifecycle`], [`HasStreaming`]) and so
//! gains [`TaskStatusBroadcast::update_and_broadcast`] for free
//! (`.claude/rules/hexagonal_architecture.md` §9). The accessors return `&dyn`
//! **ports**, never the concrete adapters behind them, so the dependency arrow
//! still points inward.
//!
//! [`TaskStatusBroadcast::update_and_broadcast`]: crate::application::TaskStatusBroadcast::update_and_broadcast

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt};

use crate::application::{HasPushNotifier, HasStreaming, HasTaskLifecycle, TaskStatusBroadcast};
use crate::domain::SendCompletion;
use crate::domain::core::task::TaskStateExt;
use crate::domain::{
    A2AError, AgentCard, DeleteTaskPushNotificationConfigParams,
    GetTaskPushNotificationConfigParams, ListTaskPushNotificationConfigsParams, ListTasksParams,
    ListTasksResult, Message, Task, TaskId, TaskPushNotificationConfig,
};
use crate::port::{
    AsyncMessageHandler, AsyncNotificationManager, AsyncNotificationManagerExt, AsyncPushNotifier,
    AsyncStreamingHandler, AsyncTaskLifecycle, AsyncTaskQuery, SeqEvent,
};
use crate::services::server::AgentInfoProvider;

/// A stream of sequenced update events for a task. Each [`SeqEvent`] carries a
/// per-task monotonic id (surfaced as the SSE `id:` field); the transport
/// adapter maps the inner update onto its wire representation.
pub type UpdateStream = Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>;

/// The optional knobs on a `SendMessage` request, decoded once from the wire
/// `SendMessageConfiguration` and shared by both transports.
///
/// Grouped into a struct rather than threaded as three more positional
/// parameters: the call already carried five, and a bare `Option<u32>` next to
/// a bare `bool` is exactly the signature where an argument gets passed in the
/// wrong slot.
#[derive(Debug, Clone, Default)]
pub struct SendOptions {
    /// Push-notification config to register for the task before processing.
    pub push_config: Option<TaskPushNotificationConfig>,
    /// Truncate the returned task's history to this many messages.
    pub history_limit: Option<u32>,
    /// Whether to hold the response until the task settles.
    pub completion: SendCompletion,
}

/// End `stream` after — and including — the event that settles the task.
///
/// The rule lives here rather than in each transport adapter because every
/// transport gets its stream from this service, and "the subscription outlives
/// the task" is the same bug in each of them. A check that runs in one entry
/// point and not the other is not a check (`NOTES.md`).
///
/// Implemented with `unfold` rather than `take_while`/`scan` for a reason that
/// is easy to get wrong: those combinators only decide to stop when the *next*
/// item arrives, and after a terminal state no next item ever arrives — the
/// underlying broadcast receiver simply parks. The stream would hang open on
/// exactly the events it is supposed to close on. Carrying the inner stream in
/// an `Option` and dropping it on settle terminates without polling again, and
/// dropping it is also what releases the subscription.
fn until_settled(stream: UpdateStream) -> UpdateStream {
    Box::pin(futures::stream::unfold(Some(stream), |state| async move {
        let mut stream = state?;
        let item = stream.next().await?;
        let settled = matches!(&item, Ok(seq) if seq.event.settles_task());
        Some((item, (!settled).then_some(stream)))
    }))
}

/// Use-case orchestration over the A2A ports.
///
/// Constructed at the composition edge with concrete adapters injected; the
/// fields are `Arc<dyn …>` so the service type carries no generic parameters.
/// All methods return domain types and [`A2AError`] — there is no transport
/// vocabulary in this layer.
#[derive(Clone)]
pub struct TaskService {
    message_handler: Arc<dyn AsyncMessageHandler>,
    task_lifecycle: Arc<dyn AsyncTaskLifecycle>,
    task_query: Arc<dyn AsyncTaskQuery>,
    notification_manager: Arc<dyn AsyncNotificationManager>,
    agent_info: Arc<dyn AgentInfoProvider>,
    streaming_handler: Arc<dyn AsyncStreamingHandler>,
    push_notifier: Arc<dyn AsyncPushNotifier>,
    send_wait: Duration,
}

/// How long a blocking `SendMessage` waits before returning the task unsettled.
///
/// **Must stay below the client's per-request timeout**, which is 30s for both
/// `JsonRpcClient` and `HttpClient` (and for `a2acli`, whose `--timeout`
/// defaults to theirs). If the server waited the full 30s the two would race,
/// and the client would report a transport timeout instead of receiving the
/// unsettled task the wait is supposed to hand back — turning a slow agent into
/// a connection error. The 5s of headroom is for the response itself.
///
/// Raising this without raising the client timeout re-creates that race.
const DEFAULT_SEND_WAIT: Duration = Duration::from_secs(25);

impl TaskService {
    /// Assemble a service from separate handlers.
    ///
    /// `tasks` supplies both the lifecycle and query capabilities; it is
    /// stored once and shared between the two `Arc<dyn …>` fields.
    pub fn new(
        message_handler: impl AsyncMessageHandler + 'static,
        tasks: impl AsyncTaskLifecycle + AsyncTaskQuery + 'static,
        notification_manager: impl AsyncNotificationManager + 'static,
        agent_info: impl AgentInfoProvider + 'static,
        streaming_handler: impl AsyncStreamingHandler + 'static,
        push_notifier: impl AsyncPushNotifier + 'static,
    ) -> Self {
        let tasks = Arc::new(tasks);
        Self {
            message_handler: Arc::new(message_handler),
            task_lifecycle: tasks.clone(),
            task_query: tasks,
            notification_manager: Arc::new(notification_manager),
            agent_info: Arc::new(agent_info),
            streaming_handler: Arc::new(streaming_handler),
            push_notifier: Arc::new(push_notifier),
            send_wait: DEFAULT_SEND_WAIT,
        }
    }

    /// Assemble a service from a single handler that implements every port.
    pub fn with_handler(
        handler: impl AsyncMessageHandler
        + AsyncTaskLifecycle
        + AsyncTaskQuery
        + AsyncNotificationManager
        + 'static,
        agent_info: impl AgentInfoProvider + 'static,
        streaming_handler: impl AsyncStreamingHandler + 'static,
        push_notifier: impl AsyncPushNotifier + 'static,
    ) -> Self {
        let handler = Arc::new(handler);
        Self {
            message_handler: handler.clone(),
            task_lifecycle: handler.clone(),
            task_query: handler.clone(),
            notification_manager: handler,
            agent_info: Arc::new(agent_info),
            streaming_handler: Arc::new(streaming_handler),
            push_notifier: Arc::new(push_notifier),
            send_wait: DEFAULT_SEND_WAIT,
        }
    }

    /// Replace the streaming handler, returning the updated service.
    pub fn with_streaming_handler(
        mut self,
        streaming_handler: impl AsyncStreamingHandler + 'static,
    ) -> Self {
        self.streaming_handler = Arc::new(streaming_handler);
        self
    }

    /// Replace the push notifier, returning the updated service.
    pub fn with_push_notifier(mut self, push_notifier: impl AsyncPushNotifier + 'static) -> Self {
        self.push_notifier = Arc::new(push_notifier);
        self
    }

    /// How long a blocking `SendMessage` waits for the task to settle before
    /// returning it unsettled. Defaults to 25s.
    ///
    /// Raise it for agents that legitimately take minutes — but raise the
    /// calling client's request timeout with it. The two are a pair: whichever
    /// is shorter decides what the caller sees, and if the client gives up
    /// first it gets a transport error instead of the task.
    pub fn with_send_wait(mut self, send_wait: Duration) -> Self {
        self.send_wait = send_wait;
        self
    }

    /// Process a message for a task, optionally configuring push notifications
    /// and limiting the returned history.
    ///
    /// With [`SendCompletion::WhenSettled`] — the spec default — the response is
    /// held until the task reaches a terminal or interrupted state, bounded by
    /// [`with_send_wait`]. The wait is driven by the streaming handler rather
    /// than a poll loop: it already broadcasts every transition, so a subscriber
    /// *is* the wait.
    ///
    /// Two ordering details are load-bearing. The subscription is opened
    /// **before** `process_message`, because a handler that finishes
    /// synchronously (the echo responder does) broadcasts its terminal event
    /// during that call — subscribing afterwards would miss it and then wait
    /// for a transition that has already happened. And the task is re-fetched
    /// after the wait rather than assembled from the event, because the event
    /// carries a status, not the artifacts and history the caller asked for.
    ///
    /// [`with_send_wait`]: TaskService::with_send_wait
    pub async fn send_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
        opts: SendOptions,
    ) -> Result<Task, A2AError> {
        if let Some(mut push_config) = opts.push_config {
            push_config.task_id = task_id.to_string();
            self.notification_manager
                .set_validated(&push_config)
                .await?;
        }

        let updates = match opts.completion {
            SendCompletion::WhenCreated => None,
            // A handler with no streaming backend (`NoopStreamingHandler`)
            // reports `UnsupportedOperation` here. That is not a reason to fail
            // the send: it means this server cannot observe transitions, so the
            // most it can honestly do is return what it has.
            SendCompletion::WhenSettled => self
                .streaming_handler
                .start_task_streaming(task_id, None)
                .await
                .ok(),
        };

        let mut task = self
            .message_handler
            .process_message(task_id, message, session_id)
            .await?;

        if let Some(updates) = updates
            && !task.status.state.is_settled()
        {
            task = self.wait_for_settled(task_id, updates).await?;
        }

        if let Some(limit) = opts.history_limit {
            task = task.with_limited_history(Some(limit));
        }

        Ok(task)
    }

    /// Block on `updates` until the task settles or the budget runs out, then
    /// return the task as stored.
    ///
    /// On expiry the *current* task is returned rather than an error. The state
    /// it carries is true — `WORKING` says exactly that the agent has not
    /// finished — so the caller gets a usable task id and can follow it, which
    /// an error would deny them. The bound exists because the spec's "MUST
    /// wait" has no escape clause, and an agent that never finishes would
    /// otherwise pin the connection for as long as the client tolerates it.
    async fn wait_for_settled(
        &self,
        task_id: &str,
        updates: UpdateStream,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;

        // `until_settled` ends the stream on the settling event, so draining it
        // to completion *is* the wait — no per-item inspection needed.
        let drained = tokio::time::timeout(self.send_wait, async {
            let mut updates = until_settled(updates);
            while updates.next().await.is_some() {}
        })
        .await;

        if drained.is_err() {
            #[cfg(feature = "tracing")]
            tracing::debug!(
                task_id,
                timeout_secs = self.send_wait.as_secs(),
                "send_message gave up waiting for the task to settle; returning it unsettled"
            );
        }

        self.task_lifecycle.get(&id, None).await
    }

    /// Process a message and subscribe to its update stream.
    ///
    /// The update stream is started **before** the message is processed so no
    /// early updates are missed. Returns the initial task and the stream; the
    /// caller is responsible for emitting the initial task ahead of stream
    /// items.
    ///
    /// The stream ends once the task settles (see [`until_settled`]), so a
    /// caller that reads to completion is not left holding an open connection
    /// to a finished task.
    pub async fn send_streaming_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
        push_config: Option<TaskPushNotificationConfig>,
        history_limit: Option<u32>,
    ) -> Result<(Task, UpdateStream), A2AError> {
        if let Some(mut push_config) = push_config {
            push_config.task_id = task_id.to_string();
            self.notification_manager
                .set_validated(&push_config)
                .await?;
        }

        // Start updates stream first so we don't miss early updates.
        let update_stream = self
            .streaming_handler
            .start_task_streaming(task_id, None)
            .await?;

        let mut task = self
            .message_handler
            .process_message(task_id, message, session_id)
            .await?;

        if let Some(limit) = history_limit {
            task = task.with_limited_history(Some(limit));
        }

        Ok((task, until_settled(update_stream)))
    }

    /// Get a task by ID with optional history length limit.
    pub async fn get(&self, id: &TaskId, history_length: Option<u32>) -> Result<Task, A2AError> {
        self.task_lifecycle.get(id, history_length).await
    }

    /// List tasks with filtering and pagination.
    pub async fn list(&self, params: &ListTasksParams) -> Result<ListTasksResult, A2AError> {
        self.task_query.list(params).await
    }

    /// Cancel a task, then announce the terminal status to streaming
    /// subscribers.
    ///
    /// Storage no longer self-broadcasts on cancellation (§4.0.2), so the
    /// service owns the "commit then announce" step via the
    /// [`TaskStatusBroadcast`] mixin it hosts.
    pub async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError> {
        self.cancel_and_broadcast(id).await
    }

    /// Subscribe to a task's update stream, returning the current task (if it
    /// exists) and the stream of subsequent updates.
    ///
    /// `from_event_id` carries a client's `Last-Event-ID` for resumption: when
    /// set, the handler replays buffered events with a greater id before
    /// streaming live updates.
    ///
    /// The stream ends once the task settles (see [`until_settled`]). If the
    /// task is *already* terminal and no resumption point was given, the caller
    /// gets its snapshot and an empty stream, because nothing further can ever
    /// be broadcast for it.
    ///
    /// That short-circuit is conditional on `from_event_id` being unset, and
    /// that condition is load-bearing: resuming after a disconnect on a task
    /// that has since finished is precisely when the replay buffer matters —
    /// the events the client missed are the ones it reconnected for. Skipping
    /// the handler because the task looks finished would turn resumption into
    /// silence.
    ///
    /// A task already sitting in an interrupted state (`INPUT_REQUIRED`,
    /// `AUTH_REQUIRED`) deliberately does **not** short-circuit: it resumes
    /// under the same id once the caller supplies what it asked for, and a
    /// subscriber that attached first is entitled to watch that happen. The
    /// asymmetry with [`UpdateEvent::settles_task`] is the point — arriving at
    /// an interrupted state ends a stream, finding one already there does not.
    ///
    /// [`UpdateEvent::settles_task`]: crate::port::UpdateEvent::settles_task
    pub async fn subscribe(
        &self,
        task_id: &str,
        from_event_id: Option<u64>,
    ) -> Result<(Option<Task>, UpdateStream), A2AError> {
        let id: TaskId = task_id.parse()?;

        let initial_task = match self.task_lifecycle.get(&id, None).await {
            Ok(task) => Some(task),
            Err(A2AError::TaskNotFound(_)) => None,
            Err(e) => return Err(e),
        };

        if from_event_id.is_none()
            && let Some(task) = &initial_task
            && task.status.state.is_terminal()
        {
            return Ok((initial_task, Box::pin(futures::stream::empty())));
        }

        let update_stream = self
            .streaming_handler
            .start_task_streaming(task_id, from_event_id)
            .await?;

        Ok((initial_task, until_settled(update_stream)))
    }

    /// Create or replace a push-notification config (validated).
    pub async fn set_push_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.notification_manager.set_validated(config).await
    }

    /// Get a push-notification config for a task.
    pub async fn get_push_config(
        &self,
        params: &GetTaskPushNotificationConfigParams,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.notification_manager.get_config(params).await
    }

    /// List push-notification configs for a task.
    pub async fn list_push_configs(
        &self,
        params: &ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        self.notification_manager.list_configs(params).await
    }

    /// Delete a push-notification config.
    pub async fn delete_push_config(
        &self,
        params: &DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        self.notification_manager.delete_config(params).await
    }

    /// Fetch the authenticated extended agent card.
    pub async fn extended_agent_card(&self) -> Result<AgentCard, A2AError> {
        self.agent_info.get_authenticated_extended_card().await
    }
}

// The service is the composed assembly holding both the lifecycle and streaming
// ports, so it exposes them as mixin ingredients (see
// `.claude/rules/hexagonal_architecture.md` §9). This grants it the
// `TaskStatusBroadcast::update_and_broadcast` "commit then announce" capability
// for free, without coupling either port to the other. The accessors return
// `&dyn` **ports**, never the concrete adapters behind them.
impl HasTaskLifecycle for TaskService {
    fn lifecycle(&self) -> &dyn AsyncTaskLifecycle {
        self.task_lifecycle.as_ref()
    }
}

impl HasStreaming for TaskService {
    fn streaming(&self) -> &dyn AsyncStreamingHandler {
        self.streaming_handler.as_ref()
    }
}

impl HasPushNotifier for TaskService {
    fn push_notifier(&self) -> &dyn AsyncPushNotifier {
        self.push_notifier.as_ref()
    }
}
