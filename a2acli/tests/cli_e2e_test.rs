//! End-to-end tests for the built `a2acli` binary against a live A2A agent.
//!
//! Every other test in the workspace drives the `Transport` port in-process.
//! These drive the **binary**: argument parsing, transport negotiation, the wait
//! for a slow agent, and the rendering of what came back — the layer a user
//! actually touches, and the one an in-process test cannot reach.
//!
//! The agent is a JSON-RPC server on an ephemeral port, spawned in the test
//! process; the CLI runs as a child process and talks to it over a real socket.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::response::IntoResponse;
use axum::{Json, Router, routing::get};
use futures::{Stream, stream};
use serde_json::Value;

use a2a_rs::adapter::business::ResponderMessageHandler;
use a2a_rs::adapter::{
    InMemoryStreamingHandler, InMemoryTaskStorage, JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router,
};
use a2a_rs::domain::{
    A2AError, AgentCard, AgentInterface, ContextId, Message, Task, TaskArtifactUpdateEvent, TaskId,
    TaskState, TaskStatus, TaskStatusUpdateEvent,
};
use a2a_rs::port::streaming_handler::{SeqEvent, Subscriber};
use a2a_rs::port::{AsyncMessageHandler, AsyncStreamingHandler, AsyncTaskLifecycle};

const AGENT_NAME: &str = "e2e-agent";
const LATE_REPLY: &str = "answered after thinking";
/// Long enough that the CLI reliably attaches before the agent finishes, short
/// enough that a full test run stays quick.
const THINKING_TIME: Duration = Duration::from_millis(400);
/// For tests that need a task to stay unsettled for their whole duration.
const NEVER: Duration = Duration::from_secs(3600);

// ---------------------------------------------------------------------------
// Agent handlers
// ---------------------------------------------------------------------------

/// An agent that acknowledges with `working` and settles later, out of band.
///
/// The shape of a peer that ignores `return_immediately` — the only shape for
/// which `a2acli send` has to wait at all.
#[derive(Clone)]
struct LateHandler {
    storage: InMemoryTaskStorage,
    streaming: InMemoryStreamingHandler,
    thinking_time: Duration,
}

#[async_trait]
impl AsyncMessageHandler for LateHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;
        let context: ContextId = "e2e".parse()?;
        if !self.storage.exists(&id).await? {
            self.storage.create(&id, &context).await?;
        }
        let acknowledgement = self
            .storage
            .update_status(&id, TaskState::Working, Some(message.clone()))
            .await?;

        let storage = self.storage.clone();
        let streaming = self.streaming.clone();
        let thinking_time = self.thinking_time;
        tokio::spawn(async move {
            tokio::time::sleep(thinking_time).await;
            let reply = Message::agent_text(LATE_REPLY.to_string(), "late-reply".to_string());
            let _ = storage
                .update_status(&id, TaskState::Completed, Some(reply.clone()))
                .await;
            let _ = streaming
                .broadcast_status_update(
                    id.as_str(),
                    TaskStatusUpdateEvent {
                        task_id: id.as_str().to_string(),
                        context_id: context.as_str().to_string(),
                        kind: "status-update".to_string(),
                        status: TaskStatus::new(TaskState::Completed, Some(reply)),
                        metadata: None,
                    },
                )
                .await;
        });

        Ok(acknowledgement)
    }
}

/// A streaming backend that plays dead exactly once.
///
/// The server's blocking `SendMessage` and its `SubscribeToTask` both open a
/// stream through this port, and the server is honest enough that a working
/// stream means it waits — leaving no way to reach the CLI's own wait. Blinding
/// the first call reproduces the peer this test is about: one that answers
/// `working` immediately *and* serves subscriptions.
#[derive(Clone)]
struct BlindOnceStreaming {
    inner: InMemoryStreamingHandler,
    blinded: Arc<AtomicBool>,
    /// Streams actually served, i.e. subscriptions that could observe a
    /// transition. A test asserts on this to prove the CLI took the streaming
    /// path rather than quietly falling back to polling.
    served: Arc<AtomicUsize>,
}

impl BlindOnceStreaming {
    fn wrapping(inner: InMemoryStreamingHandler) -> Self {
        Self {
            inner,
            blinded: Arc::new(AtomicBool::new(false)),
            served: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl AsyncStreamingHandler for BlindOnceStreaming {
    async fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.inner.add_status_subscriber(task_id, subscriber).await
    }
    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        self.inner
            .add_artifact_subscriber(task_id, subscriber)
            .await
    }
    async fn remove_subscription(&self, subscription_id: &str) -> Result<(), A2AError> {
        self.inner.remove_subscription(subscription_id).await
    }
    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        self.inner.remove_task_subscribers(task_id).await
    }
    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        self.inner.get_subscriber_count(task_id).await
    }
    async fn broadcast_status_update(
        &self,
        task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        self.inner.broadcast_status_update(task_id, update).await
    }
    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        self.inner.broadcast_artifact_update(task_id, update).await
    }
    async fn status_update_stream(
        &self,
        task_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>, A2AError>
    {
        self.inner.status_update_stream(task_id).await
    }
    async fn artifact_update_stream(
        &self,
        task_id: &str,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        self.inner.artifact_update_stream(task_id).await
    }
    async fn combined_update_stream(
        &self,
        task_id: &str,
        from_event_id: Option<u64>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<SeqEvent, A2AError>> + Send>>, A2AError> {
        if !self.blinded.swap(true, Ordering::SeqCst) {
            // An already-finished stream: the server's blocking send drains it,
            // finds the task still `working`, and returns that.
            return Ok(Box::pin(stream::empty()));
        }
        self.served.fetch_add(1, Ordering::SeqCst);
        self.inner
            .combined_update_stream(task_id, from_event_id)
            .await
    }
}

// ---------------------------------------------------------------------------
// Server harness
// ---------------------------------------------------------------------------

/// What the agent under test does with a message.
enum Agent {
    /// Answers synchronously — the conformant case, where `send` never waits.
    Echo,
    /// Acknowledges and settles later, with subscriptions available.
    Late(Duration),
    /// Acknowledges and settles later, with **no** streaming backend at all, so
    /// the CLI's subscription is refused and it has to poll.
    LateWithoutStreaming(Duration),
}

/// A running agent: where to reach it, and what it observed.
struct Harness {
    base: String,
    /// Subscriptions the agent actually served (always 0 unless the agent is
    /// [`Agent::Late`]).
    subscriptions: Arc<AtomicUsize>,
}

/// Spawn an agent on an ephemeral port.
///
/// `card_token`, when set, guards the agent-card endpoint with a bearer token —
/// the credential path a client can only exercise through `--auth`.
async fn spawn_agent(agent: Agent, card_token: Option<&'static str>) -> Harness {
    let storage = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let info = SimpleAgentInfo::new(AGENT_NAME.to_string(), base.clone());

    let mut subscriptions = Arc::new(AtomicUsize::new(0));
    let adapter = Arc::new(match agent {
        Agent::Echo => JsonRpcAdapter::new(
            ResponderMessageHandler::echo(
                storage.clone(),
                streaming.clone(),
                storage.push_notifier(),
            ),
            storage.clone(),
            storage.clone(),
            info,
        )
        .with_streaming_handler(streaming.clone()),
        Agent::Late(thinking_time) => JsonRpcAdapter::new(
            LateHandler {
                storage: storage.clone(),
                streaming: streaming.clone(),
                thinking_time,
            },
            storage.clone(),
            storage.clone(),
            info,
        )
        .with_streaming_handler({
            let handler = BlindOnceStreaming::wrapping(streaming.clone());
            subscriptions = handler.served.clone();
            handler
        }),
        Agent::LateWithoutStreaming(thinking_time) => JsonRpcAdapter::new(
            LateHandler {
                storage: storage.clone(),
                streaming: streaming.clone(),
                thinking_time,
            },
            storage.clone(),
            storage.clone(),
            info,
        ),
    });

    let card = AgentCard {
        name: AGENT_NAME.to_string(),
        version: "1.2.3".to_string(),
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
        get(move |headers: HeaderMap| {
            let card = card.clone();
            async move {
                if let Some(expected) = card_token {
                    let presented = headers
                        .get(AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.strip_prefix("Bearer "));
                    if presented != Some(expected) {
                        return (StatusCode::UNAUTHORIZED, "bearer token required").into_response();
                    }
                }
                Json(card).into_response()
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    Harness {
        base,
        subscriptions,
    }
}

// ---------------------------------------------------------------------------
// Driving the binary
// ---------------------------------------------------------------------------

/// Run the built `a2acli` against `base` and return (stdout, stderr, success).
async fn a2acli(base: &str, args: &[&str]) -> (String, String, bool) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_a2acli"))
        .arg("--url")
        .arg(base)
        .args(args)
        // The CLI reads `A2A_URL` and `A2A_AUTH_TOKEN` from the environment, so a
        // developer's own shell must not decide what the test is testing.
        .env_remove("A2A_URL")
        .env_remove("A2A_AUTH_TOKEN")
        .output()
        .await
        .expect("a2acli should be runnable");

    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// Run the CLI and assert it succeeded, returning stdout.
async fn run(base: &str, args: &[&str]) -> String {
    let (stdout, stderr, ok) = a2acli(base, args).await;
    assert!(
        ok,
        "a2acli {args:?} failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    stdout
}

/// Run the CLI with `--json` and parse what it printed.
async fn run_json(base: &str, args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    let stdout = run(base, &args).await;
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout}"))
}

fn state_of(task: &Value) -> &str {
    task["status"]["state"].as_str().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn card_reports_the_agent() {
    let agent = spawn_agent(Agent::Echo, None).await;

    let card = run_json(&agent.base, &["card"]).await;
    assert_eq!(card["name"], AGENT_NAME);
    assert_eq!(card["version"], "1.2.3");

    // The human rendering names the agent too — the report on stdout has to be
    // readable, not just parseable.
    let human = run(&agent.base, &["card"]).await;
    assert!(human.contains(AGENT_NAME), "card output: {human}");
}

/// The full unary round-trip against a conformant agent, over the binary.
#[tokio::test]
async fn send_get_roundtrip() {
    let agent = spawn_agent(Agent::Echo, None).await;

    let sent = run_json(
        &agent.base,
        &["send", "hello there", "--task-id", "t-roundtrip"],
    )
    .await;
    assert_eq!(sent["id"], "t-roundtrip");
    assert_eq!(state_of(&sent), "TASK_STATE_COMPLETED");

    let got = run_json(&agent.base, &["get", "t-roundtrip"]).await;
    assert_eq!(got["id"], "t-roundtrip");
    assert_eq!(state_of(&got), "TASK_STATE_COMPLETED");
}

/// A task still in flight can be cancelled through the binary.
#[tokio::test]
async fn cancel_stops_a_task_in_flight() {
    let agent = spawn_agent(Agent::LateWithoutStreaming(NEVER), None).await;

    let sent = run_json(
        &agent.base,
        &[
            "send",
            "take your time",
            "--task-id",
            "t-cancel",
            "--no-wait",
        ],
    )
    .await;
    assert_eq!(state_of(&sent), "TASK_STATE_WORKING");

    let cancelled = run_json(&agent.base, &["cancel", "t-cancel"]).await;
    assert_eq!(state_of(&cancelled), "TASK_STATE_CANCELED");
}

/// `--no-wait` reports the acknowledgement as-is, and says how to follow it.
#[tokio::test]
async fn no_wait_reports_the_acknowledgement() {
    let agent = spawn_agent(Agent::LateWithoutStreaming(NEVER), None).await;

    let human = run(
        &agent.base,
        &[
            "send",
            "take your time",
            "--task-id",
            "t-nowait",
            "--no-wait",
        ],
    )
    .await;
    assert!(human.contains("working"), "send output: {human}");
    assert!(
        human.contains("still working") && human.contains("stream t-nowait"),
        "an unfinished task must tell the user how to follow it: {human}"
    );
}

/// Against a peer that answers `working` but serves subscriptions, `send` waits
/// on the event stream and reports the real answer.
#[tokio::test]
async fn send_waits_for_a_late_agent_over_the_event_stream() {
    let agent = spawn_agent(Agent::Late(THINKING_TIME), None).await;

    let human = run(
        &agent.base,
        &["send", "think about it", "--task-id", "t-stream"],
    )
    .await;
    assert!(human.contains("completed"), "send output: {human}");
    assert!(
        human.contains(LATE_REPLY),
        "the agent's answer must reach stdout: {human}"
    );
    // Without this the test would pass on the polling fallback too, and the
    // thing it is named for would go untested.
    assert_eq!(
        agent.subscriptions.load(Ordering::SeqCst),
        1,
        "the wait must have gone through a subscription"
    );
}

/// And against one with no streaming at all, the same wait falls back to
/// polling rather than giving up.
#[tokio::test]
async fn send_polls_when_the_agent_cannot_stream() {
    let agent = spawn_agent(Agent::LateWithoutStreaming(THINKING_TIME), None).await;

    let human = run(
        &agent.base,
        &["send", "think about it", "--task-id", "t-poll"],
    )
    .await;
    assert!(human.contains("completed"), "send output: {human}");
    assert!(
        human.contains(LATE_REPLY),
        "the agent's answer must reach stdout: {human}"
    );
}

/// A wait that runs out reports the task it has rather than failing — the id is
/// what lets the caller pick the conversation back up.
#[tokio::test]
async fn send_gives_up_gracefully_when_the_agent_never_answers() {
    let agent = spawn_agent(Agent::LateWithoutStreaming(NEVER), None).await;

    let human = run(
        &agent.base,
        &[
            "send",
            "never mind",
            "--task-id",
            "t-timeout",
            "--wait-timeout",
            "1",
        ],
    )
    .await;
    assert!(human.contains("t-timeout"), "send output: {human}");
    assert!(human.contains("still working"), "send output: {human}");
}

/// `--auth` reaches the agent-card endpoint, in the default `auto` transport
/// mode. It used to be dropped there, so credentials only worked with an
/// explicit `--transport`.
#[tokio::test]
async fn auth_token_reaches_the_card_endpoint() {
    let agent = spawn_agent(Agent::Echo, Some("s3cret")).await;

    let (_, _, ok) = a2acli(&agent.base, &["card"]).await;
    assert!(!ok, "a guarded card must not be readable without a token");

    let card = run_json(&agent.base, &["card", "--auth", "s3cret"]).await;
    assert_eq!(card["name"], AGENT_NAME);
}

/// A bad URL fails loudly instead of hanging or exiting 0.
#[tokio::test]
async fn malformed_url_is_a_hard_error() {
    let (_, stderr, ok) = a2acli("not-a-url", &["send", "hello"]).await;
    assert!(!ok, "a malformed url must be an error");
    assert!(!stderr.is_empty(), "the failure must say something");
}
