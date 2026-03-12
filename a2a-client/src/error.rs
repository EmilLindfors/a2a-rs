//! Error types for the a2a-client library.

use thiserror::Error;

/// Errors that can occur when using the A2A client.
#[derive(Error, Debug)]
pub enum ClientError {
    /// Error from the underlying A2A protocol client
    #[error("A2A protocol error: {0}")]
    A2AError(#[from] a2a_rs::A2AError),

    /// WebSocket is not configured
    #[error("WebSocket client not configured")]
    WebSocketNotConfigured,

    /// Transport auto-detection failed
    #[error("Failed to auto-detect available transports: {0}")]
    AutoDetectionFailed(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

/// Specialized Result type for client operations
pub type Result<T> = std::result::Result<T, ClientError>;
