//! Live end-to-end test of agent-to-agent delegation.
//!
//! Spins up a real A2A agent on an ephemeral socket, then drives an
//! [`A2aAgentToolSource`] against it through the JSON-RPC [`Transport`]. This
//! proves the multi-agent keystone over the wire: the tool source sends an A2A
//! task to a peer agent, waits for it to settle, and returns the reply — exactly
//! what the LLM handler does when the model calls an `ask_<agent>` tool.

#![cfg(feature = "mcp-server")]

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum8::{Json, routing::get};
use futures::Stream;

use a2a_agents::handlers::tools::ToolSource;
use a2a_agents::{
    A2aAgentToolSource, AgentRegistry, DiscoveredPeer, InMemoryAgentRegistry, PeerResolver,
};
use a2a_llm::ToolCall;

use a2a_rs::adapter::business::{EchoResponder, Responder, ResponderMessageHandler};
use a2a_rs::adapter::{JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router};
use a2a_rs::domain::{
    A2AError, AgentCard, AgentInterface, AgentSkill, ContextId, Message, Part, Role, Task,
    TaskArtifactUpdateEvent, TaskId, TaskState, TaskStatus, TaskStatusUpdateEvent,
};
use a2a_rs::port::RequestContext;
use a2a_rs::port::streaming_handler::{SeqEvent, Subscriber};
use a2a_rs::port::{AsyncMessageHandler, AsyncStreamingHandler, AsyncTaskLifecycle};
use a2a_rs::{InMemoryStreamingHandler, InMemoryTaskStorage, JsonRpcClient, Transport};

/// The poll interval the tool source falls back to. A peer that settles well
/// inside this window separates "woke on the event stream" from "woke on the
/// timer".
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// What a peer answering out of band eventually says.
const LATE_REPLY: &str = "the answer, eventually";

/// What a peer that settles into a fixed state says while doing it.
const FIXED_REPLY: &str = "I need the invoice number";

/// A remote that acknowledges and then never finishes — the case the tool
/// source's deadline exists for.
///
/// Spelled out rather than borrowed from a built-in: a test whose subject is
/// "the peer never settles" should say so, not depend on some other type
/// happening to behave that way.
struct NeverFinishes;

#[async_trait]
impl Responder for NeverFinishes {
    async fn respond(
        &self,
        _message: &Message,
        task: &Task,
    ) -> Result<(Message, TaskState), A2AError> {
        let reply = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text("still thinking".to_string())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .task_id(task.id.clone())
            .build();
        Ok((reply, TaskState::Working))
    }
}

/// A peer that acknowledges with `working` and settles later, out of band.
///
/// The only shape for which the tool source has to wait at all — delegation
/// sends `WhenCreated`, so a peer that answers synchronously is already settled
/// when `send_task_message` returns.
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
        _ctx: &RequestContext,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;
        let context: ContextId = "delegation".parse()?;
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

/// A peer that settles straight into one state, whatever it is asked.
///
/// An echo can only ever succeed, so the outcomes the model most needs told
/// apart from an answer — the peer refusing the work, or asking a question back
/// — are unreachable without this.
#[derive(Clone)]
struct FixedHandler {
    storage: InMemoryTaskStorage,
    state: TaskState,
}

#[async_trait]
impl AsyncMessageHandler for FixedHandler {
    async fn process_message(
        &self,
        task_id: &str,
        _message: &Message,
        _ctx: &RequestContext,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;
        let context: ContextId = "delegation".parse()?;
        if !self.storage.exists(&id).await? {
            self.storage.create(&id, &context).await?;
        }
        let reply = Message::agent_text(FIXED_REPLY.to_string(), "fixed-reply".to_string());
        self.storage
            .update_status(&id, self.state, Some(reply))
            .await
    }
}

/// Counts the subscriptions actually served, so a test can prove the tool source
/// took the streaming path rather than quietly falling back to polling.
#[derive(Clone)]
struct CountingStreaming {
    inner: InMemoryStreamingHandler,
    served: Arc<AtomicUsize>,
}

impl CountingStreaming {
    fn wrapping(inner: InMemoryStreamingHandler) -> Self {
        Self {
            inner,
            served: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl AsyncStreamingHandler for CountingStreaming {
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
        self.served.fetch_add(1, Ordering::SeqCst);
        self.inner
            .combined_update_stream(task_id, from_event_id)
            .await
    }
}

/// What the peer under test does with a message.
enum Peer {
    /// Answers synchronously — the conformant case, where delegation never
    /// waits.
    Echo,
    /// Never leaves `Working`, so the deadline is the only thing that stops the
    /// wait.
    NeverFinishes,
    /// Settles synchronously into a fixed state, for the outcomes an echo
    /// cannot produce.
    Answers(TaskState),
    /// Acknowledges and settles later, with subscriptions available.
    Late(Duration),
    /// Acknowledges and settles later, with **no** streaming backend at all, so
    /// the subscription is refused and the wait has to poll.
    LateWithoutStreaming(Duration),
}

/// A running peer: where to reach it, and what it observed.
struct Harness {
    base: String,
    /// Subscriptions the peer actually served (always 0 unless it is
    /// [`Peer::Late`]).
    subscriptions: Arc<AtomicUsize>,
}

impl Harness {
    /// A tool source pointed at this peer, named `echo` so the advertised tool
    /// is `ask_echo`.
    fn tool_source(&self) -> A2aAgentToolSource {
        let transport: Arc<dyn Transport> = Arc::new(JsonRpcClient::new(self.base.clone()));
        A2aAgentToolSource::new("echo", "Answers questions.".to_string(), transport)
    }
}

/// Stand up a JSON-RPC A2A agent on an ephemeral port.
async fn spawn_peer(peer: Peer) -> Harness {
    let storage = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();
    let info = SimpleAgentInfo::new("echo".to_string(), "http://localhost".to_string());

    let mut subscriptions = Arc::new(AtomicUsize::new(0));
    let adapter = Arc::new(match peer {
        Peer::Echo => JsonRpcAdapter::new(
            ResponderMessageHandler::new(
                storage.clone(),
                streaming.clone(),
                storage.push_notifier(),
                EchoResponder,
            ),
            storage.clone(),
            storage.clone(),
            info,
        )
        .with_streaming_handler(streaming.clone()),
        Peer::NeverFinishes => JsonRpcAdapter::new(
            ResponderMessageHandler::new(
                storage.clone(),
                streaming.clone(),
                storage.push_notifier(),
                NeverFinishes,
            ),
            storage.clone(),
            storage.clone(),
            info,
        )
        .with_streaming_handler(streaming.clone()),
        Peer::Answers(state) => JsonRpcAdapter::new(
            FixedHandler {
                storage: storage.clone(),
                state,
            },
            storage.clone(),
            storage.clone(),
            info,
        )
        .with_streaming_handler(streaming.clone()),
        Peer::Late(thinking_time) => JsonRpcAdapter::new(
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
            let handler = CountingStreaming::wrapping(streaming.clone());
            subscriptions = handler.served.clone();
            handler
        }),
        Peer::LateWithoutStreaming(thinking_time) => JsonRpcAdapter::new(
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());

    // A real agent publishes a card, and transport negotiation is how a peer
    // located through the registry gets dialed at all — without one, the
    // fallback picks ConnectRPC and never reaches this JSON-RPC agent.
    let card = AgentCard {
        name: "echo".to_string(),
        version: "1.0.0".to_string(),
        supported_interfaces: vec![AgentInterface {
            url: base.clone(),
            protocol_binding: "JSONRPC".to_string(),
            protocol_version: "1.0".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let app = jsonrpc_router(adapter).route(
        "/.well-known/agent-card.json",
        get(move || {
            let card = card.clone();
            async move { Json(card) }
        }),
    );

    tokio::spawn(async move {
        axum8::serve(listener, app).await.unwrap();
    });
    Harness {
        base,
        subscriptions,
    }
}

fn tool_call(message: &str) -> ToolCall {
    ToolCall {
        id: "call-1".to_string(),
        name: "ask_echo".to_string(),
        arguments: serde_json::json!({ "message": message }).to_string(),
    }
}

#[tokio::test]
async fn delegates_to_remote_agent_over_the_wire() {
    let peer = spawn_peer(Peer::Echo).await;
    let source = peer.tool_source();

    assert_eq!(source.tool_name(), "ask_echo");

    let reply = source
        .invoke("local-orchestrator-task", &tool_call("hello world"))
        .await
        .expect("delegation should succeed");
    assert_eq!(reply, "Echo: hello world");
}

#[tokio::test]
async fn times_out_when_remote_never_settles() {
    // The peer never leaves `Working`, so the tool source must give up once its
    // deadline elapses rather than hang.
    let peer = spawn_peer(Peer::NeverFinishes).await;
    let source = peer.tool_source().with_deadline(Duration::from_millis(400));

    let err = source
        .invoke("local-orchestrator-task", &tool_call("hello"))
        .await
        .expect_err("a never-settling remote should time out");
    match err {
        A2AError::Internal(m) => assert!(m.contains("did not finish"), "unexpected message: {m}"),
        other => panic!("expected Internal timeout error, got {other:?}"),
    }
}

/// The wait wakes on the peer's own progress, not on a timer.
///
/// The peer answers an order of magnitude faster than the poll interval, so a
/// delegation that still took a poll's worth of time was sleeping through the
/// answer. The subscription count is asserted too, so this cannot pass by the
/// poll happening to be quick.
#[tokio::test]
async fn waits_on_the_peers_event_stream_rather_than_a_timer() {
    let peer = spawn_peer(Peer::Late(Duration::from_millis(20))).await;
    let source = peer.tool_source();

    let started = std::time::Instant::now();
    let reply = source
        .invoke("local-orchestrator-task", &tool_call("hello"))
        .await
        .expect("delegation should succeed");
    let elapsed = started.elapsed();

    assert_eq!(reply, LATE_REPLY);
    assert!(
        peer.subscriptions.load(Ordering::SeqCst) >= 1,
        "the peer served no subscription, so the wait polled"
    );
    assert!(
        elapsed < POLL_INTERVAL,
        "waited {elapsed:?}, which is a poll interval rather than the peer's own progress"
    );
}

/// A peer with no streaming backend refuses the subscription, and the wait falls
/// back to polling rather than reporting a timeout on a task that finished.
#[tokio::test]
async fn falls_back_to_polling_when_the_peer_serves_no_subscription() {
    let peer = spawn_peer(Peer::LateWithoutStreaming(Duration::from_millis(20))).await;
    let source = peer.tool_source().with_deadline(Duration::from_secs(5));

    let reply = source
        .invoke("local-orchestrator-task", &tool_call("hello"))
        .await
        .expect("the poll fallback should still deliver the reply");
    assert_eq!(reply, LATE_REPLY);
    assert_eq!(
        peer.subscriptions.load(Ordering::SeqCst),
        0,
        "this peer has no streaming backend to serve one"
    );
}

/// A peer that asks a question has stopped, and the wait has to stop with it.
///
/// `input-required` is not terminal, so waiting for terminal burned the whole
/// deadline and then reported a timeout — on a peer that had answered in
/// milliseconds. The model is told what happened rather than handed the question
/// as if it were the answer.
#[tokio::test]
async fn an_interrupted_peer_ends_the_wait_and_says_so() {
    let peer = spawn_peer(Peer::Answers(TaskState::InputRequired)).await;
    let source = peer.tool_source().with_deadline(Duration::from_secs(5));

    let started = std::time::Instant::now();
    let reply = source
        .invoke("local-orchestrator-task", &tool_call("refund my trip"))
        .await
        .expect("an interrupted peer is an outcome, not a transport failure");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "waited out the deadline"
    );
    assert!(
        reply.contains("needs more information"),
        "the model must be told this is a question, not an answer: {reply}"
    );
    assert!(
        reply.contains(FIXED_REPLY),
        "the question itself is missing: {reply}"
    );
}

/// An orchestrator that came up before its peer must still reach it.
///
/// Peers used to be resolved once at startup, so an agent that had not
/// registered yet was skipped and its tool was never advertised — the model
/// could not call it for the rest of the process's life, however long the peer
/// was up by then. Resolving per call fixes that, and until the peer arrives the
/// tool answers with what is wrong rather than failing the orchestrator's task.
#[tokio::test]
async fn a_peer_that_registers_later_is_reachable() {
    let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
    let peer_resolver: Arc<dyn PeerResolver> =
        Arc::new(DiscoveredPeer::by_skill(registry.clone(), "echoing"));
    let source =
        A2aAgentToolSource::resolving("echo", "Echoes the input back.".to_string(), peer_resolver);

    // Advertised regardless: the model has to be able to call it at all before
    // it can be told the peer is missing.
    assert!(source.has_tool("ask_echo"));
    let unavailable = source
        .invoke("local-orchestrator-task", &tool_call("hello"))
        .await
        .expect("a missing peer is news for the model, not a broken run");
    assert!(
        unavailable.contains("not reachable") && unavailable.contains("echoing"),
        "the model must be told which reference found nothing: {unavailable}"
    );

    // The peer comes up and registers, after this orchestrator was assembled.
    let late = spawn_peer(Peer::Echo).await;
    let mut card = AgentCard {
        name: "echo".to_string(),
        ..Default::default()
    };
    card.skills = vec![AgentSkill::new(
        "echoing".to_string(),
        "Echoing".to_string(),
        "Repeats what it is told".to_string(),
        vec![],
    )];
    registry.register(card, late.base.clone()).await.unwrap();

    let reply = source
        .invoke("local-orchestrator-task", &tool_call("hello world"))
        .await
        .expect("the late joiner should now be delegated to");
    assert_eq!(reply, "Echo: hello world");
}

/// A peer that failed says so in its state, and its message reads exactly like
/// an answer. Returned bare, the model relays the apology onward as a result.
#[tokio::test]
async fn a_failed_peer_is_not_reported_as_an_answer() {
    let peer = spawn_peer(Peer::Answers(TaskState::Failed)).await;
    let source = peer.tool_source().with_deadline(Duration::from_secs(5));

    let reply = source
        .invoke("local-orchestrator-task", &tool_call("do the thing"))
        .await
        .expect("a failed peer task is an outcome, not a transport failure");
    assert!(
        reply.contains("failed the request"),
        "a failure must not be indistinguishable from an answer: {reply}"
    );
    assert!(
        reply.contains(FIXED_REPLY),
        "the peer's own words are missing: {reply}"
    );
}
