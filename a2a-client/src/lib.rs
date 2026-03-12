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
//! ```rust,ignore
//! use a2a_client::WebA2AClient;
//! use a2a_rs::domain::Message;
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! // Create a client connected to your A2A agent
//! let client = WebA2AClient::new_http("http://localhost:8080".to_string());
//!
//! // Send a message
//! let message = Message::builder()
//!     .text("Hello, agent!")
//!     .build();
//!
//! let task = client.http.send_message(&message, None).await?;
//! println!("Task ID: {}", task.id);
//! # Ok(())
//! # }
//! ```
//!
//! ### With WebSocket Support
//!
//! ```rust,ignore
//! use a2a_client::WebA2AClient;
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! // Create client with both HTTP and WebSocket
//! let client = WebA2AClient::new_with_websocket(
//!     "http://localhost:8080".to_string(),
//!     "ws://localhost:8080/ws".to_string()
//! );
//!
//! if client.has_websocket() {
//!     println!("WebSocket support available!");
//! }
//! # Ok(())
//! # }
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
//! - Any agent implementing the A2A Protocol v0.3.0
//!
//! ## Examples
//!
//! See the `examples/` directory for complete working examples of different use cases.

pub mod components;
pub mod error;
pub mod utils;

// Re-export commonly used types
pub use error::{ClientError, Result};

use a2a_rs::{HttpClient, WebSocketClient};
use std::sync::Arc;

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
/// ## Client with WebSocket support
///
/// ```rust
/// use a2a_client::WebA2AClient;
///
/// let client = WebA2AClient::new_with_websocket(
///     "http://localhost:8080".to_string(),
///     "ws://localhost:8080/ws".to_string()
/// );
/// ```
///
/// ## Auto-detecting transports
///
/// ```rust,ignore
/// use a2a_client::WebA2AClient;
///
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let client = WebA2AClient::auto_connect("http://localhost:8080").await?;
/// # Ok(())
/// # }
/// ```
pub struct WebA2AClient {
    /// HTTP client for JSON-RPC requests
    pub http: HttpClient,
    /// Optional WebSocket client for streaming updates
    pub ws: Option<Arc<WebSocketClient>>,
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
            http: HttpClient::new(base_url),
            ws: None,
        }
    }

    /// Create a new client with both HTTP and WebSocket transports.
    ///
    /// # Arguments
    ///
    /// * `http_url` - HTTP base URL (e.g., `http://localhost:8080`)
    /// * `ws_url` - WebSocket URL (e.g., `ws://localhost:8080/ws`)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::WebA2AClient;
    ///
    /// let client = WebA2AClient::new_with_websocket(
    ///     "http://localhost:8080".to_string(),
    ///     "ws://localhost:8080/ws".to_string()
    /// );
    /// ```
    pub fn new_with_websocket(http_url: String, ws_url: String) -> Self {
        Self {
            http: HttpClient::new(http_url),
            ws: Some(Arc::new(WebSocketClient::new(ws_url))),
        }
    }

    /// Auto-connect to an agent, attempting to detect available transports.
    ///
    /// Currently defaults to HTTP-only. In the future, this will probe for
    /// WebSocket support by checking the agent card.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL of the A2A agent
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use a2a_client::WebA2AClient;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> anyhow::Result<()> {
    /// let client = WebA2AClient::auto_connect("http://localhost:8080").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn auto_connect(base_url: &str) -> anyhow::Result<Self> {
        // For now, just use HTTP
        // TODO: Try to detect WebSocket support by fetching agent card
        Ok(Self::new_http(base_url.to_string()))
    }

    /// Check if WebSocket transport is available.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::WebA2AClient;
    ///
    /// let client = WebA2AClient::new_http("http://localhost:8080".to_string());
    /// assert!(!client.has_websocket());
    ///
    /// let client = WebA2AClient::new_with_websocket(
    ///     "http://localhost:8080".to_string(),
    ///     "ws://localhost:8080/ws".to_string()
    /// );
    /// assert!(client.has_websocket());
    /// ```
    pub fn has_websocket(&self) -> bool {
        self.ws.is_some()
    }

    /// Get a reference to the WebSocket client if available.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::WebA2AClient;
    ///
    /// let client = WebA2AClient::new_with_websocket(
    ///     "http://localhost:8080".to_string(),
    ///     "ws://localhost:8080/ws".to_string()
    /// );
    ///
    /// if let Some(ws_client) = client.websocket() {
    ///     // Use WebSocket client
    /// }
    /// ```
    pub fn websocket(&self) -> Option<&Arc<WebSocketClient>> {
        self.ws.as_ref()
    }
}

/// Application state for Axum web applications.
///
/// This struct provides a convenient way to share the A2A client and
/// configuration across Axum route handlers.
///
/// # Examples
///
/// ```rust,ignore
/// use a2a_client::{WebA2AClient, AppState};
/// use axum::{Router, routing::get};
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> anyhow::Result<()> {
/// let client = WebA2AClient::new_http("http://localhost:8080".to_string());
/// let state = Arc::new(
///     AppState::new(client)
///         .with_webhook_token("secret-token".to_string())
/// );
///
/// let app = Router::new()
///     .route("/", get(|| async { "Hello!" }))
///     .with_state(state);
/// # Ok(())
/// # }
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
///
/// // Client with WebSocket support
/// let client = WebA2AClient::builder()
///     .http_url("http://localhost:8080")
///     .ws_url("ws://localhost:8080/ws")
///     .build();
/// ```
#[derive(Default)]
pub struct WebA2AClientBuilder {
    http_url: Option<String>,
    ws_url: Option<String>,
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

    /// Set the WebSocket URL.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use a2a_client::WebA2AClient;
    ///
    /// let client = WebA2AClient::builder()
    ///     .http_url("http://localhost:8080")
    ///     .ws_url("ws://localhost:8080/ws")
    ///     .build();
    /// ```
    pub fn ws_url(mut self, url: impl Into<String>) -> Self {
        self.ws_url = Some(url.into());
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

        match self.ws_url {
            Some(ws_url) => WebA2AClient::new_with_websocket(http_url, ws_url),
            None => WebA2AClient::new_http(http_url),
        }
    }
}
