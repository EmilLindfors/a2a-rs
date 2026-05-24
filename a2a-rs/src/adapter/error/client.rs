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


