//! Server adapters for the A2A protocol

pub mod error;

#[cfg(feature = "server")]
pub mod agent_info;
#[cfg(feature = "http-server")]
pub mod auth;
#[cfg(feature = "http-server")]
pub mod http;
#[cfg(feature = "server")]
pub mod push_notification;
#[cfg(feature = "server")]
pub mod request_processor;
#[cfg(feature = "server")]
pub mod task_storage;
#[cfg(all(test, feature = "server"))]
mod tests;
#[cfg(feature = "ws-server")]
pub mod ws;

// Re-export server implementations
#[cfg(feature = "server")]
pub use agent_info::SimpleAgentInfo;
#[cfg(feature = "http-server")]
pub use auth::{Authenticator, NoopAuthenticator, TokenAuthenticator, with_auth};
#[cfg(feature = "http-server")]
pub use http::HttpServer;
#[cfg(feature = "server")]
pub use push_notification::{NoopPushNotificationSender, PushNotificationRegistry, PushNotificationSender};

#[cfg(all(feature = "server", feature = "http-client"))]
pub use push_notification::HttpPushNotificationSender;
#[cfg(feature = "server")]
pub use request_processor::DefaultRequestProcessor;
#[cfg(feature = "server")]
pub use task_storage::InMemoryTaskStorage;
#[cfg(feature = "ws-server")]
pub use ws::WebSocketServer;

// Re-export error types
#[cfg(feature = "http-server")]
pub use error::HttpServerError;
#[cfg(feature = "ws-server")]
pub use error::WebSocketServerError;
