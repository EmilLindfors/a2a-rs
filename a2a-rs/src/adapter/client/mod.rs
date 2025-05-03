//! Client adapters for the A2A protocol

pub mod error;

#[cfg(feature = "http-client")]
pub mod http;
#[cfg(feature = "ws-client")]
pub mod ws;

// Re-export client implementations
#[cfg(feature = "http-client")]
pub use http::HttpClient;
#[cfg(feature = "ws-client")]
pub use ws::WebSocketClient;

// Re-export error types
#[cfg(feature = "http-client")]
pub use error::HttpClientError;
#[cfg(feature = "ws-client")]
pub use error::WebSocketClientError;
