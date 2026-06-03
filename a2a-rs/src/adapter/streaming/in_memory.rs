//! In-memory streaming fan-out adapter.
//!
//! `InMemoryStreamingHandler` is the [`AsyncStreamingHandler`] adapter. It owns
//! **only** the per-task subscriber registry and fans broadcast events out to
//! those subscribers. It deliberately does *not*:
//!
//! - touch the task store (so it cannot replay current task state on subscribe —
//!   the initial `Task` snapshot is delivered by the application service before
//!   stream items, which is spec-compliant), nor
//! - fire push-webhook notifications (that is the [`AsyncPushNotifier`] port's
//!   job, orchestrated by the
//!   [`TaskStatusBroadcast`](crate::application::TaskStatusBroadcast) mixin).
//!
//! [`AsyncPushNotifier`]: crate::port::AsyncPushNotifier

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use tokio::sync::Mutex;

use crate::domain::{A2AError, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use crate::port::streaming_handler::{Subscriber, UpdateEvent};
use crate::port::AsyncStreamingHandler;

type StatusSubscribers = Vec<Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>>;
type ArtifactSubscribers = Vec<Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>>;

/// Per-task status and artifact subscribers.
struct TaskSubscribers {
    status: StatusSubscribers,
    artifacts: ArtifactSubscribers,
}

impl TaskSubscribers {
    fn new() -> Self {
        Self {
            status: Vec::new(),
            artifacts: Vec::new(),
        }
    }
}

/// In-memory [`AsyncStreamingHandler`]: a per-task subscriber registry with
/// best-effort fan-out.
///
/// Cloning shares the underlying subscriber map (an `Arc<Mutex<…>>`), so a clone
/// observes the same subscriptions.
#[derive(Clone, Default)]
pub struct InMemoryStreamingHandler {
    subscribers: Arc<Mutex<HashMap<String, TaskSubscribers>>>,
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

        let mut subscribers_guard = self.subscribers.lock().await;
        let task_subscribers = subscribers_guard
            .entry(task_id.to_string())
            .or_insert_with(TaskSubscribers::new);
        task_subscribers.status.push(subscriber);

        Ok(format!("status-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        let mut subscribers_guard = self.subscribers.lock().await;
        let task_subscribers = subscribers_guard
            .entry(task_id.to_string())
            .or_insert_with(TaskSubscribers::new);
        task_subscribers.artifacts.push(subscriber);

        Ok(format!("artifact-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn remove_subscription(&self, _subscription_id: &str) -> Result<(), A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Subscription removal by ID is not supported by the in-memory streaming handler"
                .to_string(),
        ))
    }

    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        let mut subscribers_guard = self.subscribers.lock().await;
        subscribers_guard.remove(task_id);
        Ok(())
    }

    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        let subscribers_guard = self.subscribers.lock().await;
        Ok(subscribers_guard
            .get(task_id)
            .map(|s| s.status.len() + s.artifacts.len())
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

        let subscribers_guard = self.subscribers.lock().await;
        if let Some(task_subscribers) = subscribers_guard.get(task_id) {
            for (i, subscriber) in task_subscribers.status.iter().enumerate() {
                if let Err(e) = subscriber.on_update(update.clone()).await {
                    #[cfg(feature = "tracing")]
                    tracing::error!(
                        task_id = %task_id,
                        subscriber_index = i,
                        error = %e,
                        "❌ Failed to notify subscriber"
                    );
                    let _ = i;
                    eprintln!("Failed to notify subscriber: {}", e);
                }
            }
        }
        Ok(())
    }

    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        let subscribers_guard = self.subscribers.lock().await;
        if let Some(task_subscribers) = subscribers_guard.get(task_id) {
            for subscriber in task_subscribers.artifacts.iter() {
                if let Err(e) = subscriber.on_update(update.clone()).await {
                    eprintln!("Failed to notify subscriber: {}", e);
                }
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
            "Status update stream is not supported by the in-memory streaming handler".to_string(),
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
            "Artifact update stream is not supported by the in-memory streaming handler".to_string(),
        ))
    }

    async fn combined_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<UpdateEvent, A2AError>> + Send>>, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Combined update stream is not supported by the in-memory streaming handler".to_string(),
        ))
    }
}
