//! Error types for adapter implementations

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

// Re-export client error types
#[cfg(feature = "http-client")]
pub use client::HttpClientError;

// Re-export server error types
#[cfg(feature = "http-server")]
pub use server::HttpServerError;
