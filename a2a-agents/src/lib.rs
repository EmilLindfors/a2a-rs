//! A2A Agents - Framework for building A2A Protocol agents
//!
//! This crate provides a declarative, configuration-driven framework for building
//! agents that implement the A2A Protocol v1.0.0.
//!
//! # Architecture
//!
//! The crate is organized into three main layers:
//!
//! - **Core Framework** ([`core`]) - Builder, configuration, and runtime
//! - **Plugin System** ([`traits`]) - Traits for extending agent functionality
//! - **Utilities** ([`utils`]) - Common helpers for agent development
//! - **Example Agents** ([`agents`]) - Reference implementations
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use a2a_agents::core::AgentBuilder;
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
//!
//! # Core Framework
//!
//! The core framework provides the essential building blocks:
//!
//! - [`AgentBuilder`] - Fluent API for agent construction
//! - [`AgentConfig`] - TOML-based configuration
//! - [`AgentServer`] - Per-agent serving lifecycle
//! - [`AgentRuntime`](runtime::AgentRuntime) - Fleet supervision (provision/start/stop/health)
//!
//! # Plugin System
//!
//! Implement the [`AgentPlugin`] trait to create agents that
//! integrate seamlessly with the framework:
//!
//! ```rust
//! use a2a_agents::traits::{AgentPlugin, SkillDefinition};
//! use a2a_rs::port::AsyncMessageHandler;
//! use a2a_rs::domain::{A2AError, Message, Task};
//! use async_trait::async_trait;
//!
//! #[derive(Clone)]
//! struct MyAgent;
//!
//! impl AgentPlugin for MyAgent {
//!     fn name(&self) -> &str { "My Agent" }
//!     fn description(&self) -> &str { "An example agent" }
//!     fn skills(&self) -> Vec<SkillDefinition> { vec![] }
//! }
//!
//! #[async_trait]
//! impl AsyncMessageHandler for MyAgent {
//!     async fn process_message(
//!         &self,
//!         _task_id: &str,
//!         _message: &Message,
//!         _session_id: Option<&str>,
//!     ) -> Result<Task, A2AError> {
//!         todo!()
//!     }
//! }
//! ```
//!
//! # Features
//!
//! - `default` - What the `a2a` CLI needs: `llm`, `mcp-server`, `schema`, `sqlx`
//! - `llm` - Generic config-driven LLM handler and its tool-calling loop
//! - `mcp-server` - Expose a configured agent as an MCP server; MCP tool sources
//! - `mcp-client` - Call out to external MCP servers from an agent
//! - `schema` - JSON Schema export for `AgentConfig` (`a2a print-schema`)
//! - `sqlx` - Enable SQLx-based task storage
//! - `auth` - Enable authentication features (JWT, OAuth2)
//! - `reimbursement-agent` - Build the reimbursement sample agent (opt-in)
//! - `ap2` - Agent Payments Protocol types
//!
//! Library-only consumers can take `default-features = false` and enable just
//! what they use; the defaults exist so `cargo install a2a-agents` yields a
//! working `a2a` binary.

// Core framework modules
pub mod core;
pub mod traits;
pub mod utils;

/// Generic config-driven handlers.
pub mod handlers;

/// Agent registry / discovery — find peers by skill instead of hard-coded URLs.
pub mod registry;

/// Agent runtime — run agents as managed, isolatable units (provision/start/stop/health).
pub mod runtime;

/// Control plane — compose runtime + registry into a deployable platform with an HTTP API.
pub mod control_plane;

// Example agent implementations
// Note: public for binaries/examples; intended to become private once agents
// are extracted into their own crates.
pub mod agents;

// Convenience re-exports for the most commonly used types
pub use core::{AgentBuilder, AgentConfig, AgentServer, BuildError, ConfigError, ServerError};
pub use traits::{AgentPlugin, SkillDefinition};

pub use handlers::tools::{A2aAgentToolSource, ToolSource};

pub use registry::{
    AgentId, AgentRegistry, CardSource, CardSourceError, HttpCardSource, InMemoryAgentRegistry,
    InMemoryCardSource, RegisteredAgent, RegistryError,
};

pub use runtime::{
    AgentRuntime, AgentSpec, ContainerHardening, ContainerRuntime, EnvAllowlist,
    InMemoryAgentRuntime, LocalProcessRuntime, Recovered, RuntimeError, RuntimeHealth,
    RuntimeStatus,
};

pub use control_plane::{
    AgentLogs, AgentStatus, ControlPlane, ControlPlaneAuth, ControlPlaneClient,
    ControlPlaneClientError, ControlPlaneError, DeployedAgent, ListFilter, control_plane_router,
};

#[cfg(feature = "llm")]
pub use handlers::llm::LlmHandler;
#[cfg(feature = "mcp-server")]
pub use handlers::tools::{McpToolSource, UnusedInner};

// Re-export the reimbursement agent as a convenience
// (intended to be removed once agents are extracted into their own crates)
#[cfg(feature = "reimbursement-agent")]
pub use agents::reimbursement::ReimbursementHandler;
