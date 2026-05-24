use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::domain::{
    A2AError, ListTasksParams, ListTasksResult, Message, Task, TaskArtifactUpdateEvent,
    TaskPushNotificationConfig, TaskStatusUpdateEvent,
};

#[async_trait]
/// An async trait defining the methods an async client should implement
pub trait AsyncA2AClient: Send + Sync {

    /// Send a message to a task
    async fn send_task_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError>;

    /// Get a task by ID
    async fn get_task(&self, task_id: &str, history_length: Option<u32>) -> Result<Task, A2AError>;

    /// Cancel a task
    async fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError>;

    /// Set up push notifications for a task
    async fn set_task_push_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Get push notification configuration for a task
    async fn get_task_push_notification(
        &self,
        task_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// List tasks with filtering and pagination (v1.0.0)
    async fn list_tasks(&self, params: &ListTasksParams) -> Result<ListTasksResult, A2AError>;

    /// List all push notification configs for a task (v1.0.0)
    async fn list_push_notification_configs(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError>;

    /// Get a specific push notification config by ID (v1.0.0)
    async fn get_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Delete a specific push notification config (v1.0.0)
    async fn delete_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<(), A2AError>;

    /// Subscribe to task updates (for streaming)
    async fn subscribe_to_task(
        &self,
        task_id: &str,
        history_length: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamItem, A2AError>> + Send>>, A2AError>;
}

/// Items that can be streamed from the server during task subscriptions.\n///\n/// When subscribing to streaming updates for a task, the server can send\n/// different types of items:\n/// - `Task`: The complete initial task state when subscription starts\n/// - `StatusUpdate`: Updates to the task's status (state changes, progress)\n/// - `ArtifactUpdate`: Notifications about new or updated artifacts\n///\n/// This allows clients to receive real-time updates about task progress\n/// and results as they become available.
#[derive(Debug, Clone)]
pub enum StreamItem {
    /// The initial task state
    Task(Task),
    /// A task status update
    StatusUpdate(TaskStatusUpdateEvent),
    /// A task artifact update
    ArtifactUpdate(TaskArtifactUpdateEvent),
}
