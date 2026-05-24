//! # A2A-MCP Integration
//!
//! This crate provides **bidirectional integration** between the Agent-to-Agent (A2A) protocol
//! and the Model Context Protocol (MCP), enabling seamless communication between these protocols.
//!
//! ## Core Features
//!
//! ### 1. A2A Agents → MCP Tools (`AgentToMcpBridge`)
//!
//! Expose A2A agent skills as callable MCP tools, allowing MCP clients (like Claude Desktop)
//! to invoke A2A agent capabilities.
//!
//! ```no_run
//! use a2a_mcp::AgentToMcpBridge;
//! use a2a_rs::adapter::transport::http::HttpClient;
//! use a2a_rs::domain::AgentCard;
//! use rmcp::{transport::stdio, ServiceExt};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Point an A2A client at the agent's HTTP endpoint.
//!     let client = HttpClient::new("https://my-agent.example.com".to_string());
//!
//!     // The agent card describes the agent's skills. In a real client
//!     // you'll fetch it from `/.well-known/agent-card.json`; this is a
//!     // stand-in to keep the example self-contained.
//!     let agent_card: AgentCard = AgentCard::builder()
//!         .name("My Agent".to_string())
//!         .description("Does things".to_string())
//!         .url("https://my-agent.example.com".to_string())
//!         .version("0.1.0".to_string())
//!         .capabilities(Default::default())
//!         .default_input_modes(vec!["text".to_string()])
//!         .default_output_modes(vec!["text".to_string()])
//!         .skills(vec![])
//!         .build();
//!
//!     // MCP tool names are namespaced by agent_card.url.
//!     let bridge = AgentToMcpBridge::new(client, agent_card);
//!
//!     // Serve as an MCP server over stdio.
//!     bridge.serve(stdio()).await?.waiting().await?;
//!     Ok(())
//! }
//! ```
//!
//! ### 2. MCP Tools → A2A Agents (`McpToA2ABridge`)
//!
//! Augment A2A agents with MCP tool capabilities, allowing agents to call external MCP tools
//! as part of their processing.
//!
//! ```no_run
//! use a2a_mcp::{create_tool_call_message, McpToA2ABridge};
//! use a2a_rs::domain::{error::A2AError, Message, Task};
//! use a2a_rs::port::AsyncMessageHandler;
//! use async_trait::async_trait;
//! use rmcp::{transport::stdio, ServiceExt};
//!
//! // Your existing A2A handler — the bridge wraps it so non-tool-call
//! // messages keep flowing through your normal business logic.
//! #[derive(Clone)]
//! struct MyHandler;
//!
//! #[async_trait]
//! impl AsyncMessageHandler for MyHandler {
//!     async fn process_message(
//!         &self,
//!         _task_id: &str,
//!         _message: &Message,
//!         _session_id: Option<&str>,
//!     ) -> Result<Task, A2AError> {
//!         unimplemented!("your business logic here")
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Connect to an MCP server. In production you'll typically use
//!     // `rmcp::transport::TokioChildProcess` to spawn one; stdio works
//!     // when this process is itself the MCP client end of a pipe.
//!     let mcp_client = ().serve(stdio()).await?;
//!
//!     // Wrap your handler so messages carrying an `a2a_rs_tool_call`
//!     // metadata envelope are routed to the MCP server.
//!     let bridge = McpToA2ABridge::new(mcp_client.peer().clone(), MyHandler).await?;
//!
//!     // Build a tool-call message. The envelope rides in metadata, not text:
//!     //   metadata["a2a_rs_tool_call"] = { "name": "...", "arguments": {...} }
//!     let tool_msg = create_tool_call_message("add", serde_json::json!({"a": 5, "b": 7}));
//!     let _result = bridge.process_message("task-1", &tool_msg, None).await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Tool-call wire format
//!
//! `McpToA2ABridge` does not inspect message text. To trigger a tool call,
//! attach an [`McpToolCall`] envelope to `Message.metadata` under the
//! [`MCP_TOOL_CALL_METADATA_KEY`] key (`"a2a_rs_tool_call"`):
//!
//! ```text
//! Message {
//!   role: User,
//!   metadata: {
//!     "a2a_rs_tool_call": { "name": "calculator_add", "arguments": {"a":5,"b":3} }
//!   },
//!   ...
//! }
//! ```
//!
//! Messages without this metadata key are forwarded unchanged to the inner
//! `AsyncMessageHandler`. Use [`create_tool_call_message`] or
//! [`attach_tool_call`] to construct one without touching the constant by hand.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      a2a-mcp Crate                          │
//! ├──────────────────────┬──────────────────────────────────────┤
//! │  Direction 1:        │  Direction 2:                        │
//! │  A2A → MCP           │  MCP → A2A                           │
//! ├──────────────────────┼──────────────────────────────────────┤
//! │  AgentToMcpBridge    │  McpToA2ABridge                      │
//! │  - Wraps A2A agent   │  - Wraps MCP ServerHandler           │
//! │  - Implements        │  - Implements AsyncMessageHandler    │
//! │    ServerHandler     │  - Calls MCP tools from A2A tasks    │
//! │  - Maps skills to    │  - Augments agents with MCP tools    │
//! │    MCP tools         │                                      │
//! └──────────────────────┴──────────────────────────────────────┘
//! ```
//!
//! ## Protocol Converters
//!
//! The crate provides transparent conversion between A2A and MCP types:
//!
//! - **Messages**: `A2A Message` ↔ `MCP Content`
//! - **Skills/Tools**: `A2A AgentSkill` ↔ `MCP Tool`
//! - **Results**: `A2A Task` ↔ `MCP CallToolResult`

pub mod bridge;
pub mod converters;
pub mod error;

// Re-export key types
pub use bridge::mcp_to_a2a::{
    attach_tool_call, create_tool_call_message, McpToolCall, MCP_TOOL_CALL_METADATA_KEY,
};
pub use bridge::{AgentToMcpBridge, McpToA2ABridge};
pub use converters::{MessageConverter, SkillToolConverter, TaskResultConverter};
pub use error::{A2aMcpError, Result};

/// Current crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
