//! Client port (interface) for the A2A protocol

#[cfg(feature = "client")]
use async_trait::async_trait;

use crate::{
    application::json_rpc::{A2ARequest, JSONRPCResponse},
    domain::{
        A2AError, Message, Task, TaskArtifactUpdateEvent, TaskPushNotificationConfig,
        TaskStatusUpdateEvent,
    },
};

/// A trait defining the methods a client should implement
pub trait A2AClient {
    /// Send a raw request to the server and get a response
    fn send_raw_request(&self, request: &str) -> Result<String, A2AError>;

    /// Send a structured request to the server and get a response
    fn send_request(&self, request: &A2ARequest) -> Result<JSONRPCResponse, A2AError>;
}

#[cfg(feature = "client")]
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

    /// Get the push notification configuration for a task
    async fn get_task_push_notification<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Subscribe to task updates (for streaming)
    async fn subscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError>
    where
        Self: Sized;

    /// Resubscribe to an existing task
    async fn resubscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError>
    where
        Self: Sized;
}

#[cfg(feature = "client")]
/// An enum for items received in a streaming response
pub enum StreamItem {
    /// A status update
    StatusUpdate(TaskStatusUpdateEvent),
    /// An artifact update
    ArtifactUpdate(TaskArtifactUpdateEvent),
    /// An initial task response
    Task(Task),
}

#[cfg(feature = "client")]
pub use futures::stream::Stream;
