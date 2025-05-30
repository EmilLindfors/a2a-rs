//! Default message handler implementation

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    domain::{A2AError, Message, Task, TaskState},
    port::{AsyncMessageHandler, AsyncTaskManager},
};

/// Default message handler that processes messages and delegates to task manager
#[derive(Clone)]
pub struct DefaultMessageHandler<T>
where
    T: AsyncTaskManager + Send + Sync + 'static,
{
    /// Task manager for handling task operations
    task_manager: Arc<T>,
}

impl<T> DefaultMessageHandler<T>
where
    T: AsyncTaskManager + Send + Sync + 'static,
{
    /// Create a new message handler with the given task manager
    pub fn new(task_manager: T) -> Self {
        Self {
            task_manager: Arc::new(task_manager),
        }
    }
}

#[async_trait]
impl<T> AsyncMessageHandler for DefaultMessageHandler<T>
where
    T: AsyncTaskManager + Send + Sync + 'static,
{
    async fn process_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Get or create the task
        let mut task = if self.task_manager.task_exists(task_id).await? {
            // Get existing task
            self.task_manager.get_task(task_id, None).await?
        } else {
            // Create a new task
            let context_id = session_id.unwrap_or("default");
            self.task_manager.create_task(task_id, context_id).await?
        };

        // Update the task with the message
        // This adds the message to history inside the update_status method
        task.update_status(TaskState::Working, Some(message.clone()));

        // Update the task status through the task manager
        // This will handle broadcasting and storage updates
        self.task_manager
            .update_task_status(task_id, TaskState::Working)
            .await
    }
}
