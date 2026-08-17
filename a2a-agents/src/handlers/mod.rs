//! Generic config-driven handlers.

/// Tool sources for the LLM handler (MCP servers + A2A agents as tools).
pub mod tools;

#[cfg(feature = "llm")]
pub mod context;
pub mod llm;
/// The state bag's two tools, and how it is worded in the prompt.
#[cfg(feature = "llm")]
pub mod memory;
