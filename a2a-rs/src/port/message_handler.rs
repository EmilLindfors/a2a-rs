//! Message handling port definitions

use async_trait::async_trait;

use crate::{
    domain::{A2AError, Message, Task},
    port::RequestContext,
};

#[async_trait]
/// An async trait for handling message processing operations
pub trait AsyncMessageHandler: Send + Sync {
    /// Process a message for a specific task.
    ///
    /// `ctx` is what the transport knows about the request — the context id the
    /// caller supplied and the principal it authenticated. A handler that keeps
    /// per-caller state reads [`RequestContext::caller`]; one that does not can
    /// ignore it.
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        ctx: &RequestContext,
    ) -> Result<Task, A2AError>;

    /// Validate a message before processing
    async fn validate_message(&self, message: &Message) -> Result<(), A2AError> {
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
    async fn handle_message_flow(
        &self,
        task_id: &str,
        message: Message,
        ctx: &RequestContext,
    ) -> Result<Task, A2AError> {
        // Validate the message
        self.validate_message(&message).await?;

        // Transform the message if needed
        let transformed_message = self.transform_message(message).await?;

        // Process the message
        self.process_message(task_id, &transformed_message, ctx)
            .await
    }
}
