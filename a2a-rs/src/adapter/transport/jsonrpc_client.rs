//! Wire-compatible JSON-RPC 2.0 client adapter.
//!
//! [`JsonRpcClient`] is the client-side counterpart of
//! [`JsonRpcAdapter`](super::jsonrpc::JsonRpcAdapter): it implements the
//! [`Transport`] port by speaking the spec-mandated JSON-RPC 2.0 wire format
//! (single `POST` endpoint, SSE for streaming) that the official Go/C#/Python
//! SDKs use. This lets our client talk to any standard A2A agent.
//!
//! Request `params` and response `result` bodies are the **generated proto
//! types** (`SendMessageRequest`, `Task`, `SendMessageResponse`, …), which
//! already serialize as canonical ProtoJSON — the same representation the server
//! adapter decodes. The method names, error codes, and envelopes come from the
//! shared [`jsonrpc_wire`](super::jsonrpc_wire) module so the two directions
//! cannot drift.

use async_trait::async_trait;
use futures::stream::Stream;
use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::{
    adapter::error::HttpClientError,
    adapter::transport::codec::stream_response_to_item,
    domain::{
        A2AError, AgentCard, ListTasksParams, ListTasksResult, Message, Task,
        TaskPushNotificationConfig,
        generated::{
            CancelTaskRequest, DeleteTaskPushNotificationConfigRequest,
            GetTaskPushNotificationConfigRequest, GetTaskRequest,
            ListTaskPushNotificationConfigsRequest, ListTaskPushNotificationConfigsResponse,
            ListTasksRequest, ListTasksResponse, SendMessageConfiguration, SendMessageRequest,
            SendMessageResponse, StreamResponse, SubscribeToTaskRequest, TaskState,
            send_message_response,
        },
    },
    port::{
        CallContext, CallInterceptor, CallSide, StreamEvent, StreamItem, Transport, run_after,
        run_before,
    },
};

use super::jsonrpc_wire::{JsonRpcId, JsonRpcRequest, JsonRpcResponse, jsonrpc_to_a2a, methods};

/// A wire-compatible JSON-RPC 2.0 client for the A2A protocol.
///
/// Mirrors [`HttpClient`](super::http::HttpClient)'s constructors so an
/// application can swap the ConnectRPC transport for JSON-RPC with one line.
pub struct JsonRpcClient {
    /// Base URL of the agent (also the JSON-RPC `POST` endpoint root).
    base_url: String,
    client: Client,
    auth_token: Option<String>,
    /// Request timeout in seconds.
    timeout: u64,
    /// Client-side interceptor chain wrapping every call.
    interceptors: Vec<Arc<dyn CallInterceptor>>,
}

impl JsonRpcClient {
    /// Create a new JSON-RPC client targeting `base_url`.
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
            auth_token: None,
            timeout: 30,
            interceptors: Vec::new(),
        }
    }

    /// Create a JSON-RPC client with a bearer auth token.
    pub fn with_auth(base_url: String, auth_token: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
            auth_token: Some(auth_token),
            timeout: 30,
            interceptors: Vec::new(),
        }
    }

    /// Set the request timeout (seconds).
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    /// Append a client-side [`CallInterceptor`] to the chain.
    ///
    /// Interceptors wrap every call (`rpc` and the streaming subscribe):
    /// `before` hooks run in registration order, then the request is sent, then
    /// `after` hooks run in reverse. Chainable.
    pub fn with_interceptor(mut self, interceptor: impl CallInterceptor + 'static) -> Self {
        self.interceptors.push(Arc::new(interceptor));
        self
    }

    /// Get the base URL of the client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn headers(&self) -> Result<HeaderMap, A2AError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        if let Some(token) = &self.auth_token {
            let value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| A2AError::Internal(format!("Invalid auth token for header: {e}")))?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        Ok(headers)
    }

    /// Resolve a path relative to the base URL (handles trailing-slash variance).
    fn join(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        format!("{base}/{path}")
    }

    /// Fetch the agent card from the well-known endpoint (plain HTTP GET).
    ///
    /// Tries the spec path `/.well-known/agent-card.json` first, falling back to
    /// the legacy `/agent-card` path.
    pub async fn get_agent_card(&self) -> Result<AgentCard, A2AError> {
        for path in [".well-known/agent-card.json", "agent-card"] {
            let url = self.join(path);
            let resp = self
                .client
                .get(&url)
                .headers(self.headers()?)
                .timeout(Duration::from_secs(self.timeout))
                .send()
                .await
                .map_err(HttpClientError::Reqwest)?;
            if resp.status().is_success() {
                return resp.json::<AgentCard>().await.map_err(|e| {
                    A2AError::Internal(format!("Failed to parse agent card JSON: {e}"))
                });
            }
        }
        Err(A2AError::Internal(format!(
            "Agent card not found at {}",
            self.base_url
        )))
    }

    /// Send a JSON-RPC request envelope and decode the typed `result`, running
    /// the client interceptor chain around the call.
    async fn rpc<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
    ) -> Result<T, A2AError> {
        if self.interceptors.is_empty() {
            return self.rpc_inner(method, params).await;
        }
        let ctx = CallContext::new(method, CallSide::Client);
        run_before(&self.interceptors, &ctx).await?;
        let result = self.rpc_inner(method, params).await;
        run_after(&self.interceptors, &ctx, result.as_ref().map(|_| ())).await;
        result
    }

    /// The un-intercepted JSON-RPC round-trip.
    async fn rpc_inner<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
    ) -> Result<T, A2AError> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: JsonRpcId::Num(1),
            method: method.to_string(),
            params: Some(
                serde_json::to_value(params)
                    .map_err(|e| A2AError::Internal(format!("failed to encode params: {e}")))?,
            ),
        };

        let response = self
            .client
            .post(&self.base_url)
            .headers(self.headers()?)
            .timeout(Duration::from_secs(self.timeout))
            .json(&request)
            .send()
            .await
            .map_err(HttpClientError::Reqwest)?;

        let body: JsonRpcResponse = response
            .json()
            .await
            .map_err(|e| A2AError::Internal(format!("invalid JSON-RPC response: {e}")))?;

        if let Some(err) = body.error {
            return Err(jsonrpc_to_a2a(&err));
        }
        let result = body
            .result
            .ok_or_else(|| A2AError::Internal("JSON-RPC response missing result".to_string()))?;
        serde_json::from_value(result)
            .map_err(|e| A2AError::Internal(format!("failed to decode result: {e}")))
    }

    /// The un-intercepted streaming subscribe (SSE round-trip). `last_event_id`,
    /// when set, is sent as the `Last-Event-ID` header so the server replays
    /// events after that id before streaming live updates.
    async fn subscribe_inner(
        &self,
        task_id: &str,
        last_event_id: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>, A2AError> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: JsonRpcId::Num(1),
            method: methods::SUBSCRIBE_TO_TASK.to_string(),
            params: Some(
                serde_json::to_value(SubscribeToTaskRequest {
                    id: task_id.to_string(),
                    ..Default::default()
                })
                .map_err(|e| A2AError::Internal(format!("failed to encode params: {e}")))?,
            ),
        };

        let mut builder = self
            .client
            .post(&self.base_url)
            .headers(self.headers()?)
            .header(reqwest::header::ACCEPT, "text/event-stream");
        if let Some(id) = last_event_id {
            builder = builder.header("last-event-id", id);
        }
        let response = builder
            .json(&request)
            .send()
            .await
            .map_err(HttpClientError::Reqwest)?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(HttpClientError::Response { status, message: body }.into());
        }

        Ok(Box::pin(sse_stream(response)))
    }
}

#[async_trait]
impl Transport for JsonRpcClient {
    fn protocol(&self) -> &str {
        "JSONRPC"
    }

    async fn send_task_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        let mut msg = message.clone();
        msg.task_id = task_id.to_string();
        if let Some(sid) = session_id {
            msg.context_id = sid.to_string();
        }

        let request = SendMessageRequest {
            message: ::buffa::MessageField::some(msg),
            configuration: ::buffa::MessageField::some(SendMessageConfiguration {
                history_length: history_length.map(|l| l as i32),
                ..Default::default()
            }),
            ..Default::default()
        };

        let response: SendMessageResponse = self.rpc(methods::SEND_MESSAGE, &request).await?;
        match response.payload {
            Some(send_message_response::Payload::Task(task)) => Ok(*task),
            _ => Err(A2AError::Internal(
                "Expected task in SendMessageResponse payload".to_string(),
            )),
        }
    }

    async fn get_task(&self, task_id: &str, history_length: Option<u32>) -> Result<Task, A2AError> {
        let request = GetTaskRequest {
            id: task_id.to_string(),
            history_length: history_length.map(|l| l as i32),
            ..Default::default()
        };
        self.rpc(methods::GET_TASK, &request).await
    }

    async fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError> {
        let request = CancelTaskRequest {
            id: task_id.to_string(),
            ..Default::default()
        };
        self.rpc(methods::CANCEL_TASK, &request).await
    }

    async fn set_task_push_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.rpc(methods::CREATE_PUSH_CONFIG, config).await
    }

    async fn get_task_push_notification(
        &self,
        task_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Mirrors the ConnectRPC client: list configs and take the first.
        let configs = self.list_push_notification_configs(task_id).await?;
        configs.into_iter().next().ok_or_else(|| {
            A2AError::TaskNotFound(format!("No push notification config found for task {task_id}"))
        })
    }

    async fn list_tasks(&self, params: &ListTasksParams) -> Result<ListTasksResult, A2AError> {
        let mut request = ListTasksRequest {
            context_id: params.context_id.clone().unwrap_or_default(),
            status: ::buffa::EnumValue::from(
                params.status.unwrap_or(TaskState::TASK_STATE_UNSPECIFIED),
            ),
            page_size: params.page_size,
            page_token: params.page_token.clone().unwrap_or_default(),
            history_length: params.history_length,
            include_artifacts: params.include_artifacts,
            ..Default::default()
        };
        if let Some(ref t_str) = params.status_timestamp_after {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t_str) {
                let utc_dt = dt.with_timezone(&chrono::Utc);
                request.status_timestamp_after =
                    ::buffa::MessageField::some(::buffa_types::google::protobuf::Timestamp {
                        seconds: utc_dt.timestamp(),
                        nanos: utc_dt.timestamp_subsec_nanos() as i32,
                        ..Default::default()
                    });
            }
        }

        let response: ListTasksResponse = self.rpc(methods::LIST_TASKS, &request).await?;
        Ok(ListTasksResult {
            tasks: response.tasks,
            total_size: response.total_size,
            page_size: response.page_size,
            next_page_token: response.next_page_token,
        })
    }

    async fn list_push_notification_configs(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        let request = ListTaskPushNotificationConfigsRequest {
            task_id: task_id.to_string(),
            ..Default::default()
        };
        let response: ListTaskPushNotificationConfigsResponse =
            self.rpc(methods::LIST_PUSH_CONFIGS, &request).await?;
        Ok(response.configs)
    }

    async fn get_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        let request = GetTaskPushNotificationConfigRequest {
            task_id: task_id.to_string(),
            id: config_id.to_string(),
            ..Default::default()
        };
        self.rpc(methods::GET_PUSH_CONFIG, &request).await
    }

    async fn delete_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<(), A2AError> {
        let request = DeleteTaskPushNotificationConfigRequest {
            task_id: task_id.to_string(),
            id: config_id.to_string(),
            ..Default::default()
        };
        // The server replies with an empty object `{}`; we only care that it
        // succeeded.
        let _: serde::de::IgnoredAny = self.rpc(methods::DELETE_PUSH_CONFIG, &request).await?;
        Ok(())
    }

    async fn subscribe_to_task(
        &self,
        task_id: &str,
        _history_length: Option<u32>,
        last_event_id: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>, A2AError> {
        if self.interceptors.is_empty() {
            return self.subscribe_inner(task_id, last_event_id).await;
        }
        let ctx = CallContext::new(methods::SUBSCRIBE_TO_TASK, CallSide::Client);
        run_before(&self.interceptors, &ctx).await?;
        let result = self.subscribe_inner(task_id, last_event_id).await;
        run_after(&self.interceptors, &ctx, result.as_ref().map(|_| ())).await;
        result
    }
}

// ---------------------------------------------------------------------------
// SSE consumption
// ---------------------------------------------------------------------------

/// Reassemble an `text/event-stream` body into a stream of [`StreamEvent`]s.
///
/// Each SSE event is a `data:` payload carrying a [`JsonRpcResponse`] whose
/// `result` is a [`StreamResponse`] union (this is exactly what
/// [`JsonRpcAdapter`](super::jsonrpc::JsonRpcAdapter)'s SSE path emits), plus an
/// optional `id:` line carrying the server's per-task event id (surfaced on the
/// [`StreamEvent`] for `Last-Event-ID` resumption). Chunks from the socket may
/// split mid-event or mid-UTF-8-sequence, so we buffer and only emit on a
/// complete event boundary (`\n\n`).
fn sse_stream(response: reqwest::Response) -> impl Stream<Item = Result<StreamEvent, A2AError>> + Send {
    struct State {
        response: reqwest::Response,
        buf: String,
        pending: VecDeque<Result<StreamEvent, A2AError>>,
        done: bool,
    }

    let state = State {
        response,
        buf: String::new(),
        pending: VecDeque::new(),
        done: false,
    };

    futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.pending.pop_front() {
                return Some((item, st));
            }
            if st.done {
                return None;
            }
            match st.response.chunk().await {
                Ok(Some(chunk)) => {
                    st.buf.push_str(&String::from_utf8_lossy(&chunk));
                    drain_sse_events(&mut st.buf, &mut st.pending, false);
                }
                Ok(None) => {
                    drain_sse_events(&mut st.buf, &mut st.pending, true);
                    st.done = true;
                }
                Err(e) => {
                    st.pending
                        .push_back(Err(A2AError::Internal(format!("SSE read error: {e}"))));
                    st.done = true;
                }
            }
        }
    })
}

/// Extract complete SSE events from `buf`, pushing each decoded event to `out`.
/// When `flush` is true, a trailing event with no terminating blank line is also
/// processed (end of stream).
fn drain_sse_events(
    buf: &mut String,
    out: &mut VecDeque<Result<StreamEvent, A2AError>>,
    flush: bool,
) {
    loop {
        let event = match buf.find("\n\n") {
            Some(i) => {
                let event = buf[..i].to_string();
                *buf = buf[i + 2..].to_string();
                event
            }
            None => {
                if flush && !buf.trim().is_empty() {
                    std::mem::take(buf)
                } else {
                    return;
                }
            }
        };

        let data: String = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");

        let event_id = event
            .lines()
            .find_map(|line| line.strip_prefix("id:").map(str::trim_start))
            .and_then(|s| s.parse::<u64>().ok());

        if !data.is_empty() {
            out.push_back(parse_sse_frame(&data).map(|item| StreamEvent::new(event_id, item)));
        }

        if flush && buf.is_empty() {
            return;
        }
    }
}

/// Decode one SSE `data:` payload (a JSON-RPC response frame) into a [`StreamItem`].
fn parse_sse_frame(data: &str) -> Result<StreamItem, A2AError> {
    let frame: JsonRpcResponse = serde_json::from_str(data)
        .map_err(|e| A2AError::Internal(format!("invalid SSE JSON-RPC frame: {e}")))?;
    if let Some(err) = frame.error {
        return Err(jsonrpc_to_a2a(&err));
    }
    let value = frame
        .result
        .ok_or_else(|| A2AError::Internal("SSE frame missing result".to_string()))?;
    let stream_response: StreamResponse = serde_json::from_value(value)
        .map_err(|e| A2AError::Internal(format!("invalid StreamResponse: {e}")))?;
    stream_response_to_item(stream_response)
        .ok_or_else(|| A2AError::Internal("empty stream response payload".to_string()))
}
