//! Error types for server adapters

use std::io;
use thiserror::Error;

use crate::domain::A2AError;

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
impl From<HttpServerError> for A2AError {
    fn from(error: HttpServerError) -> Self {
        match error {
            HttpServerError::Server(msg) => {
                A2AError::Internal(format!("HTTP server error: {}", msg))
            }
            HttpServerError::Io(e) => A2AError::Io(e),
            HttpServerError::Json(e) => A2AError::JsonParse(e),
            HttpServerError::InvalidRequest(msg) => A2AError::InvalidRequest(msg),
        }
    }
}

#[cfg(feature = "ws-server")]
impl From<WebSocketServerError> for A2AError {
    fn from(error: WebSocketServerError) -> Self {
        match error {
            WebSocketServerError::Server(msg) => {
                A2AError::Internal(format!("WebSocket server error: {}", msg))
            }
            WebSocketServerError::Io(e) => A2AError::Io(e),
            WebSocketServerError::Connection(msg) => {
                A2AError::Internal(format!("WebSocket connection error: {}", msg))
            }
            WebSocketServerError::Message(msg) => {
                A2AError::Internal(format!("WebSocket message error: {}", msg))
            }
            WebSocketServerError::Json(e) => A2AError::JsonParse(e),
        }
    }
}
