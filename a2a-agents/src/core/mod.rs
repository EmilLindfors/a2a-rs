//! Core framework infrastructure for building A2A agents.
//!
//! This module provides the essential building blocks for creating A2A protocol agents:
//! - [`builder`] - Declarative agent builder with fluent API
//! - [`config`] - TOML-based configuration system
//! - [`server`] - Per-agent serving lifecycle (HTTP/WS/MCP)
//!
//! # Example
//!
//! ```rust,ignore
//! use a2a_agents::core::{AgentBuilder, AgentConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     AgentBuilder::from_file("config.toml")?
//!         .with_handler(my_handler)
//!         .build_with_auto_storage()
//!         .await?
//!         .run()
//!         .await?;
//!     Ok(())
//! }
//! ```

pub mod builder;
pub mod config;
pub mod mcp;
pub mod mcp_client;
pub mod server;

// Re-export main types for convenience
pub use builder::{AgentBuilder, BuildError};
pub use config::{
    AgentConfig, Ap2ExtensionConfig, AuthConfig, ConfigError, ExtensionsConfig, HandlerConfig,
    HandlerType, LlmHandlerConfig, McpClientConfig, McpServerConfig, McpServerConnection,
    RemoteAgentConfig, ServerConfig, StorageConfig,
};
#[cfg(feature = "mcp-client")]
pub use mcp_client::{McpClientError, McpClientManager};
pub use server::{AgentServer, ServerError};
