//! Streaming fan-out adapter over an [`AsyncEventLog`].
//!
//! [`StreamingFanout`] is the [`AsyncStreamingHandler`] adapter. It owns the
//! in-process side of streaming — a broadcast channel per task, plus an optional
//! set of synchronous callback subscribers — and delegates ids and retention to
//! the event log it was built over. It deliberately does *not*:
//!
//! - touch the task store (so it cannot replay current task state on subscribe —
//!   the initial `Task` snapshot is delivered by the application service before
//!   stream items, which is spec-compliant), nor
//! - fire push-webhook notifications (that is the [`AsyncPushNotifier`] port's
//!   job, orchestrated by the
//!   [`TaskStatusBroadcast`](crate::application::TaskStatusBroadcast) mixin).
//!
//! The split with the log is what durability turns on. Who is listening right
//! now is this process's business and a restart may forget it; what the stream
//! already said is the log's, and a durable log still has it after the restart.
//! [`InMemoryStreamingHandler`] pairs the fan-out with an in-memory log, which
//! is the zero-configuration default; `SqlxTaskStorage` implements the same port
//! for a log that outlives the process.
//!
//! [`AsyncPushNotifier`]: crate::port::AsyncPushNotifier

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use crate::adapter::storage::event_log::InMemoryEventLog;
use crate::domain::{A2AError, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use crate::port::AsyncStreamingHandler;
use crate::port::event_log::AsyncEventLog;
use crate::port::streaming_handler::{SeqEvent, Subscriber, UpdateEvent};

type StatusSubscribers = Vec<Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>>;
type ArtifactSubscribers = Vec<Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>>;

/// Capacity of the per-task broadcast channel. A reader that falls this far
/// behind is told it lagged and reconnects; the log, not this channel, is what
/// it resumes from.
const CHANNEL_CAPACITY: usize = 256;

/// Per-task in-process state: a broadcast channel for live readers and any
/// synchronous callback subscribers.
struct TaskChannel {
    sender: broadcast::Sender<SeqEvent>,
    status: StatusSubscribers,
    artifacts: ArtifactSubscribers,
}

impl TaskChannel {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            sender,
            status: Vec::new(),
            artifacts: Vec::new(),
        }
    }
}

/// Fan-out of task updates to live readers and callback subscribers, over an
/// [`AsyncEventLog`] that assigns the ids and keeps the events for replay.
///
/// Cloning shares the underlying per-task state (an `Arc<Mutex<…>>`) and the
/// log, so a clone observes the same channels and subscribers.
#[derive(Clone)]
pub struct StreamingFanout<L> {
    /// One lock per task rather than one over the map. The map lock is held
    /// only long enough to hand out a task's channel; everything that awaits —
    /// the log write, a callback subscriber — happens under that task's own
    /// lock, so two tasks streaming at once do not queue behind each other.
    /// Within a task the lock still orders the log write against the send, which
    /// is what keeps ids and delivery in the same order.
    tasks: Arc<Mutex<HashMap<String, Arc<Mutex<TaskChannel>>>>>,
    log: L,
}

/// The zero-configuration streaming handler: fan-out over an in-process event
/// log. Resumption works within one run of a server and no further — see
/// [`InMemoryEventLog`].
pub type InMemoryStreamingHandler = StreamingFanout<InMemoryEventLog>;

impl<L> StreamingFanout<L> {
    /// Build a fan-out over `log`.
    ///
    /// The log is what a client resumes from, so pass a durable one where
    /// resumption has to survive a restart.
    pub fn over(log: L) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            log,
        }
    }

    /// This task's channel, created if it is the first anyone has asked.
    async fn channel(&self, task_id: &str) -> Arc<Mutex<TaskChannel>> {
        self.tasks
            .lock()
            .await
            .entry(task_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(TaskChannel::new())))
            .clone()
    }
}

impl StreamingFanout<InMemoryEventLog> {
    /// Create a handler over a fresh in-memory log.
    pub fn new() -> Self {
        Self::over(InMemoryEventLog::new())
    }
}

impl Default for StreamingFanout<InMemoryEventLog> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: AsyncEventLog> StreamingFanout<L> {
    /// Log `event`, then publish it to live readers.
    ///
    /// Called with the task's channel locked, which is what keeps the id order
    /// and the delivery order the same: two concurrent broadcasts cannot take
    /// their ids in one order and reach the channel in the other.
    async fn publish(
        &self,
        channel: &TaskChannel,
        task_id: &str,
        event: UpdateEvent,
    ) -> Result<(), A2AError> {
        let seq = self.log.append(task_id, event).await?;
        // A send error just means there are no live readers; the log still has
        // the event for a later resume, so it is ignored.
        let _ = channel.sender.send(seq);
        Ok(())
    }
}

#[async_trait]
impl<L: AsyncEventLog + Clone + 'static> AsyncStreamingHandler for StreamingFanout<L> {
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

        self.channel(task_id)
            .await
            .lock()
            .await
            .status
            .push(subscriber);

        Ok(format!("status-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.channel(task_id)
            .await
            .lock()
            .await
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

    /// Drop the task's live readers and callback subscribers.
    ///
    /// The event log is left alone: a task with nobody listening is exactly the
    /// case a client is about to resume from. Deleting the events is
    /// [`AsyncEventLog::discard`], which an operator's retention sweep calls.
    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        let mut guard = self.tasks.lock().await;
        guard.remove(task_id);
        Ok(())
    }

    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        let Some(channel) = self.tasks.lock().await.get(task_id).cloned() else {
            return Ok(0);
        };
        let channel = channel.lock().await;
        Ok(channel.status.len() + channel.artifacts.len() + channel.sender.receiver_count())
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

        let channel = self.channel(task_id).await;
        let channel = channel.lock().await;
        self.publish(&channel, task_id, UpdateEvent::StatusUpdate(update.clone()))
            .await?;
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
        let channel = self.channel(task_id).await;
        let channel = channel.lock().await;
        self.publish(
            &channel,
            task_id,
            UpdateEvent::ArtifactUpdate(update.clone()),
        )
        .await?;
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
        // Subscribing and reading the log under the task's lock is what makes
        // the two halves meet exactly: a broadcast cannot land between them, so
        // the reader neither misses an event nor sees one twice.
        let channel = self.channel(task_id).await;
        let guard = channel.lock().await;
        let receiver = guard.sender.subscribe();
        let replay = match from_event_id {
            Some(from) => self.log.replay(task_id, from).await?,
            None => Default::default(),
        };
        drop(guard);

        // A replay that starts partway through the gap is a fragment of what the
        // client missed, not the remainder of it — and replaying it would re-apply
        // updates older than the task snapshot the service sends first, which for
        // an appending artifact means duplicated content. Stream live instead and
        // let the snapshot be the client's state.
        let replay = if replay.complete {
            replay.events
        } else {
            #[cfg(feature = "tracing")]
            tracing::warn!(
                task_id = %task_id,
                from_event_id = ?from_event_id,
                dropped = replay.events.len(),
                "event log no longer covers the requested resume point; streaming live from the task snapshot"
            );
            Vec::new()
        };

        let live = futures::stream::unfold(receiver, |mut rx| async move {
            match rx.recv().await {
                Ok(event) => Some((Ok(event), rx)),
                // Reader fell behind the broadcast channel: surface an error so a
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

    /// Subscribing with `from_event_id` replays the logged tail with a greater
    /// id before any live updates.
    #[tokio::test]
    async fn resume_replays_buffered_tail() {
        let handler = InMemoryStreamingHandler::new();
        // Emit two events with no live reader; they are retained in the log.
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

    /// Past the log's capacity the tail is a fragment of the gap rather than its
    /// remainder, so it is dropped and the client resumes from the snapshot the
    /// service sends ahead of the stream.
    #[tokio::test]
    async fn an_uncoverable_resume_streams_live_instead_of_a_partial_tail() {
        let handler = StreamingFanout::over(InMemoryEventLog::with_capacity(2));
        for _ in 0..5 {
            handler
                .broadcast_status_update("t1", status_event("t1", TaskState::Working))
                .await
                .unwrap();
        }

        let mut stream = handler.combined_update_stream("t1", Some(1)).await.unwrap();
        handler
            .broadcast_status_update("t1", status_event("t1", TaskState::Completed))
            .await
            .unwrap();

        let next = stream.next().await.unwrap().unwrap();
        assert_eq!(
            next.id, 6,
            "events 4 and 5 are a fragment of the gap, so the stream starts live at 6"
        );
    }

    /// Dropping a task's subscribers leaves its events replayable — a task with
    /// nobody listening is what a resume attaches to.
    #[tokio::test]
    async fn removing_subscribers_keeps_the_log() {
        let handler = InMemoryStreamingHandler::new();
        handler
            .broadcast_status_update("t1", status_event("t1", TaskState::Working))
            .await
            .unwrap();
        handler.remove_task_subscribers("t1").await.unwrap();

        let mut stream = handler.combined_update_stream("t1", Some(0)).await.unwrap();
        let replayed = stream.next().await.unwrap().unwrap();
        assert_eq!(replayed.id, 1);
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
