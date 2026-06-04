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
pub mod interceptor;
pub mod storage;
#[cfg(feature = "server")]
pub mod streaming;
pub mod transport;

// Legacy re-exports for backward compatibility
// These will be removed in a future major version

// Client re-exports (from transport)
#[cfg(feature = "http-client")]
pub use transport::http::HttpClient;
#[cfg(feature = "jsonrpc-client")]
pub use transport::jsonrpc_client::JsonRpcClient;
#[cfg(feature = "client")]
pub use transport::negotiation::{TransportFactory, TransportNegotiator, default_registry};
#[cfg(feature = "client")]
pub use transport::retry::{RetryingTransport, subscribe_resilient};
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub use transport::negotiation::{connect, fetch_agent_card};

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
pub use streaming::InMemoryStreamingHandler;
#[cfg(feature = "server")]
pub use transport::connectrpc::NoopStreamingHandler;
#[cfg(feature = "server")]
pub use transport::connectrpc::ConnectRpcAdapter;
#[cfg(feature = "http-server")]
pub use transport::http::HttpServer;
#[cfg(feature = "jsonrpc-server")]
pub use transport::jsonrpc::{JsonRpcAdapter, jsonrpc_router, rest_router};

// Interceptor re-exports
#[cfg(feature = "tracing")]
pub use interceptor::LoggingInterceptor;

// Error re-exports
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub use error::HttpClientError;
#[cfg(feature = "http-server")]
pub use error::HttpServerError;
