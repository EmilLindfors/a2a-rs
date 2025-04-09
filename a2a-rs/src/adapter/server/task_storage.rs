//! In-memory task storage implementation

#![cfg(feature = "server")]

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::Mutex; // Changed from std::sync::Mutex
use tokio::sync::mpsc;

use crate::domain::{
    A2AError, Artifact, Message, Task, TaskArtifactUpdateEvent, TaskIdParams,
    TaskPushNotificationConfig, TaskQueryParams, TaskSendParams, TaskState, TaskStatus,
    TaskStatusUpdateEvent,
};
use crate::port::server::{AsyncTaskHandler, Subscriber};

type StatusSubscribers = Vec<Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>>;
type ArtifactSubscribers = Vec<Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>>;

/// Structure to hold subscribers for a task
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

/// Simple in-memory task storage for testing and example purposes
pub struct InMemoryTaskStorage {
    /// Tasks stored by ID
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    /// Subscribers for task updates
    subscribers: Arc<Mutex<HashMap<String, TaskSubscribers>>>,
    /// Push notification configurations by task ID
    push_notifications: Arc<Mutex<HashMap<String, TaskPushNotificationConfig>>>,
}

impl InMemoryTaskStorage {
    /// Create a new empty task storage
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            push_notifications: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send a status update to all subscribers for a task
    async fn broadcast_status_update(
        &self,
        task_id: &str,
        status: TaskStatus,
        final_: bool,
    ) -> Result<(), A2AError> {
        // Create the update event
        let event = TaskStatusUpdateEvent {
            id: task_id.to_string(),
            status,
            final_,
            metadata: None,
        };

        // Get all subscribers for this task
        let subscribers_to_notify = {
            let subscribers_guard = self.subscribers.lock().await;
            
            if let Some(task_subscribers) = subscribers_guard.get(task_id) {
                // Clone the subscribers so we don't hold the lock during notification
                for subscriber in task_subscribers.status.iter() {
                    if let Err(e) = subscriber.on_update(event.clone()).await {
                        eprintln!("Failed to notify subscriber: {}", e);
                    }
                }
            } else {
                return Ok(());
            }
        }; // Lock is dropped here

      

        Ok(())
    }

    /// Send an artifact update to all subscribers for a task
    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        artifact: Artifact,
    ) -> Result<(), A2AError> {
        // Create the update event
        let event = TaskArtifactUpdateEvent {
            id: task_id.to_string(),
            artifact,
            metadata: None,
        };

        // Get all subscribers for this task
        let subscribers_to_notify = {
            let subscribers_guard = self.subscribers.lock().await;
            
            if let Some(task_subscribers) = subscribers_guard.get(task_id) {
                // Clone the subscribers so we don't hold the lock during notification
                for subscriber in task_subscribers.artifacts.iter() {
                    if let Err(e) = subscriber.on_update(event.clone()).await {
                        eprintln!("Failed to notify subscriber: {}", e);
                    }
                }
            } else {
                return Ok(());
            }
        }; // Lock is dropped here


        Ok(())
    }
}

#[async_trait]
impl AsyncTaskHandler for InMemoryTaskStorage {
    async fn handle_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Get or create the task
        let mut task = {
            let mut tasks_guard = self.tasks.lock().await;

            if let Some(existing_task) = tasks_guard.get(task_id) {
                existing_task.clone()
            } else {
                // Create a new task
                let mut new_task = Task::new(task_id.to_string());

                // Add session ID if provided
                if let Some(sid) = session_id {
                    new_task.session_id = Some(sid.to_string());
                }

                // Insert it
                tasks_guard.insert(task_id.to_string(), new_task.clone());
                new_task
            }
        }; // Lock is dropped here

        // Update the task with the message
        task.update_status(TaskState::Working, Some(message.clone()));

        // Update the task in storage
        {
            let mut tasks_guard = self.tasks.lock().await;
            tasks_guard.insert(task_id.to_string(), task.clone());
        } // Lock is dropped here

        // Broadcast status update
        self.broadcast_status_update(task_id, task.status.clone(), false)
            .await?;

        Ok(task)
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
                task.clone()
            } else {
                return Err(A2AError::TaskNotFound(task_id.to_string()));
            }
        }; // Lock is dropped here

        // TODO: Handle history_length if needed in a real implementation

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

                // Update the status
                updated_task.update_status(TaskState::Canceled, None);
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

    async fn set_push_notification<'a>(
        &self,
        config: &'a TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Store the push notification config
        {
            let mut push_guard = self.push_notifications.lock().await;
            push_guard.insert(config.id.clone(), config.clone());
        } // Lock is dropped here

        Ok(config.clone())
    }

    async fn get_push_notification<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Get the push notification config
        let config = {
            let push_guard = self.push_notifications.lock().await;

            if let Some(config) = push_guard.get(task_id) {
                config.clone()
            } else {
                return Err(A2AError::PushNotificationNotSupported);
            }
        }; // Lock is dropped here

        Ok(config)
    }

    async fn add_status_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        // Add the subscriber
        {
            let mut subscribers_guard = self.subscribers.lock().await;

            let task_subscribers = subscribers_guard
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);

            task_subscribers.status.push(subscriber);
        } // Lock is dropped here

        // Get the current status to send as an initial update
        let task = self.get_task(task_id, None).await?;
        self.broadcast_status_update(task_id, task.status, false)
            .await?;

        Ok(())
    }

    async fn add_artifact_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
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
                self.broadcast_artifact_update(task_id, artifact).await?;
            }
        }

        Ok(())
    }

    async fn remove_subscribers<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        // Remove all subscribers
        {
            let mut subscribers_guard = self.subscribers.lock().await;
            subscribers_guard.remove(task_id);
        } // Lock is dropped here

        Ok(())
    }
}

impl Clone for InMemoryTaskStorage {
    fn clone(&self) -> Self {
        Self {
            tasks: self.tasks.clone(),
            subscribers: self.subscribers.clone(),
            push_notifications: self.push_notifications.clone(),
        }
    }
}