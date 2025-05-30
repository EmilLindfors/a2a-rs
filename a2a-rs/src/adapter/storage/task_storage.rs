//! In-memory task storage implementation

// This module is already conditionally compiled with #[cfg(feature = "server")] in mod.rs

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex; // Changed from std::sync::Mutex

use crate::adapter::business::push_notification::{
    PushNotificationRegistry, PushNotificationSender,
};

#[cfg(feature = "http-client")]
use crate::adapter::business::push_notification::HttpPushNotificationSender;
#[cfg(not(feature = "http-client"))]
use crate::adapter::business::push_notification::NoopPushNotificationSender;
use crate::domain::{
    A2AError, Artifact, Message, Task, TaskArtifactUpdateEvent, TaskPushNotificationConfig,
    TaskState, TaskStatus, TaskStatusUpdateEvent,
};
use crate::port::{
    streaming_handler::Subscriber, AsyncNotificationManager, AsyncStreamingHandler,
    AsyncTaskManager,
};

type StatusSubscribers = Vec<Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>>;
type ArtifactSubscribers = Vec<Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>>;

/// Structure to hold subscribers for a task
pub(crate) struct TaskSubscribers {
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

/// Simple in-memory task storage for testing and example purposes
pub struct InMemoryTaskStorage {
    /// Tasks stored by ID
    pub(crate) tasks: Arc<Mutex<HashMap<String, Task>>>,
    /// Subscribers for task updates
    pub(crate) subscribers: Arc<Mutex<HashMap<String, TaskSubscribers>>>,
    /// Push notification registry
    pub(crate) push_notification_registry: Arc<PushNotificationRegistry>,
}

impl InMemoryTaskStorage {
    /// Create a new empty task storage
    pub fn new() -> Self {
        // Use the appropriate push notification sender based on available features
        #[cfg(feature = "http-client")]
        let push_sender = HttpPushNotificationSender::new();
        #[cfg(not(feature = "http-client"))]
        let push_sender = NoopPushNotificationSender::default();

        let push_registry = PushNotificationRegistry::new(push_sender);

        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            push_notification_registry: Arc::new(push_registry),
        }
    }

    /// Create a new task storage with a custom push notification sender
    pub fn with_push_sender(push_sender: impl PushNotificationSender + 'static) -> Self {
        let push_registry = PushNotificationRegistry::new(push_sender);

        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            push_notification_registry: Arc::new(push_registry),
        }
    }

    /// Add a status update subscriber for streaming (convenience method)
    pub async fn add_status_subscriber_legacy(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        self.add_status_subscriber(task_id, subscriber)
            .await
            .map(|_| ())
    }

    /// Add an artifact update subscriber for streaming (convenience method)
    pub async fn add_artifact_subscriber_legacy(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        self.add_artifact_subscriber(task_id, subscriber)
            .await
            .map(|_| ())
    }
}

impl Default for InMemoryTaskStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryTaskStorage {
    /// Send a status update to all subscribers for a task
    pub(crate) async fn broadcast_status_update(
        &self,
        task_id: &str,
        status: TaskStatus,
        final_: bool,
    ) -> Result<(), A2AError> {
        // Create the update event
        let event = TaskStatusUpdateEvent {
            task_id: task_id.to_string(),
            context_id: "default".to_string(), // TODO: get actual context_id
            kind: "status-update".to_string(),
            status,
            final_,
            metadata: None,
        };

        // Get all subscribers for this task and notify them
        {
            let subscribers_guard = self.subscribers.lock().await;

            if let Some(task_subscribers) = subscribers_guard.get(task_id) {
                // Clone the subscribers so we don't hold the lock during notification
                for subscriber in task_subscribers.status.iter() {
                    if let Err(e) = subscriber.on_update(event.clone()).await {
                        eprintln!("Failed to notify subscriber: {}", e);
                    }
                }
            }
        }; // Lock is dropped here

        // Send push notification if configured
        if let Err(e) = self
            .push_notification_registry
            .send_status_update(task_id, &event)
            .await
        {
            eprintln!("Failed to send push notification: {}", e);
        }

        Ok(())
    }

    /// Send an artifact update to all subscribers for a task
    pub(crate) async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        artifact: Artifact,
        _index: Option<u32>,
        _final: bool,
    ) -> Result<(), A2AError> {
        // Create the update event
        let event = TaskArtifactUpdateEvent {
            task_id: task_id.to_string(),
            context_id: "default".to_string(), // TODO: get actual context_id
            kind: "artifact-update".to_string(),
            artifact,
            append: None,
            last_chunk: None,
            metadata: None,
        };

        // Get all subscribers for this task
        {
            let subscribers_guard = self.subscribers.lock().await;

            if let Some(task_subscribers) = subscribers_guard.get(task_id) {
                // Clone the subscribers so we don't hold the lock during notification
                for subscriber in task_subscribers.artifacts.iter() {
                    if let Err(e) = subscriber.on_update(event.clone()).await {
                        eprintln!("Failed to notify subscriber: {}", e);
                    }
                }
            }
        }; // Lock is dropped here

        // Send push notification if configured
        if let Err(e) = self
            .push_notification_registry
            .send_artifact_update(task_id, &event)
            .await
        {
            eprintln!("Failed to send push notification: {}", e);
        }

        Ok(())
    }
}

#[async_trait]
impl AsyncTaskManager for InMemoryTaskStorage {
    async fn create_task<'a>(
        &self,
        task_id: &'a str,
        context_id: &'a str,
    ) -> Result<Task, A2AError> {
        let mut tasks_guard = self.tasks.lock().await;

        if tasks_guard.contains_key(task_id) {
            return Err(A2AError::TaskNotFound(format!(
                "Task {} already exists",
                task_id
            )));
        }

        let task = Task::new(task_id.to_string(), context_id.to_string());
        tasks_guard.insert(task_id.to_string(), task.clone());

        Ok(task)
    }

    async fn update_task_status<'a>(
        &self,
        task_id: &'a str,
        state: TaskState,
    ) -> Result<Task, A2AError> {
        let mut tasks_guard = self.tasks.lock().await;

        let task = tasks_guard
            .get_mut(task_id)
            .ok_or_else(|| A2AError::TaskNotFound(task_id.to_string()))?;

        // Update the task status
        task.update_status(state, None);

        // Return a clone of the updated task
        let updated_task = task.clone();

        // Release the lock before broadcasting
        drop(tasks_guard);

        // Broadcast status update
        self.broadcast_status_update(task_id, updated_task.status.clone(), false)
            .await?;

        Ok(updated_task)
    }

    async fn task_exists<'a>(&self, task_id: &'a str) -> Result<bool, A2AError> {
        let tasks_guard = self.tasks.lock().await;
        Ok(tasks_guard.contains_key(task_id))
    }

    async fn get_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        // Get the task
        let task = {
            let tasks_guard = self.tasks.lock().await;

            if let Some(task) = tasks_guard.get(task_id) {
                // Apply history length limitation if specified
                task.with_limited_history(history_length)
            } else {
                return Err(A2AError::TaskNotFound(task_id.to_string()));
            }
        }; // Lock is dropped here

        Ok(task)
    }

    async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError> {
        // Get and update the task
        let task = {
            let mut tasks_guard = self.tasks.lock().await;

            if let Some(task) = tasks_guard.get(task_id) {
                let mut updated_task = task.clone();

                // Only working tasks can be canceled
                if updated_task.status.state != TaskState::Working {
                    return Err(A2AError::TaskNotCancelable(format!(
                        "Task {} is in state {:?} and cannot be canceled",
                        task_id, updated_task.status.state
                    )));
                }

                // Create a cancellation message to add to history
                let cancel_message = Message {
                    role: crate::domain::Role::Agent,
                    parts: vec![crate::domain::Part::Text {
                        text: format!("Task {} canceled.", task_id),
                        metadata: None,
                    }],
                    metadata: None,
                    reference_task_ids: None,
                    message_id: uuid::Uuid::new_v4().to_string(),
                    task_id: Some(task_id.to_string()),
                    context_id: Some(updated_task.context_id.clone()),
                    kind: "message".to_string(),
                };

                // Update the status with the cancellation message to track in history
                updated_task.update_status(TaskState::Canceled, Some(cancel_message));
                tasks_guard.insert(task_id.to_string(), updated_task.clone());
                updated_task
            } else {
                return Err(A2AError::TaskNotFound(task_id.to_string()));
            }
        }; // Lock is dropped here

        // Broadcast status update (with final flag set to true)
        self.broadcast_status_update(task_id, task.status.clone(), true)
            .await?;

        Ok(task)
    }
}

// AsyncNotificationManager implementation
#[async_trait]
impl AsyncNotificationManager for InMemoryTaskStorage {
    async fn set_task_notification<'a>(
        &self,
        config: &'a TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Register with the push notification registry
        self.push_notification_registry
            .register(&config.task_id, config.push_notification_config.clone())
            .await?;

        Ok(config.clone())
    }

    async fn get_task_notification<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Get the push notification config from the registry
        match self.push_notification_registry.get_config(task_id).await? {
            Some(config) => Ok(TaskPushNotificationConfig {
                task_id: task_id.to_string(),
                push_notification_config: config,
            }),
            None => Err(A2AError::PushNotificationNotSupported),
        }
    }

    async fn remove_task_notification<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        self.push_notification_registry.unregister(task_id).await?;
        Ok(())
    }
}

// AsyncStreamingHandler implementation
#[async_trait]
impl AsyncStreamingHandler for InMemoryTaskStorage {
    async fn add_status_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        // Add the subscriber
        {
            let mut subscribers_guard = self.subscribers.lock().await;

            let task_subscribers = subscribers_guard
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);

            task_subscribers.status.push(subscriber);
        } // Lock is dropped here

        // Get the current status (with full history) to send as an initial update
        let task = self.get_task(task_id, None).await?;
        self.broadcast_status_update(task_id, task.status, false)
            .await?;

        Ok(format!("status-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn add_artifact_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        // Add the subscriber
        {
            let mut subscribers_guard = self.subscribers.lock().await;

            let task_subscribers = subscribers_guard
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);

            task_subscribers.artifacts.push(subscriber);
        } // Lock is dropped here

        // If there are existing artifacts, broadcast them
        let task = self.get_task(task_id, None).await?;
        if let Some(artifacts) = task.artifacts {
            for artifact in artifacts {
                self.broadcast_artifact_update(task_id, artifact, None, false)
                    .await?;
            }
        }

        Ok(format!("artifact-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn remove_subscription<'a>(&self, _subscription_id: &'a str) -> Result<(), A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Subscription removal by ID requires storage layer refactoring".to_string(),
        ))
    }

    async fn remove_task_subscribers<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        // Remove all subscribers
        {
            let mut subscribers_guard = self.subscribers.lock().await;
            subscribers_guard.remove(task_id);
        } // Lock is dropped here

        Ok(())
    }

    async fn get_subscriber_count<'a>(&self, task_id: &'a str) -> Result<usize, A2AError> {
        let subscribers_guard = self.subscribers.lock().await;

        if let Some(task_subscribers) = subscribers_guard.get(task_id) {
            Ok(task_subscribers.status.len() + task_subscribers.artifacts.len())
        } else {
            Ok(0)
        }
    }

    async fn broadcast_status_update<'a>(
        &self,
        task_id: &'a str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        self.broadcast_status_update(task_id, update.status, update.final_)
            .await
    }

    async fn broadcast_artifact_update<'a>(
        &self,
        task_id: &'a str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        self.broadcast_artifact_update(
            task_id,
            update.artifact,
            None,
            update.last_chunk.unwrap_or(false),
        )
        .await
    }

    async fn status_update_stream<'a>(
        &self,
        _task_id: &'a str,
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>,
        >,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Status update stream requires storage layer refactoring".to_string(),
        ))
    }

    async fn artifact_update_stream<'a>(
        &self,
        _task_id: &'a str,
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>,
        >,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Artifact update stream requires storage layer refactoring".to_string(),
        ))
    }

    async fn combined_update_stream<'a>(
        &self,
        _task_id: &'a str,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures::Stream<
                        Item = Result<crate::port::streaming_handler::UpdateEvent, A2AError>,
                    > + Send,
            >,
        >,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Combined update stream requires storage layer refactoring".to_string(),
        ))
    }
}

impl Clone for InMemoryTaskStorage {
    fn clone(&self) -> Self {
        Self {
            tasks: self.tasks.clone(),
            subscribers: self.subscribers.clone(),
            push_notification_registry: self.push_notification_registry.clone(),
        }
    }
}
