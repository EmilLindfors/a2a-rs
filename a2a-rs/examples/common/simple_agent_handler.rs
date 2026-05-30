//! Simple agent handler for examples and testing
//!
//! This provides a complete agent implementation that bundles all business capabilities
//! (message handling, task management, notifications, and streaming) with in-memory storage.
//!
//! For production agents, you typically want to implement your own message handler
//! and compose it with the storage implementations directly.

use std::sync::Arc;

use async_trait::async_trait;

use a2a_rs::{
    adapter::storage::InMemoryTaskStorage,
    domain::{
        A2AError, ContextId, Message, Task, TaskArtifactUpdateEvent, TaskId,
        TaskPushNotificationConfig, TaskState, TaskStatusUpdateEvent,
    },
    port::{
        AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskLifecycle,
        AsyncTaskQuery, streaming_handler::Subscriber,
    },
};

/// Simple agent handler that coordinates all business capability traits
/// by delegating to InMemoryTaskStorage which implements the actual functionality.
///
/// This is useful for:
/// - Quick prototyping
/// - Simple echo/test agents
/// - Examples and demos
/// - Agents that don't need custom message processing
///
/// For production agents with custom business logic, implement your own
/// `AsyncMessageHandler` and compose it with storage using `ConnectRpcAdapter`.
#[derive(Clone)]
pub struct SimpleAgentHandler {
    /// Task storage that implements all the business capabilities
    storage: Arc<InMemoryTaskStorage>,
}

impl SimpleAgentHandler {
    /// Create a new simple agent handler
    pub fn new() -> Self {
        Self {
            storage: Arc::new(InMemoryTaskStorage::new()),
        }
    }

    /// Create with a custom storage implementation
    pub fn with_storage(storage: InMemoryTaskStorage) -> Self {
        Self {
            storage: Arc::new(storage),
        }
    }

    /// Get a reference to the underlying storage
    #[allow(dead_code)]
    pub fn storage(&self) -> &Arc<InMemoryTaskStorage> {
        &self.storage
    }
}

impl Default for SimpleAgentHandler {
    fn default() -> Self {
        Self::new()
    }
}

// Asynchronous trait implementations - delegate to storage

#[async_trait]
impl AsyncMessageHandler for SimpleAgentHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        // Create a message handler and delegate
        let message_handler = a2a_rs::adapter::business::ResponderMessageHandler::echo(
            (*self.storage).clone(),
            (*self.storage).clone(),
        );
        message_handler
            .process_message(task_id, message, session_id)
            .await
    }
}

#[async_trait]
impl AsyncTaskLifecycle for SimpleAgentHandler {
    async fn create(&self, id: &TaskId, context_id: &ContextId) -> Result<Task, A2AError> {
        self.storage.create(id, context_id).await
    }

    async fn get(&self, id: &TaskId, history_length: Option<u32>) -> Result<Task, A2AError> {
        self.storage.get(id, history_length).await
    }

    async fn update_status(
        &self,
        id: &TaskId,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2AError> {
        self.storage.update_status(id, state, message).await
    }

    async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError> {
        self.storage.cancel(id).await
    }

    async fn exists(&self, id: &TaskId) -> Result<bool, A2AError> {
        self.storage.exists(id).await
    }
}

#[async_trait]
impl AsyncTaskQuery for SimpleAgentHandler {
    async fn list(
        &self,
        params: &a2a_rs::domain::ListTasksParams,
    ) -> Result<a2a_rs::domain::ListTasksResult, A2AError> {
        self.storage.list(params).await
    }
}

#[async_trait]
impl AsyncNotificationManager for SimpleAgentHandler {
    async fn set_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.storage.set_config(config).await
    }

    async fn get_config(
        &self,
        params: &a2a_rs::domain::GetTaskPushNotificationConfigParams,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.storage.get_config(params).await
    }

    async fn list_configs(
        &self,
        params: &a2a_rs::domain::ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        self.storage.list_configs(params).await
    }

    async fn delete_config(
        &self,
        params: &a2a_rs::domain::DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        self.storage.delete_config(params).await
    }
}

#[async_trait]
impl AsyncStreamingHandler for SimpleAgentHandler {
    async fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.storage
            .add_status_subscriber(task_id, subscriber)
            .await
    }

    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.storage
            .add_artifact_subscriber(task_id, subscriber)
            .await
    }

    async fn remove_subscription(&self, subscription_id: &str) -> Result<(), A2AError> {
        self.storage.remove_subscription(subscription_id).await
    }

    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        self.storage.remove_task_subscribers(task_id).await
    }

    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        self.storage.get_subscriber_count(task_id).await
    }

    async fn broadcast_status_update(
        &self,
        task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        self.storage.broadcast_status_update(task_id, update).await
    }

    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        self.storage
            .broadcast_artifact_update(task_id, update)
            .await
    }

    async fn status_update_stream(
        &self,
        task_id: &str,
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>,
        >,
        A2AError,
    > {
        self.storage.status_update_stream(task_id).await
    }

    async fn artifact_update_stream(
        &self,
        task_id: &str,
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>,
        >,
        A2AError,
    > {
        self.storage.artifact_update_stream(task_id).await
    }

    async fn combined_update_stream(
        &self,
        task_id: &str,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures::Stream<
                        Item = Result<a2a_rs::port::streaming_handler::UpdateEvent, A2AError>,
                    > + Send,
            >,
        >,
        A2AError,
    > {
        self.storage.combined_update_stream(task_id).await
    }
}
