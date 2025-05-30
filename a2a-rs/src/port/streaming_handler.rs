//! Streaming and real-time update handling port definitions

#[cfg(feature = "server")]
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::domain::{A2AError, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};

/// A trait for subscribing to real-time updates
#[cfg(feature = "server")]
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

/// A trait for managing streaming connections and real-time updates
pub trait StreamingHandler {
    /// Add a status update subscriber for a task
    fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError>; // Returns subscription ID

    /// Add an artifact update subscriber for a task
    fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError>; // Returns subscription ID

    /// Remove a specific subscription
    fn remove_subscription(&self, subscription_id: &str) -> Result<(), A2AError>;

    /// Remove all subscribers for a task
    fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError>;

    /// Get the number of active subscribers for a task
    fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError>;

    /// Check if a task has any active subscribers
    fn has_subscribers(&self, task_id: &str) -> Result<bool, A2AError> {
        let count = self.get_subscriber_count(task_id)?;
        Ok(count > 0)
    }
}

#[cfg(feature = "server")]
#[async_trait]
/// An async trait for managing streaming connections and real-time updates
pub trait AsyncStreamingHandler: Send + Sync {
    /// Add a status update subscriber for a task
    async fn add_status_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError>; // Returns subscription ID

    /// Add an artifact update subscriber for a task
    async fn add_artifact_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError>; // Returns subscription ID

    /// Remove a specific subscription
    async fn remove_subscription<'a>(&self, subscription_id: &'a str) -> Result<(), A2AError>;

    /// Remove all subscribers for a task
    async fn remove_task_subscribers<'a>(&self, task_id: &'a str) -> Result<(), A2AError>;

    /// Get the number of active subscribers for a task
    async fn get_subscriber_count<'a>(&self, task_id: &'a str) -> Result<usize, A2AError>;

    /// Check if a task has any active subscribers
    async fn has_subscribers<'a>(&self, task_id: &'a str) -> Result<bool, A2AError> {
        let count = self.get_subscriber_count(task_id).await?;
        Ok(count > 0)
    }

    /// Broadcast a status update to all subscribers of a task
    async fn broadcast_status_update<'a>(
        &self,
        task_id: &'a str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError>;

    /// Broadcast an artifact update to all subscribers of a task
    async fn broadcast_artifact_update<'a>(
        &self,
        task_id: &'a str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError>;

    /// Create a stream of status updates for a task
    async fn status_update_stream<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>, A2AError>;

    /// Create a stream of artifact updates for a task
    async fn artifact_update_stream<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    >;

    /// Create a combined stream of all updates for a task
    async fn combined_update_stream<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<UpdateEvent, A2AError>> + Send>>, A2AError>;

    /// Validate streaming parameters
    async fn validate_streaming_params<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        if task_id.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".to_string(),
                message: "Task ID cannot be empty for streaming".to_string(),
            });
        }
        Ok(())
    }

    /// Start streaming for a task with automatic cleanup
    async fn start_task_streaming<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<UpdateEvent, A2AError>> + Send>>, A2AError> {
        self.validate_streaming_params(task_id).await?;
        self.combined_update_stream(task_id).await
    }

    /// Stop all streaming for a task
    async fn stop_task_streaming<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        self.remove_task_subscribers(task_id).await
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
    pub fn task_id(&self) -> &str {
        match self {
            UpdateEvent::StatusUpdate(event) => &event.task_id,
            UpdateEvent::ArtifactUpdate(event) => &event.task_id,
        }
    }

    /// Get the context ID from the update event
    pub fn context_id(&self) -> &str {
        match self {
            UpdateEvent::StatusUpdate(event) => &event.context_id,
            UpdateEvent::ArtifactUpdate(event) => &event.context_id,
        }
    }

    /// Check if this is a final update
    pub fn is_final(&self) -> bool {
        match self {
            UpdateEvent::StatusUpdate(event) => event.final_,
            UpdateEvent::ArtifactUpdate(event) => event.last_chunk.unwrap_or(false),
        }
    }
}
