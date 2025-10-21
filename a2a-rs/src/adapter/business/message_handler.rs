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
        // Check if task exists
        let task_exists = self.task_manager.task_exists(task_id).await?;

        if !task_exists {
            // Create a new task
            let context_id = session_id.unwrap_or("default");
            self.task_manager.create_task(task_id, context_id).await?;
        }

        // First, update the task with the incoming message to add it to history
        self.task_manager
            .update_task_status(task_id, TaskState::Working, Some(message.clone()))
            .await?;

        // Create a simple echo response
        let response_message = Message::builder()
            .role(crate::domain::Role::Agent)
            .parts(vec![crate::domain::Part::text(format!(
                "Echo: {}",
                message
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        crate::domain::Part::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            ))])
            .message_id(uuid::Uuid::new_v4().to_string())
            .task_id(task_id.to_string())
            .context_id(message.context_id.clone().unwrap_or_default())
            .build();

        // For the default handler, we'll add the response message to history but keep the task in Working state
        // Real agents would process the message and determine the appropriate final state
        let final_task = self
            .task_manager
            .update_task_status(task_id, TaskState::Working, Some(response_message))
            .await?;

        Ok(final_task)
    }
}
