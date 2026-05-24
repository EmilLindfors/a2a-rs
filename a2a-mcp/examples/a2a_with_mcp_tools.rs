//! Augment an A2A message handler with MCP tool capabilities.
//!
//! This example wires an in-process MCP server (a calculator with an `add`
//! tool) to a stub A2A handler via `McpToA2ABridge`. A normal A2A message is
//! handled by the inner handler, while a message carrying an
//! `a2a_rs_tool_call` metadata envelope is routed to the MCP server.
//!
//! Run with:
//!     cargo run --example a2a_with_mcp_tools -p a2a-mcp

use std::sync::Arc;

use a2a_mcp::{create_tool_call_message, McpToA2ABridge};
use a2a_rs::{
    domain::{error::A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use rmcp::{
    model::*, service::RequestContext, ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use serde_json::json;
use tracing_subscriber::EnvFilter;

/// A stub A2A handler that echoes whatever text it receives.
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

        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id(uuid::Uuid::new_v4().to_string())
            .status(TaskStatus::new(TaskState::Completed, None))
            .history(vec![message.clone(), response])
            .build())
    }
}

/// A minimal MCP server exposing a single `add` tool.
#[derive(Clone)]
struct CalcServer {
    tools: Arc<Vec<Tool>>,
}

impl CalcServer {
    fn new() -> Self {
        let input_schema = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "number" }
            },
            "required": ["a", "b"]
        }))
        .expect("valid JSON schema");

        let add = Tool::new("add", "Add two numbers", Arc::new(input_schema));
        Self {
            tools: Arc::new(vec![add]),
        }
    }
}

#[async_trait]
#[allow(clippy::manual_async_fn)]
impl ServerHandler for CalcServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_server_info(Implementation::new("calc-server", "0.1.0"))
            .with_instructions("Calculator MCP server with an `add` tool")
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            Ok(ListToolsResult {
                tools: (*self.tools).clone(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn call_tool(
        &self,
        CallToolRequestParams {
            name, arguments, ..
        }: CallToolRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            if name != "add" {
                return Err(McpError::invalid_params(
                    format!("unknown tool: {name}"),
                    None,
                ));
            }
            let args =
                arguments.ok_or_else(|| McpError::invalid_params("missing arguments", None))?;
            let a = args
                .get("a")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| McpError::invalid_params("missing or non-numeric 'a'", None))?;
            let b = args
                .get("b")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| McpError::invalid_params("missing or non-numeric 'b'", None))?;
            Ok(CallToolResult::success(vec![Content::text(
                (a + b).to_string(),
            )]))
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Wire up an MCP client/server pair over an in-memory duplex transport.
    let (server_io, client_io) = tokio::io::duplex(4096);

    let server_task = tokio::spawn(async move {
        let running = CalcServer::new().serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });

    let mcp_client = ().serve(client_io).await?;
    let peer = mcp_client.peer().clone();

    // Wrap an A2A handler so that TOOL_CALL messages get routed to MCP.
    let bridge = McpToA2ABridge::new(peer, EchoHandler).await?;

    // 1. Normal A2A message → inner handler echoes.
    let plain = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text("hello world".to_string())])
        .message_id(uuid::Uuid::new_v4().to_string())
        .build();

    let echoed = bridge.process_message("task-echo", &plain, None).await?;
    println!("Plain message → state {:?}", echoed.status.state);
    for m in &echoed.history {
        for part in &m.parts {
            if let Some(text) = part.get_text() {
                println!("  [{:?}] {}", m.role, text);
            }
        }
    }

    // 2. Tool-call message (metadata-driven) → routed to MCP server.
    let tool_msg = create_tool_call_message("add", json!({ "a": 5, "b": 7 }));
    let tool_result = bridge.process_message("task-add", &tool_msg, None).await?;
    println!(
        "\nTool call add(5,7) → state {:?}",
        tool_result.status.state
    );
    for m in &tool_result.history {
        for part in &m.parts {
            if let Some(text) = part.get_text() {
                println!("  [{:?}] {}", m.role, text);
            }
        }
    }

    drop(mcp_client);
    let _ = server_task.await;
    Ok(())
}
