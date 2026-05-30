//! The JSON-RPC 2.0 + HTTP+JSON transport adapter (DRAFT skeleton).
//!
//! `JsonRpcAdapter` is a **sibling** of [`ConnectRpcAdapter`](super::connectrpc::ConnectRpcAdapter):
//! a thin transport adapter that wraps the same inner [`TaskService`] but speaks
//! the spec-mandated, ecosystem-interoperable **JSON-RPC 2.0** wire format
//! (and, via [`rest_router`], HTTP+JSON / REST). Its only job is to parse a
//! JSON-RPC envelope, deserialize `params` into the matching A2A param type,
//! delegate to [`TaskService`], wrap the domain result in the field-presence
//! union the spec requires, and map [`A2AError`] onto JSON-RPC error codes.
//!
//! All use-case orchestration lives in [`TaskService`]; this layer holds no port
//! traits directly — exactly the layering of `connectrpc.rs`.
//!
//! # Status: DRAFT
//!
//! This module is a structural skeleton accompanying `JSONRPC_ADAPTER_PLAN.md`.
//! It is **not yet wired into `adapter/transport/mod.rs`** and is **not yet
//! feature-gated or built**. The wire-fidelity of the message bodies depends on
//! whether `buffa`'s JSON serialization matches ProtoJSON — that must be settled
//! with golden tests (plan §6) before this is trusted for interop. Items needing
//! that decision are marked `// VERIFY:`.

#![allow(dead_code)] // DRAFT: not yet wired in.

use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};

use crate::{
    application::TaskService,
    domain::{
        A2AError, Message, Task, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
        ListTasksParams,
    },
    port::{
        AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskLifecycle,
        AsyncTaskQuery, UpdateEvent,
    },
    services::server::AgentInfoProvider,
};

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0 / HTTP+JSON transport adapter over a [`TaskService`].
///
/// Mirrors [`ConnectRpcAdapter`](super::connectrpc::ConnectRpcAdapter)'s
/// constructors so an agent author swaps transports with one line.
#[derive(Clone)]
pub struct JsonRpcAdapter {
    service: TaskService,
}

impl JsonRpcAdapter {
    /// Create an adapter from separate handlers (no real streaming backend).
    ///
    /// `tasks` supplies both the lifecycle and query capabilities. Uses the same
    /// `NoopStreamingHandler` default as the Connect adapter.
    pub fn new(
        message_handler: impl AsyncMessageHandler + 'static,
        tasks: impl AsyncTaskLifecycle + AsyncTaskQuery + 'static,
        notification_manager: impl AsyncNotificationManager + 'static,
        agent_info: impl AgentInfoProvider + 'static,
    ) -> Self {
        Self {
            service: TaskService::new(
                message_handler,
                tasks,
                notification_manager,
                agent_info,
                super::connectrpc::NoopStreamingHandler,
            ),
        }
    }

    /// Create an adapter from a single handler implementing every port.
    pub fn with_handler(
        handler: impl AsyncMessageHandler
        + AsyncTaskLifecycle
        + AsyncTaskQuery
        + AsyncNotificationManager
        + 'static,
        agent_info: impl AgentInfoProvider + 'static,
    ) -> Self {
        Self {
            service: TaskService::with_handler(
                handler,
                agent_info,
                super::connectrpc::NoopStreamingHandler,
            ),
        }
    }

    /// Inject a real streaming handler (required for the streaming methods).
    pub fn with_streaming_handler(
        self,
        streaming_handler: impl AsyncStreamingHandler + 'static,
    ) -> Self {
        Self {
            service: self.service.with_streaming_handler(streaming_handler),
        }
    }

    /// Build the inner service reference (used by the routers).
    fn service(&self) -> &TaskService {
        &self.service
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 envelopes
// ---------------------------------------------------------------------------

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

/// Incoming JSON-RPC request envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// Outgoing JSON-RPC response envelope.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: JsonRpcId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: JsonRpcId, result: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    pub fn err(id: JsonRpcId, error: JsonRpcError) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(error) }
    }
}

/// JSON-RPC request id — preserves the wire type (string, number, or null).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Str(String),
    Num(i64),
    Null,
}

impl Default for JsonRpcId {
    fn default() -> Self {
        Self::Null
    }
}

/// JSON-RPC error object. `data` carries typed A2A error details when available.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
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
            // VERIFY: shape against google.rpc.BadRequest fieldViolations.
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

// ---------------------------------------------------------------------------
// Field-presence union result types (spec wire shape)
// ---------------------------------------------------------------------------
//
// The A2A spec serializes these unions with NO discriminator tag — only the
// active field is present. This is the one piece `buffa` does not give us as a
// plain serde enum, so we hand-write the Serialize. The message bodies (Task,
// Message, events) are reused from the domain (VERIFY their JSON is ProtoJSON-
// clean; if not, convert through dedicated `wire::*` DTOs — plan §2 Option B).

/// `SendMessage` result: a finished `Task` or a direct `Message`.
pub enum SendMessageResult {
    Task(Task),
    Message(Message),
}

impl Serialize for SendMessageResult {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(1))?;
        match self {
            Self::Task(t) => m.serialize_entry("task", t)?,
            Self::Message(msg) => m.serialize_entry("message", msg)?,
        }
        m.end()
    }
}

/// Streaming result: task snapshot | message | status update | artifact update.
pub enum StreamResult {
    Task(Task),
    Message(Message),
    StatusUpdate(TaskStatusUpdateEvent),
    ArtifactUpdate(TaskArtifactUpdateEvent),
}

impl Serialize for StreamResult {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(1))?;
        match self {
            Self::Task(t) => m.serialize_entry("task", t)?,
            Self::Message(msg) => m.serialize_entry("message", msg)?,
            Self::StatusUpdate(e) => m.serialize_entry("statusUpdate", e)?,
            Self::ArtifactUpdate(e) => m.serialize_entry("artifactUpdate", e)?,
        }
        m.end()
    }
}

impl From<UpdateEvent> for StreamResult {
    fn from(evt: UpdateEvent) -> Self {
        match evt {
            UpdateEvent::StatusUpdate(e) => Self::StatusUpdate(e),
            UpdateEvent::ArtifactUpdate(e) => Self::ArtifactUpdate(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Method dispatch (transport-neutral core)
// ---------------------------------------------------------------------------

impl JsonRpcAdapter {
    /// Handle a single non-streaming JSON-RPC request, producing a response
    /// envelope. Streaming methods are handled by the SSE path in the router and
    /// must not reach here.
    ///
    /// VERIFY: each `params` deserialization target must match the spec request
    /// shape. The A2A param types (`MessageSendParams`, `TaskQueryParams`,
    /// `ListTasksParams`, …) are the hand-written serde structs in
    /// `domain/core/task.rs`.
    pub async fn handle_unary(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.clone();

        if req.jsonrpc != "2.0" {
            return JsonRpcResponse::err(
                id,
                JsonRpcError {
                    code: error_code::INVALID_REQUEST,
                    message: "jsonrpc must be \"2.0\"".to_string(),
                    data: None,
                },
            );
        }

        let result: Result<serde_json::Value, A2AError> = match req.method.as_str() {
            methods::GET_TASK => self.handle_get_task(req.params).await,
            methods::LIST_TASKS => self.handle_list_tasks(req.params).await,
            methods::CANCEL_TASK => self.handle_cancel_task(req.params).await,
            methods::SEND_MESSAGE => self.handle_send_message(req.params).await,
            methods::GET_EXTENDED_AGENT_CARD => self.handle_extended_card().await,
            // TODO: push-config CRUD methods.
            methods::SEND_STREAMING_MESSAGE | methods::SUBSCRIBE_TO_TASK => {
                return JsonRpcResponse::err(
                    id,
                    JsonRpcError {
                        code: error_code::INTERNAL_ERROR,
                        message: "streaming method routed to unary handler".to_string(),
                        data: None,
                    },
                );
            }
            unknown => Err(A2AError::MethodNotFound(unknown.to_string())),
        };

        match result {
            Ok(value) => JsonRpcResponse::ok(id, value),
            Err(e) => JsonRpcResponse::err(id, a2a_to_jsonrpc(&e)),
        }
    }

    async fn handle_get_task(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, A2AError> {
        // VERIFY: spec uses `{ "id": "...", "historyLength": n }`.
        let p: crate::domain::TaskQueryParams = parse_params(params)?;
        let id = p.id.parse()?;
        let task = self.service.get(&id, p.history_length).await?;
        to_value(&task)
    }

    async fn handle_list_tasks(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, A2AError> {
        let p: ListTasksParams = parse_params(params)?;
        let result = self.service.list(&p).await?;
        to_value(&result)
    }

    async fn handle_cancel_task(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, A2AError> {
        let p: crate::domain::TaskIdParams = parse_params(params)?;
        let id = p.id.parse()?;
        let task = self.service.cancel(&id).await?;
        to_value(&task)
    }

    async fn handle_send_message(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, A2AError> {
        let p: crate::domain::MessageSendParams = parse_params(params)?;
        // VERIFY: MessageSendParams field names + how task_id/context_id are
        // sourced (message carries them in the canonical shape). Mirror the
        // decode logic in connectrpc.rs::send_message.
        let task_id = p.message.task_id.clone();
        let session_id = (!p.message.context_id.is_empty()).then_some(p.message.context_id.as_str());
        let (push_config, history_limit) = decode_send_config(p.configuration);

        let task = self
            .service
            .send_message(&task_id, &p.message, session_id, push_config, history_limit)
            .await?;

        // Spec union: serialize as { "task": { ... } }.
        to_value(&SendMessageResult::Task(task))
    }

    async fn handle_extended_card(&self) -> Result<serde_json::Value, A2AError> {
        let card = self.service.extended_agent_card().await?;
        to_value(&card)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deserialize JSON-RPC `params` into a concrete A2A param type, mapping serde
/// failures to `InvalidParams`.
fn parse_params<T: serde::de::DeserializeOwned>(
    params: Option<serde_json::Value>,
) -> Result<T, A2AError> {
    let value = params.unwrap_or(serde_json::Value::Null);
    serde_json::from_value(value)
        .map_err(|e| A2AError::InvalidParams(format!("invalid params: {e}")))
}

/// Serialize a domain/wire value into the JSON-RPC `result`, mapping failures to
/// an internal error.
fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, A2AError> {
    serde_json::to_value(value)
        .map_err(|e| A2AError::InvalidParams(format!("failed to serialize result: {e}")))
}

/// Decode the optional `MessageSendConfiguration` into the push config + history
/// limit the service expects. Mirrors `connectrpc::decode_send_config`.
///
/// VERIFY: field names on `MessageSendConfiguration` vs the spec.
fn decode_send_config(
    config: Option<crate::domain::MessageSendConfiguration>,
) -> (Option<crate::domain::TaskPushNotificationConfig>, Option<u32>) {
    match config {
        Some(c) => (c.push_notification_config, c.history_length),
        None => (None, None),
    }
}

// ---------------------------------------------------------------------------
// Router stubs (axum) — to flesh out under the `jsonrpc-server` feature.
// ---------------------------------------------------------------------------
//
// pub fn jsonrpc_router(adapter: Arc<JsonRpcAdapter>) -> axum::Router { ... }
//   POST "/" -> parse JsonRpcRequest; if methods::is_streaming -> SSE path
//               (each event a JsonRpcResponse whose result is a StreamResult,
//                first event the initial Task — see connectrpc.rs:265-276);
//               else -> adapter.handle_unary(req).
//
// pub fn rest_router(adapter: Arc<JsonRpcAdapter>) -> axum::Router { ... }
//   REST routes from JSONRPC_ADAPTER_PLAN.md §4, delegating to the same
//   TaskService methods; HTTP status derived from A2AError.
