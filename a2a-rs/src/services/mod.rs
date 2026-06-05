//! Service layer for the A2A protocol
//!
//! Services provide application-level abstractions that orchestrate
//! between ports and adapters.

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "server")]
pub use server::AgentInfoProvider;
