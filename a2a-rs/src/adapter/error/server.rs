//! Error types for server adapters

#[cfg(feature = "http-server")]
use std::io;

#[cfg(feature = "http-server")]
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
