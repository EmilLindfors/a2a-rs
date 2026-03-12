//! Bridge implementations for A2A-MCP integration

pub mod agent_to_mcp;
pub mod mcp_to_a2a;

pub use agent_to_mcp::AgentToMcpBridge;
pub use mcp_to_a2a::McpToA2ABridge;
