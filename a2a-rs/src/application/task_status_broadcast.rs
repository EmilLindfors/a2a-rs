//! Cross-port orchestration: update a task's status *and* broadcast it.
//!
//! This is the [capability-mixin] pattern applied at the port boundary
//! (`.claude/rules/hexagonal_architecture.md` §9). Two narrow **accessor**
//! ingredients ([`HasTaskLifecycle`], [`HasStreaming`]) expose the ports a host
//! already holds; the [`TaskStatusBroadcast`] mixin provides the derived
//! "update then broadcast" behavior as a blanket-impl'd default. Any assembly
//! that exposes both ports — the request processor, the MCP bridge, a test
//! rig — gains `update_and_broadcast` for free, and on nothing inner.
//!
//! Why a mixin and not just a method on the processor: the orchestration is
//! defined independently of any one struct (reusable across hosts) and is
//! testable against a minimal rig that wires only these two ports over
//! in-memory adapters — see the tests below.
//!
//! [capability-mixin]: crate::port
//!
//! ## Layering note
//!
//! The accessor associated returns are bounded by **port traits**
//! (`&dyn AsyncTaskLifecycle`, `&dyn AsyncStreamingHandler`), never concrete
//! adapters, and the mixin default touches only those ports plus pure domain
//! constructors (`TaskStatus::new`). The dependency arrow therefore still
//! points inward even though the logic lives in a blanket impl.

use async_trait::async_trait;

use crate::domain::{
    A2AError, Message, Task, TaskArtifactUpdateEvent, TaskId, TaskState, TaskStatusUpdateEvent,
};
use crate::port::{AsyncPushNotifier, AsyncStreamingHandler, AsyncTaskLifecycle};

/// Ingredient: an assembly that can hand out a task-lifecycle port.
///
/// Note the return is a `&dyn` **port**, not a concrete adapter — that is what
/// keeps any mixin built on this ingredient inside the dependency rule.
pub trait HasTaskLifecycle {
    fn lifecycle(&self) -> &dyn AsyncTaskLifecycle;
}

/// Ingredient: an assembly that can hand out a streaming port.
pub trait HasStreaming {
    fn streaming(&self) -> &dyn AsyncStreamingHandler;
}

/// Ingredient: an assembly that can hand out a push-notifier port.
///
/// Kept separate from [`HasStreaming`] on purpose: in-process streaming fan-out
/// and out-of-band webhook delivery are distinct capabilities with distinct
/// backends, so the mixin orchestrates both rather than fusing them into one
/// adapter.
pub trait HasPushNotifier {
    fn push_notifier(&self) -> &dyn AsyncPushNotifier;
}

/// Derived capability: mutate task status through the lifecycle port, then
/// broadcast the resulting status to streaming subscribers.
///
/// Blanket-implemented for every `Send + Sync` host that exposes both
/// ingredients, so it never needs an explicit `impl`. A host that exposes only
/// one ingredient does **not** get this method — that omission is a compile
/// error at the call site, not a runtime surprise (see the `compile_fail` doc
/// test on [`update_and_broadcast`]).
///
/// [`update_and_broadcast`]: TaskStatusBroadcast::update_and_broadcast
#[async_trait]
pub trait TaskStatusBroadcast:
    HasTaskLifecycle + HasStreaming + HasPushNotifier + Send + Sync
{
    /// Update a task's status, then broadcast the new status to subscribers.
    ///
    /// The broadcast is best-effort relative to the store: the status is
    /// persisted first (via the lifecycle port) and only then announced, so a
    /// subscriber never sees a state the store hasn't committed.
    ///
    /// A host that exposes only *one* of the two ingredients does not get this
    /// method — the missing supertrait makes the blanket impl inapplicable, so
    /// the call fails to compile:
    ///
    /// ```compile_fail
    /// use std::sync::Arc;
    /// use a2a_rs::AsyncTaskLifecycle;
    /// use a2a_rs::adapter::storage::InMemoryTaskStorage;
    /// use a2a_rs::application::{HasTaskLifecycle, TaskStatusBroadcast};
    /// use a2a_rs::domain::{TaskId, TaskState};
    ///
    /// // Exposes the lifecycle ingredient, but NOT `HasStreaming`.
    /// struct HalfRig {
    ///     store: Arc<InMemoryTaskStorage>,
    /// }
    /// impl HasTaskLifecycle for HalfRig {
    ///     fn lifecycle(&self) -> &dyn AsyncTaskLifecycle {
    ///         self.store.as_ref()
    ///     }
    /// }
    ///
    /// async fn use_it(rig: HalfRig, id: TaskId) {
    ///     // `update_and_broadcast` does not exist on a one-ingredient host:
    ///     rig.update_and_broadcast(&id, TaskState::Completed, None).await.unwrap();
    /// }
    /// ```
    async fn update_and_broadcast(
        &self,
        id: &TaskId,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2AError> {
        let task = self.lifecycle().update_status(id, state, message).await?;
        self.broadcast_current_status(id, &task).await?;
        Ok(task)
    }

    /// Cancel a task through the lifecycle port, then broadcast the resulting
    /// (terminal) status to subscribers.
    ///
    /// The counterpart to [`update_and_broadcast`](Self::update_and_broadcast)
    /// for cancellation: `cancel` carries its own state transition and history
    /// message, so it cannot be expressed as an `update_status` call, but the
    /// "commit then announce" ordering is identical.
    async fn cancel_and_broadcast(&self, id: &TaskId) -> Result<Task, A2AError> {
        let task = self.lifecycle().cancel(id).await?;
        self.broadcast_current_status(id, &task).await?;
        Ok(task)
    }

    /// Broadcast an artifact update: fan it out to streaming subscribers, then
    /// deliver it to the task's push endpoint (best-effort).
    ///
    /// The artifact counterpart to the status path. Hosts that produce artifacts
    /// route through here so streaming and push stay consistent — exactly as the
    /// status mutators do via [`broadcast_current_status`](Self::broadcast_current_status).
    async fn broadcast_artifact(
        &self,
        id: &TaskId,
        event: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        self.streaming()
            .broadcast_artifact_update(id.as_str(), event.clone())
            .await?;
        self.notify_push_artifact(id, &event).await;
        Ok(())
    }

    /// Announce a task's current status to streaming subscribers, then deliver a
    /// push notification (best-effort).
    ///
    /// Shared by the mutate-then-broadcast methods above; not intended to be
    /// overridden. The event is built from the freshly-committed `task` so the
    /// announcement always reflects what the store now holds. Push delivery is
    /// best-effort: a webhook that is down is logged but does not fail the
    /// mutation that triggered it.
    #[doc(hidden)]
    async fn broadcast_current_status(&self, id: &TaskId, task: &Task) -> Result<(), A2AError> {
        let event = TaskStatusUpdateEvent {
            task_id: task.id.clone(),
            context_id: task.context_id.clone(),
            kind: "status-update".to_string(),
            status: task.status.clone().into_option().unwrap_or_default(),
            metadata: None,
        };

        self.streaming()
            .broadcast_status_update(id.as_str(), event.clone())
            .await?;
        self.notify_push_status(id, &event).await;
        Ok(())
    }

    /// Deliver a status push notification, swallowing (and logging) any delivery
    /// error so it never fails the mutation.
    #[doc(hidden)]
    async fn notify_push_status(&self, id: &TaskId, event: &TaskStatusUpdateEvent) {
        if let Err(_e) = self.push_notifier().notify_status(id.as_str(), event).await {
            #[cfg(feature = "tracing")]
            tracing::warn!(task_id = %id.as_str(), error = %_e, "push status notification failed");
        }
    }

    /// Deliver an artifact push notification, swallowing (and logging) any
    /// delivery error.
    #[doc(hidden)]
    async fn notify_push_artifact(&self, id: &TaskId, event: &TaskArtifactUpdateEvent) {
        if let Err(_e) = self
            .push_notifier()
            .notify_artifact(id.as_str(), event)
            .await
        {
            #[cfg(feature = "tracing")]
            tracing::warn!(task_id = %id.as_str(), error = %_e, "push artifact notification failed");
        }
    }
}

/// The single blanket impl — the linchpin of the pattern. `?Sized` lets the
/// mixin attach to a `dyn`-typed host as well as a concrete one.
impl<T: HasTaskLifecycle + HasStreaming + HasPushNotifier + Send + Sync + ?Sized>
    TaskStatusBroadcast for T
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::storage::InMemoryTaskStorage;
    use crate::adapter::streaming::InMemoryStreamingHandler;
    use crate::port::NoopPushNotifier;
    use crate::port::streaming_handler::Subscriber;
    use std::sync::{Arc, Mutex};

    /// A "partial platform" test rig: it wires the three ingredients this mixin
    /// needs — a persistence adapter, a separate streaming adapter, and a push
    /// notifier — over in-memory implementations. Standing this up requires
    /// neither the transport layer nor the full request processor, so the
    /// orchestration is tested in isolation. The split between `store` and
    /// `streaming` is the whole point: they are distinct ports now.
    struct BroadcastRig {
        store: Arc<InMemoryTaskStorage>,
        streaming: InMemoryStreamingHandler,
        push: NoopPushNotifier,
    }

    impl HasTaskLifecycle for BroadcastRig {
        fn lifecycle(&self) -> &dyn AsyncTaskLifecycle {
            self.store.as_ref()
        }
    }

    impl HasStreaming for BroadcastRig {
        fn streaming(&self) -> &dyn AsyncStreamingHandler {
            &self.streaming
        }
    }

    impl HasPushNotifier for BroadcastRig {
        fn push_notifier(&self) -> &dyn AsyncPushNotifier {
            &self.push
        }
    }

    /// A streaming subscriber that records every status it is handed, so a test
    /// can assert exactly which transitions reached subscribers.
    #[derive(Clone, Default)]
    struct Recorder {
        states: Arc<Mutex<Vec<::buffa::EnumValue<TaskState>>>>,
    }

    #[async_trait]
    impl Subscriber<TaskStatusUpdateEvent> for Recorder {
        async fn on_update(&self, update: TaskStatusUpdateEvent) -> Result<(), A2AError> {
            self.states.lock().unwrap().push(update.status.state);
            Ok(())
        }
    }

    fn rig(store: Arc<InMemoryTaskStorage>) -> BroadcastRig {
        BroadcastRig {
            store,
            streaming: InMemoryStreamingHandler::new(),
            push: NoopPushNotifier,
        }
    }

    #[tokio::test]
    async fn update_and_broadcast_persists_then_announces() {
        let store = Arc::new(InMemoryTaskStorage::new());
        let id = TaskId::try_from("task-1").unwrap();
        let ctx = crate::domain::ContextId::try_from("ctx-1").unwrap();

        store.create(&id, &ctx).await.unwrap();
        store
            .update_status(&id, TaskState::Working, None)
            .await
            .unwrap();

        let rig = rig(store);

        // The mixin method exists purely because the rig exposes ALL ingredients.
        let task = rig
            .update_and_broadcast(&id, TaskState::Completed, None)
            .await
            .unwrap();

        assert_eq!(task.status.state, TaskState::Completed);
    }

    /// A direct lifecycle mutation must NOT announce anything: persistence and
    /// streaming are fully separate adapters now. The subscriber lives on the
    /// streaming handler, which the bare store mutation never touches, so the
    /// recorder stays empty.
    #[tokio::test]
    async fn bare_update_status_does_not_broadcast() {
        let store = Arc::new(InMemoryTaskStorage::new());
        let id = TaskId::try_from("task-1").unwrap();
        let ctx = crate::domain::ContextId::try_from("ctx-1").unwrap();

        let streaming = InMemoryStreamingHandler::new();
        let recorder = Recorder::default();
        streaming
            .add_status_subscriber(id.as_str(), Box::new(recorder.clone()))
            .await
            .unwrap();

        store.create(&id, &ctx).await.unwrap();
        store
            .update_status(&id, TaskState::Working, None)
            .await
            .unwrap();
        store.cancel(&id).await.unwrap();

        assert!(
            recorder.states.lock().unwrap().is_empty(),
            "storage mutators must not self-broadcast"
        );
    }

    /// Routed through the mixin, the same mutations DO reach subscribers — once
    /// each, in order. (One announcement per call proves there is no lingering
    /// self-broadcast doubling the events.) The recorder is registered on the
    /// rig's *streaming* handler, which the mixin fans out to.
    #[tokio::test]
    async fn mixin_announces_each_mutation_once() {
        let store = Arc::new(InMemoryTaskStorage::new());
        let id = TaskId::try_from("task-1").unwrap();
        let ctx = crate::domain::ContextId::try_from("ctx-1").unwrap();

        store.create(&id, &ctx).await.unwrap();

        let rig = rig(store);

        let recorder = Recorder::default();
        rig.streaming
            .add_status_subscriber(id.as_str(), Box::new(recorder.clone()))
            .await
            .unwrap();

        rig.update_and_broadcast(&id, TaskState::Working, None)
            .await
            .unwrap();
        rig.cancel_and_broadcast(&id).await.unwrap();

        assert_eq!(
            *recorder.states.lock().unwrap(),
            vec![
                ::buffa::EnumValue::from(TaskState::Working),
                ::buffa::EnumValue::from(TaskState::Canceled),
            ],
        );
    }
}
