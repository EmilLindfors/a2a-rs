//! Error types for adapter implementations

#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

// Re-export client error types
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub use client::HttpClientError;

// Re-export server error types
#[cfg(feature = "http-server")]
pub use server::HttpServerError;
