//! The client-side `Transport` port.
//!
//! [`Transport`] is the outbound port a client uses to talk to a remote A2A
//! agent: the application names the capability it needs ("send a message", "get a
//! task", "subscribe to updates"), and a concrete transport **adapter**
//! (ConnectRPC, JSON-RPC 2.0, …) fulfils it over the wire. This is the mirror of
//! the inbound server ports — same hexagonal shape, opposite direction.
//!
//! Each adapter reports its wire protocol via [`Transport::protocol`] so a
//! card-driven negotiator can pick the right one from an agent card's
//! `supported_interfaces`.
//!
//! The port carries no feature gate (hex rule 5 — gate adapters, not ports); it
//! depends only on the always-available `async-trait`/`futures` and domain types.

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::domain::{
    A2AError, ListTasksParams, ListTasksResult, Message, Task, TaskArtifactUpdateEvent,
    TaskPushNotificationConfig, TaskStatusUpdateEvent,
};

/// The capability a client needs from a remote A2A agent, independent of wire
/// protocol. Implemented by each transport adapter (`HttpClient` for ConnectRPC,
/// `JsonRpcClient` for JSON-RPC 2.0, …).
#[async_trait]
pub trait Transport: Send + Sync {
    /// The wire protocol this transport speaks, matching an agent interface's
    /// `protocol_binding` (e.g. `"JSONRPC"`, `"CONNECTRPC"`, `"GRPC"`).
    fn protocol(&self) -> &str;

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

/// Items that can be streamed from the server during task subscriptions.
///
/// When subscribing to streaming updates for a task, the server can send
/// different types of items:
/// - `Task`: The complete initial task state when subscription starts
/// - `StatusUpdate`: Updates to the task's status (state changes, progress)
/// - `ArtifactUpdate`: Notifications about new or updated artifacts
///
/// This allows clients to receive real-time updates about task progress
/// and results as they become available.
#[derive(Debug, Clone)]
pub enum StreamItem {
    /// The initial task state
    Task(Task),
    /// A task status update
    StatusUpdate(TaskStatusUpdateEvent),
    /// A task artifact update
    ArtifactUpdate(TaskArtifactUpdateEvent),
}
