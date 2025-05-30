//! Error types for server adapters

#[cfg(any(feature = "http-server", feature = "ws-server"))]
use std::io;

#[cfg(any(feature = "http-server", feature = "ws-server"))]
use thiserror::Error;

/// Error type for HTTP server adapter
#[derive(Error, Debug)]
#[cfg(feature = "http-server")]
pub enum HttpServerError {
    /// HTTP server error
    #[error("HTTP server error: {0}")]
    Server(String),

    /// IO error during HTTP operations
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// JSON serialization error
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// Invalid request format
    #[error("Invalid request format: {0}")]
    InvalidRequest(String),
}

/// Error type for WebSocket server adapter
#[derive(Error, Debug)]
#[cfg(feature = "ws-server")]
pub enum WebSocketServerError {
    /// WebSocket server error
    #[error("WebSocket server error: {0}")]
    Server(String),

    /// IO error during WebSocket operations
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// WebSocket connection error
    #[error("WebSocket connection error: {0}")]
    Connection(String),

    /// WebSocket message error
    #[error("WebSocket message error: {0}")]
    Message(String),

    /// JSON serialization error
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

// Conversion from adapter errors to domain errors
#[cfg(feature = "http-server")]
impl From<HttpServerError> for crate::domain::A2AError {
    fn from(error: HttpServerError) -> Self {
        match error {
            HttpServerError::Server(msg) => {
                crate::domain::A2AError::Internal(format!("HTTP server error: {}", msg))
            }
            HttpServerError::Io(e) => crate::domain::A2AError::Io(e),
            HttpServerError::Json(e) => crate::domain::A2AError::JsonParse(e),
            HttpServerError::InvalidRequest(msg) => crate::domain::A2AError::InvalidRequest(msg),
        }
    }
}

#[cfg(feature = "ws-server")]
impl From<WebSocketServerError> for crate::domain::A2AError {
    fn from(error: WebSocketServerError) -> Self {
        match error {
            WebSocketServerError::Server(msg) => {
                crate::domain::A2AError::Internal(format!("WebSocket server error: {}", msg))
            }
            WebSocketServerError::Io(e) => crate::domain::A2AError::Io(e),
            WebSocketServerError::Connection(msg) => {
                crate::domain::A2AError::Internal(format!("WebSocket connection error: {}", msg))
            }
            WebSocketServerError::Message(msg) => {
                crate::domain::A2AError::Internal(format!("WebSocket message error: {}", msg))
            }
            WebSocketServerError::Json(e) => crate::domain::A2AError::JsonParse(e),
        }
    }
}
