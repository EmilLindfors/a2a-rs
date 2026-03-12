//! Plugin traits for extending agent functionality.
//!
//! This module defines the core plugin system that allows agents to declare their
//! capabilities, provide metadata, and integrate with the framework.

pub mod mcp_tools;
pub mod plugin;

// Re-export main types
pub use plugin::{AgentPlugin, SkillDefinition};

#[cfg(feature = "mcp-client")]
pub use mcp_tools::{McpToolsExt, extract_tool_result_text, is_tool_call_successful};
