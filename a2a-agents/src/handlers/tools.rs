//! Tool sources for the generic LLM handler.
//!
//! A [`ToolSource`] is the unifying abstraction behind the LLM tool-calling
//! loop: it advertises a set of [`ToolDefinition`]s to the model and executes a
//! [`ToolCall`] the model emits, returning a stringified result. The handler no
//! longer knows whether a tool is backed by an MCP server or by another A2A
//! agent — both are just sources.
//!
//! Two implementations ship today:
//!
//! * [`McpToolSource`] — exposes the tools of one connected MCP server (one per
//!   `[[features.mcp_client.servers]]`).
//! * [`A2aAgentToolSource`] — exposes **another A2A agent as a single tool**, so
//!   an LLM agent can delegate to peer agents (the multi-agent keystone). The
//!   remote agent is reached through the [`Transport`] port, so any wire
//!   protocol (ConnectRPC, JSON-RPC) works.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_agents_common::llm::{ToolCall, ToolDefinition};
use a2a_rs::domain::{
    A2AError, Message, Part, Role, SendCompletion, Task, TaskState, TaskStateExt,
};
use a2a_rs::port::Transport;
use a2a_rs::{StreamEvent, StreamItem};
use async_trait::async_trait;
use buffa::EnumValue;
use futures::{Stream, StreamExt};
use tokio::time::Instant;

/// A provider of LLM-callable tools, independent of what backs them.
#[async_trait]
pub trait ToolSource: Send + Sync {
    /// The LLM-facing tool definitions this source advertises.
    fn tool_defs(&self) -> Vec<ToolDefinition>;

    /// Whether this source owns (and can execute) the named tool.
    fn has_tool(&self, name: &str) -> bool;

    /// Execute a single tool call, returning the stringified result.
    async fn invoke(&self, task_id: &str, call: &ToolCall) -> Result<String, A2AError>;
}

/// Find the source that owns `name`, if any. First match wins, so callers should
/// keep tool names unique across sources.
pub fn resolve<'a>(sources: &'a [Arc<dyn ToolSource>], name: &str) -> Option<&'a dyn ToolSource> {
    sources
        .iter()
        .find(|s| s.has_tool(name))
        .map(|s| s.as_ref())
}

/// Flatten the tool definitions of every source into one list for the LLM.
pub fn collect_tool_defs(sources: &[Arc<dyn ToolSource>]) -> Vec<ToolDefinition> {
    sources.iter().flat_map(|s| s.tool_defs()).collect()
}

// --- A2A agent as a tool -----------------------------------------------------

/// A subscription to a peer's task updates.
type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>;

/// The peer cannot be reached at all: nothing matches the reference, or dialing
/// what does failed.
///
/// Deliberately not an [`A2AError`]. An error raised *during* a delegation means
/// the peer took the work and we lost it, which breaks the orchestrator's run;
/// this means the tool is unusable right now, which the model can route around
/// by asking someone else or saying so. One is a failure, the other is news.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PeerUnavailable(String);

impl PeerUnavailable {
    /// Report a peer as unreachable, with the reason the model will be shown.
    pub fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }
}

/// Finds the peer a delegation tool talks to, at the moment it is called.
///
/// *When* the lookup happens is the whole of this port: an orchestrator that
/// resolved its peers once at startup is blind to every agent that joins
/// afterwards, and under a control plane agents come and go by design.
#[async_trait]
pub trait PeerResolver: Send + Sync {
    /// A transport for the peer as it can be reached right now.
    async fn resolve(&self) -> Result<Arc<dyn Transport>, PeerUnavailable>;
}

/// A peer that is already connected — for a caller holding a transport it built
/// itself. Nothing to re-resolve.
pub struct ConnectedPeer(Arc<dyn Transport>);

impl ConnectedPeer {
    /// Wrap an existing transport as a resolver.
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self(transport)
    }
}

#[async_trait]
impl PeerResolver for ConnectedPeer {
    async fn resolve(&self) -> Result<Arc<dyn Transport>, PeerUnavailable> {
        Ok(self.0.clone())
    }
}

/// Exposes a remote A2A agent as a single LLM tool named `ask_<slug>`.
///
/// On invocation it sends the model-supplied `message` to the remote agent as an
/// A2A task, waits for that task to settle (A2A tasks are asynchronous), and
/// returns the agent's reply text.
pub struct A2aAgentToolSource {
    agent: String,
    tool_name: String,
    description: String,
    peer: Arc<dyn PeerResolver>,
    poll_interval: Duration,
    deadline: Duration,
}

impl A2aAgentToolSource {
    /// Build a tool source for a remote agent already connected. `name` is the
    /// agent's friendly name (used to derive the tool name); `description`
    /// steers the model on when to delegate (typically the agent card's
    /// description + skills).
    pub fn new(name: &str, description: String, transport: Arc<dyn Transport>) -> Self {
        Self::resolving(name, description, Arc::new(ConnectedPeer::new(transport)))
    }

    /// Build a tool source that locates its peer through `resolver` on every
    /// call, so an agent that joins after this one started is still reachable.
    pub fn resolving(name: &str, description: String, peer: Arc<dyn PeerResolver>) -> Self {
        Self {
            agent: name.to_string(),
            tool_name: tool_name_for(name),
            description,
            peer,
            poll_interval: Duration::from_millis(250),
            deadline: Duration::from_secs(60),
        }
    }

    /// Override how long to wait for the remote task to finish (default 60s).
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// The tool name this source advertises (`ask_<slug>`).
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Wait for the peer's task to settle, or for `deadline` to pass. Returns
    /// the task as it stands either way; the caller decides which happened.
    ///
    /// Prefers the peer's event stream: it wakes on the peer's actual progress
    /// instead of on a timer, and an a2a-rs peer closes the subscription the
    /// moment the task settles. Polling stays as the fallback for a peer with
    /// no streaming backend, or one whose stream drops before the task
    /// finishes — the task is re-read once the stream ends, so neither
    /// mechanism failing can turn a finished task into a timeout.
    async fn wait_until_settled(
        &self,
        transport: &dyn Transport,
        remote_task_id: &str,
        deadline: Instant,
    ) -> Result<Task, A2AError> {
        if let Ok(stream) = transport
            .subscribe_to_task(remote_task_id, Some(1), None)
            .await
        {
            watch_until_settled(stream, deadline).await;
            // Re-read rather than trusting the last event: the stream may have
            // ended on an error or on the deadline rather than on the task.
            let task = transport.get_task(remote_task_id, Some(1)).await?;
            if task_settled(&task) || Instant::now() >= deadline {
                return Ok(task);
            }
        }
        self.poll_until_settled(transport, remote_task_id, deadline)
            .await
    }

    /// Re-read the peer's task on a timer until it settles or `deadline` passes.
    async fn poll_until_settled(
        &self,
        transport: &dyn Transport,
        remote_task_id: &str,
        deadline: Instant,
    ) -> Result<Task, A2AError> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::time::sleep(self.poll_interval.min(remaining)).await;
            let task = transport.get_task(remote_task_id, Some(1)).await?;
            if task_settled(&task) || Instant::now() >= deadline {
                return Ok(task);
            }
        }
    }
}

#[async_trait]
impl ToolSource for A2aAgentToolSource {
    fn tool_defs(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: self.tool_name.clone(),
            description: self.description.clone(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The natural-language request to send to the agent."
                    }
                },
                "required": ["message"]
            }),
        }]
    }

    fn has_tool(&self, name: &str) -> bool {
        name == self.tool_name
    }

    async fn invoke(&self, _task_id: &str, call: &ToolCall) -> Result<String, A2AError> {
        let args: serde_json::Value = serde_json::from_str(&call.arguments)
            .map_err(|e| A2AError::InvalidParams(format!("tool arguments must be JSON: {e}")))?;
        let text = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| A2AError::InvalidParams("missing `message` string argument".into()))?;

        // Resolved per call, so a peer that joined after this orchestrator
        // started is reachable. Being unable to reach it at all is reported to
        // the model rather than failing the orchestrator's own task: it can ask
        // another agent, or say what it could not do.
        let transport = match self.peer.resolve().await {
            Ok(transport) => transport,
            Err(unavailable) => {
                return Ok(format!(
                    "the '{}' agent is not reachable right now: {unavailable}",
                    self.agent
                ));
            }
        };

        // Each delegation is its own remote task; let the remote assign context.
        let remote_task_id = uuid::Uuid::new_v4().to_string();
        let msg = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text(text.to_string())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        // Return as soon as the peer accepts, so `self.deadline` below is what
        // governs. Letting the peer block instead would stack its wait on top
        // of ours and silently promote the longer of the two — a 400ms
        // delegation deadline would sit behind the peer's 25s.
        let deadline = Instant::now() + self.deadline;
        let mut task = transport
            .send_task_message(
                &remote_task_id,
                &msg,
                None,
                Some(1),
                SendCompletion::WhenCreated,
            )
            .await?;

        if !task_settled(&task) {
            task = self
                .wait_until_settled(transport.as_ref(), &remote_task_id, deadline)
                .await?;
        }
        if !task_settled(&task) {
            return Err(A2AError::Internal(format!(
                "remote agent tool '{}' did not finish within {:?}",
                self.tool_name, self.deadline
            )));
        }
        Ok(delegation_result(&self.agent, &task))
    }
}

/// Drain a subscription until it reports a settled state, ends, errors, or runs
/// past `deadline`. The caller re-reads the task to find out which, so all four
/// mean the same thing here: stop watching.
async fn watch_until_settled(mut stream: EventStream, deadline: Instant) {
    let drain = async {
        while let Some(Ok(event)) = stream.next().await {
            if event_settles(&event) {
                break;
            }
        }
    };
    let _ = tokio::time::timeout_at(deadline, drain).await;
}

/// Whether an event says the peer has stopped working on the task.
///
/// An artifact chunk never does: `last_chunk` ends an artifact, and a working
/// task may emit several more.
fn event_settles(event: &StreamEvent) -> bool {
    match &event.item {
        StreamItem::Task(task) => task.status.state.is_settled(),
        StreamItem::StatusUpdate(update) => update.status.state.is_settled(),
        StreamItem::ArtifactUpdate(_) => false,
    }
}

/// Derive an LLM tool name (`ask_<slug>`) from a free-form agent name.
pub fn tool_name_for(agent: &str) -> String {
    format!("ask_{}", crate::utils::slugify(agent, '_'))
}

/// True once the peer has stopped making progress on its own — finished, or
/// interrupted waiting on its caller.
///
/// Interrupted counts: a peer that asks a question has stopped, and waiting for
/// it to go terminal burns the whole deadline on a task that will never move
/// (the question was put to a model that is not watching for it).
fn task_settled(task: &Task) -> bool {
    task.status
        .as_option()
        .map(|s| s.state.is_settled())
        .unwrap_or(false)
}

/// What to hand back to the model, given how the peer's task ended.
///
/// A completed task's reply goes back bare — it is the answer, and framing it
/// would put words in the peer's mouth. Every other settled state is labelled,
/// because the peer's text alone does not say which one happened: a `failed`
/// task's message reads exactly like an answer, so the model would relay an
/// apology onward as a result.
fn delegation_result(agent: &str, task: &Task) -> String {
    let reply = task_reply(task);
    match outcome_note(&task.status.state) {
        Some(note) => format!("the '{agent}' agent {note}: {reply}"),
        None => reply,
    }
}

/// How to introduce the peer's text, given the state its task ended in. `None`
/// means the text stands on its own.
fn outcome_note(state: &EnumValue<TaskState>) -> Option<&'static str> {
    match state {
        EnumValue::Known(TaskState::InputRequired) => {
            Some("needs more information before it can answer")
        }
        EnumValue::Known(TaskState::AuthRequired) => {
            Some("needs authentication before it can answer")
        }
        EnumValue::Known(TaskState::Failed) => Some("failed the request"),
        EnumValue::Known(TaskState::Rejected) => Some("rejected the request"),
        EnumValue::Known(TaskState::Canceled) => Some("had its task canceled"),
        _ => None,
    }
}

/// Extract the agent's reply text from a finished task's status message.
fn task_reply(task: &Task) -> String {
    task.status
        .as_option()
        .and_then(|s| s.message.as_option())
        .map(|m| {
            m.parts
                .iter()
                .filter_map(|p| p.get_text())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(the agent returned no text)".to_string())
}

// --- MCP server as a tool source --------------------------------------------

#[cfg(feature = "mcp-server")]
pub use mcp::{McpToolSource, UnusedInner};

#[cfg(feature = "mcp-server")]
mod mcp {
    use super::*;
    use a2a_mcp::McpToA2ABridge;
    use a2a_rs::domain::Task;
    use a2a_rs::port::{AsyncMessageHandler, RequestContext};

    /// Filler inner handler for [`McpToA2ABridge`], which is generic over an
    /// `AsyncMessageHandler` it would delegate non-tool messages to. The LLM
    /// tool path never delegates, so this never runs.
    #[derive(Clone)]
    pub struct UnusedInner;

    #[async_trait]
    impl AsyncMessageHandler for UnusedInner {
        async fn process_message(
            &self,
            _task_id: &str,
            _message: &Message,
            _ctx: &RequestContext,
        ) -> Result<Task, A2AError> {
            Err(A2AError::UnsupportedOperation(
                "the generic LLM handler does not delegate to the MCP bridge".to_string(),
            ))
        }
    }

    /// Exposes one connected MCP server's tools to the LLM loop.
    pub struct McpToolSource {
        bridge: Arc<McpToA2ABridge<UnusedInner>>,
    }

    impl McpToolSource {
        pub fn new(bridge: Arc<McpToA2ABridge<UnusedInner>>) -> Self {
            Self { bridge }
        }
    }

    #[async_trait]
    impl ToolSource for McpToolSource {
        fn tool_defs(&self) -> Vec<ToolDefinition> {
            self.bridge.get_llm_tools()
        }

        fn has_tool(&self, name: &str) -> bool {
            self.bridge.tools().iter().any(|t| t.name.as_ref() == name)
        }

        async fn invoke(&self, task_id: &str, call: &ToolCall) -> Result<String, A2AError> {
            self.bridge
                .execute_llm_tool_call(task_id, call)
                .await
                .map_err(|e| e.to_a2a_error())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_is_slugified_and_prefixed() {
        assert_eq!(tool_name_for("Weather Agent"), "ask_weather_agent");
        assert_eq!(tool_name_for("billing-v2"), "ask_billing_v2");
        assert_eq!(tool_name_for("  Spaces  "), "ask_spaces");
    }

    #[derive(Clone)]
    struct FakeSource {
        name: String,
        result: String,
    }

    #[async_trait]
    impl ToolSource for FakeSource {
        fn tool_defs(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: self.name.clone(),
                description: "fake".into(),
                parameters: serde_json::json!({"type": "object"}),
            }]
        }
        fn has_tool(&self, name: &str) -> bool {
            name == self.name
        }
        async fn invoke(&self, _task_id: &str, _call: &ToolCall) -> Result<String, A2AError> {
            Ok(self.result.clone())
        }
    }

    #[test]
    fn resolve_picks_owning_source_and_collects_defs() {
        let sources: Vec<Arc<dyn ToolSource>> = vec![
            Arc::new(FakeSource {
                name: "alpha".into(),
                result: "a".into(),
            }),
            Arc::new(FakeSource {
                name: "beta".into(),
                result: "b".into(),
            }),
        ];

        assert!(resolve(&sources, "beta").is_some());
        assert!(resolve(&sources, "missing").is_none());
        assert_eq!(collect_tool_defs(&sources).len(), 2);
    }

    #[tokio::test]
    async fn resolved_source_executes() {
        let sources: Vec<Arc<dyn ToolSource>> = vec![Arc::new(FakeSource {
            name: "alpha".into(),
            result: "hello".into(),
        })];
        let src = resolve(&sources, "alpha").unwrap();
        let call = ToolCall {
            id: "1".into(),
            name: "alpha".into(),
            arguments: "{}".into(),
        };
        assert_eq!(src.invoke("t1", &call).await.unwrap(), "hello");
    }
}
