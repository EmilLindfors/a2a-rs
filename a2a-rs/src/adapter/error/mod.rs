//! Error types for adapter implementations

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

// Re-export client error types
#[cfg(feature = "http-client")]
pub use client::HttpClientError;
#[cfg(feature = "ws-client")]
pub use client::WebSocketClientError;

// Re-export server error types
#[cfg(feature = "http-server")]
pub use server::HttpServerError;
#[cfg(feature = "ws-server")]
pub use server::WebSocketServerError;
