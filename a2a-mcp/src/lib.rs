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
//! ```rust,ignore
//! use a2a_mcp::{AgentToMcpBridge, Result};
//! use a2a_rs::services::client::A2AClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create A2A client
//!     let client = A2AClient::new("https://my-agent.example.com");
//!     let agent_card = client.get_agent_card().await?;
//!
//!     // Create MCP bridge
//!     let bridge = AgentToMcpBridge::new(client, agent_card, "https://my-agent.example.com".to_string());
//!
//!     // Serve as MCP server via stdio
//!     use rmcp::{ServiceExt, transport::stdio};
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
//! ```rust,ignore
//! use a2a_mcp::{McpToA2ABridge, create_tool_call_message};
//! use a2a_rs::port::AsyncMessageHandler;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Connect to MCP server
//!     use rmcp::{ServiceExt, transport::TokioChildProcess};
//!     use tokio::process::Command;
//!
//!     let mcp_client = ().serve(TokioChildProcess::new(
//!         Command::new("mcp-server-binary")
//!     )?).await?;
//!
//!     // Wrap your existing A2A handler with MCP tools
//!     let handler = MyA2AHandler::new();
//!     let augmented_handler = McpToA2ABridge::new(mcp_client, handler).await?;
//!
//!     // Use augmented handler in your A2A server
//!     // Now messages can call MCP tools using: "TOOL_CALL: tool_name"
//!     Ok(())
//! }
//! ```
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
pub use bridge::mcp_to_a2a::create_tool_call_message;
pub use bridge::{AgentToMcpBridge, McpToA2ABridge};
pub use converters::{MessageConverter, SkillToolConverter, TaskResultConverter};
pub use error::{A2aMcpError, Result};

/// Current crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
