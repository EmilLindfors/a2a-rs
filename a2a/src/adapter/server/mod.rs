//! Server adapters for the A2A protocol

#[cfg(feature = "http-server")]
pub mod http;
#[cfg(feature = "ws-server")]
pub mod ws;
#[cfg(feature = "server")]
pub mod task_storage;
#[cfg(feature = "server")]
pub mod request_processor;
#[cfg(feature = "server")]
pub mod agent_info;

// Re-export server implementations
#[cfg(feature = "http-server")]
pub use http::HttpServer;
#[cfg(feature = "ws-server")]
pub use ws::WebSocketServer;
#[cfg(feature = "server")]
pub use task_storage::InMemoryTaskStorage;
#[cfg(feature = "server")]
pub use request_processor::DefaultRequestProcessor;
#[cfg(feature = "server")]
pub use agent_info::SimpleAgentInfo;