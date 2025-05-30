//! Service layer for the A2A protocol
//!
//! Services provide application-level abstractions that orchestrate
//! between ports and adapters.

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub use client::{AsyncA2AClient, StreamItem};

#[cfg(feature = "server")]
pub use server::{AgentInfoProvider, AsyncA2ARequestProcessor};
