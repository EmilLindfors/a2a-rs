//! Adapters for the A2A protocol
//!
//! This module provides concrete implementations of the port interfaces,
//! organized by concern:
//!
//! - `transport`: Protocol-specific implementations (HTTP, WebSocket)
//! - `business`: Business logic implementations  
//! - `storage`: Data persistence implementations
//! - `auth`: Authentication and authorization implementations
//! - `error`: Error types for all adapters

pub mod auth;
pub mod business;
pub mod error;
pub mod storage;
pub mod transport;

// Legacy re-exports for backward compatibility
// These will be removed in a future major version

// Client re-exports (from transport)
#[cfg(feature = "http-client")]
pub use transport::http::HttpClient;

// Server re-exports (from various modules)
#[cfg(feature = "http-server")]
pub use auth::with_auth;
#[cfg(feature = "http-server")]
pub use auth::{ApiKeyAuthenticator, BearerTokenAuthenticator, NoopAuthenticator};
#[cfg(feature = "auth")]
pub use auth::{JwtAuthenticator, OAuth2Authenticator, OpenIdConnectAuthenticator};
#[cfg(all(feature = "server", feature = "http-client"))]
pub use business::HttpPushNotificationSender;
#[cfg(feature = "server")]
pub use business::SimpleAgentInfo;
#[cfg(feature = "server")]
pub use business::{NoopPushNotificationSender, PushNotificationRegistry, PushNotificationSender};
#[cfg(feature = "server")]
pub use storage::InMemoryTaskStorage;
#[cfg(feature = "server")]
pub use transport::connectrpc::ConnectRpcAdapter;
#[cfg(feature = "http-server")]
pub use transport::http::HttpServer;

// Error re-exports
#[cfg(feature = "http-client")]
pub use error::HttpClientError;
#[cfg(feature = "http-server")]
pub use error::HttpServerError;
