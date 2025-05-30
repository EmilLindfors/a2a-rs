//! Default business handler implementation that coordinates all business capabilities

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    adapter::storage::InMemoryTaskStorage,
    domain::{
        A2AError, Message, Task, TaskArtifactUpdateEvent, TaskPushNotificationConfig, TaskState,
        TaskStatusUpdateEvent,
    },
    port::{
        streaming_handler::Subscriber, AsyncMessageHandler, AsyncNotificationManager,
        AsyncStreamingHandler, AsyncTaskManager, MessageHandler, NotificationManager,
        StreamingHandler, TaskManager,
    },
};

/// Default business handler that coordinates all business capability traits
/// by delegating to InMemoryTaskStorage which implements the actual functionality
#[derive(Clone)]
pub struct DefaultBusinessHandler {
    /// Task storage that implements all the business capabilities
    storage: Arc<InMemoryTaskStorage>,
}

impl DefaultBusinessHandler {
    /// Create a new default business handler
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
    pub fn storage(&self) -> &Arc<InMemoryTaskStorage> {
        &self.storage
    }
}

impl Default for DefaultBusinessHandler {
    fn default() -> Self {
        Self::new()
    }
}

// Synchronous trait implementations - not supported since we use async storage
impl MessageHandler for DefaultBusinessHandler {
    fn process_message(
        &self,
        _task_id: &str,
        _message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous message processing not supported. Use async version.".to_string(),
        ))
    }
}

impl TaskManager for DefaultBusinessHandler {
    fn create_task(&self, _task_id: &str, _context_id: &str) -> Result<Task, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous task creation not supported. Use async version.".to_string(),
        ))
    }

    fn get_task(&self, _task_id: &str, _history_length: Option<u32>) -> Result<Task, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous task retrieval not supported. Use async version.".to_string(),
        ))
    }

    fn update_task_status(&self, _task_id: &str, _state: TaskState) -> Result<Task, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous task status update not supported. Use async version.".to_string(),
        ))
    }

    fn cancel_task(&self, _task_id: &str) -> Result<Task, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous task cancellation not supported. Use async version.".to_string(),
        ))
    }

    fn task_exists(&self, _task_id: &str) -> Result<bool, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous task existence check not supported. Use async version.".to_string(),
        ))
    }
}

impl NotificationManager for DefaultBusinessHandler {
    fn set_task_notification(
        &self,
        _config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous notification setup not supported. Use async version.".to_string(),
        ))
    }

    fn get_task_notification(
        &self,
        _task_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous notification retrieval not supported. Use async version.".to_string(),
        ))
    }

    fn remove_task_notification(&self, _task_id: &str) -> Result<(), A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous notification removal not supported. Use async version.".to_string(),
        ))
    }
}

impl StreamingHandler for DefaultBusinessHandler {
    fn add_status_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous streaming subscription not supported. Use async version.".to_string(),
        ))
    }

    fn add_artifact_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous streaming subscription not supported. Use async version.".to_string(),
        ))
    }

    fn remove_subscription(&self, _subscription_id: &str) -> Result<(), A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous streaming unsubscription not supported. Use async version.".to_string(),
        ))
    }

    fn remove_task_subscribers(&self, _task_id: &str) -> Result<(), A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous streaming unsubscription not supported. Use async version.".to_string(),
        ))
    }

    fn get_subscriber_count(&self, _task_id: &str) -> Result<usize, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Synchronous subscriber count not supported. Use async version.".to_string(),
        ))
    }
}

// Asynchronous trait implementations - delegate to storage

#[async_trait]
impl AsyncMessageHandler for DefaultBusinessHandler {
    async fn process_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Create a message handler and delegate
        let message_handler =
            crate::adapter::business::DefaultMessageHandler::new((*self.storage).clone());
        message_handler
            .process_message(task_id, message, session_id)
            .await
    }
}

#[async_trait]
impl AsyncTaskManager for DefaultBusinessHandler {
    async fn create_task<'a>(
        &self,
        task_id: &'a str,
        context_id: &'a str,
    ) -> Result<Task, A2AError> {
        self.storage.create_task(task_id, context_id).await
    }

    async fn get_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        self.storage.get_task(task_id, history_length).await
    }

    async fn update_task_status<'a>(
        &self,
        task_id: &'a str,
        state: TaskState,
    ) -> Result<Task, A2AError> {
        self.storage.update_task_status(task_id, state).await
    }

    async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError> {
        self.storage.cancel_task(task_id).await
    }

    async fn task_exists<'a>(&self, task_id: &'a str) -> Result<bool, A2AError> {
        self.storage.task_exists(task_id).await
    }
}

#[async_trait]
impl AsyncNotificationManager for DefaultBusinessHandler {
    async fn set_task_notification<'a>(
        &self,
        config: &'a TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.storage.set_task_notification(config).await
    }

    async fn get_task_notification<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.storage.get_task_notification(task_id).await
    }

    async fn remove_task_notification<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        self.storage.remove_task_notification(task_id).await
    }
}

#[async_trait]
impl AsyncStreamingHandler for DefaultBusinessHandler {
    async fn add_status_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.storage
            .add_status_subscriber(task_id, subscriber)
            .await
    }

    async fn add_artifact_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.storage
            .add_artifact_subscriber(task_id, subscriber)
            .await
    }

    async fn remove_subscription<'a>(&self, subscription_id: &'a str) -> Result<(), A2AError> {
        self.storage.remove_subscription(subscription_id).await
    }

    async fn remove_task_subscribers<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        self.storage.remove_task_subscribers(task_id).await
    }

    async fn get_subscriber_count<'a>(&self, task_id: &'a str) -> Result<usize, A2AError> {
        self.storage.get_subscriber_count(task_id).await
    }

    async fn broadcast_status_update<'a>(
        &self,
        task_id: &'a str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        self.storage
            .broadcast_status_update(task_id, update.status, update.final_)
            .await
    }

    async fn broadcast_artifact_update<'a>(
        &self,
        task_id: &'a str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        self.storage
            .broadcast_artifact_update(
                task_id,
                update.artifact,
                None,
                update.last_chunk.unwrap_or(false),
            )
            .await
    }

    async fn status_update_stream<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>,
        >,
        A2AError,
    > {
        self.storage.status_update_stream(task_id).await
    }

    async fn artifact_update_stream<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>,
        >,
        A2AError,
    > {
        self.storage.artifact_update_stream(task_id).await
    }

    async fn combined_update_stream<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures::Stream<
                        Item = Result<crate::port::streaming_handler::UpdateEvent, A2AError>,
                    > + Send,
            >,
        >,
        A2AError,
    > {
        self.storage.combined_update_stream(task_id).await
    }
}
