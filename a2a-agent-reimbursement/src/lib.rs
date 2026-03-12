//! A2A Reimbursement Agent
//!
//! An expense reimbursement agent built on the A2A Protocol v0.3.0.
//!
//! # Features
//!
//! - Interactive conversation flow for collecting reimbursement information
//! - AI-powered natural language responses using OpenAI-compatible APIs
//! - Structured data validation
//! - SQLite storage for task persistence
//! - WebSocket and HTTP transport support
//!
//! # Quick Start
//!
//! ```no_run
//! use a2a_agent_reimbursement::ReimbursementHandler;
//! use a2a_agents::core::AgentBuilder;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let handler = ReimbursementHandler::new();
//!
//!     AgentBuilder::from_file("reimbursement.toml")?
//!         .with_handler(handler)
//!         .build_with_auto_storage()
//!         .await?
//!         .run()
//!         .await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Configuration
//!
//! Create a `reimbursement.toml` file:
//!
//! ```toml
//! [agent]
//! name = "Reimbursement Agent"
//! description = "Helps users submit expense reimbursement requests"
//! base_url = "http://localhost:8080"
//!
//! [server]
//! http_port = 8080
//! ws_port = 8081
//!
//! [server.storage]
//! type = "sqlx"
//! url = "${DATABASE_URL}"
//! ```
//!
//! # Environment Variables
//!
//! - `DATABASE_URL` - Database connection string (for SQLx storage)
//! - `OPENAI_API_KEY` - API key for AI responses
//! - `OPENAI_API_BASE` - Base URL for OpenAI-compatible API (optional)

pub mod ai_client;
pub mod config;
pub mod handler;
pub mod plugin;
pub mod server;
pub mod types;

// Re-export key types for convenience
pub use handler::ReimbursementHandler;
pub use types::{ReimbursementRequest, ProcessingStatus, ExpenseCategory};

// Re-export core framework types for convenience
pub use a2a_agents::core::{AgentBuilder, AgentConfig};
