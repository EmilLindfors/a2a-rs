//! In-memory streaming fan-out adapter.
//!
//! `InMemoryStreamingHandler` is the [`AsyncStreamingHandler`] adapter. It owns
//! **only** the per-task fan-out state — a broadcast channel plus a bounded
//! replay buffer, and an optional set of synchronous callback subscribers — and
//! fans broadcast events out to live `combined_update_stream` readers and to
//! those subscribers. It deliberately does *not*:
//!
//! - touch the task store (so it cannot replay current task state on subscribe —
//!   the initial `Task` snapshot is delivered by the application service before
//!   stream items, which is spec-compliant), nor
//! - fire push-webhook notifications (that is the [`AsyncPushNotifier`] port's
//!   job, orchestrated by the
//!   [`TaskStatusBroadcast`](crate::application::TaskStatusBroadcast) mixin).
//!
//! Each broadcast event is assigned a per-task monotonic id and retained in a
//! bounded ring buffer, so a reconnecting client can resume after a disconnect
//! by passing the last id it observed (`from_event_id`); the handler replays the
//! buffered tail with a greater id before switching to live updates.
//!
//! [`AsyncPushNotifier`]: crate::port::AsyncPushNotifier

use std::collections::HashMap;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use crate::domain::{A2AError, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use crate::port::AsyncStreamingHandler;
use crate::port::streaming_handler::{SeqEvent, Subscriber, UpdateEvent};

type StatusSubscribers = Vec<Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>>;
type ArtifactSubscribers = Vec<Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>>;

/// Capacity of the per-task broadcast channel and replay ring buffer.
const CHANNEL_CAPACITY: usize = 256;
const RING_CAPACITY: usize = 256;

/// Per-task fan-out state: a broadcast channel for live readers, a bounded
/// replay buffer keyed by monotonic id, and any synchronous callback
/// subscribers.
struct TaskChannel {
    sender: broadcast::Sender<SeqEvent>,
    next_id: u64,
    buffer: VecDeque<SeqEvent>,
    status: StatusSubscribers,
    artifacts: ArtifactSubscribers,
}

impl TaskChannel {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            sender,
            next_id: 0,
            buffer: VecDeque::with_capacity(RING_CAPACITY),
            status: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    /// Assign the next id, retain the event for replay, and publish it to live
    /// readers. Returns the sequenced event for any further fan-out.
    fn publish(&mut self, event: UpdateEvent) -> SeqEvent {
        self.next_id += 1;
        let seq = SeqEvent::new(self.next_id, event);
        if self.buffer.len() == RING_CAPACITY {
            self.buffer.pop_front();
        }
        self.buffer.push_back(seq.clone());
        // A send error just means there are no live receivers; the buffer still
        // retains the event for a later resume, so the error is ignored.
        let _ = self.sender.send(seq.clone());
        seq
    }

    /// Buffered events with an id strictly greater than `from`, in order.
    fn replay_after(&self, from: u64) -> Vec<SeqEvent> {
        self.buffer
            .iter()
            .filter(|e| e.id > from)
            .cloned()
            .collect()
    }
}

/// In-memory [`AsyncStreamingHandler`]: per-task broadcast fan-out with a
/// bounded replay buffer for Last-Event-ID resumption.
///
/// Cloning shares the underlying per-task state (an `Arc<Mutex<…>>`), so a clone
/// observes the same channels and subscribers.
#[derive(Clone, Default)]
pub struct InMemoryStreamingHandler {
    tasks: Arc<Mutex<HashMap<String, TaskChannel>>>,
}

impl InMemoryStreamingHandler {
    /// Create an empty streaming handler.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AsyncStreamingHandler for InMemoryStreamingHandler {
    async fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        #[cfg(feature = "tracing")]
        tracing::info!(
            task_id = %task_id,
            "✅ Adding subscriber for status updates"
        );

        let mut guard = self.tasks.lock().await;
        guard
            .entry(task_id.to_string())
            .or_insert_with(TaskChannel::new)
            .status
            .push(subscriber);

        Ok(format!("status-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        let mut guard = self.tasks.lock().await;
        guard
            .entry(task_id.to_string())
            .or_insert_with(TaskChannel::new)
            .artifacts
            .push(subscriber);

        Ok(format!("artifact-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn remove_subscription(&self, _subscription_id: &str) -> Result<(), A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Subscription removal by ID is not supported by the in-memory streaming handler"
                .to_string(),
        ))
    }

    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        let mut guard = self.tasks.lock().await;
        guard.remove(task_id);
        Ok(())
    }

    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        let guard = self.tasks.lock().await;
        Ok(guard
            .get(task_id)
            .map(|c| c.status.len() + c.artifacts.len() + c.sender.receiver_count())
            .unwrap_or(0))
    }

    async fn broadcast_status_update(
        &self,
        task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        #[cfg(feature = "tracing")]
        tracing::debug!(
            task_id = %task_id,
            state = ?update.status.state,
            "📡 Broadcasting status update to subscribers"
        );

        let mut guard = self.tasks.lock().await;
        let channel = guard
            .entry(task_id.to_string())
            .or_insert_with(TaskChannel::new);
        channel.publish(UpdateEvent::StatusUpdate(update.clone()));
        for subscriber in channel.status.iter() {
            if let Err(e) = subscriber.on_update(update.clone()).await {
                #[cfg(feature = "tracing")]
                tracing::error!(task_id = %task_id, error = %e, "❌ Failed to notify subscriber");
                #[cfg(not(feature = "tracing"))]
                let _ = e;
            }
        }
        Ok(())
    }

    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        let mut guard = self.tasks.lock().await;
        let channel = guard
            .entry(task_id.to_string())
            .or_insert_with(TaskChannel::new);
        channel.publish(UpdateEvent::ArtifactUpdate(update.clone()));
        for subscriber in channel.artifacts.iter() {
            if let Err(e) = subscriber.on_update(update.clone()).await {
                #[cfg(feature = "tracing")]
                tracing::error!(task_id = %task_id, error = %e, "❌ Failed to notify subscriber");
                #[cfg(not(feature = "tracing"))]
                let _ = e;
            }
        }
        Ok(())
    }

    async fn status_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>, A2AError>
    {
        Err(A2AError::UnsupportedOperation(
            "Status-only update stream is not supported; use combined_update_stream".to_string(),
        ))
    }

    async fn artifact_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Artifact-only update stream is not supported; use combined_update_stream".to_string(),
        ))
    }

    async fn combined_update_stream(
        &self,
        task_id: &str,
        from_event_id: Option<u64>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>, A2AError> {
        let mut guard = self.tasks.lock().await;
        let channel = guard
            .entry(task_id.to_string())
            .or_insert_with(TaskChannel::new);
        let receiver = channel.sender.subscribe();
        let replay = from_event_id
            .map(|from| channel.replay_after(from))
            .unwrap_or_default();
        drop(guard);

        let live = futures::stream::unfold(receiver, |mut rx| async move {
            match rx.recv().await {
                Ok(event) => Some((Ok(event), rx)),
                // Reader fell behind the ring buffer: surface an error so a
                // resilient client reconnects and resumes from its last id.
                Err(broadcast::error::RecvError::Lagged(n)) => Some((
                    Err(A2AError::Internal(format!(
                        "streaming reader lagged, dropped {n} events"
                    ))),
                    rx,
                )),
                Err(broadcast::error::RecvError::Closed) => None,
            }
        });

        let stream = futures::stream::iter(replay.into_iter().map(Ok)).chain(live);
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{TaskState, TaskStatus, TaskStatusUpdateEvent};

    fn status_event(task_id: &str, state: TaskState) -> TaskStatusUpdateEvent {
        TaskStatusUpdateEvent {
            task_id: task_id.to_string(),
            context_id: "ctx".to_string(),
            kind: "status-update".to_string(),
            status: TaskStatus::new(state, None),
            metadata: None,
        }
    }

    fn seq_state(seq: &SeqEvent) -> ::buffa::EnumValue<TaskState> {
        match &seq.event {
            UpdateEvent::StatusUpdate(e) => e.status.state,
            UpdateEvent::ArtifactUpdate(_) => panic!("expected status update"),
        }
    }

    /// A live `combined_update_stream` reader receives broadcasts in order, each
    /// tagged with a monotonic id starting at 1.
    #[tokio::test]
    async fn live_stream_delivers_in_order_with_ids() {
        let handler = InMemoryStreamingHandler::new();
        let mut stream = handler.combined_update_stream("t1", None).await.unwrap();

        handler
            .broadcast_status_update("t1", status_event("t1", TaskState::Working))
            .await
            .unwrap();
        handler
            .broadcast_status_update("t1", status_event("t1", TaskState::Completed))
            .await
            .unwrap();

        let first = stream.next().await.unwrap().unwrap();
        let second = stream.next().await.unwrap().unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(
            seq_state(&first),
            ::buffa::EnumValue::from(TaskState::Working)
        );
        assert_eq!(second.id, 2);
        assert_eq!(
            seq_state(&second),
            ::buffa::EnumValue::from(TaskState::Completed)
        );
    }

    /// Subscribing with `from_event_id` replays the buffered tail with a greater
    /// id before any live updates.
    #[tokio::test]
    async fn resume_replays_buffered_tail() {
        let handler = InMemoryStreamingHandler::new();
        // Emit two events with no live reader; they are retained in the buffer.
        handler
            .broadcast_status_update("t1", status_event("t1", TaskState::Working))
            .await
            .unwrap();
        handler
            .broadcast_status_update("t1", status_event("t1", TaskState::Completed))
            .await
            .unwrap();

        // Resume from id 1: only event 2 should replay.
        let mut stream = handler.combined_update_stream("t1", Some(1)).await.unwrap();
        let replayed = stream.next().await.unwrap().unwrap();
        assert_eq!(replayed.id, 2);
        assert_eq!(
            seq_state(&replayed),
            ::buffa::EnumValue::from(TaskState::Completed)
        );
    }

    /// A synchronous callback subscriber still receives broadcasts (the push API
    /// rides alongside the broadcast channel).
    #[tokio::test]
    async fn callback_subscriber_still_notified() {
        use std::sync::Mutex as StdMutex;

        #[derive(Default, Clone)]
        struct Recorder {
            seen: Arc<StdMutex<Vec<::buffa::EnumValue<TaskState>>>>,
        }
        #[async_trait]
        impl Subscriber<TaskStatusUpdateEvent> for Recorder {
            async fn on_update(&self, update: TaskStatusUpdateEvent) -> Result<(), A2AError> {
                self.seen.lock().unwrap().push(update.status.state);
                Ok(())
            }
        }

        let handler = InMemoryStreamingHandler::new();
        let recorder = Recorder::default();
        handler
            .add_status_subscriber("t1", Box::new(recorder.clone()))
            .await
            .unwrap();
        handler
            .broadcast_status_update("t1", status_event("t1", TaskState::Working))
            .await
            .unwrap();

        assert_eq!(
            *recorder.seen.lock().unwrap(),
            vec![::buffa::EnumValue::from(TaskState::Working)]
        );
    }
}
