//! Core framework infrastructure for building A2A agents.
//!
//! This module provides the essential building blocks for creating A2A protocol agents:
//! - [`builder`] - Declarative agent builder with fluent API
//! - [`config`] - TOML-based configuration system
//! - [`doctor`] - What a config needs from its host (`a2a doctor`)
//! - [`fleet`] - A set of agents run together (`a2a up`)
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
pub mod doctor;
pub mod fleet;
pub mod mcp;
pub mod mcp_client;
pub mod server;
pub mod template;

// Re-export main types for convenience
pub use builder::{AgentBuilder, BuildError};
pub use config::{
    AgentConfig, Ap2ExtensionConfig, AuthConfig, ConfigError, ExtensionsConfig, HandlerConfig,
    HandlerType, LlmHandlerConfig, McpClientConfig, McpServerConfig, McpServerConnection,
    RemoteAgentConfig, ServerConfig, StorageConfig, referenced_env_vars,
};
pub use doctor::{Requirement, requirements};
pub use fleet::{
    FleetConfig, FleetConflict, FleetMember, fleet_conflicts, fleet_header, member_block,
    member_path,
};
#[cfg(feature = "mcp-client")]
pub use mcp_client::{McpClientError, McpClientManager};
pub use server::{AgentServer, ServerError};
pub use template::AgentTemplate;
