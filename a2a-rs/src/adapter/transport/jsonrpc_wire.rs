//! Shared JSON-RPC 2.0 wire vocabulary for the A2A protocol.
//!
//! These method names, error codes, request/response envelopes, and the
//! `A2AError` ⇄ JSON-RPC error mappings are the contract both the server adapter
//! ([`JsonRpcAdapter`](super::jsonrpc::JsonRpcAdapter)) and the client adapter
//! ([`JsonRpcClient`](super::jsonrpc_client::JsonRpcClient)) must agree on
//! byte-for-byte. Keeping them in one module guarantees the two directions never
//! drift.
//!
//! Both envelopes derive `Serialize + Deserialize`: the server deserializes
//! requests / serializes responses, and the client does the inverse.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{A2AError, ErrorDetail, ErrorInfo};

/// A2A JSON-RPC method names (PascalCase, per spec).
pub mod methods {
    pub const SEND_MESSAGE: &str = "SendMessage";
    pub const SEND_STREAMING_MESSAGE: &str = "SendStreamingMessage";
    pub const GET_TASK: &str = "GetTask";
    pub const LIST_TASKS: &str = "ListTasks";
    pub const CANCEL_TASK: &str = "CancelTask";
    pub const SUBSCRIBE_TO_TASK: &str = "SubscribeToTask";
    pub const CREATE_PUSH_CONFIG: &str = "CreateTaskPushNotificationConfig";
    pub const GET_PUSH_CONFIG: &str = "GetTaskPushNotificationConfig";
    pub const LIST_PUSH_CONFIGS: &str = "ListTaskPushNotificationConfigs";
    pub const DELETE_PUSH_CONFIG: &str = "DeleteTaskPushNotificationConfig";
    pub const GET_EXTENDED_AGENT_CARD: &str = "GetExtendedAgentCard";

    /// Streaming methods respond with SSE rather than a single response.
    pub fn is_streaming(method: &str) -> bool {
        matches!(method, SEND_STREAMING_MESSAGE | SUBSCRIBE_TO_TASK)
    }
}

/// Standard + A2A-specific JSON-RPC error codes (mirrors the official SDK).
pub mod error_code {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    pub const TASK_NOT_FOUND: i32 = -32001;
    pub const TASK_NOT_CANCELABLE: i32 = -32002;
    pub const PUSH_NOTIFICATION_NOT_SUPPORTED: i32 = -32003;
    pub const UNSUPPORTED_OPERATION: i32 = -32004;
    pub const CONTENT_TYPE_NOT_SUPPORTED: i32 = -32005;
    pub const INVALID_AGENT_RESPONSE: i32 = -32006;
    pub const EXTENDED_CARD_NOT_CONFIGURED: i32 = -32007;

    /// Custom application range (outside the spec's reserved codes).
    pub const VERSION_CONFLICT: i32 = -32101;
}

/// JSON-RPC request envelope (server deserializes; client serializes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: JsonRpcId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC response envelope (server serializes; client deserializes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: JsonRpcId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: JsonRpcId, result: Value) -> Self {
        Self { jsonrpc: "2.0".to_string(), id, result: Some(result), error: None }
    }

    pub fn err(id: JsonRpcId, error: JsonRpcError) -> Self {
        Self { jsonrpc: "2.0".to_string(), id, result: None, error: Some(error) }
    }
}

/// JSON-RPC request id — preserves the wire type (string, number, or null).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Str(String),
    Num(i64),
    #[default]
    Null,
}

/// JSON-RPC error object. `data` carries typed A2A error details when available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// The JSON-RPC error code for a domain [`A2AError`].
fn a2a_error_code(err: &A2AError) -> i32 {
    use error_code::*;
    match err {
        A2AError::JsonRpc { code, .. } => *code,
        A2AError::JsonParse(_) => PARSE_ERROR,
        A2AError::InvalidRequest(_) => INVALID_REQUEST,
        A2AError::MethodNotFound(_) => METHOD_NOT_FOUND,
        A2AError::InvalidParams(_) | A2AError::ValidationError { .. } => INVALID_PARAMS,
        A2AError::TaskNotFound(_) => TASK_NOT_FOUND,
        A2AError::TaskNotCancelable(_) => TASK_NOT_CANCELABLE,
        A2AError::PushNotificationNotSupported => PUSH_NOTIFICATION_NOT_SUPPORTED,
        A2AError::UnsupportedOperation(_) => UNSUPPORTED_OPERATION,
        A2AError::ContentTypeNotSupported(_) => CONTENT_TYPE_NOT_SUPPORTED,
        A2AError::InvalidAgentResponse(_) => INVALID_AGENT_RESPONSE,
        A2AError::AuthenticatedExtendedCardNotConfigured => EXTENDED_CARD_NOT_CONFIGURED,
        A2AError::VersionConflict { .. } => VERSION_CONFLICT,
        _ => INTERNAL_ERROR,
    }
}

/// Map a domain [`A2AError`] onto a JSON-RPC error object.
///
/// This is the JSON-RPC analogue of `connectrpc::map_err`. The `data` array
/// carries the error's [typed details](A2AError::error_details): a Google-RPC
/// `BadRequest` for validation failures, plus an `ErrorInfo` reason code on every
/// error so clients can branch on a stable machine code rather than the message
/// string. [`jsonrpc_to_a2a`] reverses this.
pub fn a2a_to_jsonrpc(err: &A2AError) -> JsonRpcError {
    let message = match err {
        A2AError::ValidationError { field, message } => format!("{field}: {message}"),
        other => other.to_string(),
    };
    let details = err.error_details();
    let data = (!details.is_empty()).then(|| serde_json::json!(details));
    JsonRpcError { code: a2a_error_code(err), message, data }
}

/// Map a JSON-RPC error object back onto a domain [`A2AError`] — the inverse of
/// [`a2a_to_jsonrpc`], used by the client to reconstruct typed errors.
///
/// A [`A2AError::VersionConflict`] is rebuilt from its `ErrorInfo` metadata when
/// present, so the typed expected/actual versions survive the round-trip.
pub fn jsonrpc_to_a2a(err: &JsonRpcError) -> A2AError {
    use error_code::*;
    match err.code {
        TASK_NOT_FOUND => A2AError::TaskNotFound(err.message.clone()),
        INVALID_PARAMS => A2AError::InvalidParams(err.message.clone()),
        METHOD_NOT_FOUND => A2AError::MethodNotFound(err.message.clone()),
        UNSUPPORTED_OPERATION => A2AError::UnsupportedOperation(err.message.clone()),
        EXTENDED_CARD_NOT_CONFIGURED => A2AError::AuthenticatedExtendedCardNotConfigured,
        VERSION_CONFLICT => version_conflict_from_data(err)
            .unwrap_or_else(|| A2AError::Internal(err.message.clone())),
        code => A2AError::JsonRpc { code, message: err.message.clone(), data: err.data.clone() },
    }
}

/// Reconstruct a [`A2AError::VersionConflict`] from the `ErrorInfo` metadata in a
/// wire error's `data` array, if it carries the expected/actual versions.
fn version_conflict_from_data(err: &JsonRpcError) -> Option<A2AError> {
    let details: Vec<ErrorDetail> = serde_json::from_value(err.data.clone()?).ok()?;
    let ErrorInfo { metadata, .. } = details.into_iter().find_map(|d| match d {
        ErrorDetail::ErrorInfo(info) => Some(info),
        _ => None,
    })?;
    Some(A2AError::VersionConflict {
        id: metadata.get("task_id").cloned().unwrap_or_default(),
        expected: metadata.get("expected").and_then(|s| s.parse().ok())?,
        actual: metadata.get("actual").and_then(|s| s.parse().ok())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_surfaces_bad_request_details() {
        let err = A2AError::ValidationError {
            field: "history_length".to_string(),
            message: "too large".to_string(),
        };
        let wire = a2a_to_jsonrpc(&err);
        assert_eq!(wire.code, error_code::INVALID_PARAMS);
        let data = wire.data.expect("validation errors carry data");
        // First detail is a Google-RPC BadRequest naming the field.
        assert_eq!(
            data[0]["@type"],
            "type.googleapis.com/google.rpc.BadRequest"
        );
        assert_eq!(data[0]["fieldViolations"][0]["field"], "history_length");
        // Second detail is the stable reason code.
        assert_eq!(data[1]["reason"], "VALIDATION_ERROR");
    }

    #[test]
    fn version_conflict_round_trips_through_the_wire() {
        let err = A2AError::VersionConflict {
            id: "task-42".to_string(),
            expected: 3,
            actual: 5,
        };
        let wire = a2a_to_jsonrpc(&err);
        assert_eq!(wire.code, error_code::VERSION_CONFLICT);
        match jsonrpc_to_a2a(&wire) {
            A2AError::VersionConflict { id, expected, actual } => {
                assert_eq!(id, "task-42");
                assert_eq!(expected, 3);
                assert_eq!(actual, 5);
            }
            other => panic!("expected VersionConflict, got {other:?}"),
        }
    }

    #[test]
    fn every_error_carries_a_reason_code() {
        let wire = a2a_to_jsonrpc(&A2AError::TaskNotFound("x".to_string()));
        let data = wire.data.expect("errors carry an ErrorInfo reason");
        assert_eq!(data[0]["reason"], "TASK_NOT_FOUND");
        assert_eq!(data[0]["domain"], "a2a-rs");
    }
}
