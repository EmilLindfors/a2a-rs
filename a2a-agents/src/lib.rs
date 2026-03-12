//! A2A Agents - Framework for building A2A Protocol agents
//!
//! This crate provides a declarative, configuration-driven framework for building
//! agents that implement the A2A Protocol v0.3.0.
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
//! - [`AgentBuilder`](core::AgentBuilder) - Fluent API for agent construction
//! - [`AgentConfig`](core::AgentConfig) - TOML-based configuration
//! - [`AgentRuntime`](core::AgentRuntime) - Server lifecycle management
//!
//! # Plugin System
//!
//! Implement the [`AgentPlugin`](traits::AgentPlugin) trait to create agents that
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
//! - `default` - Includes reimbursement agent example and SQLx storage
//! - `reimbursement-agent` - Include reimbursement agent example
//! - `sqlx` - Enable SQLx-based task storage
//! - `auth` - Enable authentication features (JWT, OAuth2)

// Core framework modules
pub mod core;
pub mod traits;
pub mod utils;

// Example agent implementations
// Note: Currently public for binaries/examples, will be private in Phase 3
pub mod agents;

// Convenience re-exports for the most commonly used types
pub use core::{AgentBuilder, AgentConfig, AgentRuntime, BuildError, ConfigError, RuntimeError};
pub use traits::{AgentPlugin, SkillDefinition};

// Re-export the reimbursement agent for backward compatibility
// (This will be removed in Phase 3 when agents are extracted)
#[cfg(feature = "reimbursement-agent")]
pub use agents::reimbursement::ReimbursementHandler;
