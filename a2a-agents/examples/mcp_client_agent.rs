//! Agent as an MCP **client** — connect to an MCP server from TOML config and
//! call its tools while serving A2A requests.
//!
//! Run it (the `mcp-client` feature pulls in `rmcp`):
//!
//! ```bash
//! cargo run -p a2a-agents --example mcp_client_agent --features mcp-client
//! ```
//!
//! The TOML (`mcp_client_agent.toml`) declares one downstream MCP server under
//! `[features.mcp_client]`; the framework spawns it as a child process. This
//! example points at the bundled [`mcp_echo_server`](../bin/mcp_echo_server.rs)
//! so it runs with no external setup.
//!
//! The flow that "finishes" the mcp-client integration:
//!
//! 1. Load config, then [`McpClientManager::connect`] it — this connects to
//!    every configured server and discovers their tools.
//! 2. Hand the *connected* manager to the handler, which owns it and implements
//!    [`McpToolsExt`] by returning a reference to it.
//! 3. The handler calls tools through `McpToolsExt` while processing messages.
//!
//! Talk to it once running (separate shell):
//!
//! ```bash
//! curl -s http://127.0.0.1:8080/.well-known/agent-card.json | jq .
//! ```

use a2a_agents::core::{AgentBuilder, AgentConfig, McpClientManager};
use a2a_agents::traits::{McpToolsExt, extract_tool_result_text};
use a2a_rs::{
    InMemoryTaskStorage,
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

/// A handler that forwards each message to the downstream MCP `echo` tool.
///
/// It owns the [`McpClientManager`] and surfaces [`McpToolsExt`] by handing out
/// a reference to it — that's all the wiring a handler needs to use MCP tools.
#[derive(Clone)]
struct McpEchoHandler {
    mcp: McpClientManager,
}

impl McpToolsExt for McpEchoHandler {
    fn mcp_client(&self) -> &McpClientManager {
        &self.mcp
    }
}

#[async_trait]
impl AsyncMessageHandler for McpEchoHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let text = message
            .parts
            .iter()
            .find_map(|p| p.get_text().map(str::to_string))
            .unwrap_or_else(|| "No text provided".to_string());

        // Call the downstream MCP `echo` tool and surface its result.
        let reply = match self
            .call_mcp_tool("echo", "echo", Some(json!({ "text": text })))
            .await
        {
            Ok(result) => format!("MCP echo says: {}", extract_tool_result_text(&result)),
            Err(e) => format!("MCP tool call failed: {e}"),
        };

        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(reply)])
            .message_id(Uuid::new_v4().to_string())
            .context_id(message.context_id.clone())
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
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // 1. Load config and connect to the MCP servers it declares.
    let config = AgentConfig::from_file("examples/mcp_client_agent.toml")?;
    let mcp = McpClientManager::connect(&config.features.mcp_client).await?;
    tracing::info!("connected MCP servers: {:?}", mcp.connected_servers().await);

    // 2. Hand the connected manager to the handler.
    let handler = McpEchoHandler { mcp };

    // 3. Assemble and run.
    println!("🚀 MCP echo client agent on http://127.0.0.1:8080");
    AgentBuilder::new(config)
        .with_handler(handler)
        .with_storage(InMemoryTaskStorage::new())
        .build()?
        .run()
        .await?;

    Ok(())
}
