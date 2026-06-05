//! Streaming and real-time update handling port definitions

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::domain::core::task::TaskStateExt;
use crate::domain::{A2AError, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};

/// A trait for subscribing to real-time updates
#[async_trait]
pub trait Subscriber<T>: Send + Sync {
    /// Handle an update
    async fn on_update(&self, update: T) -> Result<(), A2AError>;

    /// Handle subscription errors
    async fn on_error(&self, error: A2AError) -> Result<(), A2AError> {
        // Default implementation - log error but don't propagate
        eprintln!("Subscription error: {}", error);
        Ok(())
    }

    /// Handle subscription completion
    async fn on_complete(&self) -> Result<(), A2AError> {
        // Default implementation - no-op
        Ok(())
    }
}

#[async_trait]
/// An async trait for managing streaming connections and real-time updates
pub trait AsyncStreamingHandler: Send + Sync {
    /// Add a status update subscriber for a task
    async fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError>; // Returns subscription ID

    /// Add an artifact update subscriber for a task
    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError>; // Returns subscription ID

    /// Remove a specific subscription
    async fn remove_subscription(&self, subscription_id: &str) -> Result<(), A2AError>;

    /// Remove all subscribers for a task
    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError>;

    /// Get the number of active subscribers for a task
    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError>;

    /// Check if a task has any active subscribers
    async fn has_subscribers(&self, task_id: &str) -> Result<bool, A2AError> {
        let count = self.get_subscriber_count(task_id).await?;
        Ok(count > 0)
    }

    /// Broadcast a status update to all subscribers of a task
    async fn broadcast_status_update(
        &self,
        task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError>;

    /// Broadcast an artifact update to all subscribers of a task
    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError>;

    /// Create a stream of status updates for a task
    async fn status_update_stream(
        &self,
        task_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>, A2AError>;

    /// Create a stream of artifact updates for a task
    async fn artifact_update_stream(
        &self,
        task_id: &str,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    >;

    /// Create a combined stream of all updates for a task.
    ///
    /// Each yielded [`SeqEvent`] carries a per-task monotonic id so a client can
    /// resume after a disconnect. When `from_event_id` is `Some(n)`, the handler
    /// first replays any buffered events with id `> n` (best-effort, bounded by
    /// the handler's replay buffer) before streaming live updates.
    async fn combined_update_stream(
        &self,
        task_id: &str,
        from_event_id: Option<u64>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>, A2AError>;

    /// Validate streaming parameters
    async fn validate_streaming_params(&self, task_id: &str) -> Result<(), A2AError> {
        if task_id.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".to_string(),
                message: "Task ID cannot be empty for streaming".to_string(),
            });
        }
        Ok(())
    }

    /// Start streaming for a task with automatic cleanup.
    ///
    /// `from_event_id` is forwarded to [`combined_update_stream`] for
    /// Last-Event-ID resumption.
    ///
    /// [`combined_update_stream`]: AsyncStreamingHandler::combined_update_stream
    async fn start_task_streaming(
        &self,
        task_id: &str,
        from_event_id: Option<u64>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>, A2AError> {
        self.validate_streaming_params(task_id).await?;
        self.combined_update_stream(task_id, from_event_id).await
    }

    /// Stop all streaming for a task
    async fn stop_task_streaming(&self, task_id: &str) -> Result<(), A2AError> {
        self.remove_task_subscribers(task_id).await
    }
}

/// Forwarding impl so a type-erased `Arc<dyn AsyncStreamingHandler>` can itself
/// be passed wherever an `impl AsyncStreamingHandler` is expected (e.g.
/// `TaskService::with_streaming_handler`). This lets a single shared streaming
/// backend be injected into both a message handler and a transport adapter
/// without naming its concrete type. Only the required methods are forwarded;
/// the trait's default methods ride along on top of them.
#[async_trait]
impl AsyncStreamingHandler for std::sync::Arc<dyn AsyncStreamingHandler> {
    async fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        (**self).add_status_subscriber(task_id, subscriber).await
    }

    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        (**self).add_artifact_subscriber(task_id, subscriber).await
    }

    async fn remove_subscription(&self, subscription_id: &str) -> Result<(), A2AError> {
        (**self).remove_subscription(subscription_id).await
    }

    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        (**self).remove_task_subscribers(task_id).await
    }

    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        (**self).get_subscriber_count(task_id).await
    }

    async fn broadcast_status_update(
        &self,
        task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        (**self).broadcast_status_update(task_id, update).await
    }

    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        (**self).broadcast_artifact_update(task_id, update).await
    }

    async fn status_update_stream(
        &self,
        task_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>, A2AError>
    {
        (**self).status_update_stream(task_id).await
    }

    async fn artifact_update_stream(
        &self,
        task_id: &str,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        (**self).artifact_update_stream(task_id).await
    }

    async fn combined_update_stream(
        &self,
        task_id: &str,
        from_event_id: Option<u64>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>, A2AError> {
        (**self)
            .combined_update_stream(task_id, from_event_id)
            .await
    }
}

/// A streamed [`UpdateEvent`] tagged with a per-task monotonic id.
///
/// The id is assigned by the streaming handler when the event is broadcast and
/// is surfaced to clients as the SSE `id:` field. On reconnect a client echoes
/// the last id it saw via `Last-Event-ID`, and the handler replays buffered
/// events with a greater id (see
/// [`combined_update_stream`](AsyncStreamingHandler::combined_update_stream)).
///
/// This id/`Last-Event-ID` resumption is an a2a-rs enhancement on top of the
/// W3C SSE standard, **not** part of the A2A v1.0 spec. Emitting the `id:` field
/// is inert for spec clients (they read only the event payload), so it does not
/// affect interop.
#[derive(Debug, Clone)]
pub struct SeqEvent {
    /// Per-task monotonic event id (starts at 1; `0` is reserved for the
    /// initial task snapshot, which carries no replayable id).
    pub id: u64,
    /// The update payload.
    pub event: UpdateEvent,
}

impl SeqEvent {
    /// Construct a sequenced event.
    #[inline]
    pub fn new(id: u64, event: UpdateEvent) -> Self {
        Self { id, event }
    }
}

/// Union type for different kinds of updates that can be streamed
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    StatusUpdate(TaskStatusUpdateEvent),
    ArtifactUpdate(TaskArtifactUpdateEvent),
}

impl UpdateEvent {
    /// Get the task ID from the update event
    #[inline]
    pub fn task_id(&self) -> &str {
        match self {
            UpdateEvent::StatusUpdate(event) => &event.task_id,
            UpdateEvent::ArtifactUpdate(event) => &event.task_id,
        }
    }

    /// Get the context ID from the update event
    #[inline]
    pub fn context_id(&self) -> &str {
        match self {
            UpdateEvent::StatusUpdate(event) => &event.context_id,
            UpdateEvent::ArtifactUpdate(event) => &event.context_id,
        }
    }

    /// Check if this is a final update
    #[inline]
    pub fn is_final(&self) -> bool {
        match self {
            UpdateEvent::StatusUpdate(event) => event.status.state.is_terminal(),
            UpdateEvent::ArtifactUpdate(event) => event.last_chunk.unwrap_or(false),
        }
    }
}
