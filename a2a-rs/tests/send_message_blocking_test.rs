//! `SendMessage` blocking semantics: `SendMessageConfiguration.return_immediately`.
//!
//! The proto3 default is `false`, and `false` obliges the server to wait until
//! the task reaches a terminal or interrupted state before returning
//! (`spec/a2a.proto:155`). So a conformant client that sends **no configuration
//! at all** — which is what the official SDKs do — is promised a settled task.
//! The server used to behave as if `return_immediately` were always `true`,
//! handing back `WORKING` and leaving the client to poll.
//!
//! These tests drive an agent that answers *asynchronously* (accept now, finish
//! later), because a synchronous agent settles before the wait is even reached
//! and so cannot tell the two behaviours apart.

#![cfg(feature = "jsonrpc-server")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header::CONTENT_TYPE};
use serde_json::{Value, json};
use tower::ServiceExt;

use a2a_rs::adapter::streaming::InMemoryStreamingHandler;
use a2a_rs::adapter::{InMemoryTaskStorage, JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router};
use a2a_rs::application::{SendOptions, TaskService};
use a2a_rs::domain::{
    A2AError, ContextId, Message, SendCompletion, Task, TaskId, TaskState, TaskStatus,
    TaskStatusUpdateEvent,
};
use a2a_rs::port::{AsyncMessageHandler, AsyncStreamingHandler, AsyncTaskLifecycle};

// ---------------------------------------------------------------------------
// An agent that accepts now and finishes later
// ---------------------------------------------------------------------------

/// Accepts the message, reports `WORKING`, and reaches `settles_to` only after
/// `delay` — the shape the `llm` template scaffolds, and the one a blocking
/// `SendMessage` exists for.
///
/// `settles_to: None` models an agent that never finishes at all, which is what
/// the wait's bound has to survive.
#[derive(Clone)]
struct AsyncAgent {
    storage: Arc<InMemoryTaskStorage>,
    streaming: InMemoryStreamingHandler,
    settles_to: Option<TaskState>,
    delay: Duration,
}

#[async_trait]
impl AsyncMessageHandler for AsyncAgent {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;
        let ctx: ContextId = session_id.unwrap_or("ctx").parse()?;
        if !self.storage.exists(&id).await? {
            self.storage.create(&id, &ctx).await?;
        }
        let task = self
            .storage
            .update_status(&id, TaskState::Working, Some(message.clone()))
            .await?;

        if let Some(final_state) = self.settles_to {
            let storage = self.storage.clone();
            let streaming = self.streaming.clone();
            let delay = self.delay;
            let task_id = task_id.to_string();
            let context_id = task.context_id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let id: TaskId = task_id.parse().expect("valid id");
                storage
                    .update_status(&id, final_state, None)
                    .await
                    .expect("commit final state");
                // Commit, then announce — the transition the waiter is parked on.
                streaming
                    .broadcast_status_update(
                        &task_id,
                        TaskStatusUpdateEvent {
                            task_id: task_id.clone(),
                            context_id,
                            kind: "status-update".to_string(),
                            status: TaskStatus::new(final_state, None),
                            metadata: None,
                        },
                    )
                    .await
                    .expect("broadcast final state");
            });
        }

        Ok(task)
    }
}

/// A service over an async agent with a real streaming backend, which is what
/// makes the wait observable.
fn service_for(settles_to: Option<TaskState>, delay: Duration) -> TaskService {
    let storage = Arc::new(InMemoryTaskStorage::new());
    let streaming = InMemoryStreamingHandler::new();
    let agent = AsyncAgent {
        storage: storage.clone(),
        streaming: streaming.clone(),
        settles_to,
        delay,
    };
    TaskService::new(
        agent,
        (*storage).clone(),
        (*storage).clone(),
        SimpleAgentInfo::new("blocking-test".to_string(), "http://localhost".to_string()),
        streaming,
        storage.push_notifier(),
    )
}

fn message() -> Message {
    Message::user_text("hello".to_string(), "m1".to_string())
}

// ---------------------------------------------------------------------------
// Service-level semantics
// ---------------------------------------------------------------------------

/// The headline: no configuration means `return_immediately = false`, so the
/// call holds until the agent is done. This returned `WORKING` before.
#[tokio::test]
async fn send_message_waits_for_an_async_agent_by_default() {
    let service = service_for(Some(TaskState::Completed), Duration::from_millis(150));

    let task = service
        .send_message("t-default", &message(), None, SendOptions::default())
        .await
        .expect("send");

    assert_eq!(
        task.status.state,
        ::buffa::EnumValue::from(TaskState::Completed),
        "a client that sent no configuration is owed a settled task"
    );
}

/// An *interrupted* state ends the wait too — the agent has stopped and is
/// waiting on the caller, so holding the response would deadlock the pair.
#[tokio::test]
async fn an_interrupted_state_ends_the_wait() {
    let service = service_for(Some(TaskState::InputRequired), Duration::from_millis(150));

    let task = service
        .send_message("t-input", &message(), None, SendOptions::default())
        .await
        .expect("send");

    assert_eq!(
        task.status.state,
        ::buffa::EnumValue::from(TaskState::InputRequired)
    );
}

/// The opt-out still works, and is now the thing you have to ask for.
#[tokio::test]
async fn return_immediately_does_not_wait() {
    let service = service_for(Some(TaskState::Completed), Duration::from_secs(30));

    let started = Instant::now();
    let task = service
        .send_message(
            "t-immediate",
            &message(),
            None,
            SendOptions {
                completion: SendCompletion::WhenCreated,
                ..Default::default()
            },
        )
        .await
        .expect("send");

    assert_eq!(
        task.status.state,
        ::buffa::EnumValue::from(TaskState::Working)
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "must not have waited on the agent"
    );
}

/// An agent that never settles must not pin the connection open forever. The
/// bound expires and the caller gets the task *unsettled* rather than an error:
/// `WORKING` is true, and it leaves them a task id to follow.
#[tokio::test]
async fn the_wait_is_bounded_when_the_agent_never_settles() {
    let service = service_for(None, Duration::ZERO).with_send_wait(Duration::from_millis(200));

    let started = Instant::now();
    let task = service
        .send_message("t-hang", &message(), None, SendOptions::default())
        .await
        .expect("send must return, not error");
    let elapsed = started.elapsed();

    assert_eq!(
        task.status.state,
        ::buffa::EnumValue::from(TaskState::Working),
        "the unsettled task is the honest answer once the budget is spent"
    );
    assert!(
        elapsed >= Duration::from_millis(150),
        "should have actually waited, took {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "should have given up at the bound, took {elapsed:?}"
    );
}

/// A server with no streaming backend cannot observe transitions. It must still
/// answer — degrading to "return what I have" — rather than failing the send or
/// blocking for the full budget on a stream it never got.
#[tokio::test]
async fn a_server_without_streaming_still_answers() {
    let storage = Arc::new(InMemoryTaskStorage::new());
    let agent = AsyncAgent {
        storage: storage.clone(),
        streaming: InMemoryStreamingHandler::new(),
        settles_to: None,
        delay: Duration::ZERO,
    };
    // `JsonRpcAdapter::new` wires `NoopStreamingHandler`, whose
    // `combined_update_stream` reports `UnsupportedOperation`.
    let adapter = Arc::new(JsonRpcAdapter::new(
        agent,
        (*storage).clone(),
        (*storage).clone(),
        SimpleAgentInfo::new("no-stream".to_string(), "http://localhost".to_string()),
    ));

    let started = Instant::now();
    let (_, body) = rpc_send(&adapter, send_params("t-nostream", None)).await;

    assert_eq!(state_of(&body), "TASK_STATE_WORKING");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "must not block on a stream it could not open"
    );
}

/// The server's wait must expire *before* the client's per-request timeout, or
/// the two race and a slow agent surfaces as a transport error instead of the
/// unsettled task the bound exists to return. The two constants live in
/// different modules, so nothing but this test relates them.
///
/// Runs on tokio's virtual clock: this is the one test that has to exercise the
/// *default* budget rather than a short one injected via `with_send_wait`, and
/// spending 25s of wall time to watch a timer expire is 25s no CI run gets back.
/// With time paused the runtime jumps to each deadline as soon as nothing is
/// runnable, so the elapsed figure below is the real budget, measured instantly.
#[tokio::test(start_paused = true)]
async fn the_default_wait_expires_before_the_default_client_timeout() {
    // `JsonRpcClient::new` / `HttpClient::new` both default to 30s, as does
    // `a2acli` (its `--timeout` falls through to theirs).
    const CLIENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

    // `tokio::time::Instant`, not `std::time::Instant` — only the former is
    // advanced by the paused clock.
    let started = tokio::time::Instant::now();
    let task = service_for(None, Duration::ZERO)
        .send_message("t-default-bound", &message(), None, SendOptions::default())
        .await
        .expect("send");
    let waited = started.elapsed();

    assert_eq!(
        task.status.state,
        ::buffa::EnumValue::from(TaskState::Working)
    );
    assert!(
        waited < CLIENT_REQUEST_TIMEOUT,
        "server waited {waited:?}, at or past the {CLIENT_REQUEST_TIMEOUT:?} client timeout — \
         a slow agent would surface as a connection error, not a task"
    );
    // The upper bound alone would also pass if the default budget were zero, or
    // if the wait were skipped entirely — which is the behaviour this whole file
    // exists to prevent. A returning-too-soon default is as wrong as a late one.
    assert!(
        waited > Duration::from_secs(1),
        "server returned after {waited:?} — the default budget has to be a real wait"
    );
}

// ---------------------------------------------------------------------------
// Wire-level: the flag is actually read off the request
// ---------------------------------------------------------------------------

fn send_params(task_id: &str, return_immediately: Option<bool>) -> Value {
    let mut params = json!({
        "message": {
            "messageId": "m1",
            "role": "ROLE_USER",
            "parts": [{ "text": "hello" }],
            "taskId": task_id,
        }
    });
    if let Some(flag) = return_immediately {
        params["configuration"] = json!({ "returnImmediately": flag });
    }
    params
}

/// Drive one JSON-RPC `SendMessage` through the real router.
async fn rpc_send(adapter: &Arc<JsonRpcAdapter>, params: Value) -> (u16, Value) {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "SendMessage",
        "params": params,
    });
    let req = Request::builder()
        .method("POST")
        .uri("/")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = jsonrpc_router(adapter.clone()).oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn state_of(body: &Value) -> &str {
    body["result"]["task"]["status"]["state"]
        .as_str()
        .unwrap_or_else(|| panic!("no task state in {body}"))
}

/// An adapter over an async agent, with a real streaming backend so the wait can
/// resolve.
fn streaming_adapter(settles_to: Option<TaskState>, delay: Duration) -> Arc<JsonRpcAdapter> {
    let storage = Arc::new(InMemoryTaskStorage::new());
    let streaming = InMemoryStreamingHandler::new();
    let agent = AsyncAgent {
        storage: storage.clone(),
        streaming: streaming.clone(),
        settles_to,
        delay,
    };
    Arc::new(
        JsonRpcAdapter::new(
            agent,
            (*storage).clone(),
            (*storage).clone(),
            SimpleAgentInfo::new("wire-test".to_string(), "http://localhost".to_string()),
        )
        .with_streaming_handler(streaming),
    )
}

/// Absent configuration is not "no opinion" — it is `return_immediately = false`
/// and therefore the *waiting* branch. This is the exact request the official
/// SDKs send.
#[tokio::test]
async fn absent_configuration_waits_over_the_wire() {
    let adapter = streaming_adapter(Some(TaskState::Completed), Duration::from_millis(150));

    let (status, body) = rpc_send(&adapter, send_params("t-wire-default", None)).await;

    assert_eq!(status, 200);
    assert_eq!(state_of(&body), "TASK_STATE_COMPLETED");
}

/// And an explicit `false` behaves the same as absent.
#[tokio::test]
async fn explicit_false_waits_over_the_wire() {
    let adapter = streaming_adapter(Some(TaskState::Completed), Duration::from_millis(150));

    let (_, body) = rpc_send(&adapter, send_params("t-wire-false", Some(false))).await;

    assert_eq!(state_of(&body), "TASK_STATE_COMPLETED");
}

/// `returnImmediately: true` is read off the wire and honoured.
#[tokio::test]
async fn return_immediately_true_is_read_from_the_wire() {
    let adapter = streaming_adapter(Some(TaskState::Completed), Duration::from_secs(30));

    let started = Instant::now();
    let (_, body) = rpc_send(&adapter, send_params("t-wire-true", Some(true))).await;

    assert_eq!(state_of(&body), "TASK_STATE_WORKING");
    assert!(started.elapsed() < Duration::from_secs(5));
}
