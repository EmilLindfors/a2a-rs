//! Expose an A2A agent as MCP tools.
//!
//! This example wires up a minimal A2A HTTP agent in-process, fetches its
//! agent card, wraps it with `AgentToMcpBridge`, and demonstrates an MCP
//! client listing the resulting tools and invoking one — all over an
//! in-memory duplex transport so it runs with no external setup.
//!
//! Run with:
//!     cargo run --example a2a_as_mcp_server -p a2a-mcp

use std::sync::Arc;
use std::time::Duration;

use a2a_mcp::AgentToMcpBridge;
use a2a_rs::{
    adapter::{
        business::{DefaultMessageHandler, DefaultRequestProcessor},
        storage::InMemoryTaskStorage,
        transport::http::HttpClient,
        HttpServer, SimpleAgentInfo,
    },
    domain::{error::A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
    services::AgentInfoProvider,
};
use async_trait::async_trait;
use rmcp::{model::CallToolRequestParams, ServiceExt};
use tracing_subscriber::EnvFilter;

const AGENT_ADDR: &str = "127.0.0.1:18182";
const AGENT_URL: &str = "http://127.0.0.1:18182";

/// Minimal A2A handler that echoes incoming text.
///
/// Wraps a `DefaultMessageHandler` to satisfy the storage-touching bits
/// (task creation, history persistence) and overrides response generation
/// with a simple echo.
#[derive(Clone)]
struct EchoHandler {
    storage: Arc<InMemoryTaskStorage>,
}

#[async_trait]
impl AsyncMessageHandler for EchoHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        // Delegate to DefaultMessageHandler for proper storage semantics, then
        // synthesize an echo response on top of whatever it returned.
        let inner = DefaultMessageHandler::new((*self.storage).clone());
        let mut task = inner.process_message(task_id, message, session_id).await?;

        let echoed = message
            .parts
            .iter()
            .filter_map(|p| p.get_text())
            .collect::<Vec<_>>()
            .join(" ");

        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("echo: {echoed}"))])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        task.status = TaskStatus::new(TaskState::Completed, Some(response.clone())).into();
        task.history.push(response);
        Ok(task)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    // 1. Spin up a tiny A2A agent on localhost:18182 in a background task.
    let storage = Arc::new(InMemoryTaskStorage::new());
    let handler = EchoHandler {
        storage: storage.clone(),
    };

    let agent_info = SimpleAgentInfo::new("Echo Agent".to_string(), AGENT_URL.to_string())
        .with_description("Echoes back whatever you send.".to_string())
        .add_skill(
            "echo".to_string(),
            "Echo".to_string(),
            Some("Repeat the input back".to_string()),
        );

    let processor = DefaultRequestProcessor::new(
        handler,
        (*storage).clone(),
        (*storage).clone(),
        agent_info.clone(),
    );
    let server = HttpServer::new(processor, agent_info.clone(), AGENT_ADDR.to_string());

    let _server_task = tokio::spawn(async move {
        let _ = server.start().await;
    });

    // Wait for the listener to bind.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 2. Build the agent card. SimpleAgentInfo produces the same card the
    //    HTTP server publishes at /.well-known/agent-card.json.
    let agent_card = agent_info.get_agent_card().await?;
    println!(
        "Agent card: name={}, skills={}",
        agent_card.name,
        agent_card.skills.len()
    );

    // 3. Bridge the A2A agent into an MCP server handler.
    //    The tool namespace is read from agent_card.url.
    let bridge = AgentToMcpBridge::new(HttpClient::new(AGENT_URL.to_string()), agent_card);

    // 4. Pair an MCP client and the bridge over an in-memory duplex stream.
    let (server_io, client_io) = tokio::io::duplex(4096);
    let bridge_task = tokio::spawn(async move {
        let running = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });

    let mcp_client = ().serve(client_io).await?;
    let peer = mcp_client.peer().clone();

    // 5. Discover tools.
    let tools = peer.list_tools(None).await?;
    println!("\nMCP tools exposed by the bridge:");
    for tool in &tools.tools {
        println!(
            "  - {} : {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    // 6. Call the first tool with a sample message.
    if let Some(tool) = tools.tools.first() {
        let params = CallToolRequestParams::new(tool.name.to_string()).with_arguments(
            serde_json::json!({ "message": "hello over MCP" })
                .as_object()
                .cloned()
                .unwrap(),
        );
        let result = peer.call_tool(params).await?;
        println!("\nCalled `{}`:", tool.name);
        for content in result.content {
            if let Some(text) = content.as_text() {
                println!("  → {}", text.text);
            }
        }
    }

    drop(mcp_client);
    let _ = bridge_task.await;
    Ok(())
}
