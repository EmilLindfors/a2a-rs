//! Test business handler for integration tests
//!
//! This provides a complete agent implementation that bundles all business capabilities
//! for use in integration tests.

use std::sync::Arc;

use async_trait::async_trait;

use a2a_rs::{
    adapter::{
        business::ResponderMessageHandler, storage::InMemoryTaskStorage,
        streaming::InMemoryStreamingHandler,
    },
    domain::{
        A2AError, ContextId, Message, Task, TaskArtifactUpdateEvent, TaskId, TaskState,
        TaskStatusUpdateEvent,
    },
    port::{
        AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskLifecycle,
        AsyncTaskQuery, streaming_handler::Subscriber,
    },
};

/// Test business handler that coordinates all business capability traits
/// by delegating to InMemoryTaskStorage
#[derive(Clone)]
pub struct TestBusinessHandler {
    /// Task storage (persistence + push-config CRUD)
    storage: Arc<InMemoryTaskStorage>,
    /// Dedicated streaming fan-out
    streaming: InMemoryStreamingHandler,
}

impl TestBusinessHandler {
    /// Create a new test business handler
    pub fn new() -> Self {
        Self {
            storage: Arc::new(InMemoryTaskStorage::new()),
            streaming: InMemoryStreamingHandler::new(),
        }
    }

    /// Create with a custom storage implementation
    #[allow(dead_code)]
    pub fn with_storage(storage: InMemoryTaskStorage) -> Self {
        Self {
            storage: Arc::new(storage),
            streaming: InMemoryStreamingHandler::new(),
        }
    }

    /// Get a reference to the underlying storage
    #[allow(dead_code)]
    pub fn storage(&self) -> &Arc<InMemoryTaskStorage> {
        &self.storage
    }
}

impl Default for TestBusinessHandler {
    fn default() -> Self {
        Self::new()
    }
}

// Asynchronous trait implementations - delegate to storage

#[async_trait]
impl AsyncMessageHandler for TestBusinessHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        // Create a message handler and delegate, sharing the streaming handler.
        let message_handler = ResponderMessageHandler::echo(
            (*self.storage).clone(),
            self.streaming.clone(),
            self.storage.push_notifier(),
        );
        message_handler
            .process_message(task_id, message, session_id)
            .await
    }
}

#[async_trait]
impl AsyncTaskLifecycle for TestBusinessHandler {
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
impl AsyncTaskQuery for TestBusinessHandler {
    async fn list(
        &self,
        params: &a2a_rs::domain::ListTasksParams,
    ) -> Result<a2a_rs::domain::ListTasksResult, A2AError> {
        self.storage.list(params).await
    }
}

#[async_trait]
impl AsyncNotificationManager for TestBusinessHandler {
    async fn set_config(
        &self,
        config: &a2a_rs::domain::TaskPushNotificationConfig,
    ) -> Result<a2a_rs::domain::TaskPushNotificationConfig, A2AError> {
        self.storage.set_config(config).await
    }

    async fn get_config(
        &self,
        params: &a2a_rs::domain::GetTaskPushNotificationConfigParams,
    ) -> Result<a2a_rs::domain::TaskPushNotificationConfig, A2AError> {
        self.storage.get_config(params).await
    }

    async fn list_configs(
        &self,
        params: &a2a_rs::domain::ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<a2a_rs::domain::TaskPushNotificationConfig>, A2AError> {
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
impl AsyncStreamingHandler for TestBusinessHandler {
    async fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.streaming
            .add_status_subscriber(task_id, subscriber)
            .await
    }

    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.streaming
            .add_artifact_subscriber(task_id, subscriber)
            .await
    }

    async fn remove_subscription(&self, subscription_id: &str) -> Result<(), A2AError> {
        self.streaming.remove_subscription(subscription_id).await
    }

    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        self.streaming.remove_task_subscribers(task_id).await
    }

    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        self.streaming.get_subscriber_count(task_id).await
    }

    async fn broadcast_status_update(
        &self,
        task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        self.streaming.broadcast_status_update(task_id, update).await
    }

    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        self.streaming
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
        self.streaming.status_update_stream(task_id).await
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
        self.streaming.artifact_update_stream(task_id).await
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
        self.streaming.combined_update_stream(task_id).await
    }
}
