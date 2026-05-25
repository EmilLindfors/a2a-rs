//! Error types for the a2a-client library.

use thiserror::Error;

/// Errors that can occur when using the A2A client.
#[derive(Error, Debug)]
pub enum ClientError {
    /// Error from the underlying A2A protocol client
    #[error("A2A protocol error: {0}")]
    A2AError(#[from] a2a_rs::A2AError),

    /// WebSocket is not configured
    #[error(
        "WebSocket client not configured. Actionable Suggestion: Ensure that a WebSocket interface is defined in preferred_transport or additional_interfaces on the agent card, or manually configure a WebSocket URL using WebA2AClientBuilder::ws_url."
    )]
    WebSocketNotConfigured,

    /// Transport auto-detection failed
    #[error(
        "Failed to auto-detect available transports: {0}. Actionable Suggestion: Make sure the agent server is running, the host is reachable, and the `/agent-card` endpoint is accessible."
    )]
    AutoDetectionFailed(String),

    /// Invalid configuration
    #[error(
        "Invalid configuration: {0}. Actionable Suggestion: Verify that builder configuration parameters, such as base URLs, are correctly specified."
    )]
    InvalidConfiguration(String),

    /// URL parsing/formatting failure
    #[error(
        "Invalid URL format: '{url}'. Reason: {reason}. Actionable Suggestion: Double check that the URL scheme and address are valid and formatted correctly."
    )]
    InvalidUrl { url: String, reason: String },

    /// Serialization/deserialization error
    #[error(
        "Data serialization error: {0}. Actionable Suggestion: Check that payloads adhere to the expected format and structure."
    )]
    SerializationError(String),
}

/// Specialized Result type for client operations
pub type Result<T> = std::result::Result<T, ClientError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_error_formatting() {
        let err = ClientError::WebSocketNotConfigured;
        let msg = err.to_string();
        assert!(msg.contains("WebSocket client not configured."));
        assert!(msg.contains("Actionable Suggestion:"));

        let err = ClientError::AutoDetectionFailed("No matching transport".to_string());
        let msg = err.to_string();
        assert!(msg.contains("No matching transport"));
        assert!(msg.contains("Failed to auto-detect"));
        assert!(msg.contains("Actionable Suggestion:"));

        let err = ClientError::InvalidConfiguration("Missing field".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Missing field"));
        assert!(msg.contains("Actionable Suggestion:"));

        let err = ClientError::InvalidUrl {
            url: "http://invalid".to_string(),
            reason: "Bad format".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("http://invalid"));
        assert!(msg.contains("Bad format"));
        assert!(msg.contains("Actionable Suggestion:"));

        let err = ClientError::SerializationError("JSON error".to_string());
        let msg = err.to_string();
        assert!(msg.contains("JSON error"));
        assert!(msg.contains("Actionable Suggestion:"));
    }
}
