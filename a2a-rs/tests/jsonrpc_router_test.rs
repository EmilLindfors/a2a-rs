//! End-to-end tests for the JSON-RPC / REST **routers** (the surface the
//! dispatch tests don't reach).
//!
//! [`jsonrpc_dispatch_test`] drives [`JsonRpcAdapter::handle_unary`] directly;
//! this file stands up the real `axum` routers and drives them with
//! `tower::ServiceExt::oneshot`, so it covers the parts that only exist at the
//! router layer: REST path/query extraction, the `/tasks/{id}/cancel` slash
//! alias, HTTP status mapping, and — most importantly — the two **SSE framings**
//! (`jsonrpc_sse` wraps each event in a JSON-RPC envelope; `rest_sse` emits the
//! bare ProtoJSON `StreamResponse`).

#![cfg(feature = "jsonrpc-server")]

mod common;

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header::CONTENT_TYPE};
use common::TestBusinessHandler;
use futures::{Stream, StreamExt, stream};
use serde_json::{Value, json};
use tower::ServiceExt;

use a2a_rs::adapter::{
    InMemoryTaskStorage, JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router, rest_router,
};
use a2a_rs::domain::{A2AError, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_rs::port::AsyncStreamingHandler;
use a2a_rs::port::streaming_handler::{SeqEvent, Subscriber};

/// A streaming handler whose pull-streams are empty but valid.
///
/// `InMemoryTaskStorage` models streaming as subscriber push and returns
/// `UnsupportedOperation` from `combined_update_stream`, so `TaskService`'s
/// stream methods can't run against it. These tests only need the SSE *framing*
/// to be exercised — the initial task snapshot is emitted by the adapter ahead
/// of the (here empty) update stream — so a handler that returns an empty stream
/// is enough to drive `open_stream` to success.
#[derive(Clone)]
struct EmptyStreamHandler;

type StatusStream = Pin<Box<dyn Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>;
type ArtifactStream = Pin<Box<dyn Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>;
type CombinedStream = Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>;

#[async_trait]
impl AsyncStreamingHandler for EmptyStreamHandler {
    async fn add_status_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Ok("status-sub".to_string())
    }

    async fn add_artifact_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Ok("artifact-sub".to_string())
    }

    async fn remove_subscription(&self, _subscription_id: &str) -> Result<(), A2AError> {
        Ok(())
    }

    async fn remove_task_subscribers(&self, _task_id: &str) -> Result<(), A2AError> {
        Ok(())
    }

    async fn get_subscriber_count(&self, _task_id: &str) -> Result<usize, A2AError> {
        Ok(0)
    }

    async fn broadcast_status_update(
        &self,
        _task_id: &str,
        _update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        Ok(())
    }

    async fn broadcast_artifact_update(
        &self,
        _task_id: &str,
        _update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        Ok(())
    }

    async fn status_update_stream(&self, _task_id: &str) -> Result<StatusStream, A2AError> {
        Ok(Box::pin(stream::empty::<Result<TaskStatusUpdateEvent, A2AError>>()))
    }

    async fn artifact_update_stream(&self, _task_id: &str) -> Result<ArtifactStream, A2AError> {
        Ok(Box::pin(stream::empty::<Result<TaskArtifactUpdateEvent, A2AError>>()))
    }

    async fn combined_update_stream(
        &self,
        _task_id: &str,
        _from_event_id: Option<u64>,
    ) -> Result<CombinedStream, A2AError> {
        Ok(Box::pin(stream::empty::<Result<SeqEvent, A2AError>>()))
    }
}

/// An adapter wired with a working (empty) streaming backend.
fn streaming_adapter() -> Arc<JsonRpcAdapter> {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let agent_info = SimpleAgentInfo::new("router-test".to_string(), "http://localhost".to_string());
    Arc::new(
        JsonRpcAdapter::with_handler(handler, agent_info).with_streaming_handler(EmptyStreamHandler),
    )
}

/// Build an adapter backed by a real in-memory streaming handler so the SSE
/// methods emit the initial task snapshot.
fn adapter() -> Arc<JsonRpcAdapter> {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let agent_info = SimpleAgentInfo::new("router-test".to_string(), "http://localhost".to_string());
    Arc::new(
        JsonRpcAdapter::with_handler(handler.clone(), agent_info).with_streaming_handler(handler),
    )
}

fn send_message_body(task_id: &str) -> Value {
    json!({
        "message": {
            "messageId": "m1",
            "role": "ROLE_USER",
            "parts": [{ "text": "hello" }],
            "taskId": task_id,
        }
    })
}

fn post(uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().method("GET").uri(uri).body(Body::empty()).unwrap()
}

/// Drive a request through the REST router and return `(status, json_body)`.
async fn rest_call(adapter: &Arc<JsonRpcAdapter>, req: Request<Body>) -> (StatusCode, Value) {
    let resp = rest_router(adapter.clone()).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// Drive a request through the JSON-RPC router and return `(status, json_body)`.
async fn jsonrpc_call(adapter: &Arc<JsonRpcAdapter>, body: &Value) -> (StatusCode, Value) {
    let resp = jsonrpc_router(adapter.clone()).oneshot(post("/", body)).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// Read the first SSE event's `data:` payload as JSON, with a timeout so a
/// keep-alive stream that never yields fails the test rather than hanging.
async fn first_sse_event(resp: axum::response::Response) -> Value {
    assert_eq!(resp.status(), StatusCode::OK, "SSE endpoint should return 200");
    let ct = resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    assert!(ct.starts_with("text/event-stream"), "expected SSE content-type, got {ct:?}");

    let mut stream = resp.into_body().into_data_stream();
    let mut buf = String::new();
    let deadline = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => panic!("timed out waiting for an SSE data line; buffered: {buf:?}"),
            chunk = stream.next() => {
                let chunk = chunk.expect("stream ended before a data line").expect("stream error");
                buf.push_str(&String::from_utf8_lossy(&chunk));
                if let Some(line) = buf.lines().find(|l| l.starts_with("data:")) {
                    let payload = line.trim_start_matches("data:").trim();
                    return serde_json::from_str(payload).expect("SSE data is not JSON");
                }
            }
        }
    }
}

// --- REST unary ------------------------------------------------------------

#[tokio::test]
async fn rest_send_then_get_round_trips() {
    let a = adapter();

    let (status, body) = rest_call(&a, post("/message:send", &send_message_body("t1"))).await;
    assert_eq!(status, StatusCode::OK);
    // SendMessageResponse field-presence union: `{ "task": { ... } }`.
    assert_eq!(body["task"]["id"], "t1");

    let (status, body) = rest_call(&a, get("/tasks/t1")).await;
    assert_eq!(status, StatusCode::OK);
    // GetTask returns a bare Task, not a union.
    assert_eq!(body["id"], "t1");
}

#[tokio::test]
async fn rest_cancel_slash_alias_works() {
    let a = adapter();
    rest_call(&a, post("/message:send", &send_message_body("t2"))).await;

    // The canonical `/tasks/{id}:cancel` colon form is unroutable in matchit;
    // the adapter serves the slash alias instead. Official clients accept both.
    let (status, body) = rest_call(&a, post("/tasks/t2/cancel", &json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], "t2");
}

#[tokio::test]
async fn rest_get_missing_task_is_404() {
    let a = adapter();
    let (status, _body) = rest_call(&a, get("/tasks/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rest_list_tasks_via_query() {
    let a = adapter();
    rest_call(&a, post("/message:send", &send_message_body("t3"))).await;

    let (status, body) = rest_call(&a, get("/tasks?pageSize=10")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["tasks"].is_array());
}

// --- JSON-RPC unary --------------------------------------------------------

#[tokio::test]
async fn jsonrpc_send_message_envelope() {
    let a = adapter();
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "SendMessage",
        "params": send_message_body("j1"),
    });
    let (status, resp) = jsonrpc_call(&a, &body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert!(resp.get("error").is_none(), "unexpected error: {resp:?}");
    assert_eq!(resp["result"]["task"]["id"], "j1");
}

#[tokio::test]
async fn jsonrpc_rejects_wrong_version() {
    let a = adapter();
    let body = json!({ "jsonrpc": "1.0", "id": 1, "method": "GetTask", "params": { "id": "x" } });
    let (status, resp) = jsonrpc_call(&a, &body).await;
    assert_eq!(status, StatusCode::OK); // JSON-RPC errors ride in the body, not the HTTP status
    assert_eq!(resp["error"]["code"], -32600); // INVALID_REQUEST
}

// --- SSE framing (the part only the router exercises) ----------------------

#[tokio::test]
async fn jsonrpc_stream_frames_events_in_envelopes() {
    let a = streaming_adapter();
    let body = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "SendStreamingMessage",
        "params": send_message_body("s1"),
    });
    let resp = jsonrpc_router(a.clone()).oneshot(post("/", &body)).await.unwrap();
    let event = first_sse_event(resp).await;

    // JSON-RPC SSE: each event is a full response envelope whose `result` is the
    // tag-free `StreamResponse` union — here the initial task snapshot.
    assert_eq!(event["jsonrpc"], "2.0");
    assert_eq!(event["id"], 7);
    assert_eq!(event["result"]["task"]["id"], "s1");
}

#[tokio::test]
async fn rest_stream_frames_bare_protojson() {
    let a = streaming_adapter();
    let resp = rest_router(a.clone())
        .oneshot(post("/message:stream", &send_message_body("s2")))
        .await
        .unwrap();
    let event = first_sse_event(resp).await;

    // REST SSE has no envelope: the event data IS the bare `StreamResponse`.
    assert!(event.get("jsonrpc").is_none(), "REST SSE must not carry a JSON-RPC envelope");
    assert_eq!(event["task"]["id"], "s2");
}
