//! Both bridges in a single process.
//!
//! Demonstrates that an A2A agent that *consumes* MCP tools (via
//! [`McpToA2ABridge`]) can itself be re-exposed as an MCP server (via
//! [`AgentToMcpBridge`]). The full path of a single invocation looks like:
//!
//! ```text
//!  Upstream MCP client
//!         │  call_tool("…_compute", {"message": "add 5 7"})
//!         ▼
//!  AgentToMcpBridge          ← exposes A2A agent as MCP tools
//!         │  message/send (HTTP, JSON-RPC)
//!         ▼
//!  A2A HTTP server  ┐
//!     MathHandler   │  parses "add 5 7", builds a tool-call envelope
//!         │         │
//!         ▼         │
//!  McpToA2ABridge   │  routes envelope to the MCP peer
//!         │         ┘
//!         ▼
//!  Downstream MCP server (Calculator)
//! ```
//!
//! Both MCP hops use in-memory `tokio::io::duplex` transports, so the demo
//! runs with no external setup. The only network hop is the A2A loopback
//! HTTP server.
//!
//! Run with:
//!     cargo run --example bidirectional_demo -p a2a-mcp

use std::sync::Arc;
use std::time::Duration;

use a2a_mcp::{create_tool_call_message, AgentToMcpBridge, McpToA2ABridge};
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
use rmcp::{
    model::*, service::RequestContext, ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use serde_json::json;
use tracing_subscriber::EnvFilter;

const AGENT_ADDR: &str = "127.0.0.1:18183";
const AGENT_URL: &str = "http://127.0.0.1:18183";

// ---------------------------------------------------------------------------
// Downstream: a tiny MCP server with one tool.
// ---------------------------------------------------------------------------

/// MCP server exposing a single `add` tool.
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

// ---------------------------------------------------------------------------
// Middle: A2A handler that consumes MCP tools via McpToA2ABridge.
// ---------------------------------------------------------------------------

/// Fallback handler used when the input does not look like a math request.
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
        let inner = DefaultMessageHandler::new((*self.storage).clone());
        let mut task = inner.process_message(task_id, message, session_id).await?;

        let echoed = extract_text(message);
        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("echo: {echoed}"))])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        task.status = ::buffa::MessageField::some(TaskStatus::new(TaskState::Completed, Some(response.clone())));
        task.history.push(response);
        Ok(task)
    }
}

/// Top-level A2A handler. If the incoming text parses as `add X Y`, it
/// constructs an MCP tool-call envelope and routes via the bridge to the
/// calculator. Anything else falls through to the inner echo handler.
#[derive(Clone)]
struct MathHandler {
    bridge: Arc<McpToA2ABridge<EchoHandler>>,
}

#[async_trait]
impl AsyncMessageHandler for MathHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let text = extract_text(message);
        if let Some((a, b)) = parse_add(&text) {
            let tool_msg = create_tool_call_message("add", json!({ "a": a, "b": b }));
            return self
                .bridge
                .process_message(task_id, &tool_msg, session_id)
                .await;
        }
        self.bridge
            .process_message(task_id, message, session_id)
            .await
    }
}

fn extract_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|p| p.get_text())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse `add A B` (whitespace-tolerant) into two numbers.
fn parse_add(text: &str) -> Option<(f64, f64)> {
    let mut parts = text.split_whitespace();
    if !parts.next()?.eq_ignore_ascii_case("add") {
        return None;
    }
    let a: f64 = parts.next()?.parse().ok()?;
    let b: f64 = parts.next()?.parse().ok()?;
    Some((a, b))
}

// ---------------------------------------------------------------------------
// Demo wiring.
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    // 1. Spin up the downstream MCP server (calculator) over a duplex pipe.
    let (calc_server_io, calc_client_io) = tokio::io::duplex(4096);
    let calc_server_task = tokio::spawn(async move {
        let running = CalcServer::new().serve(calc_server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });

    // 2. Connect an MCP client to the calculator and grab its peer.
    let calc_mcp_client = ().serve(calc_client_io).await?;
    let calc_peer = calc_mcp_client.peer().clone();

    // 3. Build the McpToA2ABridge: any A2A message carrying an
    //    `a2a_rs_tool_call` envelope will be routed to the calc peer.
    let storage = Arc::new(InMemoryTaskStorage::new());
    let echo = EchoHandler {
        storage: storage.clone(),
    };
    let mcp_to_a2a = Arc::new(McpToA2ABridge::new(calc_peer, echo).await?);
    let math_handler = MathHandler { bridge: mcp_to_a2a };

    // 4. Run the math handler behind an A2A HTTP server.
    let agent_info = SimpleAgentInfo::new("Math Agent".to_string(), AGENT_URL.to_string())
        .with_description("Parses 'add X Y' and computes the sum via an MCP tool.".to_string())
        .add_skill(
            "compute".to_string(),
            "Compute".to_string(),
            Some(
                "Parse natural-language math (e.g. 'add 5 7') and delegate to an MCP tool"
                    .to_string(),
            ),
        );

    let processor = DefaultRequestProcessor::new(
        math_handler,
        (*storage).clone(),
        (*storage).clone(),
        agent_info.clone(),
    );
    let server = HttpServer::new(processor, agent_info.clone(), AGENT_ADDR.to_string());

    let _server_task = tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 5. Re-expose the A2A agent as an MCP server via AgentToMcpBridge.
    let agent_card = agent_info.get_agent_card().await?;
    println!(
        "Agent card: name={}, skills={}",
        agent_card.name,
        agent_card.skills.len()
    );

    let agent_bridge = AgentToMcpBridge::new(HttpClient::new(AGENT_URL.to_string()), agent_card);

    // 6. Hook up an upstream MCP client to the AgentToMcpBridge.
    let (upstream_server_io, upstream_client_io) = tokio::io::duplex(4096);
    let upstream_bridge_task = tokio::spawn(async move {
        let running = agent_bridge.serve(upstream_server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });
    let upstream_mcp_client = ().serve(upstream_client_io).await?;
    let upstream_peer = upstream_mcp_client.peer().clone();

    // 7. List tools the upstream client sees.
    let tools = upstream_peer.list_tools(None).await?;
    println!("\nMCP tools exposed by the A2A agent:");
    for tool in &tools.tools {
        println!(
            "  - {} : {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    let compute_tool = tools
        .tools
        .iter()
        .find(|t| t.name.ends_with("compute"))
        .ok_or_else(|| anyhow::anyhow!("compute tool not exposed"))?
        .name
        .to_string();

    // 8a. Call `compute` with text that triggers an MCP tool call downstream.
    println!("\n[1] Upstream MCP call: compute('add 5 7')");
    let params = CallToolRequestParams::new(compute_tool.clone()).with_arguments(
        json!({ "message": "add 5 7" })
            .as_object()
            .cloned()
            .unwrap(),
    );
    let result = upstream_peer.call_tool(params).await?;
    for content in result.content {
        if let Some(text) = content.as_text() {
            println!("  → {}", text.text);
        }
    }

    // 8b. Call `compute` with text that does NOT match — should fall through
    //     to the echo handler, demonstrating the non-tool-call path.
    println!("\n[2] Upstream MCP call: compute('hello there')");
    let params = CallToolRequestParams::new(compute_tool).with_arguments(
        json!({ "message": "hello there" })
            .as_object()
            .cloned()
            .unwrap(),
    );
    let result = upstream_peer.call_tool(params).await?;
    for content in result.content {
        if let Some(text) = content.as_text() {
            println!("  → {}", text.text);
        }
    }

    // Clean shutdown.
    drop(upstream_mcp_client);
    let _ = upstream_bridge_task.await;
    drop(calc_mcp_client);
    let _ = calc_server_task.await;
    Ok(())
}
