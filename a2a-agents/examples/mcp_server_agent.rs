//! Expose a declarative A2A agent as an MCP stdio server.
//!
//! This example flips `features.mcp_server.enabled = true` in TOML and lets
//! `AgentBuilder` / `AgentServer` do the rest: it starts an MCP stdio bridge
//! that dispatches calls to the agent handler in-process (without needing to
//! start an HTTP/WebSocket server). The agent's skills are callable as MCP
//! tools by clients like Claude Desktop.
//!
//! Requires the `mcp-server` feature:
//!
//! ```text
//! cargo run --example mcp_server_agent -p a2a-agents --features mcp-server
//! ```
//!
//! Logs go to stderr; stdout is reserved for the MCP protocol channel.

use a2a_agents::core::{AgentBuilder, BuildError};
use a2a_rs::{
    InMemoryTaskStorage,
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use uuid::Uuid;

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
        let text = message
            .parts
            .iter()
            .find_map(|p| p.get_text())
            .unwrap_or("<empty>")
            .to_string();

        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("echo: {text}"))])
            .message_id(Uuid::new_v4().to_string())
            .build();

        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id(message.context_id.clone())
            .status(TaskStatus::new(
                TaskState::Completed,
                Some(response.clone()),
            ))
            .history(vec![message.clone(), response])
            .build())
    }
}

#[tokio::main]
async fn main() -> Result<(), BuildError> {
    // tracing_subscriber writes to stderr by default — safe alongside MCP stdio.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    AgentBuilder::from_file("examples/mcp_server_agent.toml")?
        .with_handler(EchoHandler)
        .with_storage(InMemoryTaskStorage::new())
        .build()?
        .run()
        .await
        .map_err(|e| BuildError::RuntimeError(e.to_string()))?;

    Ok(())
}
