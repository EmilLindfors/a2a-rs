//! WebSocket transport implementations

#[cfg(feature = "ws-client")]
pub mod client;

#[cfg(feature = "ws-server")]
pub mod server;

// Re-export WebSocket implementations
#[cfg(feature = "ws-client")]
pub use client::WebSocketClient;

#[cfg(feature = "ws-server")]
pub use server::WebSocketServer;
