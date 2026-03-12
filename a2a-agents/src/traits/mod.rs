//! Plugin traits for extending agent functionality.
//!
//! This module defines the core plugin system that allows agents to declare their
//! capabilities, provide metadata, and integrate with the framework.

pub mod plugin;
pub mod mcp_tools;

// Re-export main types
pub use plugin::{AgentPlugin, SkillDefinition};

#[cfg(feature = "mcp-client")]
pub use mcp_tools::{extract_tool_result_text, is_tool_call_successful, McpToolsExt};
