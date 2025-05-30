//! HTTP transport implementations

#[cfg(feature = "http-client")]
pub mod client;

#[cfg(feature = "http-server")]
pub mod server;

// Re-export HTTP implementations
#[cfg(feature = "http-client")]
pub use client::HttpClient;

#[cfg(feature = "http-server")]
pub use server::HttpServer;
