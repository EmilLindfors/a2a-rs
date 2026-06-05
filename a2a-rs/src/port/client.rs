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

    /// Subscribe to task updates (for streaming).
    ///
    /// Passing `last_event_id = None` is the spec-compliant subscribe: it maps
    /// to the A2A `SubscribeToTask` call and streams from the task's current
    /// state — exactly what a spec client expects.
    ///
    /// `last_event_id = Some(..)` opts into the a2a-rs **`Last-Event-ID`
    /// resumption enhancement** (not part of the A2A v1.0 spec): a resumable
    /// transport sends it as the SSE `Last-Event-ID` header so an a2a-rs server
    /// replays the events after that id before streaming live. A spec-compliant
    /// server ignores the header and simply streams from current state, so this
    /// stays interoperable either way.
    async fn subscribe_to_task(
        &self,
        task_id: &str,
        history_length: Option<u32>,
        last_event_id: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>, A2AError>;
}

/// A streamed [`StreamItem`] tagged with the server's SSE event id (when the
/// transport supports it). A resilient client records the most recent `event_id`
/// and echoes it as `Last-Event-ID` on reconnect to resume without gaps.
///
/// The `event_id` is part of the a2a-rs resumption enhancement (see
/// [`subscribe_to_task`](Transport::subscribe_to_task)); spec clients that only
/// read `item` are unaffected.
#[derive(Debug, Clone)]
pub struct StreamEvent {
    /// The server-assigned per-task event id, parsed from the SSE `id:` field.
    /// `None` for the initial task snapshot, for transports without event ids,
    /// or when talking to a spec-compliant server that does not emit `id:`.
    pub event_id: Option<u64>,
    /// The update payload.
    pub item: StreamItem,
}

impl StreamEvent {
    /// Construct a stream event.
    #[inline]
    pub fn new(event_id: Option<u64>, item: StreamItem) -> Self {
        Self { event_id, item }
    }

    /// A stream event with no id (initial snapshot / id-less transport).
    #[inline]
    pub fn untagged(item: StreamItem) -> Self {
        Self {
            event_id: None,
            item,
        }
    }
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
