//! Error types for client adapters

use crate::domain::A2AError;
use std::io;
use thiserror::Error;

/// Error type for HTTP client adapter
#[derive(Error, Debug)]
#[cfg(feature = "http-client")]
pub enum HttpClientError {
    /// Reqwest client error
    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),

    /// IO error during HTTP operations
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Error during request processing
    #[error("Request error: {0}")]
    Request(String),

    /// Error with HTTP response
    #[error("Response error: {status} - {message}")]
    Response { status: u16, message: String },

    /// Connection timeout
    #[error("Connection timeout")]
    Timeout,
}

/// Error type for WebSocket client adapter
#[derive(Error, Debug)]
#[cfg(feature = "ws-client")]
pub enum WebSocketClientError {
    /// WebSocket connection error
    #[error("WebSocket connection error: {0}")]
    Connection(String),

    /// WebSocket message error
    #[error("WebSocket message error: {0}")]
    Message(String),

    /// IO error during WebSocket operations
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// WebSocket protocol error
    #[error("WebSocket protocol error: {0}")]
    Protocol(String),

    /// Connection timeout
    #[error("Connection timeout")]
    Timeout,

    /// Connection closed
    #[error("Connection closed")]
    Closed,
}

// Conversion from adapter errors to domain errors
#[cfg(feature = "http-client")]
impl From<HttpClientError> for A2AError {
    fn from(error: HttpClientError) -> Self {
        match error {
            HttpClientError::Reqwest(e) => A2AError::Internal(format!("HTTP client error: {}", e)),
            HttpClientError::Io(e) => A2AError::Io(e),
            HttpClientError::Request(msg) => {
                A2AError::Internal(format!("HTTP request error: {}", msg))
            }
            HttpClientError::Response { status, message } => {
                A2AError::Internal(format!("HTTP response error: {} - {}", status, message))
            }
            HttpClientError::Timeout => A2AError::Internal("HTTP request timeout".to_string()),
        }
    }
}

#[cfg(feature = "ws-client")]
impl From<WebSocketClientError> for A2AError {
    fn from(error: WebSocketClientError) -> Self {
        match error {
            WebSocketClientError::Connection(msg) => {
                A2AError::Internal(format!("WebSocket connection error: {}", msg))
            }
            WebSocketClientError::Message(msg) => {
                A2AError::Internal(format!("WebSocket message error: {}", msg))
            }
            WebSocketClientError::Io(e) => A2AError::Io(e),
            WebSocketClientError::Protocol(msg) => {
                A2AError::Internal(format!("WebSocket protocol error: {}", msg))
            }
            WebSocketClientError::Timeout => A2AError::Internal("WebSocket timeout".to_string()),
            WebSocketClientError::Closed => {
                A2AError::Internal("WebSocket connection closed".to_string())
            }
        }
    }
}
