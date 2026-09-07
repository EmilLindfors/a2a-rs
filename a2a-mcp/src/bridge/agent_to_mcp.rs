//! Bridge that exposes A2A agents as MCP tools

use crate::{
    converters::{SkillSchemas, SkillToolConverter, TASK_ID_PROPERTY, TaskResultConverter},
    error::{A2aMcpError, Result},
};
use a2a_rs::{
    adapter::transport::http::HttpClient,
    domain::{AgentCard, Message, Part, Role, SendCompletion, Task, error::A2AError},
    port::AsyncMessageHandler,
    port::client::Transport,
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, model::*, service::RequestContext};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

/// Backend abstraction the bridge uses to reach the wrapped A2A agent.
///
/// Implementations of this trait are responsible for invoking skill messages,
/// subscribing to progress streams, and fetching task states.
#[async_trait]
pub trait BridgeBackend: Send + Sync {
    /// Send a message to invoke or continue an A2A task.
    async fn invoke(
        &self,
        task_id: &str,
        message: &Message,
        context_id: Option<&str>,
    ) -> std::result::Result<Task, A2AError>;

    /// Subscribe to real-time status and artifact updates for a running task.
    ///
    /// Returns `Ok(Some(stream))` if streaming is supported, or `Ok(None)` otherwise.
    async fn subscribe(
        &self,
        _task_id: &str,
    ) -> std::result::Result<
        Option<
            Pin<Box<dyn Stream<Item = std::result::Result<a2a_rs::StreamItem, A2AError>> + Send>>,
        >,
        A2AError,
    > {
        Ok(None)
    }

    /// Fetch the current state of a task by ID.
    ///
    /// Returns `Ok(Some(task))` if task retrieval is supported, or `Ok(None)` otherwise.
    async fn get_task(&self, _task_id: &str) -> std::result::Result<Option<Task>, A2AError> {
        Ok(None)
    }

    /// Fetch list of tasks (optional fallback).
    async fn list_tasks(
        &self,
        _params: &a2a_rs::domain::core::task::ListTasksParams,
    ) -> std::result::Result<Option<Vec<Task>>, A2AError> {
        Ok(None)
    }

    /// Cancel a running task.
    async fn cancel_task(&self, _task_id: &str) -> std::result::Result<Task, A2AError> {
        Err(A2AError::InvalidParams(
            "Cancellation not supported by this backend".to_string(),
        ))
    }
}

/// HTTP backend for the bridge, communicating with the agent via JSON-RPC over HTTP.
pub struct HttpBackend {
    pub client: HttpClient,
}

#[async_trait]
impl BridgeBackend for HttpBackend {
    async fn invoke(
        &self,
        task_id: &str,
        message: &Message,
        context_id: Option<&str>,
    ) -> std::result::Result<Task, A2AError> {
        // An MCP tool call is request/response: the caller wants the agent's
        // answer, not an acknowledgement, and has no poll loop of its own.
        self.client
            .send_task_message(
                Some(task_id),
                message,
                context_id,
                None,
                SendCompletion::WhenSettled,
            )
            .await
    }

    async fn get_task(&self, task_id: &str) -> std::result::Result<Option<Task>, A2AError> {
        self.client.get_task(task_id, None::<u32>).await.map(Some)
    }

    async fn list_tasks(
        &self,
        params: &a2a_rs::domain::core::task::ListTasksParams,
    ) -> std::result::Result<Option<Vec<Task>>, A2AError> {
        self.client
            .list_tasks(params)
            .await
            .map(|res| Some(res.tasks))
    }

    async fn cancel_task(&self, task_id: &str) -> std::result::Result<Task, A2AError> {
        self.client.cancel_task(task_id).await
    }
}

/// In-process backend for the bridge, dispatching calls directly to local handlers.
pub struct HandlerBackend<H: AsyncMessageHandler + Send + Sync + 'static> {
    pub handler: H,
    pub streaming_handler: Option<Arc<dyn a2a_rs::port::AsyncStreamingHandler>>,
}

impl<H> HandlerBackend<H>
where
    H: AsyncMessageHandler + Send + Sync + 'static,
{
    /// Create a new in-process handler backend without streaming.
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            streaming_handler: None,
        }
    }

    /// Create a new in-process handler backend with a streaming handler.
    pub fn with_streaming(
        handler: H,
        streaming_handler: Arc<dyn a2a_rs::port::AsyncStreamingHandler>,
    ) -> Self {
        Self {
            handler,
            streaming_handler: Some(streaming_handler),
        }
    }
}

#[async_trait]
impl<H> BridgeBackend for HandlerBackend<H>
where
    H: AsyncMessageHandler + Send + Sync + 'static,
{
    async fn invoke(
        &self,
        task_id: &str,
        message: &Message,
        context_id: Option<&str>,
    ) -> std::result::Result<Task, A2AError> {
        // An MCP tool call authenticates to the MCP server, not to the A2A
        // agent, so there is no A2A principal to name here.
        let ctx =
            a2a_rs::port::RequestContext::anonymous().with_context(context_id.unwrap_or_default());
        self.handler.process_message(task_id, message, &ctx).await
    }

    async fn subscribe(
        &self,
        task_id: &str,
    ) -> std::result::Result<
        Option<
            Pin<Box<dyn Stream<Item = std::result::Result<a2a_rs::StreamItem, A2AError>> + Send>>,
        >,
        A2AError,
    > {
        if let Some(ref sh) = self.streaming_handler {
            let stream = sh.combined_update_stream(task_id, None).await?;
            let mapped = stream.map(|res| {
                res.map(|seq| match seq.event {
                    a2a_rs::port::UpdateEvent::StatusUpdate(status) => {
                        a2a_rs::StreamItem::StatusUpdate(status)
                    }
                    a2a_rs::port::UpdateEvent::ArtifactUpdate(artifact) => {
                        a2a_rs::StreamItem::ArtifactUpdate(artifact)
                    }
                })
            });
            Ok(Some(Box::pin(mapped)))
        } else {
            Ok(None)
        }
    }
}

/// Bridge that exposes A2A agent skills as MCP tools
///
/// This allows MCP clients to invoke A2A agent capabilities through the MCP protocol.
/// Each skill from the A2A agent becomes a callable MCP tool.
///
/// The bridge can reach the agent in two ways:
///
/// * **HTTP** — [`AgentToMcpBridge::new`] takes an [`HttpClient`] and speaks
///   A2A's JSON-RPC over HTTP. Use this when the agent lives in another
///   process or on another host.
/// * **In-process** — [`AgentToMcpBridge::with_handler`] takes an
///   [`AsyncMessageHandler`] directly and calls it without going through the
///   network. Use this when the bridge and the agent live in the same process
///   to avoid a loopback HTTP server.
#[derive(Clone)]
pub struct AgentToMcpBridge {
    /// Backend used to dispatch tool calls to the wrapped agent
    backend: Arc<dyn BridgeBackend>,
    /// Agent card containing skills and metadata
    agent_card: Arc<AgentCard>,
    /// Cached list of MCP tools generated from agent skills
    tools: Arc<Vec<Tool>>,
    /// The typed skills' schemas, read once from the card's extension. A
    /// skill with an entry here is called with a data part; one without, with
    /// a `message` string.
    schemas: Arc<SkillSchemas>,
    /// Namespace prefix used for tools/prompts/resources
    namespace: String,
    /// Cache of tasks processed by this bridge (useful for in-process backends)
    tasks_cache: Arc<Mutex<HashMap<String, Task>>>,
    /// The skill each task in flight belongs to, so `tasks/update` can send
    /// the next message on it. Dropped when the task settles.
    task_skills: Arc<Mutex<HashMap<String, String>>>,
    /// How long a `tools/call` blocks before answering with a task id, for a
    /// client that declared the tasks extension. `None` never makes a task,
    /// so every call blocks to the end.
    task_grace: Option<std::time::Duration>,
    /// Optional custom name for the MCP server
    mcp_server_name: Option<String>,
    /// Optional custom version for the MCP server
    mcp_server_version: Option<String>,
}

impl AgentToMcpBridge {
    /// Create a new bridge from an A2A agent reached over HTTP.
    ///
    /// The agent's **name** — [`AgentCard::name`] — is the namespace of the
    /// resulting MCP tool names, so several agents can coexist on one MCP
    /// server. It used to be the agent's URL, which changes when the
    /// deployment does and gave a loopback agent a tool name starting with a
    /// digit; the name is what a tool name should follow. Gemini caps a
    /// function name at 64 characters, so a long agent name with a long skill
    /// id is the caller's to shorten through [`Self::with_namespace`].
    ///
    /// When the agent lives in the same process as the bridge, prefer
    /// [`Self::with_handler`] to skip the loopback HTTP hop.
    ///
    /// # Arguments
    ///
    /// * `client` - A2A client configured to communicate with the agent
    /// * `agent_card` - The agent's capabilities card (its `name` is used for namespacing)
    pub fn new(client: HttpClient, agent_card: AgentCard) -> Self {
        let namespace = agent_card.name.clone();
        Self::with_namespace(client, agent_card, namespace)
    }

    /// Create a new HTTP-backed bridge with an explicit tool-name namespace.
    ///
    /// Prefer [`Self::new`] unless you need the MCP tool names to be namespaced
    /// by something other than the agent's `name` (a short alias for a long
    /// name, or one that stays put across a rename).
    pub fn with_namespace(client: HttpClient, agent_card: AgentCard, namespace: String) -> Self {
        Self::from_backend(Arc::new(HttpBackend { client }), agent_card, namespace)
    }

    /// Create a new bridge that calls an in-process A2A handler directly.
    ///
    /// Use this when the bridge and the A2A agent live in the same process —
    /// it avoids spawning a loopback HTTP server and threads the call straight
    /// through [`AsyncMessageHandler::process_message`]. The namespace defaults
    /// to [`AgentCard::name`]; override with [`Self::with_handler_and_namespace`].
    ///
    /// # Arguments
    ///
    /// * `handler` - The A2A message handler to dispatch tool calls to
    /// * `agent_card` - The agent's capabilities card (its `name` is used for namespacing)
    pub fn with_handler<H>(handler: H, agent_card: AgentCard) -> Self
    where
        H: AsyncMessageHandler + Send + Sync + 'static,
    {
        let namespace = agent_card.name.clone();
        Self::with_handler_and_namespace(handler, agent_card, namespace)
    }

    /// Create a new in-process bridge with an explicit tool-name namespace.
    ///
    /// See [`Self::with_handler`] for when to prefer the in-process backend
    /// over the HTTP one, and [`Self::with_namespace`] for when an explicit
    /// namespace is useful.
    pub fn with_handler_and_namespace<H>(
        handler: H,
        agent_card: AgentCard,
        namespace: String,
    ) -> Self
    where
        H: AsyncMessageHandler + Send + Sync + 'static,
    {
        Self::from_backend(
            Arc::new(HandlerBackend::new(handler)),
            agent_card,
            namespace,
        )
    }

    /// Create a new in-process bridge that supports streaming updates.
    pub fn with_handler_and_streaming<H, S>(
        handler: H,
        streaming_handler: S,
        agent_card: AgentCard,
    ) -> Self
    where
        H: AsyncMessageHandler + Send + Sync + 'static,
        S: a2a_rs::port::AsyncStreamingHandler + 'static,
    {
        let namespace = agent_card.name.clone();
        Self::with_handler_streaming_and_namespace(
            handler,
            streaming_handler,
            agent_card,
            namespace,
        )
    }

    /// Create a new in-process bridge that supports streaming updates with an explicit namespace.
    pub fn with_handler_streaming_and_namespace<H, S>(
        handler: H,
        streaming_handler: S,
        agent_card: AgentCard,
        namespace: String,
    ) -> Self
    where
        H: AsyncMessageHandler + Send + Sync + 'static,
        S: a2a_rs::port::AsyncStreamingHandler + 'static,
    {
        Self::from_backend(
            Arc::new(HandlerBackend::with_streaming(
                handler,
                Arc::new(streaming_handler),
            )),
            agent_card,
            namespace,
        )
    }

    /// Create a bridge from a custom backend implementation.
    pub fn from_backend(
        backend: Arc<dyn BridgeBackend>,
        agent_card: AgentCard,
        namespace: String,
    ) -> Self {
        let schemas = SkillSchemas::from_card(&agent_card);
        let tools: Vec<Tool> = agent_card
            .skills
            .iter()
            .map(|skill| {
                SkillToolConverter::skill_to_tool(skill, &namespace, schemas.get(&skill.id))
            })
            .collect();

        info!(
            "Created AgentToMcpBridge for agent '{}' with {} tools ({} typed)",
            agent_card.name,
            tools.len(),
            schemas.0.len()
        );

        Self {
            backend,
            agent_card: Arc::new(agent_card),
            tools: Arc::new(tools),
            schemas: Arc::new(schemas),
            namespace,
            tasks_cache: Arc::new(Mutex::new(HashMap::new())),
            task_skills: Arc::new(Mutex::new(HashMap::new())),
            task_grace: Some(DEFAULT_TASK_GRACE),
            mcp_server_name: None,
            mcp_server_version: None,
        }
    }

    /// Set custom MCP server metadata (name and version) to be advertised in `ServerInfo`.
    pub fn with_mcp_metadata(mut self, name: Option<String>, version: Option<String>) -> Self {
        self.mcp_server_name = name;
        self.mcp_server_version = version;
        self
    }

    /// How long a `tools/call` waits before answering with a task id instead
    /// of a result. Defaults to [`DEFAULT_TASK_GRACE`].
    ///
    /// Only a client that declared the `io.modelcontextprotocol/tasks`
    /// extension is answered this way; every other client blocks to the end
    /// whatever this says.
    pub fn with_task_grace_period(mut self, grace: std::time::Duration) -> Self {
        self.task_grace = Some(grace);
        self
    }

    /// Never answer a `tools/call` with a task: every call blocks until the
    /// agent is done, as it did before the tasks extension.
    ///
    /// For a deployment whose agents are all quick, where a task id is one
    /// more round trip for nothing.
    pub fn without_task_results(mut self) -> Self {
        self.task_grace = None;
        self
    }

    fn create_artifact_uri(&self, task_id: &str, artifact_id: &str) -> String {
        let sanitized_ns = SkillToolConverter::sanitize_namespace(&self.namespace);
        format!(
            "a2a-artifact://{}/{}/{}",
            sanitized_ns, task_id, artifact_id
        )
    }

    fn parse_artifact_uri(uri: &str) -> std::result::Result<(String, String), McpError> {
        let prefix = "a2a-artifact://";
        if !uri.starts_with(prefix) {
            return Err(McpError::invalid_params(
                format!("Invalid URI scheme (expected a2a-artifact://): {}", uri),
                None,
            ));
        }
        let path = &uri[prefix.len()..];
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != 3 {
            return Err(McpError::invalid_params(
                format!("Invalid URI format: {}", uri),
                None,
            ));
        }
        Ok((parts[1].to_string(), parts[2].to_string()))
    }
}

struct TaskCancelGuard {
    backend: Arc<dyn BridgeBackend>,
    task_id: Option<String>,
}

impl Drop for TaskCancelGuard {
    fn drop(&mut self) {
        if let Some(ref id) = self.task_id {
            let backend = self.backend.clone();
            let id = id.clone();
            debug!("TaskCancelGuard dropping, canceling A2A task {}", id);
            tokio::spawn(async move {
                let _ = backend.cancel_task(&id).await;
            });
        }
    }
}

/// How often the polling fallback asks the agent for a task's state, and the
/// interval a task handed back to an MCP client is told to poll at.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// How long a `tools/call` waits for the agent before answering with a task
/// id instead of a result, for a client that declared the tasks extension.
///
/// SEP-2663 has no per-call opt-in and no `tasks/result`, so when to stop
/// blocking is the server's policy. Five seconds is under every MCP client
/// timeout seen so far and over the length of an ordinary agent turn, so a
/// tool that answers promptly still answers in the call.
pub const DEFAULT_TASK_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Where a call in flight reports, and what it does when the agent asks a
/// question.
///
/// The difference is whether the `tools/call` that started it is still open.
/// It is not a property of the bridge: the same tool, on the same bridge, is
/// a blocking call for a client that did not declare the tasks extension and
/// a task for one that did.
#[derive(Clone)]
enum Drive {
    /// The request is open and this call is its answer. Progress goes on the
    /// request's token, and `InputRequired` is elicited from the client's
    /// user before the call returns.
    InRequest {
        peer: rmcp::service::Peer<RoleServer>,
        progress: Option<ProgressToken>,
        /// Whether the client declared elicitation. Read from the request
        /// rather than the handshake, which the discover lifecycle does not
        /// fill.
        can_elicit: bool,
    },
    /// The request has been answered with a task id and the call is still
    /// running. Every change is a `notifications/tasks`, and `InputRequired`
    /// parks the task for `tasks/update` to answer.
    AsTask {
        peer: rmcp::service::Peer<RoleServer>,
    },
}

impl Drive {
    /// A call whose length nobody can predict has no measurable progress, so
    /// the state stands in for one.
    fn progress_for(state: &buffa::enumeration::EnumValue<a2a_rs::domain::TaskState>) -> f64 {
        use a2a_rs::domain::TaskState;
        use buffa::enumeration::EnumValue::Known;
        match state {
            Known(TaskState::Submitted) => 10.0,
            Known(TaskState::Working) => 50.0,
            Known(TaskState::InputRequired) => 75.0,
            Known(
                TaskState::Completed
                | TaskState::Failed
                | TaskState::Rejected
                | TaskState::Canceled,
            ) => 100.0,
            _ => 30.0,
        }
    }

    /// The task moved. Reported on the request's progress token, and never
    /// backwards — `floor` is the highest reported so far, because a stream
    /// that revisits `Working` after `InputRequired` would otherwise walk a
    /// progress bar back.
    async fn progress(&self, task: &Task, floor: &mut f64) {
        let Self::InRequest {
            peer,
            progress: Some(token),
            ..
        } = self
        else {
            return;
        };
        *floor = floor.max(Self::progress_for(&task.status.state));
        let mut param = ProgressNotificationParam::new(token.clone(), *floor).with_total(100.0);
        if let Some(message) = AgentToMcpBridge::status_text(task) {
            param = param.with_message(message);
        }
        let _ = peer.notify_progress(param).await;
    }

    /// A poll went round. Only a request in flight has anywhere to put that;
    /// a client holding a task is told about changes, not about attempts.
    async fn polled(&self, attempt: u32) {
        let Self::InRequest {
            peer,
            progress: Some(token),
            ..
        } = self
        else {
            return;
        };
        let param =
            ProgressNotificationParam::new(token.clone(), (f64::from(attempt) * 5.0).min(95.0))
                .with_total(100.0)
                .with_message(format!("Polling task status (attempt {attempt})"));
        let _ = peer.notify_progress(param).await;
    }

    /// The task changed, announced to a client holding it as a task.
    ///
    /// rmcp has no `notify_*` helper for `notifications/tasks`, so the
    /// notification is built and sent by hand. The body is the same
    /// `DetailedTask` `tasks/get` would return at this moment, which is what
    /// the extension specifies.
    async fn task_changed(&self, task: &Task) {
        let Self::AsTask { peer } = self else {
            return;
        };
        let detailed = match AgentToMcpBridge::convert_to_mcp_task(task) {
            Ok(detailed) => detailed,
            Err(e) => {
                debug!("Task {} could not be described to the client: {e}", task.id);
                return;
            }
        };
        let notification = TaskStatusNotification::new(TaskStatusNotificationParams::new(detailed));
        if let Err(e) = peer
            .send_notification(ServerNotification::TaskStatusNotification(notification))
            .await
        {
            debug!("notifications/tasks for {} was not delivered: {e}", task.id);
        }
    }
}

impl AgentToMcpBridge {
    fn map_task_state(
        state: &buffa::enumeration::EnumValue<a2a_rs::domain::TaskState>,
    ) -> rmcp::model::TaskStatus {
        match state {
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Submitted)
            | buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Working) => {
                rmcp::model::TaskStatus::Working
            }
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::InputRequired) => {
                rmcp::model::TaskStatus::InputRequired
            }
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Completed) => {
                rmcp::model::TaskStatus::Completed
            }
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Failed)
            | buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Rejected) => {
                rmcp::model::TaskStatus::Failed
            }
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Canceled) => {
                rmcp::model::TaskStatus::Cancelled
            }
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::AuthRequired) => {
                rmcp::model::TaskStatus::InputRequired
            }
            _ => rmcp::model::TaskStatus::Working,
        }
    }

    /// The text of a task's status message, if it has one with any.
    fn status_text(task: &a2a_rs::domain::Task) -> Option<String> {
        task.status
            .message
            .as_option()
            .map(|msg| {
                msg.parts
                    .iter()
                    .filter_map(|part| part.get_text())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|text| !text.is_empty())
    }

    /// The elicitation that asks the MCP client's user what an
    /// `InputRequired` task is waiting for: the status message as the
    /// question, one required string as the answer.
    fn elicitation_for(task: &a2a_rs::domain::Task) -> ElicitRequestParams {
        let question = Self::status_text(task)
            .unwrap_or_else(|| "The agent needs more input to continue.".to_string());
        let requested_schema = ElicitationSchema::builder()
            .required_string("answer")
            .build()
            .expect("one required string is a valid elicitation schema");
        ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: question,
            requested_schema,
        }
    }

    /// The text a `tasks/update` carries, out of the responses to the input
    /// requests `tasks/get` surfaced.
    ///
    /// One question is asked at a time — the `input` key of
    /// [`Self::convert_to_mcp_task`] — so the first response is the answer,
    /// whatever the client keyed it under. An elicitation result answers with
    /// `{action, content}`; a client that sends the bare string it would have
    /// typed is taken at its word rather than refused, since the A2A task
    /// receives text either way.
    ///
    /// `None` is a refusal: the elicitation was declined or cancelled, or the
    /// responses carry nothing.
    fn answer_from(responses: &InputResponses) -> Option<String> {
        let value = responses
            .get("input")
            .or_else(|| responses.values().next())?;
        if let Some(text) = value.as_str() {
            return Some(text.to_string());
        }
        let object = value.as_object()?;
        match object.get("action").and_then(serde_json::Value::as_str) {
            Some("accept") | None => {}
            Some(_) => return None,
        }
        let content = object.get("content")?;
        content
            .get("answer")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| Some(content.to_string()))
    }

    /// The A2A task as the tasks extension's `DetailedTask`: the state
    /// mapped, and what the state implies carried with it — the tool result
    /// for a completed task, the failure for a failed one, the question for
    /// one waiting on input.
    fn convert_to_mcp_task(
        task: &a2a_rs::domain::Task,
    ) -> std::result::Result<DetailedTask, McpError> {
        use a2a_rs::domain::TaskState;
        let updated_at_dt = task.status.timestamp_utc().unwrap_or_else(chrono::Utc::now);
        let updated_at = updated_at_dt.to_rfc3339();

        let status = Self::map_task_state(&task.status.state);
        let mut mcp_task =
            rmcp::model::Task::new(task.id.clone(), status, updated_at.clone(), updated_at);
        if let Some(text) = Self::status_text(task) {
            mcp_task = mcp_task.with_status_message(text);
        }

        let payload = match task.status.state {
            buffa::enumeration::EnumValue::Known(TaskState::Completed) => {
                let result = TaskResultConverter::task_to_result(task, None)
                    .map_err(|e| e.to_mcp_error())?;
                let result = serde_json::to_value(result)
                    .ok()
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default();
                TaskPayload::Completed { result }
            }
            buffa::enumeration::EnumValue::Known(TaskState::Failed | TaskState::Rejected) => {
                let message = Self::status_text(task).unwrap_or_else(|| "Task failed".to_string());
                let error = serde_json::json!({
                    "code": ErrorCode::INTERNAL_ERROR.0,
                    "message": message,
                });
                TaskPayload::Failed {
                    error: error.as_object().cloned().unwrap_or_default(),
                }
            }
            buffa::enumeration::EnumValue::Known(TaskState::Canceled) => TaskPayload::Cancelled,
            buffa::enumeration::EnumValue::Known(
                TaskState::InputRequired | TaskState::AuthRequired,
            ) => {
                let mut input_requests = InputRequests::new();
                input_requests.insert(
                    "input".to_string(),
                    InputRequest::Elicitation(ElicitRequest::new(Self::elicitation_for(task))),
                );
                TaskPayload::InputRequired { input_requests }
            }
            _ => TaskPayload::Working,
        };

        Ok(DetailedTask::new(mcp_task, payload))
    }

    /// The answer to a task's `InputRequired`, asked of the MCP client's user
    /// through elicitation — which is what `InputRequired` means: the agent
    /// has a question only the person can answer.
    ///
    /// `None` when there is nobody to ask: the client did not declare the
    /// elicitation capability, the request failed, or the user declined or
    /// cancelled. The caller then suspends the task and returns it with its
    /// id, which is what the bridge always did when sampling was unavailable
    /// and is now the only fallback. Sampling — asking the client's *model*
    /// to answer on the user's behalf — is deprecated by SEP-2577 with no
    /// replacement, and it was the wrong party to ask.
    async fn ask_for_input(&self, task: &Task, drive: &Drive) -> Option<String> {
        let Drive::InRequest {
            peer,
            can_elicit: true,
            ..
        } = drive
        else {
            debug!(
                "Task {} requires input and there is nobody this call can ask",
                task.id
            );
            return None;
        };

        let params = Self::elicitation_for(task);

        let result = match peer.create_elicitation(params).await {
            Ok(result) => result,
            Err(e) => {
                debug!("Elicitation for task {} failed: {e}", task.id);
                return None;
            }
        };
        match result.action {
            ElicitationAction::Accept => result
                .content
                .as_ref()
                .and_then(|content| content.get("answer"))
                .and_then(|answer| answer.as_str())
                .map(str::to_string),
            ElicitationAction::Decline | ElicitationAction::Cancel => {
                debug!("User declined to answer task {}", task.id);
                None
            }
            _ => None,
        }
    }

    /// Helper to call an A2A agent skill with support for streaming, progress
    /// notifications, and elicitation. `parts` is the request as the agent
    /// receives it: one text part for an untyped skill, one data part holding
    /// the typed arguments for a skill with an input schema.
    ///
    /// `drive` says who is listening and what to do with a question; it is
    /// owned rather than borrowed from the request so this can be spawned and
    /// outlive the `tools/call` that started it.
    async fn call_skill(
        &self,
        skill_id: &str,
        task_id: &str,
        parts: Vec<Part>,
        drive: Drive,
    ) -> Result<CallToolResult> {
        debug!(
            "Calling A2A skill '{}' with {} part(s)",
            skill_id,
            parts.len()
        );

        // Create an A2A message
        let message = Message::builder()
            .role(Role::User)
            .parts(parts)
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        // Dispatch to the configured backend (HTTP or in-process).
        let mut task = self
            .backend
            .invoke(task_id, &message, Some(skill_id))
            .await
            .map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;

        debug!("A2A agent returned task: {}", task.id);
        self.record(&task, &drive).await;

        let mut cancel_guard = TaskCancelGuard {
            backend: self.backend.clone(),
            task_id: Some(task.id.clone()),
        };

        if !TaskResultConverter::is_task_final(&task) {
            let stream_opt = self
                .backend
                .subscribe(&task.id)
                .await
                .map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;

            if let Some(mut stream) = stream_opt {
                debug!("Subscribed to task stream for task: {}", task.id);
                let mut last_progress: f64 = 0.0;
                while let Some(item_res) = stream.next().await {
                    let item =
                        item_res.map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;
                    match item {
                        a2a_rs::StreamItem::Task(t) => {
                            debug!("Stream initial task for {}: {:?}", t.id, t.status.state);
                            task = t;
                            self.record(&task, &drive).await;
                            drive.progress(&task, &mut last_progress).await;

                            if task.status.state == a2a_rs::domain::TaskState::InputRequired
                                && !self
                                    .answer_input_required(&mut task, task_id, skill_id, &drive)
                                    .await?
                            {
                                break;
                            }

                            if TaskResultConverter::is_task_final(&task) {
                                break;
                            }
                        }
                        a2a_rs::StreamItem::StatusUpdate(event) => {
                            debug!(
                                "Stream status update for {}: {:?}",
                                task.id, event.status.state
                            );
                            task.status = ::buffa::MessageField::some(event.status.clone());
                            self.record(&task, &drive).await;
                            drive.progress(&task, &mut last_progress).await;

                            if task.status.state == a2a_rs::domain::TaskState::InputRequired
                                && !self
                                    .answer_input_required(&mut task, task_id, skill_id, &drive)
                                    .await?
                            {
                                break;
                            }

                            if TaskResultConverter::is_task_final(&task) {
                                break;
                            }
                        }
                        a2a_rs::StreamItem::ArtifactUpdate(event) => {
                            debug!(
                                "Stream artifact update for {}: {}",
                                task.id, event.artifact.artifact_id
                            );
                            if event.append.unwrap_or(false) {
                                if let Some(existing) = task
                                    .artifacts
                                    .iter_mut()
                                    .find(|a| a.artifact_id == event.artifact.artifact_id)
                                {
                                    existing.parts.extend(event.artifact.parts.clone());
                                } else {
                                    task.artifacts.push(event.artifact);
                                }
                            } else if let Some(pos) = task
                                .artifacts
                                .iter()
                                .position(|a| a.artifact_id == event.artifact.artifact_id)
                            {
                                task.artifacts[pos] = event.artifact;
                            } else {
                                task.artifacts.push(event.artifact);
                            }
                            self.record(&task, &drive).await;
                        }
                    }
                }
            } else {
                debug!(
                    "Streaming not supported, falling back to polling for task: {}",
                    task.id
                );
                let mut last_state = task.status.state;
                let mut poll_count = 0;
                loop {
                    tokio::time::sleep(POLL_INTERVAL).await;
                    poll_count += 1;
                    drive.polled(poll_count).await;

                    if let Ok(Some(updated_task)) = self.backend.get_task(&task.id).await {
                        task = updated_task;
                        self.record(&task, &drive).await;

                        if task.status.state != last_state {
                            debug!(
                                "Polled task {} state changed to: {:?}",
                                task.id, task.status.state
                            );
                            last_state = task.status.state;
                        }

                        if task.status.state == a2a_rs::domain::TaskState::InputRequired
                            && !self
                                .answer_input_required(&mut task, task_id, skill_id, &drive)
                                .await?
                        {
                            break;
                        }

                        if TaskResultConverter::is_task_final(&task) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            // Do a final query to fetch the full task history and final state if supported
            if let Ok(Some(final_task)) = self.backend.get_task(&task.id).await {
                task = final_task;
            }
        }

        self.record(&task, &drive).await;

        // Defuse the cancel guard as the task has successfully completed/finished in this request
        cancel_guard.task_id = None;

        // Convert task to MCP result
        let output_schema = self
            .schemas
            .get(skill_id)
            .and_then(|schema| schema.output_schema.as_ref());
        let result = TaskResultConverter::task_to_result(&task, output_schema)?;

        info!(
            "A2A skill '{}' completed with state: {:?}",
            skill_id, task.status.state
        );

        Ok(result)
    }

    /// A task that stopped to ask something: answered where there is somebody
    /// to ask, parked where there is not.
    ///
    /// `true` when the answer went back to the agent and `task` now holds
    /// what it said next, so the caller keeps driving. `false` when the
    /// question stands — the client has no elicitation capability, the user
    /// declined, or the call is a task, where `InputRequired` is a state the
    /// client answers with `tasks/update` rather than a question the bridge
    /// raises on its own.
    async fn answer_input_required(
        &self,
        task: &mut Task,
        task_id: &str,
        skill_id: &str,
        drive: &Drive,
    ) -> Result<bool> {
        let Some(response_text) = self.ask_for_input(task, drive).await else {
            debug!(
                "No input obtainable for task {}; leaving it for the caller",
                task.id
            );
            return Ok(false);
        };

        let reply_msg = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text(response_text)])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        *task = self
            .backend
            .invoke(task_id, &reply_msg, Some(skill_id))
            .await
            .map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;
        self.record(task, drive).await;
        Ok(true)
    }

    /// The drive for a call the request is waiting on.
    ///
    /// Under the discover lifecycle (2026-07-28) the client's capabilities
    /// ride on each request rather than the handshake; `client_capabilities`
    /// reads either.
    fn in_request(ctx: &RequestContext<RoleServer>) -> Drive {
        Drive::InRequest {
            peer: ctx.peer.clone(),
            progress: ctx.meta.get_progress_token(),
            can_elicit: ctx
                .client_capabilities()
                .is_some_and(|caps| caps.elicitation.is_some()),
        }
    }

    /// Run the call, and answer either with its result or with the task it
    /// became.
    ///
    /// A client that did not declare the tasks extension has nowhere to put a
    /// task id, so its call blocks to the end as it always did — including
    /// the cancel-on-drop, which only works while the call is the request.
    ///
    /// A client that did gets whichever comes first: the result, or the grace
    /// period. On the grace period the call keeps running detached, the cache
    /// keeps up with it, and `notifications/tasks` says so. A call that
    /// *finished* inside the grace without settling — an agent that stopped
    /// to ask something — is a task too, because the answer comes through
    /// `tasks/update` and there is no result to return yet.
    async fn dispatch(
        &self,
        skill_id: String,
        task_id: String,
        parts: Vec<Part>,
        ctx: &RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, McpError> {
        let grace = match (self.task_grace, ctx.client_capabilities()) {
            (Some(grace), Some(caps)) if caps.supports_tasks() => grace,
            _ => {
                return match self
                    .call_skill(&skill_id, &task_id, parts, Self::in_request(ctx))
                    .await
                {
                    Ok(result) => Ok(result.into()),
                    Err(e) => Err(e.to_mcp_error()),
                };
            }
        };

        // Which skill this task belongs to, so `tasks/update` can send the
        // next message on it without the client naming the skill again.
        self.task_skills
            .lock()
            .await
            .insert(task_id.clone(), skill_id.clone());

        // `tasks/get` has to answer for this id from the moment the client is
        // told it, and the agent has not replied yet — a slow agent is the
        // whole reason this path exists. The seed is replaced by the agent's
        // own task on the first reply.
        self.tasks_cache
            .lock()
            .await
            .entry(task_id.clone())
            .or_insert_with(|| {
                Task::builder()
                    .id(task_id.clone())
                    .context_id(skill_id.clone())
                    .status(a2a_rs::domain::TaskStatus::new(
                        a2a_rs::domain::TaskState::Submitted,
                        None,
                    ))
                    .build()
            });

        // Not aborted on timeout: dropping a `JoinHandle` detaches the task,
        // which is the whole point — the call outlives the request.
        let mut handle = self.spawn_driver(skill_id.clone(), task_id.clone(), parts, &ctx.peer);

        match tokio::time::timeout(grace, &mut handle).await {
            Ok(Ok(Ok(result))) if self.has_settled(&task_id).await => Ok(result.into()),
            Ok(Ok(Ok(_))) => {
                debug!("Task {task_id} stopped short of an answer; handing it to the client");
                Ok(self.as_task(&task_id).await.into())
            }
            Ok(Ok(Err(e))) => Err(e.to_mcp_error()),
            Ok(Err(join)) => {
                self.task_skills.lock().await.remove(&task_id);
                Err(McpError::internal_error(
                    format!("The call to skill '{skill_id}' did not finish: {join}"),
                    None,
                ))
            }
            Err(_elapsed) => {
                info!("Skill '{skill_id}' is still running after the grace period; task {task_id}");
                Ok(self.as_task(&task_id).await.into())
            }
        }
    }

    /// Drive a call in the background, as a task the client polls.
    ///
    /// Used both for the call that outran its grace period and for the answer
    /// a `tasks/update` carries; `call_skill` starts by sending its parts, so
    /// resuming a parked task is the same code as starting one.
    fn spawn_driver(
        &self,
        skill_id: String,
        task_id: String,
        parts: Vec<Part>,
        peer: &rmcp::service::Peer<RoleServer>,
    ) -> tokio::task::JoinHandle<Result<CallToolResult>> {
        let driver = self.clone();
        let drive = Drive::AsTask { peer: peer.clone() };
        tokio::spawn(async move {
            let outcome = driver
                .call_skill(&skill_id, &task_id, parts, drive.clone())
                .await;
            if let Err(ref e) = outcome {
                // Nobody is waiting on this call's return value, so a failure
                // that only came back here would leave the task reading
                // `Working` for as long as the client cared to poll it.
                error!("Task {task_id} failed after it left the request: {e}");
                driver.fail_task(&task_id, &e.to_string(), &drive).await;
            }
            // The skill is only wanted while the task can still take another
            // message. A settled task cannot, and neither can a failed one.
            if outcome.is_err() || driver.has_settled(&task_id).await {
                driver.task_skills.lock().await.remove(&task_id);
            }
            outcome
        })
    }

    /// Mark a task failed in the cache, with the reason, so a client polling
    /// it is told rather than left waiting.
    async fn fail_task(&self, task_id: &str, reason: &str, drive: &Drive) {
        let mut task = match self.tasks_cache.lock().await.get(task_id).cloned() {
            Some(task) => task,
            None => return,
        };
        if TaskResultConverter::is_task_final(&task) {
            return;
        }
        let message = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(reason.to_string())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();
        task.status = ::buffa::MessageField::some(a2a_rs::domain::TaskStatus::new(
            a2a_rs::domain::TaskState::Failed,
            Some(message),
        ));
        self.record(&task, drive).await;
    }

    /// Whether the cached task has reached a state nothing follows.
    async fn has_settled(&self, task_id: &str) -> bool {
        self.tasks_cache
            .lock()
            .await
            .get(task_id)
            .is_some_and(TaskResultConverter::is_task_final)
    }

    /// The task handle a `tools/call` is answered with: the A2A task id as
    /// the MCP task id, its state as the cache last saw it, and the interval
    /// the bridge itself polls at as the one to poll it at.
    async fn as_task(&self, task_id: &str) -> CreateTaskResult {
        let cached = self.tasks_cache.lock().await.get(task_id).cloned();
        let now = chrono::Utc::now().to_rfc3339();
        let (status, message) = match &cached {
            Some(task) => (
                Self::map_task_state(&task.status.state),
                Self::status_text(task),
            ),
            None => (rmcp::model::TaskStatus::Working, None),
        };
        let mut task = rmcp::model::Task::new(task_id.to_string(), status, now.clone(), now)
            .with_poll_interval_ms(u64::try_from(POLL_INTERVAL.as_millis()).unwrap_or(u64::MAX));
        if let Some(message) = message {
            task = task.with_status_message(message);
        }
        CreateTaskResult::new(task)
    }

    /// The task as it now stands: cached, so `tasks/get` answers from it, and
    /// announced to a client that is holding it as a task.
    async fn record(&self, task: &Task, drive: &Drive) {
        self.tasks_cache
            .lock()
            .await
            .insert(task.id.clone(), task.clone());
        drive.task_changed(task).await;
    }
}

#[async_trait]
#[allow(clippy::manual_async_fn)]
impl ServerHandler for AgentToMcpBridge {
    fn get_info(&self) -> ServerInfo {
        let server_name = self
            .mcp_server_name
            .as_deref()
            .unwrap_or(&self.agent_card.name);
        // Fall back to agent_card.version, then to "0.1.0"
        let server_version =
            self.mcp_server_version
                .as_deref()
                .unwrap_or(if self.agent_card.version.is_empty() {
                    "0.1.0"
                } else {
                    &self.agent_card.version
                });

        let implementation =
            Implementation::new(format!("a2a-mcp-bridge:{}", server_name), server_version)
                .with_title(format!("A2A Agent: {}", server_name))
                .with_website_url(self.agent_card.url().to_string());

        let instructions = format!(
            "A2A Agent '{}' exposed as MCP tools. Available tools: {}",
            server_name,
            self.tools
                .iter()
                .map(|t| t.name.as_ref())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let mut extensions = ExtensionCapabilities::new();
        for scheme in self.agent_card.security_schemes.values() {
            if let Some(a2a_rs::domain::generated::security_scheme::Scheme::Oauth2SecurityScheme(
                oauth2_scheme,
            )) = &scheme.scheme
                && let Some(flows) = oauth2_scheme.flows.as_option()
                && let Some(a2a_rs::domain::generated::o_auth_flows::Flow::ClientCredentials(cc)) =
                    &flows.flow
            {
                let mut cc_settings = serde_json::Map::new();
                cc_settings.insert(
                    "tokenUrl".to_string(),
                    serde_json::Value::String(cc.token_url.clone()),
                );
                if !oauth2_scheme.oauth2_metadata_url.is_empty() {
                    cc_settings.insert(
                        "metadataUrl".to_string(),
                        serde_json::Value::String(oauth2_scheme.oauth2_metadata_url.clone()),
                    );
                }
                extensions.insert(
                    "io.modelcontextprotocol/oauth-client-credentials".to_string(),
                    cc_settings,
                );
            }
        }

        let caps = if !extensions.is_empty() {
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .enable_extensions_with(extensions)
                .enable_tasks()
                .build()
        } else {
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .enable_tasks()
                .build()
        };

        ServerInfo::new(caps)
            .with_server_info(implementation)
            .with_instructions(instructions)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_
    {
        async move {
            debug!("MCP client requested tool list");

            Ok(ListToolsResult::with_all_items((*self.tools).clone()))
        }
    }

    fn call_tool(
        &self,
        params: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<CallToolResponse, McpError>> + Send + '_
    {
        async move {
            let name = &params.name;
            info!("MCP client calling tool: {}", name);

            // The skill whose generated name this is — matched, not parsed,
            // so a skill id with an underscore in it resolves too.
            let skill_id = match SkillToolConverter::resolve_skill(
                &self.agent_card.skills,
                &self.namespace,
                name,
            ) {
                Some(skill) => skill.id.clone(),
                None => {
                    error!("No skill is served as tool '{}'", name);
                    return Err(McpError::internal_error(
                        format!("No skill is served as tool '{}'", name),
                        None,
                    ));
                }
            };

            let mut arguments = params.arguments.unwrap_or_default();

            let task_id = arguments
                .remove(TASK_ID_PROPERTY)
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            // A typed skill gets its arguments as they came, in one data part;
            // an untyped one gets the `message` string as text.
            let typed = self
                .schemas
                .get(&skill_id)
                .is_some_and(|schema| schema.input_schema.is_some());
            let parts = if typed {
                let value: ::buffa_types::google::protobuf::Value =
                    serde_json::from_value(serde_json::Value::Object(arguments)).map_err(|e| {
                        McpError::invalid_params(
                            format!("Arguments are not a JSON value: {e}"),
                            None,
                        )
                    })?;
                vec![Part::data(value)]
            } else {
                let message_text = arguments
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if message_text.is_empty() {
                    return Err(McpError::invalid_params(
                        "Missing required parameter 'message'",
                        None,
                    ));
                }
                vec![Part::text(message_text)]
            };

            self.dispatch(skill_id, task_id, parts, &ctx).await
        }
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<ListPromptsResult, McpError>> + Send + '_
    {
        async move {
            debug!("MCP client requested prompt list");

            let prompts = self
                .agent_card
                .skills
                .iter()
                .map(|skill| {
                    let prompt_name =
                        SkillToolConverter::create_tool_name(&self.namespace, &skill.id);
                    let arg = PromptArgument::new("message")
                        .with_description("The message or query to send to the agent skill")
                        .with_required(true);

                    Prompt::new(
                        prompt_name,
                        Some(skill.description.clone()),
                        Some(vec![arg]),
                    )
                    .with_title(skill.name.clone())
                })
                .collect();

            Ok(ListPromptsResult::with_all_items(prompts))
        }
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<GetPromptResponse, McpError>> + Send + '_
    {
        async move {
            let name = &request.name;
            info!("MCP client getting prompt: {}", name);

            // The skill whose generated name this is — matched, not parsed,
            // so a skill id with an underscore in it resolves too.
            let skill_id = match SkillToolConverter::resolve_skill(
                &self.agent_card.skills,
                &self.namespace,
                name,
            ) {
                Some(skill) => skill.id.clone(),
                None => {
                    error!("No skill is served as prompt '{}'", name);
                    return Err(McpError::internal_error(
                        format!("No skill is served as prompt '{}'", name),
                        None,
                    ));
                }
            };

            // Extract the message parameter from arguments
            let message_text = request
                .arguments
                .as_ref()
                .and_then(|args| args.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if message_text.is_empty() {
                return Err(McpError::invalid_params(
                    "Missing required parameter 'message'",
                    None,
                ));
            }

            let task_id = request
                .arguments
                .as_ref()
                .and_then(|args| args.get("task_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            // Call the A2A agent skill
            let tool_result = match self
                .call_skill(
                    &skill_id,
                    &task_id,
                    vec![Part::text(message_text.clone())],
                    Self::in_request(&ctx),
                )
                .await
            {
                Ok(result) => result,
                Err(e) => return Err(e.to_mcp_error()),
            };

            // Convert tool result content to prompt messages
            let mut prompt_messages = vec![PromptMessage::new(
                rmcp::model::Role::User,
                ContentBlock::text(message_text),
            )];
            for c in tool_result.content {
                prompt_messages.push(PromptMessage::new(rmcp::model::Role::Assistant, c));
            }

            Ok(GetPromptResult::new(prompt_messages).into())
        }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<ListResourcesResult, McpError>> + Send + '_
    {
        async move {
            debug!("MCP client requested resource list");

            // 1. Attempt to get tasks from the backend.
            let list_params = a2a_rs::domain::core::task::ListTasksParams {
                include_artifacts: Some(true),
                page_size: Some(100),
                ..Default::default()
            };

            let mut tasks = match self.backend.list_tasks(&list_params).await {
                Ok(Some(tasks)) => tasks,
                _ => {
                    // Fall back to tasks cache
                    let cache = self.tasks_cache.lock().await;
                    cache.values().cloned().collect()
                }
            };

            // Also merge any unique tasks from cache that might not be returned by backend
            {
                let cache = self.tasks_cache.lock().await;
                for (id, cached_task) in cache.iter() {
                    if !tasks.iter().any(|t| t.id == *id) {
                        tasks.push(cached_task.clone());
                    }
                }
            }

            let mut resources = Vec::new();
            for task in tasks {
                for artifact in &task.artifacts {
                    let uri = self.create_artifact_uri(&task.id, &artifact.artifact_id);
                    let name = if artifact.name.is_empty() {
                        artifact.artifact_id.clone()
                    } else {
                        artifact.name.clone()
                    };

                    // Try to determine mime type from parts
                    let mime_type = artifact
                        .parts
                        .iter()
                        .find_map(|p| match &p.content {
                            Some(a2a_rs::domain::generated::part::Content::Url(_))
                            | Some(a2a_rs::domain::generated::part::Content::Raw(_)) => {
                                if p.media_type.is_empty() {
                                    None
                                } else {
                                    Some(p.media_type.clone())
                                }
                            }
                            Some(a2a_rs::domain::generated::part::Content::Data(_)) => {
                                Some("application/json".to_string())
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| "text/plain".to_string());

                    resources.push(
                        Resource::new(uri, name)
                            .with_title(artifact.name.clone())
                            .with_description(artifact.description.clone())
                            .with_mime_type(mime_type),
                    );
                }
            }

            Ok(ListResourcesResult::with_all_items(resources))
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<ReadResourceResponse, McpError>> + Send + '_
    {
        async move {
            info!("MCP client reading resource: {}", request.uri);

            let (task_id, artifact_id) = Self::parse_artifact_uri(&request.uri)?;

            // 1. Attempt to fetch task from backend
            let mut task = match self.backend.get_task(&task_id).await {
                Ok(Some(t)) => Some(t),
                _ => None,
            };

            // 2. Fall back to cache if not found or backend retrieval is unsupported
            if task.is_none() {
                let cache = self.tasks_cache.lock().await;
                task = cache.get(&task_id).cloned();
            }

            let task = match task {
                Some(t) => t,
                None => {
                    return Err(McpError::invalid_params(
                        format!("Task not found: {}", task_id),
                        None,
                    ));
                }
            };

            // Find the artifact
            let artifact = task
                .artifacts
                .into_iter()
                .find(|a| a.artifact_id == artifact_id)
                .ok_or_else(|| {
                    McpError::invalid_params(
                        format!("Artifact {} not found in task {}", artifact_id, task_id),
                        None,
                    )
                })?;

            let mut contents = Vec::new();
            for part in artifact.parts {
                match part.content {
                    Some(a2a_rs::domain::generated::part::Content::Text(text)) => {
                        contents.push(ResourceContents::text(text, request.uri.clone()));
                    }
                    Some(a2a_rs::domain::generated::part::Content::Raw(bytes)) => {
                        use base64::Engine as _;
                        let blob = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        contents.push(
                            ResourceContents::blob(blob, request.uri.clone()).with_mime_type(
                                if part.media_type.is_empty() {
                                    "application/octet-stream".to_string()
                                } else {
                                    part.media_type.clone()
                                },
                            ),
                        );
                    }
                    Some(a2a_rs::domain::generated::part::Content::Url(uri)) => {
                        contents.push(ResourceContents::text(
                            format!("File URI: {}", uri),
                            request.uri.clone(),
                        ));
                    }
                    Some(a2a_rs::domain::generated::part::Content::Data(data)) => {
                        let data_json = match serde_json::to_string_pretty(&data) {
                            Ok(json) => json,
                            Err(e) => {
                                return Err(McpError::internal_error(
                                    format!("Failed to serialize data: {}", e),
                                    None,
                                ));
                            }
                        };
                        contents.push(
                            ResourceContents::text(data_json, request.uri.clone())
                                .with_mime_type("application/json"),
                        );
                    }
                    None => {
                        contents.push(ResourceContents::text(
                            format!("File: {}", part.filename),
                            request.uri.clone(),
                        ));
                    }
                }
            }

            Ok(ReadResourceResult::new(contents).into())
        }
    }

    /// `tasks/get` of the tasks extension: the A2A task as a detailed MCP
    /// task, its final result inlined when it has one.
    fn get_task(
        &self,
        request: GetTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<GetTaskResult, McpError>> + Send + '_
    {
        async move {
            info!("MCP client getting task: {}", request.task_id);
            let mut a2a_task = match self.backend.get_task(&request.task_id).await {
                Ok(Some(t)) => Some(t),
                _ => None,
            };

            if a2a_task.is_none() {
                let cache = self.tasks_cache.lock().await;
                a2a_task = cache.get(&request.task_id).cloned();
            }

            let a2a_task = a2a_task.ok_or_else(|| {
                McpError::invalid_params(format!("Task {} not found", request.task_id), None)
            })?;

            Ok(GetTaskResult::new(Self::convert_to_mcp_task(&a2a_task)?))
        }
    }

    /// `tasks/update` of the tasks extension: the client's answer to a task
    /// that stopped to ask something.
    ///
    /// The answer becomes the next A2A message on that task, which is what
    /// `InputRequired` is waiting for, and driving resumes in the background
    /// — so this acknowledges immediately, as the extension requires, and the
    /// result arrives through `tasks/get` or `notifications/tasks`.
    ///
    /// A declined or cancelled answer cancels the A2A task. The agent asked a
    /// question it cannot continue without, so leaving the task parked would
    /// leave it parked forever.
    fn update_task(
        &self,
        request: UpdateTaskParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<(), McpError>> + Send + '_ {
        async move {
            info!("MCP client updating task: {}", request.task_id);

            let skill_id = self
                .task_skills
                .lock()
                .await
                .get(&request.task_id)
                .cloned()
                .ok_or_else(|| {
                    McpError::invalid_params(
                        format!("Task {} is not waiting for input", request.task_id),
                        None,
                    )
                })?;

            match Self::answer_from(&request.input_responses) {
                Some(answer) => {
                    self.spawn_driver(
                        skill_id,
                        request.task_id.clone(),
                        vec![Part::text(answer)],
                        &context.peer,
                    );
                }
                None => {
                    debug!(
                        "The answer to task {} declines; cancelling it",
                        request.task_id
                    );
                    let cancelled =
                        self.backend
                            .cancel_task(&request.task_id)
                            .await
                            .map_err(|e| {
                                McpError::internal_error(
                                    format!("Failed to cancel A2A task {}: {}", request.task_id, e),
                                    None,
                                )
                            })?;
                    self.task_skills.lock().await.remove(&request.task_id);
                    self.record(
                        &cancelled,
                        &Drive::AsTask {
                            peer: context.peer.clone(),
                        },
                    )
                    .await;
                }
            }

            Ok(())
        }
    }

    /// `tasks/cancel`: cooperative, acknowledged with nothing — the state
    /// change shows on the next `tasks/get`.
    fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<(), McpError>> + Send + '_ {
        async move {
            info!("MCP client canceling task: {}", request.task_id);
            let a2a_task = self
                .backend
                .cancel_task(&request.task_id)
                .await
                .map_err(|e| {
                    McpError::internal_error(
                        format!("Failed to cancel A2A task {}: {}", request.task_id, e),
                        None,
                    )
                })?;
            self.tasks_cache
                .lock()
                .await
                .insert(a2a_task.id.clone(), a2a_task);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_rs::domain::core::agent::AgentSkill;

    #[test]
    fn test_bridge_creation() {
        let agent_card = AgentCard::builder()
            .name("Test Agent".to_string())
            .description("A test agent".to_string())
            .url("https://example.com".to_string())
            .version("1.0.0".to_string())
            .capabilities(Default::default())
            .default_input_modes(vec!["text".to_string()])
            .default_output_modes(vec!["text".to_string()])
            .skills(vec![AgentSkill::new(
                "test_skill".to_string(),
                "Test Skill".to_string(),
                "A test skill".to_string(),
                vec![],
            )])
            .build();

        let client = HttpClient::new("https://example.com".to_string());
        let bridge = AgentToMcpBridge::new(client, agent_card);

        assert_eq!(bridge.tools.len(), 1);
        assert!(bridge.tools[0].name.contains("test_skill"));
    }

    #[test]
    fn test_bridge_uses_card_name_for_namespacing() {
        // new() derives the tool namespace from agent_card.name with no need
        // for a caller to pass it separately — and not from the url, which
        // changes with the deployment.
        let agent_card = AgentCard::builder()
            .name("Test Agent".to_string())
            .description("A test agent".to_string())
            .url("https://card-url.example.com".to_string())
            .version("1.0.0".to_string())
            .capabilities(Default::default())
            .default_input_modes(vec!["text".to_string()])
            .default_output_modes(vec!["text".to_string()])
            .skills(vec![AgentSkill::new(
                "do_thing".to_string(),
                "Do Thing".to_string(),
                "Does a thing".to_string(),
                vec![],
            )])
            .build();

        let client = HttpClient::new("https://card-url.example.com".to_string());
        let bridge = AgentToMcpBridge::new(client, agent_card);

        assert_eq!(bridge.tools[0].name.as_ref(), "test_agent_do_thing");
    }

    #[test]
    fn test_with_namespace_overrides_card_name() {
        let agent_card = AgentCard::builder()
            .name("Test Agent".to_string())
            .description("A test agent".to_string())
            .url("https://public.example.com".to_string())
            .version("1.0.0".to_string())
            .capabilities(Default::default())
            .default_input_modes(vec!["text".to_string()])
            .default_output_modes(vec!["text".to_string()])
            .skills(vec![AgentSkill::new(
                "do_thing".to_string(),
                "Do Thing".to_string(),
                "Does a thing".to_string(),
                vec![],
            )])
            .build();

        let client = HttpClient::new("https://public.example.com".to_string());
        let bridge =
            AgentToMcpBridge::with_namespace(client, agent_card, "internal-alias".to_string());

        assert_eq!(bridge.tools[0].name.as_ref(), "internal_alias_do_thing");
    }
}
