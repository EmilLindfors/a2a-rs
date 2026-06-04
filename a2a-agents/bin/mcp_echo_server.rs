//! Minimal MCP **server** over stdio — a fixture for the `mcp-client` story.
//!
//! Exposes two tools, `echo` and `add`, over MCP's stdio transport. It exists
//! so the [`mcp_client_agent`](../examples/mcp_client_agent.rs) example and the
//! `mcp_client_test` integration test have a real MCP server to spawn as a
//! child process — no external dependencies (npx, Node, …) required.
//!
//! Run it directly to poke at it with an MCP inspector:
//!
//! ```bash
//! cargo run -p a2a-agents --features mcp-client --bin mcp_echo_server
//! ```
//!
//! Everything it logs goes to **stderr** — stdout is reserved for the MCP wire
//! protocol, so writing anything else there would corrupt the stream.

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, Content, Implementation, JsonObject,
        ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo,
        Tool,
    },
    service::RequestContext,
    transport::stdio,
};
use serde_json::json;
use std::sync::Arc;

/// An MCP server exposing `echo` and `add`.
#[derive(Clone)]
struct EchoServer {
    tools: Arc<Vec<Tool>>,
}

impl EchoServer {
    fn new() -> Self {
        let echo_arg: Arc<JsonObject> = Arc::new(
            serde_json::from_value(json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }))
            .expect("valid JSON schema"),
        );
        let number_pair: Arc<JsonObject> = Arc::new(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                },
                "required": ["a", "b"]
            }))
            .expect("valid JSON schema"),
        );
        let tools = vec![
            Tool::new("echo", "Echo back the provided text", echo_arg),
            Tool::new("add", "Add two numbers a + b", number_pair),
        ];
        Self {
            tools: Arc::new(tools),
        }
    }
}

// rmcp's `ServerHandler` methods are RPITIT (`impl Future`), so they're written
// in that form here rather than with `async fn`.
#[allow(clippy::manual_async_fn)]
impl ServerHandler for EchoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_server_info(Implementation::new("mcp-echo-server", "0.1.0"))
            .with_instructions("Echo and arithmetic tools for the a2a-agents mcp-client example")
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
        CallToolRequestParams { name, arguments, .. }: CallToolRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            let args = arguments.unwrap_or_default();
            let text = match name.as_ref() {
                "echo" => args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| McpError::invalid_params("missing 'text'", None))?
                    .to_string(),
                "add" => {
                    let a = number_arg(&args, "a")?;
                    let b = number_arg(&args, "b")?;
                    (a + b).to_string()
                }
                other => {
                    return Err(McpError::invalid_params(
                        format!("unknown tool: {other}"),
                        None,
                    ));
                }
            };
            Ok(CallToolResult::success(vec![Content::text(text)]))
        }
    }
}

fn number_arg(args: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<f64, McpError> {
    args.get(key)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| McpError::invalid_params(format!("missing or non-numeric '{key}'"), None))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs to stderr only — stdout carries the MCP protocol.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("mcp_echo_server starting on stdio");
    let running = EchoServer::new().serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}
