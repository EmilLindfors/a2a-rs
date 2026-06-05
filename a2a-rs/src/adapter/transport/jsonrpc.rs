//! The JSON-RPC 2.0 + HTTP+JSON (REST) transport adapter.
//!
//! `JsonRpcAdapter` is a **sibling** of [`ConnectRpcAdapter`](super::connectrpc::ConnectRpcAdapter):
//! a thin transport adapter that wraps the same inner [`TaskService`] but speaks
//! the spec-mandated, ecosystem-interoperable **JSON-RPC 2.0** wire format
//! (and, via [`rest_router`], HTTP+JSON / REST). Its only job is to parse a
//! JSON-RPC envelope, deserialize `params` into the matching A2A request type,
//! delegate to [`TaskService`], and re-encode the domain result — mapping
//! [`A2AError`] onto JSON-RPC error codes.
//!
//! All use-case orchestration lives in [`TaskService`]; this layer holds no port
//! traits directly — exactly the layering of `connectrpc.rs`.
//!
//! # Wire format
//!
//! Request `params` and the `result` body are the **generated proto types**
//! (`SendMessageRequest`, `Task`, `SendMessageResponse`, …). Those already
//! serialize as canonical ProtoJSON — camelCase fields, SCREAMING_SNAKE enums,
//! RFC3339 timestamps, base64 `bytes`, bare `Struct` metadata, and tag-free
//! field-presence unions (`{"task": …}` / `{"statusUpdate": …}`). This is the
//! same representation the official SDK and the Go/C#/Python SDKs use, so an
//! off-the-shelf A2A client can talk to this server. The decode/encode helpers
//! (`decode_send_config`, `list_request_to_params`, `map_update_event`) are
//! **shared with the Connect adapter** so both transports agree on the wire.
//!
//! The hand-written A2A param types in `domain/core/task.rs` (`MessageSendParams`
//! with `pushNotificationConfig`/`blocking`) are the *legacy* JSON-RPC v0.x shape
//! and are intentionally **not** used here — the proto request types are the
//! v1.0 contract.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures::{Stream, StreamExt};
use serde::Serialize;
use serde_json::Value;

use crate::{
    application::TaskService,
    domain::{
        A2AError, TaskId, TaskPushNotificationConfig,
        generated::{
            CancelTaskRequest, DeleteTaskPushNotificationConfigRequest,
            GetTaskPushNotificationConfigRequest, GetTaskRequest,
            ListTaskPushNotificationConfigsRequest, ListTaskPushNotificationConfigsResponse,
            ListTasksRequest, ListTasksResponse, SendMessageRequest, SendMessageResponse,
            StreamResponse, SubscribeToTaskRequest, send_message_response, stream_response,
        },
    },
    port::{
        AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskLifecycle,
        AsyncTaskQuery, CallContext, CallInterceptor, CallSide, run_after, run_before,
    },
    services::server::AgentInfoProvider,
};

use super::connectrpc::{
    NoopStreamingHandler, decode_send_config, list_request_to_params, map_update_event,
};
// Re-exported so existing `transport::jsonrpc::{methods, error_code, JsonRpc*}`
// paths keep working now that these live in the shared wire module.
pub use super::jsonrpc_wire::{
    JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse, a2a_to_jsonrpc, error_code, methods,
};

/// A stream of wire [`StreamResponse`]s — the unified output of both streaming
/// methods, before it is framed as SSE (enveloped for JSON-RPC, bare for REST).
/// A stream of wire responses, each tagged with an optional per-task event id.
/// The id (when present) is emitted as the SSE `id:` field so a client can
/// resume via `Last-Event-ID`. The initial task snapshot carries `None`.
type StreamResponseStream =
    Pin<Box<dyn Stream<Item = Result<(Option<u64>, StreamResponse), A2AError>> + Send>>;

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
    /// Server-side interceptor chain wrapping every unary/streaming dispatch.
    interceptors: Vec<Arc<dyn CallInterceptor>>,
}

impl JsonRpcAdapter {
    /// Create an adapter from separate handlers (no real streaming backend).
    ///
    /// `tasks` supplies both the lifecycle and query capabilities. Uses the same
    /// [`NoopStreamingHandler`] default as the Connect adapter.
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
                NoopStreamingHandler,
                crate::port::NoopPushNotifier,
            ),
            interceptors: Vec::new(),
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
                NoopStreamingHandler,
                crate::port::NoopPushNotifier,
            ),
            interceptors: Vec::new(),
        }
    }

    /// Inject a real streaming handler (required for the streaming methods).
    pub fn with_streaming_handler(
        self,
        streaming_handler: impl AsyncStreamingHandler + 'static,
    ) -> Self {
        Self {
            service: self.service.with_streaming_handler(streaming_handler),
            interceptors: self.interceptors,
        }
    }

    /// Inject a real push notifier (required for webhook delivery).
    pub fn with_push_notifier(
        self,
        push_notifier: impl crate::port::AsyncPushNotifier + 'static,
    ) -> Self {
        Self {
            service: self.service.with_push_notifier(push_notifier),
            interceptors: self.interceptors,
        }
    }

    /// Append a server-side [`CallInterceptor`] to the chain.
    ///
    /// Interceptors wrap every unary and streaming dispatch: `before` hooks run
    /// in registration order, then the method runs, then `after` hooks run in
    /// reverse. Chainable, so callers can register several.
    pub fn with_interceptor(mut self, interceptor: impl CallInterceptor + 'static) -> Self {
        self.interceptors.push(Arc::new(interceptor));
        self
    }
}

// ---------------------------------------------------------------------------
// Method dispatch (transport-neutral core, shared by JSON-RPC and REST)
// ---------------------------------------------------------------------------

impl JsonRpcAdapter {
    /// Handle a single non-streaming JSON-RPC request, producing a response
    /// envelope. Streaming methods are handled by the SSE path in the router and
    /// must not reach here.
    pub async fn handle_unary(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.clone();
        let result = self.dispatch_intercepted(&req.method, req.params).await;
        match result {
            Ok(value) => JsonRpcResponse::ok(id, value),
            Err(e) => JsonRpcResponse::err(id, a2a_to_jsonrpc(&e)),
        }
    }

    /// Run the server interceptor chain around a unary [`dispatch_unary`], so
    /// both the JSON-RPC and REST entry points share one interception point.
    ///
    /// [`dispatch_unary`]: Self::dispatch_unary
    async fn dispatch_intercepted(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, A2AError> {
        if self.interceptors.is_empty() {
            return self.dispatch_unary(method, params).await;
        }
        let ctx = CallContext::new(method, CallSide::Server);
        if let Err(e) = run_before(&self.interceptors, &ctx).await {
            run_after(&self.interceptors, &ctx, Err(&e)).await;
            return Err(e);
        }
        let result = self.dispatch_unary(method, params).await;
        run_after(&self.interceptors, &ctx, result.as_ref().map(|_| ())).await;
        result
    }

    /// Route a unary method name + `params` to the service and return the wire
    /// `result` value. Reused by both JSON-RPC and REST.
    async fn dispatch_unary(&self, method: &str, params: Option<Value>) -> Result<Value, A2AError> {
        match method {
            methods::GET_TASK => self.get_task(params).await,
            methods::LIST_TASKS => self.list_tasks(params).await,
            methods::CANCEL_TASK => self.cancel_task(params).await,
            methods::SEND_MESSAGE => self.send_message(params).await,
            methods::CREATE_PUSH_CONFIG => self.create_push_config(params).await,
            methods::GET_PUSH_CONFIG => self.get_push_config(params).await,
            methods::LIST_PUSH_CONFIGS => self.list_push_configs(params).await,
            methods::DELETE_PUSH_CONFIG => self.delete_push_config(params).await,
            methods::GET_EXTENDED_AGENT_CARD => self.extended_card().await,
            methods::SEND_STREAMING_MESSAGE | methods::SUBSCRIBE_TO_TASK => Err(
                A2AError::InvalidParams("streaming method requires SSE transport".to_string()),
            ),
            unknown => Err(A2AError::MethodNotFound(unknown.to_string())),
        }
    }

    async fn get_task(&self, params: Option<Value>) -> Result<Value, A2AError> {
        let req: GetTaskRequest = parse_params(params)?;
        let id: TaskId = req.id.parse()?;
        let task = self
            .service
            .get(&id, req.history_length.map(|l| l as u32))
            .await?;
        to_value(&task)
    }

    async fn list_tasks(&self, params: Option<Value>) -> Result<Value, A2AError> {
        let req: ListTasksRequest = parse_params(params)?;
        let result = self.service.list(&list_request_to_params(req)).await?;
        let response = ListTasksResponse {
            tasks: result.tasks,
            next_page_token: result.next_page_token,
            page_size: result.page_size,
            total_size: result.total_size,
            ..Default::default()
        };
        to_value(&response)
    }

    async fn cancel_task(&self, params: Option<Value>) -> Result<Value, A2AError> {
        let req: CancelTaskRequest = parse_params(params)?;
        let id: TaskId = req.id.parse()?;
        let task = self.service.cancel(&id).await?;
        to_value(&task)
    }

    async fn send_message(&self, params: Option<Value>) -> Result<Value, A2AError> {
        let (task_id, message, session_id, push_config, history_limit) =
            decode_send_message(parse_params(params)?)?;
        let task = self
            .service
            .send_message(
                &task_id,
                &message,
                session_id.as_deref(),
                push_config,
                history_limit,
            )
            .await?;
        let response = SendMessageResponse {
            payload: Some(send_message_response::Payload::Task(Box::new(task))),
            ..Default::default()
        };
        to_value(&response)
    }

    async fn create_push_config(&self, params: Option<Value>) -> Result<Value, A2AError> {
        let config: TaskPushNotificationConfig = parse_params(params)?;
        let created = self.service.set_push_config(&config).await?;
        to_value(&created)
    }

    async fn get_push_config(&self, params: Option<Value>) -> Result<Value, A2AError> {
        let req: GetTaskPushNotificationConfigRequest = parse_params(params)?;
        let domain_params = crate::domain::GetTaskPushNotificationConfigParams {
            id: req.task_id,
            push_notification_config_id: Some(req.id),
            metadata: None,
        };
        let config = self.service.get_push_config(&domain_params).await?;
        to_value(&config)
    }

    async fn list_push_configs(&self, params: Option<Value>) -> Result<Value, A2AError> {
        let req: ListTaskPushNotificationConfigsRequest = parse_params(params)?;
        let domain_params = crate::domain::ListTaskPushNotificationConfigsParams {
            id: req.task_id,
            metadata: None,
        };
        let configs = self.service.list_push_configs(&domain_params).await?;
        let response = ListTaskPushNotificationConfigsResponse {
            configs,
            ..Default::default()
        };
        to_value(&response)
    }

    async fn delete_push_config(&self, params: Option<Value>) -> Result<Value, A2AError> {
        let req: DeleteTaskPushNotificationConfigRequest = parse_params(params)?;
        let domain_params = crate::domain::DeleteTaskPushNotificationConfigParams {
            id: req.task_id,
            push_notification_config_id: req.id,
            metadata: None,
        };
        self.service.delete_push_config(&domain_params).await?;
        Ok(serde_json::json!({}))
    }

    async fn extended_card(&self) -> Result<Value, A2AError> {
        let card = self.service.extended_agent_card().await?;
        to_value(&card)
    }

    // -- streaming --------------------------------------------------------

    /// Open the SSE stream for a streaming method, running the server
    /// interceptor chain around the open (per-frame interception is out of
    /// scope — `after` observes whether the stream opened, not each event).
    async fn open_stream(
        &self,
        method: &str,
        params: Option<Value>,
        from_event_id: Option<u64>,
    ) -> Result<StreamResponseStream, A2AError> {
        if self.interceptors.is_empty() {
            return self.open_stream_inner(method, params, from_event_id).await;
        }
        let ctx = CallContext::new(method, CallSide::Server);
        if let Err(e) = run_before(&self.interceptors, &ctx).await {
            run_after(&self.interceptors, &ctx, Err(&e)).await;
            return Err(e);
        }
        let result = self.open_stream_inner(method, params, from_event_id).await;
        run_after(&self.interceptors, &ctx, result.as_ref().map(|_| ())).await;
        result
    }

    /// Open the SSE stream for a streaming method, returning a unified stream of
    /// wire [`StreamResponse`]s (initial task snapshot first, then updates).
    ///
    /// `from_event_id` carries the client's `Last-Event-ID` for resumption; it
    /// applies to `tasks/subscribe` (a fresh `message/stream` always starts from
    /// the beginning).
    async fn open_stream_inner(
        &self,
        method: &str,
        params: Option<Value>,
        from_event_id: Option<u64>,
    ) -> Result<StreamResponseStream, A2AError> {
        match method {
            methods::SEND_STREAMING_MESSAGE => {
                let (task_id, message, session_id, push_config, history_limit) =
                    decode_send_message(parse_params(params)?)?;
                let (task, updates) = self
                    .service
                    .send_streaming_message(
                        &task_id,
                        &message,
                        session_id.as_deref(),
                        push_config,
                        history_limit,
                    )
                    .await?;
                Ok(chain_initial_task(Some(task), updates))
            }
            methods::SUBSCRIBE_TO_TASK => {
                let req: SubscribeToTaskRequest = parse_params(params)?;
                let (initial, updates) = self.service.subscribe(&req.id, from_event_id).await?;
                Ok(chain_initial_task(initial, updates))
            }
            unknown => Err(A2AError::MethodNotFound(unknown.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Routers (axum)
// ---------------------------------------------------------------------------

/// Build the JSON-RPC 2.0 router: a single `POST /` endpoint.
///
/// Compose it at the edge with [`rest_router`] and the agent-card route, e.g.
/// `jsonrpc_router(adapter.clone()).merge(rest_router(adapter))`.
pub fn jsonrpc_router(adapter: Arc<JsonRpcAdapter>) -> Router {
    Router::new()
        .route("/", post(jsonrpc_handler))
        .with_state(adapter)
}

async fn jsonrpc_handler(
    State(adapter): State<Arc<JsonRpcAdapter>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let req: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return Json(JsonRpcResponse::err(
                JsonRpcId::Null,
                JsonRpcError {
                    code: error_code::PARSE_ERROR,
                    message: e.to_string(),
                    data: None,
                },
            ))
            .into_response();
        }
    };

    if req.jsonrpc != "2.0" {
        return Json(JsonRpcResponse::err(
            req.id,
            JsonRpcError {
                code: error_code::INVALID_REQUEST,
                message: "jsonrpc must be \"2.0\"".to_string(),
                data: None,
            },
        ))
        .into_response();
    }

    if methods::is_streaming(&req.method) {
        let id = req.id.clone();
        let from_event_id = parse_last_event_id(&headers);
        match adapter
            .open_stream(&req.method, req.params, from_event_id)
            .await
        {
            Ok(stream) => jsonrpc_sse(id, stream).into_response(),
            Err(e) => Json(JsonRpcResponse::err(id, a2a_to_jsonrpc(&e))).into_response(),
        }
    } else {
        Json(adapter.handle_unary(req).await).into_response()
    }
}

/// Frame a [`StreamResponseStream`] as JSON-RPC SSE — each event is a
/// `JsonRpcResponse` whose `result` is the (tag-free union) `StreamResponse`.
fn jsonrpc_sse(
    id: JsonRpcId,
    stream: StreamResponseStream,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = stream.map(move |item| {
        let (seq_id, resp) = match item {
            Ok((seq_id, sr)) => (
                seq_id,
                JsonRpcResponse::ok(id.clone(), serde_json::to_value(&sr).unwrap_or(Value::Null)),
            ),
            Err(e) => (None, JsonRpcResponse::err(id.clone(), a2a_to_jsonrpc(&e))),
        };
        let event = Event::default().data(serde_json::to_string(&resp).unwrap_or_default());
        Ok(match seq_id {
            Some(n) => event.id(n.to_string()),
            None => event,
        })
    });
    Sse::new(events).keep_alive(KeepAlive::default())
}

/// Build the HTTP+JSON (REST) router with the official-SDK paths (no `/v1`
/// prefix). Bodies and responses are bare ProtoJSON (no JSON-RPC envelope).
///
/// The canonical custom-method paths use a `:`-suffix on a collection segment
/// (`/message:send`) — those work as pure-literal segments. The *task*
/// custom-method paths (`/tasks/{id}:cancel`) would put a `:`-suffix on the
/// **same segment as a path parameter**, which axum's matchit router rejects
/// (it conflicts with `/tasks/{id}`). We therefore serve the equivalent
/// slash-form aliases (`/tasks/{id}/cancel`) for those, which official clients
/// also accept.
pub fn rest_router(adapter: Arc<JsonRpcAdapter>) -> Router {
    Router::new()
        .route("/message:send", post(rest_send_message))
        .route("/message/send", post(rest_send_message))
        .route("/message:stream", post(rest_stream_message))
        .route("/message/stream", post(rest_stream_message))
        .route("/tasks", get(rest_list_tasks))
        .route("/tasks/{id}", get(rest_get_task))
        .route("/tasks/{id}/cancel", post(rest_cancel_task))
        .route("/tasks/{id}/subscribe", get(rest_subscribe))
        .route(
            "/tasks/{id}/pushNotificationConfigs",
            post(rest_create_push_config).get(rest_list_push_configs),
        )
        .route(
            "/tasks/{id}/pushNotificationConfigs/{cfg}",
            get(rest_get_push_config).delete(rest_delete_push_config),
        )
        .route("/extendedAgentCard", get(rest_extended_card))
        .route("/card", get(rest_extended_card))
        .with_state(adapter)
}

/// Convert a unary `Result<Value, A2AError>` into a REST HTTP response: 200 with
/// the bare ProtoJSON body, or an error status + `{code, message, data?}`.
fn rest_result(result: Result<Value, A2AError>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => a2a_to_http(&e),
    }
}

/// Map a domain [`A2AError`] onto an HTTP status + JSON error body for REST.
fn a2a_to_http(err: &A2AError) -> Response {
    let status = match err {
        A2AError::TaskNotFound(_) | A2AError::MethodNotFound(_) => StatusCode::NOT_FOUND,
        A2AError::InvalidParams(_) | A2AError::ValidationError { .. } => StatusCode::BAD_REQUEST,
        A2AError::UnsupportedOperation(_) => StatusCode::NOT_IMPLEMENTED,
        A2AError::AuthenticatedExtendedCardNotConfigured => StatusCode::PRECONDITION_FAILED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(a2a_to_jsonrpc(err))).into_response()
}

async fn rest_send_message(State(a): State<Arc<JsonRpcAdapter>>, body: Bytes) -> Response {
    rest_result(
        a.dispatch_intercepted(methods::SEND_MESSAGE, parse_body(&body))
            .await,
    )
}

async fn rest_list_tasks(
    State(a): State<Arc<JsonRpcAdapter>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    rest_result(
        a.dispatch_intercepted(methods::LIST_TASKS, Some(query_to_list_request(&q)))
            .await,
    )
}

async fn rest_get_task(
    State(a): State<Arc<JsonRpcAdapter>>,
    Path(id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let mut req = serde_json::json!({ "id": id });
    if let Some(h) = q.get("historyLength").and_then(|s| s.parse::<i64>().ok()) {
        req["historyLength"] = h.into();
    }
    rest_result(a.dispatch_intercepted(methods::GET_TASK, Some(req)).await)
}

async fn rest_cancel_task(
    State(a): State<Arc<JsonRpcAdapter>>,
    Path(id): Path<String>,
) -> Response {
    rest_result(
        a.dispatch_intercepted(methods::CANCEL_TASK, Some(serde_json::json!({ "id": id })))
            .await,
    )
}

async fn rest_create_push_config(
    State(a): State<Arc<JsonRpcAdapter>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    // The path task id is authoritative for the config's parent.
    let mut config = parse_body(&body).unwrap_or_else(|| serde_json::json!({}));
    config["taskId"] = id.into();
    rest_result(
        a.dispatch_intercepted(methods::CREATE_PUSH_CONFIG, Some(config))
            .await,
    )
}

async fn rest_list_push_configs(
    State(a): State<Arc<JsonRpcAdapter>>,
    Path(id): Path<String>,
) -> Response {
    rest_result(
        a.dispatch_intercepted(
            methods::LIST_PUSH_CONFIGS,
            Some(serde_json::json!({ "taskId": id })),
        )
        .await,
    )
}

async fn rest_get_push_config(
    State(a): State<Arc<JsonRpcAdapter>>,
    Path((id, cfg)): Path<(String, String)>,
) -> Response {
    rest_result(
        a.dispatch_intercepted(
            methods::GET_PUSH_CONFIG,
            Some(serde_json::json!({ "taskId": id, "id": cfg })),
        )
        .await,
    )
}

async fn rest_delete_push_config(
    State(a): State<Arc<JsonRpcAdapter>>,
    Path((id, cfg)): Path<(String, String)>,
) -> Response {
    rest_result(
        a.dispatch_intercepted(
            methods::DELETE_PUSH_CONFIG,
            Some(serde_json::json!({ "taskId": id, "id": cfg })),
        )
        .await,
    )
}

async fn rest_extended_card(State(a): State<Arc<JsonRpcAdapter>>) -> Response {
    rest_result(
        a.dispatch_intercepted(methods::GET_EXTENDED_AGENT_CARD, None)
            .await,
    )
}

async fn rest_stream_message(
    State(a): State<Arc<JsonRpcAdapter>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let from_event_id = parse_last_event_id(&headers);
    match a
        .open_stream(
            methods::SEND_STREAMING_MESSAGE,
            parse_body(&body),
            from_event_id,
        )
        .await
    {
        Ok(stream) => rest_sse(stream).into_response(),
        Err(e) => a2a_to_http(&e),
    }
}

async fn rest_subscribe(
    State(a): State<Arc<JsonRpcAdapter>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let from_event_id = parse_last_event_id(&headers);
    match a
        .open_stream(
            methods::SUBSCRIBE_TO_TASK,
            Some(serde_json::json!({ "id": id })),
            from_event_id,
        )
        .await
    {
        Ok(stream) => rest_sse(stream).into_response(),
        Err(e) => a2a_to_http(&e),
    }
}

/// Parse the SSE `Last-Event-ID` header into a per-task event id for resumption.
///
/// This is the server half of the a2a-rs resumption enhancement (not an A2A v1.0
/// spec feature). Spec-compliant clients never send the header, so they always
/// get a fresh stream from current state — the `SubscribeToTask` behavior the
/// spec defines. The complementary SSE `id:` field is emitted by [`jsonrpc_sse`]
/// / [`rest_sse`] and is inert for clients that don't use it.
fn parse_last_event_id(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
}

/// Frame a [`StreamResponseStream`] as bare-ProtoJSON SSE (REST has no envelope).
fn rest_sse(stream: StreamResponseStream) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = stream.map(|item| {
        let (seq_id, data) = match item {
            Ok((seq_id, sr)) => (seq_id, serde_json::to_string(&sr).unwrap_or_default()),
            Err(e) => (
                None,
                serde_json::to_string(&a2a_to_jsonrpc(&e)).unwrap_or_default(),
            ),
        };
        let event = Event::default().data(data);
        Ok(match seq_id {
            Some(n) => event.id(n.to_string()),
            None => event,
        })
    });
    Sse::new(events).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deserialize JSON-RPC `params` into a concrete proto request type, mapping
/// serde failures to `InvalidParams`.
fn parse_params<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, A2AError> {
    serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|e| A2AError::InvalidParams(format!("invalid params: {e}")))
}

/// Parse a REST request body into a JSON value (empty body → `None`).
fn parse_body(body: &Bytes) -> Option<Value> {
    if body.is_empty() {
        None
    } else {
        serde_json::from_slice(body).ok()
    }
}

/// Serialize a domain/wire value into a JSON `result`, mapping failures to an
/// internal error.
fn to_value<T: Serialize>(value: &T) -> Result<Value, A2AError> {
    serde_json::to_value(value)
        .map_err(|e| A2AError::InvalidParams(format!("failed to serialize result: {e}")))
}

/// Decode a [`SendMessageRequest`] into the arguments [`TaskService::send_message`]
/// expects. Mirrors the Connect adapter's `send_message` decode exactly.
type SendArgs = (
    String,
    crate::domain::Message,
    Option<String>,
    Option<TaskPushNotificationConfig>,
    Option<u32>,
);
fn decode_send_message(req: SendMessageRequest) -> Result<SendArgs, A2AError> {
    let message = req
        .message
        .into_option()
        .ok_or_else(|| A2AError::InvalidParams("missing message".to_string()))?;
    let task_id = message.task_id.clone();
    let session_id = (!message.context_id.is_empty()).then(|| message.context_id.clone());
    let (push_config, history_limit) = decode_send_config(req.configuration.into_option());
    Ok((task_id, message, session_id, push_config, history_limit))
}

/// Build the SSE stream: initial task snapshot (if present) followed by the
/// mapped update events. Mirrors `connectrpc.rs`'s `stream::once(task).chain(...)`.
fn chain_initial_task(
    initial: Option<crate::domain::Task>,
    updates: crate::application::UpdateStream,
) -> StreamResponseStream {
    let mapped = updates.map(|item| item.map(|seq| (Some(seq.id), map_update_event(seq.event))));
    match initial {
        Some(task) => {
            let head = StreamResponse {
                payload: Some(stream_response::Payload::Task(Box::new(task))),
                ..Default::default()
            };
            Box::pin(futures::stream::once(async move { Ok((None, head)) }).chain(mapped))
        }
        None => Box::pin(mapped),
    }
}

/// Assemble a `ListTasksRequest`-shaped JSON object from REST query parameters,
/// coercing numeric/boolean fields to their proto JSON types.
fn query_to_list_request(q: &std::collections::HashMap<String, String>) -> Value {
    let mut req = serde_json::Map::new();
    if let Some(v) = q.get("contextId") {
        req.insert("contextId".to_string(), v.clone().into());
    }
    if let Some(v) = q.get("status") {
        req.insert("status".to_string(), v.clone().into());
    }
    if let Some(v) = q.get("pageToken") {
        req.insert("pageToken".to_string(), v.clone().into());
    }
    if let Some(v) = q.get("pageSize").and_then(|s| s.parse::<i64>().ok()) {
        req.insert("pageSize".to_string(), v.into());
    }
    if let Some(v) = q.get("historyLength").and_then(|s| s.parse::<i64>().ok()) {
        req.insert("historyLength".to_string(), v.into());
    }
    if let Some(v) = q
        .get("includeArtifacts")
        .and_then(|s| s.parse::<bool>().ok())
    {
        req.insert("includeArtifacts".to_string(), v.into());
    }
    if let Some(v) = q.get("statusTimestampAfter") {
        req.insert("statusTimestampAfter".to_string(), v.clone().into());
    }
    Value::Object(req)
}
