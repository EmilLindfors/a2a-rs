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

use crate::domain::A2AError;

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

/// Map a domain [`A2AError`] onto a JSON-RPC error object.
///
/// This is the JSON-RPC analogue of `connectrpc::map_err`. For
/// [`A2AError::ValidationError`] we surface a Google-RPC-style `BadRequest` in
/// `data` so clients can read field violations.
pub fn a2a_to_jsonrpc(err: &A2AError) -> JsonRpcError {
    use error_code::*;
    match err {
        A2AError::TaskNotFound(msg) => JsonRpcError {
            code: TASK_NOT_FOUND,
            message: msg.clone(),
            data: None,
        },
        A2AError::InvalidParams(msg) => JsonRpcError {
            code: INVALID_PARAMS,
            message: msg.clone(),
            data: None,
        },
        A2AError::ValidationError { field, message } => JsonRpcError {
            code: INVALID_PARAMS,
            message: format!("{field}: {message}"),
            data: Some(serde_json::json!([{
                "@type": "type.googleapis.com/google.rpc.BadRequest",
                "fieldViolations": [{ "field": field, "description": message }],
            }])),
        },
        A2AError::UnsupportedOperation(msg) => JsonRpcError {
            code: UNSUPPORTED_OPERATION,
            message: msg.clone(),
            data: None,
        },
        A2AError::AuthenticatedExtendedCardNotConfigured => JsonRpcError {
            code: EXTENDED_CARD_NOT_CONFIGURED,
            message: "Authenticated extended card not configured".to_string(),
            data: None,
        },
        A2AError::MethodNotFound(msg) => JsonRpcError {
            code: METHOD_NOT_FOUND,
            message: msg.clone(),
            data: None,
        },
        other => JsonRpcError {
            code: INTERNAL_ERROR,
            message: other.to_string(),
            data: None,
        },
    }
}

/// Map a JSON-RPC error object back onto a domain [`A2AError`] — the inverse of
/// [`a2a_to_jsonrpc`], used by the client to reconstruct typed errors.
pub fn jsonrpc_to_a2a(err: &JsonRpcError) -> A2AError {
    use error_code::*;
    match err.code {
        TASK_NOT_FOUND => A2AError::TaskNotFound(err.message.clone()),
        INVALID_PARAMS => A2AError::InvalidParams(err.message.clone()),
        METHOD_NOT_FOUND => A2AError::MethodNotFound(err.message.clone()),
        UNSUPPORTED_OPERATION => A2AError::UnsupportedOperation(err.message.clone()),
        EXTENDED_CARD_NOT_CONFIGURED => A2AError::AuthenticatedExtendedCardNotConfigured,
        code => A2AError::JsonRpc { code, message: err.message.clone(), data: err.data.clone() },
    }
}
