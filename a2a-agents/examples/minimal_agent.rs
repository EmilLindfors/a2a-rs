//! Minimal agent example using the new declarative API
//!
//! This example shows how to create a simple agent with just ~30 lines of code.
//! The agent echoes back any message it receives.

use a2a_agents::core::{AgentBuilder, BuildError};
use a2a_rs::{
    InMemoryTaskStorage,
    domain::{A2AError, Message, Part, Role, Task, TaskState},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use uuid::Uuid;

/// Simple echo handler that echoes back messages
#[derive(Clone)]
struct EchoHandler;

#[async_trait]
impl AsyncMessageHandler for EchoHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        // Extract text from the message
        let text = message
            .parts
            .iter()
            .find_map(|part| match part {
                Part::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "No text provided".to_string());

        // Create echo response
        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("Echo: {}", text))])
            .message_id(Uuid::new_v4().to_string())
            .context_id(message.context_id.clone().unwrap_or_default())
            .build();

        // Return completed task with echo response
        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id(message.context_id.clone().unwrap_or_default())
            .status(a2a_rs::domain::TaskStatus {
                state: TaskState::Completed,
                message: Some(response.clone()),
                timestamp: Some(chrono::Utc::now()),
            })
            .history(vec![message.clone(), response])
            .build())
    }

    async fn validate_message(&self, message: &Message) -> Result<(), A2AError> {
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), BuildError> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    println!("🚀 Starting Echo Agent with declarative configuration");
    println!();

    // Build and run the agent from configuration file
    // This is all you need - the rest is handled by the framework!
    AgentBuilder::from_file("examples/minimal_agent.toml")?
        .with_handler(EchoHandler)
        .with_storage(InMemoryTaskStorage::new())
        .build()?
        .run()
        .await
        .map_err(|e| BuildError::RuntimeError(e.to_string()))?;

    Ok(())
}
