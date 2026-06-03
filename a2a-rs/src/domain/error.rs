use thiserror::Error;

use crate::domain::error_details::{ErrorDetail, FieldViolation};

/// Convenience alias for results that fail with [`A2AError`].
///
/// Mirrors the `std::io::Result` / `serde_json::Result` convention so call
/// sites can write `Result<Task>` instead of `Result<Task, A2AError>`.
pub type Result<T, E = A2AError> = std::result::Result<T, E>;

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
pub const CONTENT_TYPE_NOT_SUPPORTED: i32 = -32005;
pub const INVALID_AGENT_RESPONSE: i32 = -32006;
pub const AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED: i32 = -32007;

/// Custom application-specific error codes (outside spec range)
pub const DATABASE_ERROR: i32 = -32100;
/// Optimistic-concurrency version mismatch on a task mutation.
pub const VERSION_CONFLICT: i32 = -32101;

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

    #[error("Content type not supported: {0}")]
    ContentTypeNotSupported(String),

    #[error("Invalid agent response: {0}")]
    InvalidAgentResponse(String),

    #[error("Authenticated extended card not configured")]
    AuthenticatedExtendedCardNotConfigured,

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Validation error in {field}: {message}")]
    ValidationError { field: String, message: String },

    #[error(
        "Version conflict for task {id}: expected {expected}, found {actual}"
    )]
    VersionConflict {
        id: String,
        expected: u64,
        actual: u64,
    },

    #[error("Database error: {0}")]
    DatabaseError(String),

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
            A2AError::ContentTypeNotSupported(_) => {
                (CONTENT_TYPE_NOT_SUPPORTED, "Incompatible content types")
            }
            A2AError::InvalidAgentResponse(_) => (INVALID_AGENT_RESPONSE, "Invalid agent response"),
            A2AError::AuthenticatedExtendedCardNotConfigured => (
                AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED,
                "Authenticated Extended Card is not configured",
            ),
            A2AError::ValidationError { .. } => (INVALID_PARAMS, "Validation error"),
            A2AError::VersionConflict { .. } => (VERSION_CONFLICT, "Task version conflict"),
            A2AError::DatabaseError(_) => (DATABASE_ERROR, "Database error"),
            A2AError::Internal(_) => (INTERNAL_ERROR, "Internal error"),
            _ => (INTERNAL_ERROR, "Internal error"),
        };

        serde_json::json!({
            "code": code,
            "message": message,
            "data": null,
        })
    }

    /// Stable, machine-readable reason code for this error
    /// (`SCREAMING_SNAKE_CASE`, used as the `ErrorInfo.reason` on the wire).
    pub fn reason_code(&self) -> &'static str {
        match self {
            A2AError::JsonRpc { .. } => "JSON_RPC_ERROR",
            A2AError::JsonParse(_) => "PARSE_ERROR",
            A2AError::InvalidRequest(_) => "INVALID_REQUEST",
            A2AError::InvalidParams(_) => "INVALID_PARAMS",
            A2AError::MethodNotFound(_) => "METHOD_NOT_FOUND",
            A2AError::TaskNotFound(_) => "TASK_NOT_FOUND",
            A2AError::TaskNotCancelable(_) => "TASK_NOT_CANCELABLE",
            A2AError::PushNotificationNotSupported => "PUSH_NOTIFICATION_NOT_SUPPORTED",
            A2AError::UnsupportedOperation(_) => "UNSUPPORTED_OPERATION",
            A2AError::ContentTypeNotSupported(_) => "CONTENT_TYPE_NOT_SUPPORTED",
            A2AError::InvalidAgentResponse(_) => "INVALID_AGENT_RESPONSE",
            A2AError::AuthenticatedExtendedCardNotConfigured => {
                "AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED"
            }
            A2AError::Internal(_) => "INTERNAL_ERROR",
            A2AError::ValidationError { .. } => "VALIDATION_ERROR",
            A2AError::VersionConflict { .. } => "VERSION_CONFLICT",
            A2AError::DatabaseError(_) => "DATABASE_ERROR",
            A2AError::Io(_) => "IO_ERROR",
        }
    }

    /// Typed details for the JSON-RPC `error.data` array.
    ///
    /// Validation failures surface as a Google-RPC `BadRequest` with field
    /// violations; version conflicts attach the expected/actual versions as
    /// `ErrorInfo` metadata; every other variant carries at least its stable
    /// [`reason_code`](Self::reason_code) as an `ErrorInfo`, so a client can
    /// branch on a machine code instead of parsing the message string.
    pub fn error_details(&self) -> Vec<ErrorDetail> {
        match self {
            A2AError::ValidationError { field, message } => vec![
                ErrorDetail::BadRequest {
                    field_violations: vec![FieldViolation::new(field, message)],
                },
                ErrorDetail::reason(self.reason_code()),
            ],
            A2AError::VersionConflict { id, expected, actual } => {
                let mut info = crate::domain::error_details::ErrorInfo::new(self.reason_code());
                info = info
                    .with_metadata("task_id", id)
                    .with_metadata("expected", expected.to_string())
                    .with_metadata("actual", actual.to_string());
                vec![ErrorDetail::ErrorInfo(info)]
            }
            _ => vec![ErrorDetail::reason(self.reason_code())],
        }
    }
}
