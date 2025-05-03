//! Ports (interfaces) for the A2A protocol

pub mod client;
pub mod server;

// Re-export key traits for convenience
pub use client::A2AClient;

#[cfg(feature = "client")]
pub use client::{AsyncA2AClient, StreamItem};

pub use server::{A2ARequestProcessor, TaskHandler};

#[cfg(feature = "server")]
pub use server::{AgentInfoProvider, AsyncA2ARequestProcessor, AsyncTaskHandler, Subscriber};
