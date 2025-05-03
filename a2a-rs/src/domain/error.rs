use thiserror::Error;

/// Standard JSON-RPC error codes
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

/// A2A specific error codes
pub const TASK_NOT_FOUND: i32 = -32001;
pub const TASK_NOT_CANCELABLE: i32 = -32002;
pub const PUSH_NOTIFICATION_NOT_SUPPORTED: i32 = -32003;
pub const UNSUPPORTED_OPERATION: i32 = -32004;

/// Error type for the A2A protocol operations
#[derive(Error, Debug)]
pub enum A2AError {
    #[error("JSON-RPC error: {code} - {message}")]
    JsonRpc {
        code: i32,
        message: String,
        data: Option<serde_json::Value>,
    },

    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("Method not found: {0}")]
    MethodNotFound(String),

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Task not cancelable: {0}")]
    TaskNotCancelable(String),

    #[error("Push notification not supported")]
    PushNotificationNotSupported,

    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl A2AError {
    /// Convert an A2AError to a JSON-RPC error value
    pub fn to_jsonrpc_error(&self) -> serde_json::Value {
        let (code, message) = match self {
            A2AError::JsonParse(_) => (PARSE_ERROR, "Invalid JSON payload"),
            A2AError::InvalidRequest(_) => (INVALID_REQUEST, "Request payload validation error"),
            A2AError::MethodNotFound(_) => (METHOD_NOT_FOUND, "Method not found"),
            A2AError::InvalidParams(_) => (INVALID_PARAMS, "Invalid parameters"),
            A2AError::TaskNotFound(_) => (TASK_NOT_FOUND, "Task not found"),
            A2AError::TaskNotCancelable(_) => (TASK_NOT_CANCELABLE, "Task cannot be canceled"),
            A2AError::PushNotificationNotSupported => (
                PUSH_NOTIFICATION_NOT_SUPPORTED,
                "Push Notification is not supported",
            ),
            A2AError::UnsupportedOperation(_) => {
                (UNSUPPORTED_OPERATION, "This operation is not supported")
            }
            A2AError::Internal(_) => (INTERNAL_ERROR, "Internal error"),
            _ => (INTERNAL_ERROR, "Internal error"),
        };

        serde_json::json!({
            "code": code,
            "message": message,
            "data": null,
        })
    }
}
