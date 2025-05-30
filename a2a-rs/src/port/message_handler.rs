//! Message handling port definitions

#[cfg(feature = "server")]
use async_trait::async_trait;

use crate::domain::{A2AError, Message, Task};

/// A trait for handling message processing operations
pub trait MessageHandler {
    /// Process a message for a specific task
    fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError>;

    /// Validate a message before processing
    fn validate_message(&self, message: &Message) -> Result<(), A2AError> {
        // Default implementation - can be overridden
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "message.parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }
        Ok(())
    }

    /// Transform a message before processing (e.g., for content filtering)
    fn transform_message(&self, message: Message) -> Result<Message, A2AError> {
        // Default implementation - pass through unchanged
        Ok(message)
    }
}

#[cfg(feature = "server")]
#[async_trait]
/// An async trait for handling message processing operations
pub trait AsyncMessageHandler: Send + Sync {
    /// Process a message for a specific task
    async fn process_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError>;

    /// Validate a message before processing
    async fn validate_message<'a>(&self, message: &'a Message) -> Result<(), A2AError> {
        // Default implementation - can be overridden
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "message.parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }
        Ok(())
    }

    /// Transform a message before processing (e.g., for content filtering)
    async fn transform_message(&self, message: Message) -> Result<Message, A2AError> {
        // Default implementation - pass through unchanged
        Ok(message)
    }

    /// Handle message processing with validation and transformation
    async fn handle_message_flow<'a>(
        &self,
        task_id: &'a str,
        message: Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Validate the message
        self.validate_message(&message).await?;

        // Transform the message if needed
        let transformed_message = self.transform_message(message).await?;

        // Process the message
        self.process_message(task_id, &transformed_message, session_id)
            .await
    }
}
