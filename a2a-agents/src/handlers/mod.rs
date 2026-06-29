//! Generic config-driven handlers.

/// Tool sources for the LLM handler (MCP servers + A2A agents as tools).
pub mod tools;

#[cfg(feature = "llm")]
pub mod llm;
