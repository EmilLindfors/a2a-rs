//! Client adapters for the A2A protocol

#[cfg(feature = "http-client")]
pub mod http;
#[cfg(feature = "ws-client")]
pub mod ws;

// Re-export client implementations
#[cfg(feature = "http-client")]
pub use http::HttpClient;
#[cfg(feature = "ws-client")]
pub use ws::WebSocketClient;