use async_trait::async_trait;
// No StreamExt import needed
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::reimbursement_agent::agent::ReimbursementAgent;
use a2a_rs::domain::{
    A2AError, Artifact, Message, Part, Task, TaskArtifactUpdateEvent, TaskPushNotificationConfig,
    TaskState, TaskStatus, TaskStatusUpdateEvent,
};
use a2a_rs::port::server::{AsyncTaskHandler, Subscriber};

// Type aliases to reduce complexity
type StatusSubscriber = Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>;
type ArtifactSubscriber = Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>;
type StatusSubscriberMap = HashMap<String, Vec<StatusSubscriber>>;
type ArtifactSubscriberMap = HashMap<String, Vec<ArtifactSubscriber>>;

/// Task manager that bridges the A2A protocol with the ReimbursementAgent
#[derive(Clone)]
pub struct AgentTaskManager {
    agent: Arc<ReimbursementAgent>,
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    #[allow(clippy::type_complexity)]
    status_subscribers: Arc<Mutex<StatusSubscriberMap>>,
    #[allow(clippy::type_complexity)]
    artifact_subscribers: Arc<Mutex<ArtifactSubscriberMap>>,
}

impl AgentTaskManager {
    /// Create a new agent task manager
    pub fn new(agent: ReimbursementAgent) -> Self {
        Self {
            agent: Arc::new(agent),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            status_subscribers: Arc::new(Mutex::new(HashMap::new())),
            artifact_subscribers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Extract user query from message
    fn get_user_query(&self, message: &Message) -> Result<String, A2AError> {
        if message.parts.is_empty() {
            return Err(A2AError::InvalidParams("Message has no parts".to_string()));
        }

        match &message.parts[0] {
            Part::Text { text, .. } => Ok(text.clone()),
            _ => Err(A2AError::InvalidParams(
                "Only text parts are supported".to_string(),
            )),
        }
    }

    // Method removed to avoid unused code warning

    /// Update task in storage and notify subscribers
    async fn update_task(
        &self,
        task_id: &str,
        status: TaskStatus,
        artifacts: Option<Vec<Artifact>>,
    ) -> Result<Task, A2AError> {
        let mut tasks = self.tasks.lock().await;

        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| A2AError::TaskNotFound(task_id.to_string()))?;

        // Update task status
        task.status = status.clone();

        // Add artifacts if provided
        if let Some(new_artifacts) = artifacts.as_ref() {
            if task.artifacts.is_none() {
                task.artifacts = Some(Vec::new());
            }

            if let Some(task_artifacts) = &mut task.artifacts {
                task_artifacts.extend(new_artifacts.clone());
            }
        }

        let task_clone = task.clone();

        // Drop the lock before notifying subscribers
        drop(tasks);

        // Notify status subscribers
        let status_update = TaskStatusUpdateEvent {
            id: task_id.to_string(),
            status: status.clone(),
            final_: false,
            metadata: None,
        };

        self.notify_status_subscribers(task_id, &status_update)
            .await?;

        // Notify artifact subscribers if there are artifacts
        if let Some(new_artifacts) = artifacts.as_ref() {
            for artifact in new_artifacts.iter() {
                let artifact_update = TaskArtifactUpdateEvent {
                    id: task_id.to_string(),
                    artifact: artifact.clone(),
                    metadata: None,
                };

                self.notify_artifact_subscribers(task_id, &artifact_update)
                    .await?;
            }
        }

        Ok(task_clone)
    }

    /// Notify status subscribers about an update
    async fn notify_status_subscribers(
        &self,
        task_id: &str,
        update: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        let subscribers = self.status_subscribers.lock().await;

        if let Some(subs) = subscribers.get(task_id) {
            for subscriber in subs {
                if let Err(e) = subscriber.on_update(update.clone()).await {
                    tracing::warn!("Failed to notify status subscriber: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Notify artifact subscribers about an update
    async fn notify_artifact_subscribers(
        &self,
        task_id: &str,
        update: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        let subscribers = self.artifact_subscribers.lock().await;

        if let Some(subs) = subscribers.get(task_id) {
            for subscriber in subs {
                if let Err(e) = subscriber.on_update(update.clone()).await {
                    tracing::warn!("Failed to notify artifact subscriber: {}", e);
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl AsyncTaskHandler for AgentTaskManager {
    async fn handle_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Get the user query from the message
        let query = self.get_user_query(message)?;

        // Create or get the session ID
        let session_id = session_id.unwrap_or("default_session").to_string();

        // Invoke the agent
        let response = self.agent.invoke(&query, &session_id);

        // Determine if this is a completed response or requires input
        let task_state = if response.contains("form") {
            TaskState::InputRequired
        } else {
            TaskState::Completed
        };

        // Create response message and parts
        let parts = vec![Part::Text {
            text: response.clone(),
            metadata: None,
        }];

        let message = Message {
            role: a2a_rs::domain::Role::Agent,
            parts: parts.clone(),
            metadata: None,
        };

        // Create artifact
        let artifact = Artifact {
            name: None,
            description: None,
            parts: parts.clone(),
            index: 0,
            append: None,
            last_chunk: None,
            metadata: None,
        };

        // Create task status
        let status = TaskStatus {
            state: task_state,
            message: Some(message),
            timestamp: Some(chrono::Utc::now()),
        };

        // Update the task
        self.update_task(task_id, status, Some(vec![artifact]))
            .await
    }

    async fn get_task<'a>(
        &self,
        task_id: &'a str,
        _history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        let tasks = self.tasks.lock().await;

        tasks
            .get(task_id)
            .cloned()
            .ok_or_else(|| A2AError::TaskNotFound(task_id.to_string()))
    }

    async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError> {
        let mut tasks = self.tasks.lock().await;

        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| A2AError::TaskNotFound(task_id.to_string()))?;

        // Only working tasks can be canceled
        if task.status.state != TaskState::Working {
            return Err(A2AError::TaskNotCancelable(format!(
                "Task {} is in state {:?} and cannot be canceled",
                task_id, task.status.state
            )));
        }

        // Update the task status
        task.status = TaskStatus {
            state: TaskState::Canceled,
            message: None,
            timestamp: Some(chrono::Utc::now()),
        };

        let task_clone = task.clone();

        // Drop the lock before notifying subscribers
        drop(tasks);

        // Notify status subscribers with final flag set to true
        let status_update = TaskStatusUpdateEvent {
            id: task_id.to_string(),
            status: task_clone.status.clone(),
            final_: true,
            metadata: None,
        };

        self.notify_status_subscribers(task_id, &status_update)
            .await?;

        Ok(task_clone)
    }

    async fn set_push_notification<'a>(
        &self,
        _config: &'a TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // For simplicity, we don't implement push notifications in this example
        Err(A2AError::PushNotificationNotSupported)
    }

    async fn get_push_notification<'a>(
        &self,
        _task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // For simplicity, we don't implement push notifications in this example
        Err(A2AError::PushNotificationNotSupported)
    }

    async fn add_status_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        let mut subscribers = self.status_subscribers.lock().await;

        let task_subscribers = subscribers
            .entry(task_id.to_string())
            .or_insert_with(Vec::new);

        task_subscribers.push(subscriber);

        // Drop the lock before getting the task
        drop(subscribers);

        // Get the task to send an initial update
        let task = self.get_task(task_id, None).await?;

        // Notify the new subscriber with the current status
        let status_update = TaskStatusUpdateEvent {
            id: task_id.to_string(),
            status: task.status,
            final_: false,
            metadata: None,
        };

        self.notify_status_subscribers(task_id, &status_update)
            .await?;

        Ok(())
    }

    async fn add_artifact_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        let mut subscribers = self.artifact_subscribers.lock().await;

        let task_subscribers = subscribers
            .entry(task_id.to_string())
            .or_insert_with(Vec::new);

        task_subscribers.push(subscriber);

        // Drop the lock before getting the task
        drop(subscribers);

        // Get the task to send initial updates for existing artifacts
        let task = self.get_task(task_id, None).await?;

        // If there are existing artifacts, notify the subscriber
        if let Some(artifacts) = task.artifacts {
            for artifact in artifacts {
                let artifact_update = TaskArtifactUpdateEvent {
                    id: task_id.to_string(),
                    artifact: artifact.clone(),
                    metadata: None,
                };

                self.notify_artifact_subscribers(task_id, &artifact_update)
                    .await?;
            }
        }

        Ok(())
    }

    async fn remove_subscribers<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        {
            let mut status_subscribers = self.status_subscribers.lock().await;
            status_subscribers.remove(task_id);
        }

        {
            let mut artifact_subscribers = self.artifact_subscribers.lock().await;
            artifact_subscribers.remove(task_id);
        }

        Ok(())
    }
}
