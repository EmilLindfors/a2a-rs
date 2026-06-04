//! The task application service: use-case orchestration over the port traits.
//!
//! `TaskService` is the **inner** half of the service/transport split: it owns
//! the ports (`Arc<dyn …>`), orchestrates them, and speaks only the domain
//! vocabulary (`Task`, `Message`, `TaskId`, `A2AError`). It knows nothing about
//! ConnectRPC, `buffa` views, or wire error codes — that glue lives in the
//! transport adapter ([`ConnectRpcAdapter`](crate::adapter::ConnectRpcAdapter)),
//! which decodes wire requests into these domain calls and re-encodes the
//! results.
//!
//! Because the service holds both the lifecycle and streaming ports it exposes
//! them as mixin ingredients ([`HasTaskLifecycle`], [`HasStreaming`]) and so
//! gains [`TaskStatusBroadcast::update_and_broadcast`] for free
//! (`.claude/rules/hexagonal_architecture.md` §9). The accessors return `&dyn`
//! **ports**, never the concrete adapters behind them, so the dependency arrow
//! still points inward.
//!
//! [`TaskStatusBroadcast::update_and_broadcast`]: crate::application::TaskStatusBroadcast::update_and_broadcast

use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;

use crate::application::{HasPushNotifier, HasStreaming, HasTaskLifecycle, TaskStatusBroadcast};
use crate::domain::{
    A2AError, AgentCard, DeleteTaskPushNotificationConfigParams,
    GetTaskPushNotificationConfigParams, ListTaskPushNotificationConfigsParams, ListTasksParams,
    ListTasksResult, Message, Task, TaskId, TaskPushNotificationConfig,
};
use crate::port::{
    AsyncMessageHandler, AsyncNotificationManager, AsyncNotificationManagerExt, AsyncPushNotifier,
    AsyncStreamingHandler, AsyncTaskLifecycle, AsyncTaskQuery, SeqEvent,
};
use crate::services::server::AgentInfoProvider;

/// A stream of sequenced update events for a task. Each [`SeqEvent`] carries a
/// per-task monotonic id (surfaced as the SSE `id:` field); the transport
/// adapter maps the inner update onto its wire representation.
pub type UpdateStream = Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>;

/// Use-case orchestration over the A2A ports.
///
/// Constructed at the composition edge with concrete adapters injected; the
/// fields are `Arc<dyn …>` so the service type carries no generic parameters.
/// All methods return domain types and [`A2AError`] — there is no transport
/// vocabulary in this layer.
#[derive(Clone)]
pub struct TaskService {
    message_handler: Arc<dyn AsyncMessageHandler>,
    task_lifecycle: Arc<dyn AsyncTaskLifecycle>,
    task_query: Arc<dyn AsyncTaskQuery>,
    notification_manager: Arc<dyn AsyncNotificationManager>,
    agent_info: Arc<dyn AgentInfoProvider>,
    streaming_handler: Arc<dyn AsyncStreamingHandler>,
    push_notifier: Arc<dyn AsyncPushNotifier>,
}

impl TaskService {
    /// Assemble a service from separate handlers.
    ///
    /// `tasks` supplies both the lifecycle and query capabilities; it is
    /// stored once and shared between the two `Arc<dyn …>` fields.
    pub fn new(
        message_handler: impl AsyncMessageHandler + 'static,
        tasks: impl AsyncTaskLifecycle + AsyncTaskQuery + 'static,
        notification_manager: impl AsyncNotificationManager + 'static,
        agent_info: impl AgentInfoProvider + 'static,
        streaming_handler: impl AsyncStreamingHandler + 'static,
        push_notifier: impl AsyncPushNotifier + 'static,
    ) -> Self {
        let tasks = Arc::new(tasks);
        Self {
            message_handler: Arc::new(message_handler),
            task_lifecycle: tasks.clone(),
            task_query: tasks,
            notification_manager: Arc::new(notification_manager),
            agent_info: Arc::new(agent_info),
            streaming_handler: Arc::new(streaming_handler),
            push_notifier: Arc::new(push_notifier),
        }
    }

    /// Assemble a service from a single handler that implements every port.
    pub fn with_handler(
        handler: impl AsyncMessageHandler
        + AsyncTaskLifecycle
        + AsyncTaskQuery
        + AsyncNotificationManager
        + 'static,
        agent_info: impl AgentInfoProvider + 'static,
        streaming_handler: impl AsyncStreamingHandler + 'static,
        push_notifier: impl AsyncPushNotifier + 'static,
    ) -> Self {
        let handler = Arc::new(handler);
        Self {
            message_handler: handler.clone(),
            task_lifecycle: handler.clone(),
            task_query: handler.clone(),
            notification_manager: handler,
            agent_info: Arc::new(agent_info),
            streaming_handler: Arc::new(streaming_handler),
            push_notifier: Arc::new(push_notifier),
        }
    }

    /// Replace the streaming handler, returning the updated service.
    pub fn with_streaming_handler(
        mut self,
        streaming_handler: impl AsyncStreamingHandler + 'static,
    ) -> Self {
        self.streaming_handler = Arc::new(streaming_handler);
        self
    }

    /// Replace the push notifier, returning the updated service.
    pub fn with_push_notifier(mut self, push_notifier: impl AsyncPushNotifier + 'static) -> Self {
        self.push_notifier = Arc::new(push_notifier);
        self
    }

    /// Process a message for a task, optionally configuring push notifications
    /// and limiting the returned history.
    pub async fn send_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
        push_config: Option<TaskPushNotificationConfig>,
        history_limit: Option<u32>,
    ) -> Result<Task, A2AError> {
        if let Some(mut push_config) = push_config {
            push_config.task_id = task_id.to_string();
            self.notification_manager.set_validated(&push_config).await?;
        }

        let mut task = self
            .message_handler
            .process_message(task_id, message, session_id)
            .await?;

        if let Some(limit) = history_limit {
            task = task.with_limited_history(Some(limit));
        }

        Ok(task)
    }

    /// Process a message and subscribe to its update stream.
    ///
    /// The update stream is started **before** the message is processed so no
    /// early updates are missed. Returns the initial task and the stream; the
    /// caller is responsible for emitting the initial task ahead of stream
    /// items.
    pub async fn send_streaming_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
        push_config: Option<TaskPushNotificationConfig>,
        history_limit: Option<u32>,
    ) -> Result<(Task, UpdateStream), A2AError> {
        if let Some(mut push_config) = push_config {
            push_config.task_id = task_id.to_string();
            self.notification_manager.set_validated(&push_config).await?;
        }

        // Start updates stream first so we don't miss early updates.
        let update_stream = self
            .streaming_handler
            .start_task_streaming(task_id, None)
            .await?;

        let mut task = self
            .message_handler
            .process_message(task_id, message, session_id)
            .await?;

        if let Some(limit) = history_limit {
            task = task.with_limited_history(Some(limit));
        }

        Ok((task, update_stream))
    }

    /// Get a task by ID with optional history length limit.
    pub async fn get(&self, id: &TaskId, history_length: Option<u32>) -> Result<Task, A2AError> {
        self.task_lifecycle.get(id, history_length).await
    }

    /// List tasks with filtering and pagination.
    pub async fn list(&self, params: &ListTasksParams) -> Result<ListTasksResult, A2AError> {
        self.task_query.list(params).await
    }

    /// Cancel a task, then announce the terminal status to streaming
    /// subscribers.
    ///
    /// Storage no longer self-broadcasts on cancellation (§4.0.2), so the
    /// service owns the "commit then announce" step via the
    /// [`TaskStatusBroadcast`] mixin it hosts.
    pub async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError> {
        self.cancel_and_broadcast(id).await
    }

    /// Subscribe to a task's update stream, returning the current task (if it
    /// exists) and the stream of subsequent updates.
    ///
    /// `from_event_id` carries a client's `Last-Event-ID` for resumption: when
    /// set, the handler replays buffered events with a greater id before
    /// streaming live updates.
    pub async fn subscribe(
        &self,
        task_id: &str,
        from_event_id: Option<u64>,
    ) -> Result<(Option<Task>, UpdateStream), A2AError> {
        let id: TaskId = task_id.parse()?;

        let initial_task = match self.task_lifecycle.get(&id, None).await {
            Ok(task) => Some(task),
            Err(A2AError::TaskNotFound(_)) => None,
            Err(e) => return Err(e),
        };

        let update_stream = self
            .streaming_handler
            .start_task_streaming(task_id, from_event_id)
            .await?;

        Ok((initial_task, update_stream))
    }

    /// Create or replace a push-notification config (validated).
    pub async fn set_push_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.notification_manager.set_validated(config).await
    }

    /// Get a push-notification config for a task.
    pub async fn get_push_config(
        &self,
        params: &GetTaskPushNotificationConfigParams,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.notification_manager.get_config(params).await
    }

    /// List push-notification configs for a task.
    pub async fn list_push_configs(
        &self,
        params: &ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        self.notification_manager.list_configs(params).await
    }

    /// Delete a push-notification config.
    pub async fn delete_push_config(
        &self,
        params: &DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        self.notification_manager.delete_config(params).await
    }

    /// Fetch the authenticated extended agent card.
    pub async fn extended_agent_card(&self) -> Result<AgentCard, A2AError> {
        self.agent_info.get_authenticated_extended_card().await
    }
}

// The service is the composed assembly holding both the lifecycle and streaming
// ports, so it exposes them as mixin ingredients (see
// `.claude/rules/hexagonal_architecture.md` §9). This grants it the
// `TaskStatusBroadcast::update_and_broadcast` "commit then announce" capability
// for free, without coupling either port to the other. The accessors return
// `&dyn` **ports**, never the concrete adapters behind them.
impl HasTaskLifecycle for TaskService {
    fn lifecycle(&self) -> &dyn AsyncTaskLifecycle {
        self.task_lifecycle.as_ref()
    }
}

impl HasStreaming for TaskService {
    fn streaming(&self) -> &dyn AsyncStreamingHandler {
        self.streaming_handler.as_ref()
    }
}

impl HasPushNotifier for TaskService {
    fn push_notifier(&self) -> &dyn AsyncPushNotifier {
        self.push_notifier.as_ref()
    }
}
