//! In-process interop round-trip: the JSON-RPC **client** against the JSON-RPC
//! **server**, over a real socket.
//!
//! This proves byte-compatibility of [`JsonRpcClient`] with
//! [`JsonRpcAdapter`](a2a_rs::adapter::JsonRpcAdapter): the client's JSON-RPC
//! envelopes + ProtoJSON bodies are exactly what the server decodes, and the
//! client's SSE reassembly parses exactly what the server emits. A real
//! `TcpListener` (not `tower::oneshot`) is required because the streaming path
//! drives `reqwest` over a live connection.

#![cfg(all(feature = "jsonrpc-client", feature = "jsonrpc-server"))]

mod common;

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{Json, Router, routing::get};
use common::TestBusinessHandler;
use futures::{Stream, StreamExt, stream};

use a2a_rs::adapter::{InMemoryTaskStorage, JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router};
use a2a_rs::domain::{
    A2AError, AgentCard, AgentInterface, ContextId, Message, SendCompletion,
    TaskArtifactUpdateEvent, TaskId, TaskPushNotificationConfig, TaskState, TaskStatus,
    TaskStatusUpdateEvent,
};
use a2a_rs::port::streaming_handler::{SeqEvent, Subscriber};
use a2a_rs::port::{AsyncStreamingHandler, AsyncTaskLifecycle};
use a2a_rs::{JsonRpcClient, StreamItem, Transport, connect, default_registry};

// ---------------------------------------------------------------------------
// A streaming handler whose pull-streams are empty but valid, so `subscribe`
// emits the initial task snapshot then completes. (InMemoryTaskStorage returns
// `UnsupportedOperation` from `combined_update_stream`, which would fail
// `subscribe`.)
// ---------------------------------------------------------------------------

type StatusStream = Pin<Box<dyn Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>;
type ArtifactStream = Pin<Box<dyn Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>;
type CombinedStream = Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>;

#[derive(Clone)]
struct EmptyStreamHandler;

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
        Ok(Box::pin(stream::empty()))
    }
    async fn artifact_update_stream(&self, _task_id: &str) -> Result<ArtifactStream, A2AError> {
        Ok(Box::pin(stream::empty()))
    }
    async fn combined_update_stream(
        &self,
        _task_id: &str,
        _from_event_id: Option<u64>,
    ) -> Result<CombinedStream, A2AError> {
        Ok(Box::pin(stream::empty()))
    }
}

// ---------------------------------------------------------------------------
// Server harness
// ---------------------------------------------------------------------------

/// Spawn the JSON-RPC server (with an agent-card route) on an ephemeral port and
/// return its base URL.
async fn spawn_server() -> String {
    spawn_server_with_handler().await.0
}

/// As [`spawn_server`], also handing back the handler — the seam a test needs to
/// put a task into a state the wire cannot reach on its own.
async fn spawn_server_with_handler() -> (String, TestBusinessHandler) {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let agent_info = SimpleAgentInfo::new("interop".to_string(), "http://localhost".to_string());
    let adapter = Arc::new(
        JsonRpcAdapter::with_handler(handler.clone(), agent_info)
            .with_streaming_handler(EmptyStreamHandler),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());

    let card = AgentCard {
        supported_interfaces: vec![AgentInterface {
            url: base.clone(),
            protocol_binding: "JSONRPC".to_string(),
            protocol_version: "1.0".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };

    let app: Router = jsonrpc_router(adapter).route(
        "/.well-known/agent-card.json",
        get(move || {
            let card = card.clone();
            async move { Json(card) }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (base, handler)
}

fn message() -> Message {
    Message::user_text("hello".to_string(), "m1".to_string())
}

/// Spawn a server whose streaming backend is a real (shared) in-memory handler,
/// returning the base URL and a handle to broadcast through the same backend.
async fn spawn_server_streaming() -> (String, TestBusinessHandler) {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let agent_info = SimpleAgentInfo::new("interop".to_string(), "http://localhost".to_string());
    let adapter = Arc::new(
        JsonRpcAdapter::with_handler(handler.clone(), agent_info)
            .with_streaming_handler(handler.clone()),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app: Router = jsonrpc_router(adapter);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, handler)
}

fn status_update(task_id: &str, state: TaskState) -> TaskStatusUpdateEvent {
    TaskStatusUpdateEvent {
        task_id: task_id.to_string(),
        context_id: "ctx".to_string(),
        kind: "status-update".to_string(),
        status: TaskStatus::new(state, None),
        metadata: None,
    }
}

/// End-to-end Last-Event-ID resumption: the server emits SSE `id:` fields, the
/// client parses them, and reconnecting with `Last-Event-ID` replays only the
/// buffered events after that id (preceded by the initial task snapshot).
#[tokio::test]
async fn subscribe_resumes_from_last_event_id() {
    let (base, handler) = spawn_server_streaming().await;
    let client = JsonRpcClient::new(base);

    // Create the task so subscribe emits an initial snapshot.
    client
        .send_task_message(
            "task-resume",
            &message(),
            None,
            None,
            SendCompletion::WhenSettled,
        )
        .await
        .unwrap();

    // Two updates broadcast before any subscriber — buffered as event ids 1 and 2.
    handler
        .broadcast_status_update(
            "task-resume",
            status_update("task-resume", TaskState::Working),
        )
        .await
        .unwrap();
    handler
        .broadcast_status_update(
            "task-resume",
            status_update("task-resume", TaskState::Completed),
        )
        .await
        .unwrap();

    // First subscription replays everything (id > 0); discover the id the server
    // assigned to the Completed event (the message handler may emit its own
    // events too, so we don't assume absolute ids).
    let mut all = client
        .subscribe_to_task("task-resume", None, Some("0"))
        .await
        .unwrap();
    let mut completed_id = None;
    for _ in 0..16 {
        match tokio::time::timeout(Duration::from_secs(2), all.next()).await {
            Ok(Some(Ok(ev))) => {
                if let StreamItem::StatusUpdate(e) = &ev.item
                    && e.status.state == ::buffa::EnumValue::from(TaskState::Completed)
                {
                    completed_id = ev.event_id;
                    break;
                }
            }
            _ => break,
        }
    }
    let completed_id = completed_id.expect("should observe the Completed event with an id");
    drop(all);

    // Resume from just before Completed: only that event replays, after the snapshot.
    let mut stream = client
        .subscribe_to_task("task-resume", None, Some(&(completed_id - 1).to_string()))
        .await
        .unwrap();

    let mut got = Vec::new();
    while got.len() < 2 {
        let ev = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("event within 5s")
            .expect("stream not empty")
            .expect("ok event");
        got.push(ev);
    }

    // First: initial task snapshot (no id). Second: the replayed Completed event.
    assert!(
        matches!(got[0].item, StreamItem::Task(_)),
        "first must be the snapshot"
    );
    assert_eq!(got[0].event_id, None);
    assert_eq!(
        got[1].event_id,
        Some(completed_id),
        "only the Completed event should replay after Last-Event-ID = completed-1"
    );
    match &got[1].item {
        StreamItem::StatusUpdate(e) => {
            assert_eq!(
                e.status.state,
                ::buffa::EnumValue::from(TaskState::Completed)
            )
        }
        other => panic!("expected StatusUpdate, got {other:?}"),
    }
}

/// Attach a live subscription to a freshly created (non-terminal) task and
/// consume the initial snapshot, leaving the stream positioned on updates.
async fn subscribe_to_live_task(
    client: &JsonRpcClient,
    handler: &TestBusinessHandler,
    task_id: &str,
) -> Pin<Box<dyn Stream<Item = Result<a2a_rs::StreamEvent, A2AError>> + Send>> {
    let id: TaskId = task_id.parse().unwrap();
    let ctx: ContextId = "ctx".parse().unwrap();
    handler.create(&id, &ctx).await.unwrap();

    let mut stream = client.subscribe_to_task(task_id, None, None).await.unwrap();
    let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("snapshot within 5s")
        .expect("stream not empty")
        .expect("ok event");
    assert!(
        matches!(first.item, StreamItem::Task(_)),
        "first event must be the initial snapshot"
    );
    stream
}

/// Read the next event, then assert the stream **ends** rather than parking
/// open. Before this, a subscriber stayed connected after the task finished and
/// every run had to be capped with an external `timeout`.
async fn assert_closes_after_next(
    stream: &mut Pin<Box<dyn Stream<Item = Result<a2a_rs::StreamEvent, A2AError>> + Send>>,
    expected: TaskState,
) {
    let ev = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("settling event within 5s")
        .expect("stream not empty")
        .expect("ok event");
    match &ev.item {
        StreamItem::StatusUpdate(e) => {
            assert_eq!(e.status.state, ::buffa::EnumValue::from(expected))
        }
        other => panic!("expected StatusUpdate, got {other:?}"),
    }

    // The settling event is delivered *and* the stream then closes on its own.
    let end = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("stream must close within 5s rather than hang open");
    assert!(end.is_none(), "stream should be closed, got {end:?}");
}

/// A subscription ends once the task reaches a terminal state.
#[tokio::test]
async fn subscribe_stream_closes_on_terminal_state() {
    let (base, handler) = spawn_server_streaming().await;
    let client = JsonRpcClient::new(base);

    let mut stream = subscribe_to_live_task(&client, &handler, "task-close-terminal").await;
    handler
        .broadcast_status_update(
            "task-close-terminal",
            status_update("task-close-terminal", TaskState::Completed),
        )
        .await
        .unwrap();

    assert_closes_after_next(&mut stream, TaskState::Completed).await;
}

/// And on an *interrupted* state, which is the worse hang of the two: the agent
/// is waiting on the caller, so a stream left open has both sides waiting on
/// each other.
#[tokio::test]
async fn subscribe_stream_closes_on_interrupted_state() {
    let (base, handler) = spawn_server_streaming().await;
    let client = JsonRpcClient::new(base);

    let mut stream = subscribe_to_live_task(&client, &handler, "task-close-input").await;
    handler
        .broadcast_status_update(
            "task-close-input",
            status_update("task-close-input", TaskState::InputRequired),
        )
        .await
        .unwrap();

    assert_closes_after_next(&mut stream, TaskState::InputRequired).await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unary_roundtrip_send_get_list_cancel() {
    let (base, handler) = spawn_server_with_handler().await;
    let client = JsonRpcClient::new(base);

    // send → returns a task
    let task = client
        .send_task_message(
            "task-1",
            &message(),
            None,
            None,
            SendCompletion::WhenSettled,
        )
        .await
        .unwrap();
    let id = task.id.clone();
    assert!(!id.is_empty());
    assert_eq!(
        task.status.state,
        TaskState::Completed,
        "the echo agent finishes what it is sent"
    );

    // get → same task
    let got = client.get_task(&id, None).await.unwrap();
    assert_eq!(got.id, id);

    // list → contains it
    let listed = client.list_tasks(&Default::default()).await.unwrap();
    assert!(
        listed.tasks.iter().any(|t| t.id == id),
        "listed tasks should contain {id}"
    );

    // cancel → a task still in flight. The one above is already `Completed`, and
    // a completed task is correctly *not* cancelable, so cancelling it would
    // exercise the error path rather than `cancel_task`'s success decoding.
    let pending: TaskId = "task-pending".parse().unwrap();
    handler
        .create(&pending, &"ctx".parse::<ContextId>().unwrap())
        .await
        .unwrap();
    handler
        .update_status(&pending, TaskState::Working, None)
        .await
        .unwrap();
    let canceled = client.cancel_task(pending.as_str()).await.unwrap();
    assert_eq!(canceled.id, "task-pending");
    assert_eq!(canceled.status.state, TaskState::Canceled);
}

/// The other half of that pair: a finished task refuses cancellation, and the
/// refusal survives the trip back with its typed details intact.
#[tokio::test]
async fn cancelling_a_completed_task_is_refused_over_the_wire() {
    let base = spawn_server().await;
    let client = JsonRpcClient::new(base);

    client
        .send_task_message(
            "task-done",
            &message(),
            None,
            None,
            SendCompletion::WhenSettled,
        )
        .await
        .unwrap();

    let err = client
        .cancel_task("task-done")
        .await
        .expect_err("a completed task cannot be canceled");
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("TASK_NOT_CANCELABLE"),
        "the reason has to survive the trip: {rendered}"
    );
}

#[tokio::test]
async fn push_config_lifecycle() {
    let base = spawn_server().await;
    let client = JsonRpcClient::new(base);

    let task = client
        .send_task_message(
            "task-pc",
            &message(),
            None,
            None,
            SendCompletion::WhenSettled,
        )
        .await
        .unwrap();
    let id = task.id.clone();

    let config = TaskPushNotificationConfig {
        task_id: id.clone(),
        id: "cfg-1".to_string(),
        url: "https://example.com/webhook".to_string(),
        token: "tok".to_string(),
        ..Default::default()
    };

    client.set_task_push_notification(&config).await.unwrap();

    let configs = client.list_push_notification_configs(&id).await.unwrap();
    assert!(
        !configs.is_empty(),
        "config list should be non-empty after create"
    );

    let got = client
        .get_push_notification_config(&id, "cfg-1")
        .await
        .unwrap();
    assert_eq!(got.url, "https://example.com/webhook");

    client
        .delete_push_notification_config(&id, "cfg-1")
        .await
        .unwrap();
}

#[tokio::test]
async fn subscribe_yields_initial_task_over_sse() {
    let base = spawn_server().await;
    let client = JsonRpcClient::new(base);

    let task = client
        .send_task_message(
            "task-sub",
            &message(),
            None,
            None,
            SendCompletion::WhenSettled,
        )
        .await
        .unwrap();
    let id = task.id.clone();

    let mut stream = client.subscribe_to_task(&id, None, None).await.unwrap();

    // First SSE event must be the initial task snapshot — proving the client's
    // SSE reassembly + JSON-RPC frame + StreamResponse union decode all work.
    let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("subscribe stream should yield within 5s")
        .expect("subscribe stream should not be empty")
        .expect("first event should be Ok");

    match first.item {
        StreamItem::Task(t) => assert_eq!(t.id, id),
        other => panic!("expected initial Task snapshot, got {other:?}"),
    }
}

#[tokio::test]
async fn connect_negotiates_jsonrpc_from_card() {
    let base = spawn_server().await;

    // connect() fetches the card and negotiates; the card only offers JSONRPC.
    let transport = connect(&base, &default_registry()).await.unwrap();
    assert_eq!(transport.protocol(), "JSONRPC");

    let task = transport
        .send_task_message(
            "task-neg",
            &message(),
            None,
            None,
            SendCompletion::WhenSettled,
        )
        .await
        .unwrap();
    let got = transport.get_task(&task.id, None).await.unwrap();
    assert_eq!(got.id, task.id);
}

#[tokio::test]
async fn get_task_not_found_maps_to_typed_error() {
    let base = spawn_server().await;
    let client = JsonRpcClient::new(base);

    let err = client.get_task("does-not-exist", None).await.unwrap_err();
    assert!(
        matches!(err, A2AError::TaskNotFound(_)),
        "expected TaskNotFound, got {err:?}"
    );
}
