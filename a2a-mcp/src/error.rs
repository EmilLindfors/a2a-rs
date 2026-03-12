//! Error types for A2A-MCP integration

use thiserror::Error;

/// Result type for A2A-MCP operations
pub type Result<T> = std::result::Result<T, A2aMcpError>;

/// Errors that can occur during A2A-MCP bridging
#[derive(Error, Debug)]
pub enum A2aMcpError {
    /// Error during protocol conversion
    #[error("Protocol conversion error: {0}")]
    Conversion(String),

    /// Error from A2A protocol operations
    #[error("A2A protocol error: {0}")]
    A2AError(#[from] a2a_rs::domain::error::A2AError),

    /// Error from MCP protocol operations
    #[error("MCP protocol error: {0}")]
    McpError(#[from] rmcp::ErrorData),

    /// Tool not found
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Skill not found
    #[error("Skill not found: {0}")]
    SkillNotFound(String),

    /// Invalid message format
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    /// Invalid tool call
    #[error("Invalid tool call: {0}")]
    InvalidToolCall(String),

    /// Agent communication error
    #[error("Agent communication error: {0}")]
    AgentCommunication(String),

    /// MCP server error
    #[error("MCP server error: {0}")]
    McpServer(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Other error
    #[error("{0}")]
    Other(String),
}

impl A2aMcpError {
    /// Convert this error into an MCP ErrorData
    pub fn to_mcp_error(&self) -> rmcp::ErrorData {
        match self {
            A2aMcpError::ToolNotFound(name) => {
                rmcp::ErrorData::internal_error(format!("Tool not found: {}", name), None)
            }
            A2aMcpError::InvalidToolCall(msg) => rmcp::ErrorData::invalid_params(msg.clone(), None),
            A2aMcpError::McpError(e) => e.clone(),
            _ => rmcp::ErrorData::internal_error(self.to_string(), None),
        }
    }

    /// Convert this error into an A2A error
    pub fn to_a2a_error(&self) -> a2a_rs::domain::error::A2AError {
        // A2AError doesn't implement Clone, so we just wrap everything as Internal
        a2a_rs::domain::error::A2AError::Internal(self.to_string())
    }
}
