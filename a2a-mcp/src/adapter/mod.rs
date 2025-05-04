//! Adapters for converting between A2A and RMCP

mod tool_to_agent;
mod agent_to_tool;

pub use tool_to_agent::ToolToAgentAdapter;
pub use agent_to_tool::AgentToToolAdapter;