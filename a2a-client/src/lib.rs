//! # a2a-client
//!
//! Reusable Rust library for building web-based frontends for A2A (Agent-to-Agent) Protocol agents.
//!
//! ## Overview
//!
//! This library provides components and utilities for creating web applications that interact
//! with A2A protocol agents. It wraps the lower-level [`a2a-rs`](https://docs.rs/a2a-rs) clients
//! with a web-friendly API and includes ready-to-use components for common use cases.
//!
//! ## Features
//!
//! - **Unified Client API** - Single interface for both HTTP and WebSocket transports
//! - **SSE Streaming** - Server-Sent Events with automatic fallback to HTTP polling
//! - **View Models** - Ready-to-use view models for tasks and messages
//! - **Auto-reconnection** - Resilient WebSocket connections with retry logic
//! - **Type-safe** - Leverages Rust's type system for protocol correctness
//!
//! ## Quick Start
//!
//! ### Basic HTTP Client
//!
//! ```rust,no_run
//! use a2a_client::WebA2AClient;
//! use a2a_rs::domain::Message;
//! use a2a_rs::Transport;
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! // Create a client connected to your A2A agent
//! let client = WebA2AClient::new_http("http://localhost:8080".to_string());
//!
//! // Send a message
//! let message = Message::user_text("Hello, agent!".to_string(), "msg-1".to_string());
//!
//! let task = client.transport.send_task_message("task-1", &message, None, None).await?;
//! println!("Task ID: {}", task.id);
//! # Ok(())
//! # }
//! ```
//!
//! ### With SSE Streaming via ConnectRPC
//!
//! ```rust
//! use a2a_client::WebA2AClient;
//!
//! // Create client with HTTP (which handles ConnectRPC streaming automatically)
//! let client = WebA2AClient::new_http("http://localhost:8080".to_string());
//!
//! println!("HTTP client configured for streaming!");
//! ```
//!
//! ### SSE Streaming with Axum (requires `axum-components` feature)
//!
//! ```rust,ignore
//! # #[cfg(feature = "axum-components")]
//! # {
//! use a2a_client::{WebA2AClient, components::create_sse_stream};
//! use axum::{Router, routing::get, extract::{State, Path}};
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! let client = Arc::new(WebA2AClient::new_http("http://localhost:8080".to_string()));
//!
//! let app = Router::new()
//!     .route("/stream/:task_id", get(stream_handler))
//!     .with_state(client);
//!
//! // Start your Axum server...
//! # Ok(())
//! # }
//!
//! async fn stream_handler(
//!     State(client): State<Arc<WebA2AClient>>,
//!     Path(task_id): Path<String>,
//! ) -> axum::response::sse::Sse<impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
//!     create_sse_stream(client, task_id)
//! }
//! # }
//! ```
//!
//! ## Components
//!
//! - [`WebA2AClient`] - Main client wrapper for HTTP and WebSocket transports
//! - [`components::TaskView`] - View model for displaying tasks in lists
//! - [`components::MessageView`] - View model for displaying individual messages
//! - [`components::create_sse_stream`] - SSE stream creation with auto-fallback (requires `axum-components`)
//! - [`utils::formatters`] - Formatting utilities for A2A types
//!
//! ## Feature Flags
//!
//! - `axum-components` (default) - Enables Axum-specific SSE streaming components
//!
//! ## Integration
//!
//! This library integrates with:
//! - [`a2a-rs`](https://docs.rs/a2a-rs) - Core A2A protocol implementation
//! - [`a2a-agents`](https://docs.rs/a2a-agents) - Declarative agent framework
//! - Any agent implementing the A2A Protocol v1.0.0
//!
//! ## Examples
//!
//! See the `examples/` directory for complete working examples of different use cases.

pub mod components;
pub mod error;
pub mod utils;

// Re-export commonly used types
pub use error::{ClientError, Result};

use std::pin::Pin;
use std::sync::Arc;

use a2a_rs::domain::A2AError;
use a2a_rs::{HttpClient, RetryPolicy, StreamEvent, Transport, subscribe_resilient};
use futures::Stream;

/// Web-friendly A2A client that wraps both HTTP and WebSocket clients.
///
/// This is the main entry point for interacting with A2A agents from web applications.
/// It provides a unified interface for both HTTP and WebSocket transports, with automatic
/// fallback and retry logic.
///
/// # Examples
///
/// ## HTTP-only client
///
/// ```rust
/// use a2a_client::WebA2AClient;
///
/// let client = WebA2AClient::new_http("http://localhost:8080".to_string());
/// ```
///
/// ## Client configured for streaming
///
/// ```rust
/// use a2a_client::WebA2AClient;
///
/// let client = WebA2AClient::new_http("http://localhost:8080".to_string());
/// ```
///
/// ## Auto-detecting transports
///
/// ```rust,no_run
/// use a2a_client::WebA2AClient;
///
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let client = WebA2AClient::auto_connect("http://localhost:8080").await?;
/// # Ok(())
/// # }
/// ```
pub struct WebA2AClient {
    /// The negotiated transport for A2A requests and streaming.
    ///
    /// Held behind an `Arc<dyn Transport>` so the client is agnostic to the
    /// underlying wire protocol (ConnectRPC, JSON-RPC 2.0, …) and can share the
    /// transport with a reconnecting subscription stream.
    pub transport: Arc<dyn Transport>,
}

impl WebA2AClient {
    /// Create a builder for configuring the client.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::WebA2AClient;
    ///
    /// let client = WebA2AClient::builder()
    ///     .http_url("http://localhost:8080")
    ///     .build();
    /// ```
    pub fn builder() -> WebA2AClientBuilder {
        WebA2AClientBuilder::default()
    }

    /// Create a new client with HTTP transport only.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL of the A2A agent (e.g., `http://localhost:8080`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::WebA2AClient;
    ///
    /// let client = WebA2AClient::new_http("http://localhost:8080".to_string());
    /// ```
    pub fn new_http(base_url: String) -> Self {
        Self {
            transport: Arc::new(HttpClient::new(base_url)),
        }
    }

    /// Auto-connect to an agent by fetching its card and negotiating a transport.
    ///
    /// Fetches the agent card from the well-known endpoint and selects a transport
    /// from the card's `supported_interfaces` (ConnectRPC preferred, JSON-RPC 2.0
    /// as interop fallback). If the card can't be fetched or none of its interfaces
    /// match a compiled-in transport, falls back to a ConnectRPC client on
    /// `base_url` so the call still works against a bare agent URL.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL of the A2A agent
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use a2a_client::WebA2AClient;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> anyhow::Result<()> {
    /// let client = WebA2AClient::auto_connect("http://localhost:8080").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn auto_connect(base_url: &str) -> Result<Self> {
        // Validate URL format up front so a malformed URL is a hard error.
        let _ = reqwest::Url::parse(base_url).map_err(|e| ClientError::InvalidUrl {
            url: base_url.to_string(),
            reason: e.to_string(),
        })?;

        match a2a_rs::connect(base_url, &a2a_rs::default_registry()).await {
            Ok(transport) => Ok(Self {
                transport: Arc::from(transport),
            }),
            // Card fetch / negotiation failed — fall back to a direct ConnectRPC client.
            Err(_) => Ok(Self::new_http(base_url.to_string())),
        }
    }

    /// Subscribe to a task's updates as a protocol-neutral stream of
    /// [`StreamEvent`]s.
    ///
    /// This is the **spec-compliant** path: a single A2A `SubscribeToTask` round
    /// trip with no reconnection and no `Last-Event-ID` — what any A2A agent
    /// expects. For automatic reconnection (and gap-free resume against an
    /// a2a-rs server) use
    /// [`subscribe_resilient`](WebA2AClient::subscribe_resilient).
    pub async fn subscribe(
        &self,
        task_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamEvent, A2AError>> + Send>>> {
        self.transport
            .subscribe_to_task(task_id, None, None)
            .await
            .map_err(Into::into)
    }

    /// Subscribe to a task's updates with automatic reconnect + exponential
    /// backoff. The stream ends when the task reaches a terminal state (or
    /// retries are exhausted).
    ///
    /// Reconnection itself is spec-compliant (it re-issues `SubscribeToTask`).
    /// Resuming *without gaps* via `Last-Event-ID` is an **a2a-rs enhancement**
    /// beyond the A2A v1.0 spec: it works against an a2a-rs server and degrades
    /// gracefully (reconnect-from-current-state) against any spec-compliant one.
    ///
    /// This is the reusable core that [`create_sse_stream`](components::create_sse_stream)
    /// builds on; framework-agnostic, so non-Axum frontends can consume it
    /// directly.
    pub fn subscribe_resilient(
        &self,
        task_id: &str,
        policy: RetryPolicy,
    ) -> Pin<Box<dyn Stream<Item = std::result::Result<StreamEvent, A2AError>> + Send>> {
        subscribe_resilient(self.transport.clone(), task_id.to_string(), None, None, policy)
    }
}
/// Application state for Axum web applications.
///
/// This struct provides a convenient way to share the A2A client and
/// configuration across Axum route handlers.
///
/// # Examples
///
/// ```rust
/// use a2a_client::{WebA2AClient, AppState};
/// use std::sync::Arc;
///
/// let client = WebA2AClient::new_http("http://localhost:8080".to_string());
/// let state = Arc::new(
///     AppState::new(client)
///         .with_webhook_token("secret-token".to_string())
/// );
/// ```
pub struct AppState {
    /// The A2A client for interacting with agents
    pub client: WebA2AClient,
    /// Optional webhook authentication token
    pub webhook_token: Option<String>,
}

impl AppState {
    /// Create new application state with the given client.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::{WebA2AClient, AppState};
    ///
    /// let client = WebA2AClient::new_http("http://localhost:8080".to_string());
    /// let state = AppState::new(client);
    /// ```
    pub fn new(client: WebA2AClient) -> Self {
        Self {
            client,
            webhook_token: None,
        }
    }

    /// Set the webhook authentication token.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::{WebA2AClient, AppState};
    ///
    /// let client = WebA2AClient::new_http("http://localhost:8080".to_string());
    /// let state = AppState::new(client)
    ///     .with_webhook_token("secret-token".to_string());
    /// ```
    pub fn with_webhook_token(mut self, token: String) -> Self {
        self.webhook_token = Some(token);
        self
    }
}

/// Builder for [`WebA2AClient`].
///
/// Provides a fluent API for configuring the client with optional WebSocket support.
///
/// # Examples
///
/// ```rust
/// use a2a_client::WebA2AClient;
///
/// // HTTP-only client
/// let client = WebA2AClient::builder()
///     .http_url("http://localhost:8080")
///     .build();
/// ```
#[derive(Default)]
pub struct WebA2AClientBuilder {
    http_url: Option<String>,
}

impl WebA2AClientBuilder {
    /// Set the HTTP base URL.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::WebA2AClient;
    ///
    /// let client = WebA2AClient::builder()
    ///     .http_url("http://localhost:8080")
    ///     .build();
    /// ```
    pub fn http_url(mut self, url: impl Into<String>) -> Self {
        self.http_url = Some(url.into());
        self
    }

    /// Build the [`WebA2AClient`].
    ///
    /// # Panics
    ///
    /// Panics if `http_url` was not set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::WebA2AClient;
    ///
    /// let client = WebA2AClient::builder()
    ///     .http_url("http://localhost:8080")
    ///     .build();
    /// ```
    pub fn build(self) -> WebA2AClient {
        let http_url = self
            .http_url
            .expect("http_url is required for WebA2AClient");

        WebA2AClient::new_http(http_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_builder_http_only() {
        let _client = WebA2AClient::builder()
            .http_url("http://test.local")
            .build();
    }

    #[test]
    #[should_panic(expected = "http_url is required for WebA2AClient")]
    fn test_client_builder_panics_without_http() {
        let _client = WebA2AClient::builder().build();
    }

    #[test]
    fn test_app_state_creation() {
        let client = WebA2AClient::new_http("http://test.local".to_string());
        let state = AppState::new(client);
        assert!(state.webhook_token.is_none());
    }

    #[test]
    fn test_app_state_with_token() {
        let client = WebA2AClient::new_http("http://test.local".to_string());
        let state = AppState::new(client).with_webhook_token("secret".to_string());
        assert_eq!(state.webhook_token.as_deref(), Some("secret"));
    }

    #[tokio::test]
    async fn test_auto_connect_invalid_url() {
        let result = WebA2AClient::auto_connect("invalid-url-no-scheme").await;
        assert!(result.is_err());
        match result {
            Err(ClientError::InvalidUrl { url, reason }) => {
                assert_eq!(url, "invalid-url-no-scheme");
                assert!(!reason.is_empty());
            }
            _ => panic!("Expected ClientError::InvalidUrl"),
        }
    }
}
