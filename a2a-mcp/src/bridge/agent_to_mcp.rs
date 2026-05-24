//! Bridge that exposes A2A agents as MCP tools

use crate::{
    converters::{SkillToolConverter, TaskResultConverter},
    error::{A2aMcpError, Result},
};
use a2a_rs::{
    adapter::transport::http::HttpClient,
    domain::{error::A2AError, AgentCard, Message, Part, Role, Task},
    port::AsyncMessageHandler,
    services::client::AsyncA2AClient,
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use rmcp::{model::*, service::RequestContext, ErrorData as McpError, RoleServer, ServerHandler};
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
        session_id: Option<&str>,
    ) -> std::result::Result<Task, A2AError>;

    /// Subscribe to real-time status and artifact updates for a running task.
    ///
    /// Returns `Ok(Some(stream))` if streaming is supported, or `Ok(None)` otherwise.
    async fn subscribe(
        &self,
        _task_id: &str,
    ) -> std::result::Result<
        Option<
            Pin<
                Box<
                    dyn Stream<Item = std::result::Result<a2a_rs::services::StreamItem, A2AError>>
                        + Send,
                >,
            >,
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
        Err(A2AError::InvalidParams("Cancellation not supported by this backend".to_string()))
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
        session_id: Option<&str>,
    ) -> std::result::Result<Task, A2AError> {
        self.client
            .send_task_message(task_id, message, session_id, None)
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
        session_id: Option<&str>,
    ) -> std::result::Result<Task, A2AError> {
        self.handler
            .process_message(task_id, message, session_id)
            .await
    }

    async fn subscribe(
        &self,
        task_id: &str,
    ) -> std::result::Result<
        Option<
            Pin<
                Box<
                    dyn Stream<Item = std::result::Result<a2a_rs::services::StreamItem, A2AError>>
                        + Send,
                >,
            >,
        >,
        A2AError,
    > {
        if let Some(ref sh) = self.streaming_handler {
            let stream = sh.combined_update_stream(task_id).await?;
            let mapped = stream.map(|res| {
                res.map(|event| match event {
                    a2a_rs::port::UpdateEvent::StatusUpdate(status) => {
                        a2a_rs::services::StreamItem::StatusUpdate(status)
                    }
                    a2a_rs::port::UpdateEvent::ArtifactUpdate(artifact) => {
                        a2a_rs::services::StreamItem::ArtifactUpdate(artifact)
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
    /// Namespace prefix used for tools/prompts/resources
    namespace: String,
    /// Cache of tasks processed by this bridge (useful for in-process backends)
    tasks_cache: Arc<Mutex<HashMap<String, Task>>>,
    /// Optional custom name for the MCP server
    mcp_server_name: Option<String>,
    /// Optional custom version for the MCP server
    mcp_server_version: Option<String>,
}

impl AgentToMcpBridge {
    /// Create a new bridge from an A2A agent reached over HTTP.
    ///
    /// The agent's URL — read from [`AgentCard::url`] — is used to namespace
    /// the resulting MCP tool names so multiple agents can coexist on one
    /// MCP server. If you need a different namespace (e.g. an internal alias
    /// distinct from the publicly-advertised URL), use [`Self::with_namespace`].
    ///
    /// When the agent lives in the same process as the bridge, prefer
    /// [`Self::with_handler`] to skip the loopback HTTP hop.
    ///
    /// # Arguments
    ///
    /// * `client` - A2A client configured to communicate with the agent
    /// * `agent_card` - The agent's capabilities card (its `url` is used for namespacing)
    pub fn new(client: HttpClient, agent_card: AgentCard) -> Self {
        let namespace = agent_card.url().to_string();
        Self::with_namespace(client, agent_card, namespace)
    }

    /// Create a new HTTP-backed bridge with an explicit tool-name namespace.
    ///
    /// Prefer [`Self::new`] unless you need the MCP tool names to be namespaced
    /// by something other than the agent's advertised `url` (for example, to
    /// keep stable tool names when the agent is reached through a tunnel or
    /// reverse proxy with a different host).
    pub fn with_namespace(client: HttpClient, agent_card: AgentCard, namespace: String) -> Self {
        Self::from_backend(Arc::new(HttpBackend { client }), agent_card, namespace)
    }

    /// Create a new bridge that calls an in-process A2A handler directly.
    ///
    /// Use this when the bridge and the A2A agent live in the same process —
    /// it avoids spawning a loopback HTTP server and threads the call straight
    /// through [`AsyncMessageHandler::process_message`]. The namespace defaults
    /// to [`AgentCard::url`]; override with [`Self::with_handler_and_namespace`].
    ///
    /// # Arguments
    ///
    /// * `handler` - The A2A message handler to dispatch tool calls to
    /// * `agent_card` - The agent's capabilities card (its `url` is used for namespacing)
    pub fn with_handler<H>(handler: H, agent_card: AgentCard) -> Self
    where
        H: AsyncMessageHandler + Send + Sync + 'static,
    {
        let namespace = agent_card.url().to_string();
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
        let namespace = agent_card.url().to_string();
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
        let tools: Vec<Tool> = agent_card
            .skills
            .iter()
            .map(|skill| SkillToolConverter::skill_to_tool(skill, &namespace))
            .collect();

        info!(
            "Created AgentToMcpBridge for agent '{}' with {} tools",
            agent_card.name,
            tools.len()
        );

        Self {
            backend,
            agent_card: Arc::new(agent_card),
            tools: Arc::new(tools),
            namespace,
            tasks_cache: Arc::new(Mutex::new(HashMap::new())),
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

    fn create_artifact_uri(&self, task_id: &str, artifact_id: &str) -> String {
        let sanitized_ns = self
            .namespace
            .replace("https://", "")
            .replace("http://", "")
            .replace(['/', ':', '.'], "_");
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

impl AgentToMcpBridge {
    fn map_task_state(state: &buffa::enumeration::EnumValue<a2a_rs::domain::TaskState>) -> rmcp::model::TaskStatus {
        match state {
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Submitted) |
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Working) => {
                rmcp::model::TaskStatus::Working
            }
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::InputRequired) => rmcp::model::TaskStatus::InputRequired,
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Completed) => rmcp::model::TaskStatus::Completed,
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Failed) |
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Rejected) => {
                rmcp::model::TaskStatus::Failed
            }
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Canceled) => rmcp::model::TaskStatus::Cancelled,
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::AuthRequired) => rmcp::model::TaskStatus::InputRequired,
            _ => rmcp::model::TaskStatus::Working,
        }
    }

    fn convert_to_mcp_task(task: &a2a_rs::domain::Task) -> rmcp::model::Task {
        let status = Self::map_task_state(&task.status.state);
        let updated_at_dt = task
            .status
            .timestamp_utc()
            .unwrap_or_else(chrono::Utc::now);
        let updated_at = updated_at_dt.to_rfc3339();
        
        let mut mcp_task = rmcp::model::Task::new(
            task.id.clone(),
            status,
            updated_at.clone(),
            updated_at,
        );
        
        if let Some(msg) = task.status.message.as_option() {
            let text_parts: Vec<String> = msg
                .parts
                .iter()
                .filter_map(|part| part.get_text().map(String::from))
                .collect();
            if !text_parts.is_empty() {
                mcp_task = mcp_task.with_status_message(text_parts.join("\n"));
            }
        }
        
        mcp_task
    }

    /// Helper to call an A2A agent skill with support for streaming, progress notifications, and elicitation.
    async fn call_skill(
        &self,
        skill_id: &str,
        task_id: &str,
        message_text: &str,
        progress_token: Option<ProgressToken>,
        ctx: &RequestContext<RoleServer>,
    ) -> Result<CallToolResult> {
        debug!(
            "Calling A2A skill '{}' with message: {}",
            skill_id, message_text
        );

        // Create an A2A message
        let message = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text(message_text.to_string())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        // Dispatch to the configured backend (HTTP or in-process).
        let mut task = self
            .backend
            .invoke(task_id, &message, Some(skill_id))
            .await
            .map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;

        debug!("A2A agent returned task: {}", task.id);
        self.tasks_cache
            .lock()
            .await
            .insert(task.id.clone(), task.clone());

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
                        a2a_rs::services::StreamItem::Task(t) => {
                            debug!("Stream initial task for {}: {:?}", t.id, t.status.state);
                            task = t;
                            self.tasks_cache
                                .lock()
                                .await
                                .insert(task.id.clone(), task.clone());

                            // Send progress notification if token is provided
                            if let Some(ref token) = progress_token {
                                let progress_val = match task.status.state {
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Submitted) => 10.0,
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Working) => 50.0,
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::InputRequired) => 75.0,
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Completed) => 100.0,
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Failed)
                                    | buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Rejected
                                    | a2a_rs::domain::TaskState::Canceled) => 100.0,
                                    _ => 30.0,
                                };
                                last_progress = last_progress.max(progress_val);

                                let message_str = task.status.message.as_option().map(|msg| {
                                    msg.parts
                                        .iter()
                                        .filter_map(|part: &Part| part.get_text().map(String::from))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                });

                                let progress_param = ProgressNotificationParam {
                                    progress_token: token.clone(),
                                    progress: last_progress,
                                    total: Some(100.0),
                                    message: message_str,
                                };
                                let _ = ctx.peer.notify_progress(progress_param).await;
                            }

                            // Handle InputRequired
                            if task.status.state == a2a_rs::domain::TaskState::InputRequired {
                                debug!("Task {} requires input. Requesting sampling...", task.id);
                                let messages = translate_task_to_sampling_messages(&task);

                                let sampling_params = CreateMessageRequestParams::new(messages, 1024)
                                    .with_system_prompt("You are an assistant providing input to an agent task. Respond directly to the agent's request.");

                                let sampling_res_result = ctx
                                    .peer
                                    .create_message(sampling_params)
                                    .await;

                                let sampling_res = match sampling_res_result {
                                    Ok(res) => res,
                                    Err(e) => {
                                        debug!("Sampling failed or unavailable: {e}. Suspending task {} and returning to LLM.", task.id);
                                        break;
                                    }
                                };

                                let response_text = match sampling_res.message.content {
                                    SamplingContent::Single(SamplingMessageContent::Text(raw)) => {
                                        raw.text
                                    }
                                    SamplingContent::Multiple(items) => items
                                        .into_iter()
                                        .filter_map(|item| match item {
                                            SamplingMessageContent::Text(raw) => Some(raw.text),
                                            _ => None,
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                    _ => String::new(),
                                };

                                debug!(
                                    "Sampling response received: {}. Resuming task...",
                                    response_text
                                );

                                let reply_msg = Message::builder()
                                    .role(Role::User)
                                    .parts(vec![Part::text(response_text)])
                                    .message_id(uuid::Uuid::new_v4().to_string())
                                    .build();

                                task = self
                                    .backend
                                    .invoke(task_id, &reply_msg, Some(skill_id))
                                    .await
                                    .map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;
                                self.tasks_cache
                                    .lock()
                                    .await
                                    .insert(task.id.clone(), task.clone());
                            }

                            if TaskResultConverter::is_task_final(&task) {
                                break;
                            }
                        }
                        a2a_rs::services::StreamItem::StatusUpdate(event) => {
                            debug!(
                                "Stream status update for {}: {:?}",
                                task.id, event.status.state
                            );

                            // Send progress notification if token is provided
                            if let Some(ref token) = progress_token {
                                let progress_val = match event.status.state {
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Submitted) => 10.0,
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Working) => 50.0,
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::InputRequired) => 75.0,
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Completed) => 100.0,
                                    buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Failed)
                                    | buffa::enumeration::EnumValue::Known(a2a_rs::domain::TaskState::Rejected
                                    | a2a_rs::domain::TaskState::Canceled) => 100.0,
                                    _ => 30.0,
                                };
                                last_progress = last_progress.max(progress_val);

                                let message_str = event.status.message.as_option().map(|msg| {
                                     msg.parts
                                         .iter()
                                         .filter_map(|part: &Part| part.get_text().map(String::from))
                                         .collect::<Vec<_>>()
                                         .join("\n")
                                 });

                                 let progress_param = ProgressNotificationParam {
                                     progress_token: token.clone(),
                                     progress: last_progress,
                                     total: Some(100.0),
                                     message: message_str,
                                 };
                                 let _ = ctx.peer.notify_progress(progress_param).await;
                             }

                             task.status = ::buffa::MessageField::some(event.status.clone());
                            self.tasks_cache
                                .lock()
                                .await
                                .insert(task.id.clone(), task.clone());

                            // Handle InputRequired
                            if task.status.state == a2a_rs::domain::TaskState::InputRequired {
                                debug!("Task {} requires input. Requesting sampling...", task.id);
                                let messages = translate_task_to_sampling_messages(&task);

                                let sampling_params = CreateMessageRequestParams::new(messages, 1024)
                                    .with_system_prompt("You are an assistant providing input to an agent task. Respond directly to the agent's request.");

                                let sampling_res_result = ctx
                                    .peer
                                    .create_message(sampling_params)
                                    .await;

                                let sampling_res = match sampling_res_result {
                                    Ok(res) => res,
                                    Err(e) => {
                                        debug!("Sampling failed or unavailable: {e}. Suspending task {} and returning to LLM.", task.id);
                                        break;
                                    }
                                };

                                let response_text = match sampling_res.message.content {
                                    SamplingContent::Single(SamplingMessageContent::Text(raw)) => {
                                        raw.text
                                    }
                                    SamplingContent::Multiple(items) => items
                                        .into_iter()
                                        .filter_map(|item| match item {
                                            SamplingMessageContent::Text(raw) => Some(raw.text),
                                            _ => None,
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                    _ => String::new(),
                                };

                                debug!(
                                    "Sampling response received: {}. Resuming task...",
                                    response_text
                                );

                                let reply_msg = Message::builder()
                                    .role(Role::User)
                                    .parts(vec![Part::text(response_text)])
                                    .message_id(uuid::Uuid::new_v4().to_string())
                                    .build();

                                task = self
                                    .backend
                                    .invoke(task_id, &reply_msg, Some(skill_id))
                                    .await
                                    .map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;
                                self.tasks_cache
                                    .lock()
                                    .await
                                    .insert(task.id.clone(), task.clone());
                            }

                            if TaskResultConverter::is_task_final(&task) {
                                break;
                            }
                        }
                        a2a_rs::services::StreamItem::ArtifactUpdate(event) => {
                            debug!(
                                "Stream artifact update for {}: {}",
                                task.id, event.artifact.artifact_id
                            );
                            if event.append.unwrap_or(false) {
                                if let Some(existing) = task.artifacts
                                    .iter_mut()
                                    .find(|a| a.artifact_id == event.artifact.artifact_id)
                                {
                                    existing.parts.extend(event.artifact.parts.clone());
                                } else {
                                    task.artifacts.push(event.artifact);
                                }
                            } else if let Some(pos) = task.artifacts
                                .iter()
                                .position(|a| a.artifact_id == event.artifact.artifact_id)
                            {
                                task.artifacts[pos] = event.artifact;
                            } else {
                                task.artifacts.push(event.artifact);
                            }
                            self.tasks_cache
                                .lock()
                                .await
                                .insert(task.id.clone(), task.clone());
                        }
                    }
                }
            } else {
                debug!(
                    "Streaming not supported, falling back to polling for task: {}",
                    task.id
                );
                let mut last_state = task.status.state.clone();
                let mut poll_count = 0;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    poll_count += 1;

                    if let Some(ref token) = progress_token {
                        let progress_param = ProgressNotificationParam {
                            progress_token: token.clone(),
                            progress: (poll_count as f64 * 5.0).min(95.0),
                            total: Some(100.0),
                            message: Some(format!("Polling task status (attempt {})", poll_count)),
                        };
                        let _ = ctx.peer.notify_progress(progress_param).await;
                    }

                    if let Ok(Some(updated_task)) = self.backend.get_task(&task.id).await {
                        task = updated_task;
                        self.tasks_cache
                            .lock()
                            .await
                            .insert(task.id.clone(), task.clone());

                        if task.status.state != last_state {
                            debug!(
                                "Polled task {} state changed to: {:?}",
                                task.id, task.status.state
                            );
                            last_state = task.status.state.clone();
                        }

                        if task.status.state == a2a_rs::domain::TaskState::InputRequired {
                            debug!(
                                "Polled task {} requires input. Requesting sampling...",
                                task.id
                            );
                            let messages = translate_task_to_sampling_messages(&task);

                            let sampling_params = CreateMessageRequestParams::new(messages, 1024)
                                .with_system_prompt("You are an assistant providing input to an agent task. Respond directly to the agent's request.");

                            let sampling_res_result = ctx
                                .peer
                                .create_message(sampling_params)
                                .await;

                            let sampling_res = match sampling_res_result {
                                Ok(res) => res,
                                Err(e) => {
                                    debug!("Sampling failed or unavailable: {e}. Suspending task {} and returning to LLM.", task.id);
                                    break;
                                }
                            };

                            let response_text = match sampling_res.message.content {
                                SamplingContent::Single(SamplingMessageContent::Text(raw)) => {
                                    raw.text
                                }
                                SamplingContent::Multiple(items) => items
                                    .into_iter()
                                    .filter_map(|item| match item {
                                        SamplingMessageContent::Text(raw) => Some(raw.text),
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                _ => String::new(),
                            };

                            debug!(
                                "Sampling response received (polling): {}. Resuming task...",
                                response_text
                            );

                            let reply_msg = Message::builder()
                                .role(Role::User)
                                .parts(vec![Part::text(response_text)])
                                .message_id(uuid::Uuid::new_v4().to_string())
                                .build();

                            task = self
                                .backend
                                .invoke(task_id, &reply_msg, Some(skill_id))
                                .await
                                .map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;
                            self.tasks_cache
                                .lock()
                                .await
                                .insert(task.id.clone(), task.clone());
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

        self.tasks_cache
            .lock()
            .await
            .insert(task.id.clone(), task.clone());

        // Defuse the cancel guard as the task has successfully completed/finished in this request
        cancel_guard.task_id = None;

        // Convert task to MCP result
        let result = TaskResultConverter::task_to_result(&task)?;

        info!(
            "A2A skill '{}' completed with state: {:?}",
            skill_id, task.status.state
        );

        Ok(result)
    }
}

/// Helper function to translate A2A Task history to MCP sampling messages
fn translate_task_to_sampling_messages(task: &Task) -> Vec<SamplingMessage> {
    let mut messages = Vec::new();

    for msg in &task.history {
        let role = match msg.role {
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::Role::ROLE_USER) => rmcp::model::Role::User,
            buffa::enumeration::EnumValue::Known(a2a_rs::domain::Role::ROLE_AGENT) => rmcp::model::Role::Assistant,
            _ => rmcp::model::Role::User,
        };

        let text_parts: Vec<String> = msg
            .parts
            .iter()
            .filter_map(|part| part.get_text().map(String::from))
            .collect();

        if !text_parts.is_empty() {
            let content = SamplingMessageContent::text(text_parts.join("\n"));
            messages.push(SamplingMessage::new(role, content));
        }
    }

    if let Some(msg) = task.status.message.as_option() {
        let already_in_history = task
            .history
            .last()
            .map(|last_msg| last_msg.message_id == msg.message_id)
            .unwrap_or(false);

        if !already_in_history {
            let role = match msg.role {
                buffa::enumeration::EnumValue::Known(a2a_rs::domain::Role::ROLE_USER) => rmcp::model::Role::User,
                buffa::enumeration::EnumValue::Known(a2a_rs::domain::Role::ROLE_AGENT) => rmcp::model::Role::Assistant,
                _ => rmcp::model::Role::User,
            };

            let text_parts: Vec<String> = msg
                .parts
                .iter()
                .filter_map(|part| part.get_text().map(String::from))
                .collect();

            if !text_parts.is_empty() {
                let content = SamplingMessageContent::text(text_parts.join("\n"));
                messages.push(SamplingMessage::new(role, content));
            }
        }
    }

    messages
}

#[async_trait]
#[allow(clippy::manual_async_fn)]
impl ServerHandler for AgentToMcpBridge {
    fn get_info(&self) -> ServerInfo {
        let server_name = self.mcp_server_name.as_deref().unwrap_or(&self.agent_card.name);
        // Fall back to agent_card.version, then to "0.1.0"
        let server_version = self.mcp_server_version.as_deref()
            .unwrap_or(if self.agent_card.version.is_empty() { "0.1.0" } else { &self.agent_card.version });

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
            if let Some(a2a_rs::domain::generated::security_scheme::Scheme::Oauth2SecurityScheme(ref oauth2_scheme)) = &scheme.scheme {
                if let Some(ref flows) = oauth2_scheme.flows.as_option() {
                    if let Some(a2a_rs::domain::generated::o_auth_flows::Flow::ClientCredentials(ref cc)) = &flows.flow {
                        let mut cc_settings = serde_json::Map::new();
                        cc_settings.insert("tokenUrl".to_string(), serde_json::Value::String(cc.token_url.clone()));
                        if !oauth2_scheme.oauth2_metadata_url.is_empty() {
                            cc_settings.insert("metadataUrl".to_string(), serde_json::Value::String(oauth2_scheme.oauth2_metadata_url.clone()));
                        }
                        extensions.insert(
                            "io.modelcontextprotocol/oauth-client-credentials".to_string(),
                            cc_settings,
                        );
                    }
                }
            }
        }

        let caps = if !extensions.is_empty() {
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .enable_extensions_with(extensions)
                .build()
        } else {
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build()
        };

        ServerInfo::new(caps)
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
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

            Ok(ListToolsResult {
                tools: (*self.tools).clone(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn call_tool(
        &self,
        params: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<CallToolResult, McpError>> + Send + '_
    {
        async move {
            let name = &params.name;
            info!("MCP client calling tool: {}", name);

            // Parse the tool name to extract skill ID
            let (_agent_id, skill_id) = match SkillToolConverter::parse_tool_name(name) {
                Ok(result) => result,
                Err(e) => return Err(e.to_mcp_error()),
            };

            // Verify the skill exists
            if !self.agent_card.skills.iter().any(|s| s.id == skill_id) {
                error!("Skill not found: {}", skill_id);
                return Err(McpError::internal_error(
                    format!("Skill '{}' not found", skill_id),
                    None,
                ));
            }

        let message_text = params
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

        let task_id = params
            .arguments
            .as_ref()
            .and_then(|args| args.get("task_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let progress_token = ctx.meta.get_progress_token();

        // Call the A2A agent skill
        match self
            .call_skill(&skill_id, &task_id, &message_text, progress_token, &ctx)
            .await
        {
            Ok(result) => Ok(result),
            Err(e) => Err(e.to_mcp_error()),
        }
        }
    }

    fn initialize(
        &self,
        _request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<InitializeResult, McpError>> + Send + '_
    {
        async move {
            info!(
                "MCP client initializing with agent '{}'",
                self.agent_card.name
            );
            Ok(self.get_info())
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

            Ok(ListPromptsResult {
                prompts,
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<GetPromptResult, McpError>> + Send + '_
    {
        async move {
            let name = &request.name;
            info!("MCP client getting prompt: {}", name);

            // Parse the prompt name to extract skill ID
            let (_agent_id, skill_id) = match SkillToolConverter::parse_tool_name(name) {
                Ok(result) => result,
                Err(e) => return Err(e.to_mcp_error()),
            };

            // Verify the skill exists
            if !self.agent_card.skills.iter().any(|s| s.id == skill_id) {
                error!("Skill not found: {}", skill_id);
                return Err(McpError::internal_error(
                    format!("Skill '{}' not found", skill_id),
                    None,
                ));
            }

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
            let progress_token = ctx.meta.get_progress_token();
            let tool_result = match self
                .call_skill(&skill_id, &task_id, &message_text, progress_token, &ctx)
                .await
            {
                Ok(result) => result,
                Err(e) => return Err(e.to_mcp_error()),
            };

            // Convert tool result content to prompt messages
            let mut prompt_messages = vec![PromptMessage::new(
                PromptMessageRole::User,
                PromptMessageContent::text(message_text),
            )];

            for c in tool_result.content {
                let raw = c.raw;
                let annotations = c.annotations;
                let msg_content = match raw {
                    RawContent::Text(t) => PromptMessageContent::Text { text: t.text },
                    RawContent::Image(img) => PromptMessageContent::Image {
                        image: Annotated::new(img, annotations),
                    },
                    RawContent::Resource(r) => PromptMessageContent::Resource {
                        resource: Annotated::new(r, annotations),
                    },
                    RawContent::ResourceLink(link) => PromptMessageContent::ResourceLink {
                        link: Annotated::new(link, annotations),
                    },
                    RawContent::Audio(_) => PromptMessageContent::Text {
                        text: "[Audio content]".to_string(),
                    },
                };
                prompt_messages.push(PromptMessage::new(
                    PromptMessageRole::Assistant,
                    msg_content,
                ));
            }

            Ok(GetPromptResult::new(prompt_messages))
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
                            Some(a2a_rs::domain::generated::part::Content::Url(_)) |
                            Some(a2a_rs::domain::generated::part::Content::Raw(_)) => {
                                if p.media_type.is_empty() { None } else { Some(p.media_type.clone()) }
                            }
                            Some(a2a_rs::domain::generated::part::Content::Data(_)) => {
                                Some("application/json".to_string())
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| "text/plain".to_string());

                    let raw = RawResource::new(uri, name)
                        .with_title(artifact.name.clone())
                        .with_description(artifact.description.clone())
                        .with_mime_type(mime_type);

                    resources.push(Annotated::new(raw, None));
                }
            }

            Ok(ListResourcesResult {
                resources,
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<ReadResourceResult, McpError>> + Send + '_
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
            let artifact = task.artifacts
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
                        contents.push(ResourceContents::TextResourceContents {
                            uri: request.uri.clone(),
                            mime_type: Some("text/plain".to_string()),
                            text,
                            meta: None,
                        });
                    }
                    Some(a2a_rs::domain::generated::part::Content::Raw(bytes)) => {
                        contents.push(ResourceContents::BlobResourceContents {
                            uri: request.uri.clone(),
                            mime_type: Some(if part.media_type.is_empty() { "application/octet-stream".to_string() } else { part.media_type.clone() }),
                            blob: {
                                use base64::Engine as _;
                                base64::engine::general_purpose::STANDARD.encode(&bytes)
                            },
                            meta: None,
                        });
                    }
                    Some(a2a_rs::domain::generated::part::Content::Url(uri)) => {
                        contents.push(ResourceContents::TextResourceContents {
                            uri: request.uri.clone(),
                            mime_type: Some("text/plain".to_string()),
                            text: format!("File URI: {}", uri),
                            meta: None,
                        });
                    }
                    Some(a2a_rs::domain::generated::part::Content::Data(data)) => {
                        let data_json = match serde_json::to_string_pretty(&data) {
                            Ok(json) => json,
                            Err(e) => {
                                return Err(McpError::internal_error(
                                    format!("Failed to serialize data: {}", e),
                                    None,
                                ))
                            }
                        };
                        contents.push(ResourceContents::TextResourceContents {
                            uri: request.uri.clone(),
                            mime_type: Some("application/json".to_string()),
                            text: data_json,
                            meta: None,
                        });
                    }
                    None => {
                        contents.push(ResourceContents::TextResourceContents {
                            uri: request.uri.clone(),
                            mime_type: Some("text/plain".to_string()),
                            text: format!("File: {}", part.filename),
                            meta: None,
                        });
                    }
                }
            }

            Ok(ReadResourceResult::new(contents))
        }
    }

    fn get_task_info(
        &self,
        request: GetTaskInfoParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<GetTaskResult, McpError>> + Send + '_
    {
        async move {
            info!("MCP client getting task info: {}", request.task_id);
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

            let mcp_task = Self::convert_to_mcp_task(&a2a_task);
            Ok(GetTaskResult {
                meta: None,
                task: mcp_task,
            })
        }
    }

    fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<CancelTaskResult, McpError>> + Send + '_
    {
        async move {
            info!("MCP client canceling task: {}", request.task_id);
            let a2a_task = self.backend.cancel_task(&request.task_id).await.map_err(|e| {
                McpError::internal_error(
                    format!("Failed to cancel A2A task {}: {}", request.task_id, e),
                    None,
                )
            })?;

            let mcp_task = Self::convert_to_mcp_task(&a2a_task);
            Ok(CancelTaskResult {
                meta: None,
                task: mcp_task,
            })
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
    fn test_bridge_uses_card_url_for_namespacing() {
        // new() should derive the tool namespace from agent_card.url with no
        // need for a caller to pass it separately.
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

        let tool_name = bridge.tools[0].name.as_ref();
        // Sanitizer turns dots/colons/slashes into underscores; hyphens stay.
        assert!(
            tool_name.contains("card-url_example_com"),
            "tool name {tool_name} should be namespaced by the card url"
        );
        assert!(tool_name.ends_with("do_thing"));
    }

    #[test]
    fn test_with_namespace_overrides_card_url() {
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

        assert!(bridge.tools[0].name.contains("internal-alias"));
    }

}
