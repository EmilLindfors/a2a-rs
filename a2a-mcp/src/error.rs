//! Error types for a2a-mcp integration

use thiserror::Error;

/// Errors that can occur in a2a-mcp integration
#[derive(Error, Debug)]
pub enum Error {
    /// Error related to A2A protocol
    #[error("A2A error: {0}")]
    A2a(String),

    /// Error related to RMCP protocol
    #[error("RMCP error: {0}")]
    Rmcp(String),

    /// Error in protocol translation
    #[error("Protocol translation error: {0}")]
    Translation(String),

    /// Task not found
    #[error("Task not found: {0}")]
    TaskNotFound(String),

    /// Error in task processing
    #[error("Task processing error: {0}")]
    TaskProcessing(String),

    /// Agent not found
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    /// Invalid tool method format
    #[error("Invalid tool method format: {0}")]
    InvalidToolMethod(String),

    /// Server error
    #[error("Server error: {0}")]
    Server(String),

    /// RMCP tool call error
    #[error("RMCP tool call error: {0}")]
    RmcpToolCall(String),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// HTTP request error
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for a2a-mcp operations
pub type Result<T> = std::result::Result<T, Error>;

/// Convenience function to convert a string error to an Error
pub fn err<E: ToString>(e: E) -> Error {
    Error::Translation(e.to_string())
}

/// Convert a2a_rs error to our Error type
impl From<a2a_rs::Error> for Error {
    fn from(e: a2a_rs::Error) -> Self {
        Error::A2a(e.to_string())
    }
}

// A utility function to convert an RMCP error to A2A error code
pub(crate) fn rmcp_error_to_a2a_code(rmcp_err: &rmcp::ServerJsonRpcMessage) -> i32 {
    if let Some(error) = &rmcp_err.error {
        match error.code {
            -32700 => -32700, // Parse error
            -32600 => -32600, // Invalid request
            -32601 => -32601, // Method not found
            -32602 => -32602, // Invalid params
            -32603 => -32603, // Internal error
            _ => -32000, // Server error
        }
    } else {
        -32000 // Default server error
    }
}