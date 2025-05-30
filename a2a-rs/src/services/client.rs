//! Client interface traits

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::{
    application::{json_rpc::A2ARequest, JSONRPCResponse},
    domain::{A2AError, Message, Task, TaskArtifactUpdateEvent, TaskPushNotificationConfig, TaskStatusUpdateEvent},
};

#[async_trait]
/// An async trait defining the methods an async client should implement
pub trait AsyncA2AClient: Send + Sync {
    /// Send a raw request to the server and get a response
    async fn send_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError>;

    /// Send a structured request to the server and get a response
    async fn send_request<'a>(&self, request: &'a A2ARequest) -> Result<JSONRPCResponse, A2AError>;

    /// Send a message to a task
    async fn send_task_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError>;

    /// Get a task by ID
    async fn get_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError>;

    /// Cancel a task
    async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError>;

    /// Set up push notifications for a task
    async fn set_task_push_notification<'a>(
        &self,
        config: &'a TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Get push notification configuration for a task
    async fn get_task_push_notification<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Subscribe to task updates (for streaming)
    async fn subscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamItem, A2AError>> + Send>>, A2AError>;
}

/// Items that can be streamed from the server
#[derive(Debug, Clone)]
pub enum StreamItem {
    /// The initial task state
    Task(Task),
    /// A task status update
    StatusUpdate(TaskStatusUpdateEvent),
    /// A task artifact update
    ArtifactUpdate(TaskArtifactUpdateEvent),
}