//! Bridge that provides MCP tools as capabilities to A2A agents

use crate::{
    converters::{MessageConverter, llm_tool::LlmToolConverter},
    error::{A2aMcpError, Result},
};
use a2a_llm::{ToolCall, ToolDefinition, ToolResult};
use a2a_rs::{
    domain::{AgentExtension, AgentSkill, Message, Part, Role, Task, TaskState, TaskStatus},
    port::{AsyncMessageHandler, RequestContext},
};
use async_trait::async_trait;
use rmcp::{
    Peer, RoleClient,
    handler::client::progress::ProgressDispatcher,
    model::*,
    service::{NotificationContext, PeerRequestOptions},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use crate::converters::{SkillSchemas, SkillToolConverter};

/// Metadata key used to mark an A2A [`Message`] as an MCP tool-call request.
///
/// When [`McpToA2ABridge`] receives a message whose `metadata` map contains
/// this key, it deserialises the value as an [`McpToolCall`] envelope and
/// invokes the named tool on the underlying MCP server instead of delegating
/// to the inner A2A handler.
pub const MCP_TOOL_CALL_METADATA_KEY: &str = "a2a_rs_tool_call";

/// Envelope describing an MCP tool invocation carried inside an A2A [`Message`].
///
/// Place a serialised [`McpToolCall`] in `Message.metadata` under the
/// [`MCP_TOOL_CALL_METADATA_KEY`] key. The message's `parts` are ignored by
/// the bridge for routing — they remain free for any display/logging payload.
///
/// Use [`create_tool_call_message`] to build a properly-shaped [`Message`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolCall {
    /// MCP tool name (as advertised by the MCP server's `tools/list`).
    pub name: String,
    /// JSON arguments forwarded to the MCP tool. Must be an object (or null).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub arguments: Value,
}

/// Metadata key used to mark an A2A [`Message`] as an MCP prompt-call request.
pub const MCP_PROMPT_CALL_METADATA_KEY: &str = "a2a_rs_prompt_call";

/// Envelope describing an MCP prompt invocation carried inside an A2A [`Message`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpPromptCall {
    /// MCP prompt name (as advertised by the MCP server's `prompts/list`).
    pub name: String,
    /// JSON arguments forwarded to the MCP prompt. Must be an object (or null).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub arguments: Value,
}

/// Metadata key used to mark an A2A [`Message`] as an MCP resource read.
pub const MCP_RESOURCE_READ_METADATA_KEY: &str = "a2a_rs_resource_read";

/// Envelope describing an MCP resource read carried inside an A2A [`Message`].
///
/// The third envelope beside tools and prompts: a data server's catalogue is
/// a resource, read once and held, so an A2A caller needs a way to ask for
/// one that is shaped like the other two.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpResourceRead {
    /// The resource URI, as advertised by `resources/list` or matching one of
    /// the server's templates.
    pub uri: String,
}

/// A request drop guard to trigger request cancellation on drop.
struct RequestCancelGuard {
    peer: Arc<Peer<RoleClient>>,
    request_id: RequestId,
    completed: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for RequestCancelGuard {
    fn drop(&mut self) {
        if !self.completed.load(std::sync::atomic::Ordering::SeqCst) {
            let peer = self.peer.clone();
            let request_id = self.request_id.clone();
            tokio::spawn(async move {
                debug!(
                    "RequestCancelGuard triggered: notifying server of cancellation for request: {:?}",
                    request_id
                );
                let _ = peer
                    .notify_cancelled(CancelledNotificationParam::new(
                        Some(request_id),
                        Some("Task canceled or handler dropped".to_string()),
                    ))
                    .await;
            });
        }
    }
}

/// Client handler that dispatches progress notifications to a `ProgressDispatcher`.
#[derive(Clone, Default)]
pub struct ProgressClientHandler {
    dispatcher: ProgressDispatcher,
}

impl ProgressClientHandler {
    /// Create a new progress client handler
    pub fn new(dispatcher: ProgressDispatcher) -> Self {
        Self { dispatcher }
    }
}

impl rmcp::ClientHandler for ProgressClientHandler {
    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.dispatcher.handle_notification(params).await;
    }
}

/// Bridge that provides MCP tools as additional capabilities to A2A agents
///
/// This allows A2A agents to call MCP tools by sending specially formatted messages.
/// Tool requests are detected in incoming messages and routed to the MCP server.
///
/// # Tool-call wire format
///
/// To invoke an MCP tool, the A2A message must carry an [`McpToolCall`]
/// envelope in [`Message::metadata`] under [`MCP_TOOL_CALL_METADATA_KEY`]:
///
/// ```text
/// Message {
///     role: "user",
///     parts: [],                              // optional, free for display
///     metadata: {
///         "a2a_rs_tool_call": {
///             "name": "calculator_add",
///             "arguments": { "a": 5, "b": 3 }
///         }
///     },
///     ...
/// }
/// ```
///
/// Messages without this metadata key are forwarded unchanged to the inner
/// [`AsyncMessageHandler`].
#[derive(Clone)]
pub struct McpToA2ABridge<H: AsyncMessageHandler> {
    /// The MCP client peer for calling tools and prompts
    mcp_peer: Arc<Peer<RoleClient>>,
    /// Available MCP tools. Re-listed on `notifications/tools/list_changed`.
    tools: Arc<RwLock<Vec<Tool>>>,
    /// Available MCP prompts. Re-listed on `notifications/prompts/list_changed`.
    prompts: Arc<RwLock<Vec<Prompt>>>,
    /// Available MCP resources. Re-listed on `notifications/resources/list_changed`.
    resources: Arc<RwLock<Vec<Resource>>>,
    /// URIs the server has said changed since they were last taken, so a
    /// consumer re-reads the one URI rather than everything.
    updated_resources: Arc<std::sync::Mutex<BTreeSet<String>>>,
    /// The underlying A2A message handler to delegate non-tool messages
    inner_handler: Arc<H>,
    /// Progress dispatcher to route progress updates
    progress_dispatcher: ProgressDispatcher,
    /// Optional streaming handler for status update broadcasting
    streaming_handler: Option<Arc<dyn a2a_rs::port::AsyncStreamingHandler>>,
    /// Tool calls the server paused for input, by A2A task id, until the
    /// next message on that task answers them.
    paused: Arc<std::sync::Mutex<HashMap<String, PausedCall>>>,
}

/// A tool call the server answered with `input_required`: what it asked, and
/// what to send back with the answer so the server can resume.
///
/// The answer is a later `message/send` on the same task, so the call is
/// not held open across it. rmcp's own `call_tool` drives these rounds
/// through the client handler in one request; the bridge cannot, because
/// the party being asked is on the A2A side and answers on its own clock.
///
/// Only a session at MCP 2026-07-28 or newer can carry this: rmcp refuses
/// to send an `input_required` result on an older one. `initialize` tops
/// out at 2025-11-25, so a consumer that opens its peer with `serve()`
/// never sees a pause; the discover lifecycle
/// (`serve_client_with_lifecycle`) is what reaches 2026-07-28.
#[derive(Debug, Clone)]
struct PausedCall {
    tool: String,
    arguments: Value,
    /// How the answer reaches the server.
    resume: Resume,
    /// What the server wants answered, by key.
    input_requests: InputRequests,
}

/// Where a paused call's answer goes: back into `tools/call`, or to the
/// task the server made of the call.
#[derive(Debug, Clone)]
enum Resume {
    /// `tools/call` again with the answer, and the server's state echoed
    /// unchanged; it is opaque here.
    Retry { request_state: Option<String> },
    /// `tasks/update` with the answer, then watch the task again.
    Task {
        mcp_task_id: String,
        poll_interval_ms: Option<u64>,
    },
}

/// What a tool call came to, once the bridge has driven it as far as it can
/// without the A2A side.
#[derive(Debug)]
enum ToolOutcome {
    Complete(CallToolResult),
    InputRequired {
        input_requests: InputRequests,
        resume: Resume,
    },
    /// The server cancelled the task it made of the call.
    Cancelled,
}

/// Cancels the MCP task the bridge was watching if the watcher is dropped
/// before the task settles: the A2A side went away, so nobody will read
/// the result.
struct McpTaskCancelGuard {
    peer: Arc<Peer<RoleClient>>,
    mcp_task_id: String,
    settled: bool,
}

impl Drop for McpTaskCancelGuard {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let peer = self.peer.clone();
        let id = self.mcp_task_id.clone();
        tokio::spawn(async move {
            debug!("watcher dropped; cancelling MCP task {id}");
            let _ = peer.cancel_task(CancelTaskParams::new(id)).await;
        });
    }
}

impl<H: AsyncMessageHandler + Clone + Send + Sync + 'static> McpToA2ABridge<H> {
    /// Create a new MCP → A2A bridge
    ///
    /// # Arguments
    ///
    /// * `mcp_peer` - MCP client peer for calling tools
    /// * `inner_handler` - Underlying A2A handler for non-tool messages
    pub async fn new(mcp_peer: Peer<RoleClient>, inner_handler: H) -> Result<Self> {
        let listed = Listed::from_peer(&mcp_peer).await?;
        info!(
            "McpToA2ABridge initialized with {} MCP tools, {} MCP prompts and {} MCP resources",
            listed.tools.len(),
            listed.prompts.len(),
            listed.resources.len()
        );
        Ok(Self::assemble(
            mcp_peer,
            listed,
            inner_handler,
            ProgressDispatcher::new(),
            None,
        ))
    }

    /// Create a new MCP → A2A bridge with streaming progress support
    pub async fn with_streaming(
        mcp_peer: Peer<RoleClient>,
        inner_handler: H,
        progress_dispatcher: ProgressDispatcher,
        streaming_handler: Arc<dyn a2a_rs::port::AsyncStreamingHandler>,
    ) -> Result<Self> {
        let listed = Listed::from_peer(&mcp_peer).await?;
        info!(
            "McpToA2ABridge (streaming) initialized with {} MCP tools, {} MCP prompts and {} MCP resources",
            listed.tools.len(),
            listed.prompts.len(),
            listed.resources.len()
        );
        Ok(Self::assemble(
            mcp_peer,
            listed,
            inner_handler,
            progress_dispatcher,
            Some(streaming_handler),
        ))
    }

    fn assemble(
        mcp_peer: Peer<RoleClient>,
        listed: Listed,
        inner_handler: H,
        progress_dispatcher: ProgressDispatcher,
        streaming_handler: Option<Arc<dyn a2a_rs::port::AsyncStreamingHandler>>,
    ) -> Self {
        Self {
            mcp_peer: Arc::new(mcp_peer),
            tools: Arc::new(RwLock::new(listed.tools)),
            prompts: Arc::new(RwLock::new(listed.prompts)),
            resources: Arc::new(RwLock::new(listed.resources)),
            updated_resources: Arc::new(std::sync::Mutex::new(BTreeSet::new())),
            inner_handler: Arc::new(inner_handler),
            progress_dispatcher,
            streaming_handler,
            paused: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Whether `task_id` is waiting on an answer to a server's question.
    pub fn is_awaiting_input(&self, task_id: &str) -> bool {
        self.paused.lock().unwrap().contains_key(task_id)
    }

    /// Get the available MCP tools.
    pub async fn tools(&self) -> Vec<Tool> {
        self.tools.read().await.clone()
    }

    /// Get the available MCP prompts.
    pub async fn prompts(&self) -> Vec<Prompt> {
        self.prompts.read().await.clone()
    }

    /// Get the available MCP resources — the third list beside tools and
    /// prompts. Empty when the server does not serve resources.
    pub async fn resources(&self) -> Vec<Resource> {
        self.resources.read().await.clone()
    }

    /// Ask the server to send `notifications/resources/updated` for `uri`.
    /// Only meaningful when the server declared `resources.subscribe`; the
    /// URIs that arrive are collected for [`take_updated_resources`].
    ///
    /// [`take_updated_resources`]: Self::take_updated_resources
    pub async fn subscribe_resource(&self, uri: &str) -> Result<()> {
        // `resources/subscribe` is the legacy shape; a peer opened with
        // `serve()` negotiates a legacy version, which is what this bridge's
        // consumers do today. `subscriptions/listen` is the 2026-07-28 shape
        // and wants the peer opened with a discover lifecycle first.
        #[allow(deprecated)]
        self.mcp_peer
            .subscribe(SubscribeRequestParams::new(uri.to_string()))
            .await
            .map_err(|e| A2aMcpError::McpServer(format!("Subscribe failed: {e:?}")))
    }

    /// The URIs the server has reported changed since this was last called.
    /// A consumer holding a resource as durable context re-reads these and
    /// nothing else.
    pub fn take_updated_resources(&self) -> Vec<String> {
        let mut updated = self
            .updated_resources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *updated).into_iter().collect()
    }

    /// Read one resource. A URI not in the list is still sent — templates
    /// exist — and the server's refusal comes back as an error.
    pub async fn read_resource(&self, uri: &str) -> Result<Vec<ResourceContents>> {
        debug!("Reading MCP resource: {}", uri);
        let handle = self
            .mcp_peer
            .send_request_with_option(
                ClientRequest::ReadResourceRequest(ReadResourceRequest::new(
                    ReadResourceRequestParams::new(uri.to_string()),
                )),
                PeerRequestOptions::no_options(),
            )
            .await
            .map_err(|e| A2aMcpError::McpServer(format!("Resource read failed to send: {e:?}")))?;

        let request_id = handle.id.clone();
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _guard = RequestCancelGuard {
            peer: self.mcp_peer.clone(),
            request_id,
            completed: completed.clone(),
        };

        let response = handle.await_response().await;
        completed.store(true, std::sync::atomic::Ordering::SeqCst);

        match response {
            Ok(ServerResult::ReadResourceResult(r)) => Ok(r.contents),
            Ok(_) => Err(A2aMcpError::McpServer(
                "Unexpected response from MCP server".to_string(),
            )),
            Err(e) => Err(e.into()),
        }
    }

    /// Read one resource as an A2A message carrying its contents — a text
    /// resource's text, a blob resource's bytes — with `role`.
    pub async fn read_resource_message(&self, uri: &str, role: Role) -> Result<Message> {
        let contents = self.read_resource(uri).await?;
        let mut parts: Vec<Part> = contents
            .iter()
            .map(MessageConverter::resource_contents_to_part)
            .collect();
        if parts.is_empty() {
            parts.push(Part::text(String::new()));
        }
        Ok(Message::builder()
            .role(role)
            .parts(parts)
            .message_id(uuid::Uuid::new_v4().to_string())
            .build())
    }

    /// The server's tools as A2A skills, with the schema extension that
    /// carries their types. A consumer builds a card from these, and
    /// [`AgentToMcpBridge`](crate::AgentToMcpBridge) serves that card as the
    /// same typed tools — the round trip.
    pub async fn agent_skills(&self) -> (Vec<AgentSkill>, AgentExtension) {
        let mut schemas = SkillSchemas::new();
        let skills = self
            .tools
            .read()
            .await
            .iter()
            .map(|tool| {
                let (skill, schema) = SkillToolConverter::tool_to_skill(tool);
                if let Some(schema) = schema {
                    schemas.insert(skill.id.clone(), schema);
                }
                skill
            })
            .collect();
        (skills, schemas.to_extension())
    }

    /// Get the available tools converted into LLM ToolDefinition objects.
    pub async fn get_llm_tools(&self) -> Vec<ToolDefinition> {
        LlmToolConverter::mcp_to_llm_tools(&self.tools.read().await)
    }

    /// Natively execute an LLM `ToolCall` directly against the MCP client.
    ///
    /// This bypasses the A2A Message serialization format and is intended for
    /// agents interacting directly with `LlmProvider` results. The text is
    /// what the model is told; `structuredContent`, when the server sent it,
    /// is kept beside it rather than flattened.
    pub async fn execute_llm_tool_call(
        &self,
        task_id: &str,
        tool_call: &ToolCall,
    ) -> Result<ToolResult> {
        // Convert to MCP parameters
        let params = LlmToolConverter::llm_tool_call_to_mcp_request(tool_call)?;

        let args = if let Some(a) = params.arguments {
            serde_json::Value::Object(a)
        } else {
            serde_json::Value::Null
        };

        // A model's tool call has no task to park a question on: a server
        // that asks one gets no answer here, and the model is told so.
        match self
            .call_mcp_tool(task_id, &params.name, args, None, None)
            .await?
        {
            ToolOutcome::Complete(result) => Ok(LlmToolConverter::mcp_result_to_llm(&result)),
            ToolOutcome::InputRequired { input_requests, .. } => {
                Err(A2aMcpError::McpServer(format!(
                    "tool '{}' asked for input, which a model's tool call cannot answer: {}",
                    params.name,
                    render_questions(&input_requests)
                )))
            }
            ToolOutcome::Cancelled => Err(A2aMcpError::McpServer(format!(
                "tool '{}' was cancelled by the server",
                params.name
            ))),
        }
    }

    /// Extract a typed tool-call envelope from a message, if present.
    ///
    /// Returns `Some(call)` only when the message carries a well-formed
    /// [`McpToolCall`] at [`MCP_TOOL_CALL_METADATA_KEY`]. A malformed value
    /// is treated as "not a tool call" so the inner handler still gets a
    /// chance at it; this matches the previous string-prefix behaviour.
    fn extract_tool_call(message: &Message) -> Option<McpToolCall> {
        let metadata_struct = message.metadata.as_option()?;
        let metadata_val = serde_json::to_value(metadata_struct).ok()?;
        let raw = metadata_val.get(MCP_TOOL_CALL_METADATA_KEY)?;
        match serde_json::from_value::<McpToolCall>(raw.clone()) {
            Ok(call) => Some(call),
            Err(e) => {
                debug!(
                    "Message has '{}' metadata but it failed to deserialise as McpToolCall: {}",
                    MCP_TOOL_CALL_METADATA_KEY, e
                );
                None
            }
        }
    }

    /// Extract a typed prompt-call envelope from a message, if present.
    fn extract_prompt_call(message: &Message) -> Option<McpPromptCall> {
        let metadata_struct = message.metadata.as_option()?;
        let metadata_val = serde_json::to_value(metadata_struct).ok()?;
        let raw = metadata_val.get(MCP_PROMPT_CALL_METADATA_KEY)?;
        match serde_json::from_value::<McpPromptCall>(raw.clone()) {
            Ok(call) => Some(call),
            Err(e) => {
                debug!(
                    "Message has '{}' metadata but it failed to deserialise as McpPromptCall: {}",
                    MCP_PROMPT_CALL_METADATA_KEY, e
                );
                None
            }
        }
    }

    /// Extract a typed resource-read envelope from a message, if present.
    fn extract_resource_read(message: &Message) -> Option<McpResourceRead> {
        let metadata_struct = message.metadata.as_option()?;
        let metadata_val = serde_json::to_value(metadata_struct).ok()?;
        let raw = metadata_val.get(MCP_RESOURCE_READ_METADATA_KEY)?;
        match serde_json::from_value::<McpResourceRead>(raw.clone()) {
            Ok(read) => Some(read),
            Err(e) => {
                debug!(
                    "Message has '{}' metadata but it failed to deserialise as McpResourceRead: {}",
                    MCP_RESOURCE_READ_METADATA_KEY, e
                );
                None
            }
        }
    }

    /// Call an MCP tool
    /// One `tools/call`, driven until the server either completes it or asks
    /// something only the A2A side can answer.
    ///
    /// A `request_state` with no `input_requests` is the server saying "not
    /// yet, ask again with this": the bridge retries with backoff, as rmcp's
    /// own driver does, up to `DEFAULT_MRTR_MAX_ROUNDS`. Anything asked
    /// comes back as `InputRequired` for the caller to turn into a task.
    async fn call_mcp_tool(
        &self,
        task_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
        input_responses: Option<InputResponses>,
        request_state: Option<String>,
    ) -> Result<ToolOutcome> {
        debug!("Calling MCP tool: {} with args: {}", tool_name, arguments);

        // Verify tool exists
        if !self.tools.read().await.iter().any(|t| t.name == tool_name) {
            return Err(A2aMcpError::ToolNotFound(tool_name.to_string()));
        }

        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let serde_json::Value::Object(map) = arguments {
            params = params.with_arguments(map);
        }
        params.input_responses = input_responses;
        params.request_state = request_state;

        for round in 0..DEFAULT_MRTR_MAX_ROUNDS {
            let result = match self.call_mcp_tool_once(task_id, params.clone()).await? {
                CallToolResponse::Complete(result) => return Ok(ToolOutcome::Complete(result)),
                CallToolResponse::InputRequired(result) => result,
                // The server made a task of the call; it is watched from
                // here, and its outcome is the call's.
                CallToolResponse::Task(created) => {
                    info!(
                        "MCP tool '{tool_name}' answered with task {}; watching it",
                        created.task.task_id
                    );
                    return self
                        .watch_task(
                            task_id,
                            created.task.task_id.clone(),
                            created.task.poll_interval_ms,
                        )
                        .await;
                }
                _ => {
                    return Err(A2aMcpError::McpServer(format!(
                        "tool '{tool_name}' answered with a response kind this bridge does not know"
                    )));
                }
            };
            let requests = result.input_requests.unwrap_or_default();
            if requests.is_empty() && result.request_state.is_none() {
                return Err(A2aMcpError::McpServer(
                    "the server said input is required and named neither a request nor a state"
                        .to_string(),
                ));
            }
            if !requests.is_empty() {
                return Ok(ToolOutcome::InputRequired {
                    input_requests: requests,
                    resume: Resume::Retry {
                        request_state: result.request_state,
                    },
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(50 << round.min(6))).await;
            params.input_responses = None;
            params.request_state = result.request_state;
        }
        Err(A2aMcpError::McpServer(format!(
            "tool '{tool_name}' asked for input {DEFAULT_MRTR_MAX_ROUNDS} times without completing"
        )))
    }

    /// One round of `tools/call`, with the server's progress relayed while it
    /// runs.
    async fn call_mcp_tool_once(
        &self,
        task_id: &str,
        params: CallToolRequestParams,
    ) -> Result<CallToolResponse> {
        let tool_name = params.name.clone();
        let handle = self
            .mcp_peer
            .send_request_with_option(
                ClientRequest::CallToolRequest(CallToolRequest::new(params)),
                PeerRequestOptions::no_options(),
            )
            .await
            .map_err(|e| A2aMcpError::McpServer(format!("Tool call failed to send: {:?}", e)))?;

        // Subscribe to the progress token generated by rmcp if streaming is enabled
        let progress_rx_task = if let Some(ref sh) = self.streaming_handler {
            let actual_token = handle.progress_token.clone();
            let mut subscriber = self.progress_dispatcher.subscribe(actual_token).await;
            let sh = sh.clone();
            let t_id = task_id.to_string();

            let rx_task = tokio::spawn(async move {
                use futures::StreamExt;
                while let Some(notification) = subscriber.next().await {
                    let msg_text = if let Some(total) = notification.total {
                        format!("Progress: {}/{}", notification.progress, total)
                    } else {
                        format!("Progress: {}", notification.progress)
                    };

                    let progress_message = Message::builder()
                        .role(Role::Agent)
                        .parts(vec![Part::text(msg_text)])
                        .message_id(uuid::Uuid::new_v4().to_string())
                        .build();

                    let update = a2a_rs::domain::TaskStatusUpdateEvent {
                        task_id: t_id.clone(),
                        context_id: uuid::Uuid::new_v4().to_string(),
                        kind: "status-update".to_string(),
                        status: TaskStatus::new(TaskState::Working, Some(progress_message)),
                        metadata: None,
                    };

                    if let Err(e) = sh.broadcast_status_update(&t_id, update).await {
                        error!("Failed to broadcast progress status update: {:?}", e);
                    }
                }
            });
            Some(rx_task)
        } else {
            None
        };

        let request_id = handle.id.clone();
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _guard = RequestCancelGuard {
            peer: self.mcp_peer.clone(),
            request_id,
            completed: completed.clone(),
        };

        // Await the response
        let response = handle.await_response().await;

        // Mark as completed so the guard doesn't send cancel on drop
        completed.store(true, std::sync::atomic::Ordering::SeqCst);

        // Abort the progress receiver task if it's still running
        if let Some(rx_task) = progress_rx_task {
            // Yield to allow any pending progress notifications in the channel to be processed
            tokio::task::yield_now().await;
            rx_task.abort();
        }

        match response {
            Ok(ServerResult::CallToolResult(result)) => {
                info!("MCP tool '{tool_name}' returned result");
                Ok(CallToolResponse::Complete(result))
            }
            Ok(ServerResult::InputRequiredResult(result)) => {
                info!("MCP tool '{tool_name}' asked for input");
                Ok(CallToolResponse::InputRequired(result))
            }
            Ok(ServerResult::CreateTaskResult(created)) => Ok(CallToolResponse::Task(created)),
            Ok(_) => Err(A2aMcpError::McpServer(
                "Unexpected response from MCP server".to_string(),
            )),
            Err(e) => Err(e.into()),
        }
    }

    /// Watch the MCP task the server made of a tool call until it settles or
    /// asks for input, and say what it came to. `tasks/get` is polled at the
    /// interval the server named; each status message the server sets is
    /// relayed as a `Working` status update on the A2A task. A watcher
    /// dropped before the task settles cancels it.
    ///
    /// The server's `notifications/tasks` are not consumed: the bridge is
    /// seldom the session's client handler, and polling is how rmcp observes
    /// a task too.
    async fn watch_task(
        &self,
        task_id: &str,
        mcp_task_id: String,
        poll_interval_ms: Option<u64>,
    ) -> Result<ToolOutcome> {
        let interval = std::time::Duration::from_millis(
            poll_interval_ms.unwrap_or(rmcp::task_manager::DEFAULT_POLL_INTERVAL_MS),
        );
        let mut guard = McpTaskCancelGuard {
            peer: self.mcp_peer.clone(),
            mcp_task_id: mcp_task_id.clone(),
            settled: false,
        };
        let mut last_status_message: Option<String> = None;
        loop {
            let detailed = self
                .mcp_peer
                .get_task(GetTaskParams::new(mcp_task_id.clone()))
                .await
                .map_err(|e| {
                    A2aMcpError::McpServer(format!("tasks/get for {mcp_task_id} failed: {e}"))
                })?
                .task;
            if detailed.task.status_message != last_status_message {
                if let Some(text) = &detailed.task.status_message {
                    self.broadcast_working(task_id, text.clone()).await;
                }
                last_status_message = detailed.task.status_message.clone();
            }
            match detailed.payload {
                TaskPayload::Working => {}
                TaskPayload::InputRequired { input_requests } => {
                    // The task is paused, not abandoned: the answer comes
                    // through `tasks/update` on the next A2A message.
                    guard.settled = true;
                    return Ok(ToolOutcome::InputRequired {
                        input_requests,
                        resume: Resume::Task {
                            mcp_task_id,
                            poll_interval_ms,
                        },
                    });
                }
                TaskPayload::Completed { result } => {
                    guard.settled = true;
                    let result: CallToolResult =
                        serde_json::from_value(Value::Object(result)).map_err(|e| {
                            A2aMcpError::McpServer(format!(
                                "task {mcp_task_id} completed with a result that is not a tool result: {e}"
                            ))
                        })?;
                    info!("MCP task {mcp_task_id} completed");
                    return Ok(ToolOutcome::Complete(result));
                }
                TaskPayload::Failed { error } => {
                    guard.settled = true;
                    let message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("task failed")
                        .to_string();
                    return Err(A2aMcpError::McpServer(format!(
                        "task {mcp_task_id} failed: {message}"
                    )));
                }
                TaskPayload::Cancelled => {
                    guard.settled = true;
                    return Ok(ToolOutcome::Cancelled);
                }
                _ => {
                    return Err(A2aMcpError::McpServer(format!(
                        "task {mcp_task_id} is in a state this bridge does not know"
                    )));
                }
            }
            tokio::time::sleep(interval).await;
        }
    }

    /// Relay one line from the server as a `Working` status update on the
    /// A2A task, when there is a streaming handler to relay it through.
    async fn broadcast_working(&self, task_id: &str, text: String) {
        let Some(sh) = &self.streaming_handler else {
            return;
        };
        let message = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(text)])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();
        let update = a2a_rs::domain::TaskStatusUpdateEvent {
            task_id: task_id.to_string(),
            context_id: uuid::Uuid::new_v4().to_string(),
            kind: "status-update".to_string(),
            status: TaskStatus::new(TaskState::Working, Some(message)),
            metadata: None,
        };
        if let Err(e) = sh.broadcast_status_update(task_id, update).await {
            error!("Failed to broadcast status update: {:?}", e);
        }
    }

    /// The task for a tool call the server completed: the text as the
    /// agent's reply, each embedded or linked resource as an artifact,
    /// `isError` as `Failed`.
    fn completed_task(task_id: &str, message: &Message, result: &CallToolResult) -> Task {
        let task_state = if result.is_error.unwrap_or(false) {
            TaskState::Failed
        } else {
            TaskState::Completed
        };

        let message_text = MessageConverter::extract_text_from_content(&result.content);

        let agent_message = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(message_text)])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        let mut artifacts = Vec::new();
        for content_item in &result.content {
            match content_item {
                ContentBlock::Resource(res) => {
                    // The artifact carries what the resource holds, named by
                    // its URI. It used to be a reference to the URI, which
                    // dropped the bytes the server had just sent.
                    let name = match &res.resource {
                        ResourceContents::TextResourceContents { uri, .. }
                        | ResourceContents::BlobResourceContents { uri, .. } => uri.clone(),
                        _ => continue,
                    };
                    let part = MessageConverter::resource_contents_to_part(&res.resource);
                    artifacts.push(a2a_rs::domain::Artifact {
                        artifact_id: uuid::Uuid::new_v4().to_string(),
                        name,
                        description: String::new(),
                        parts: vec![part],
                        metadata: ::buffa::MessageField::none(),
                        extensions: Vec::new(),
                        ..Default::default()
                    });
                }
                ContentBlock::ResourceLink(link) => {
                    let part = Part::file_from_uri(
                        link.uri.clone(),
                        Some(link.name.clone()),
                        link.mime_type.clone(),
                    );
                    artifacts.push(a2a_rs::domain::Artifact {
                        artifact_id: uuid::Uuid::new_v4().to_string(),
                        name: link.name.clone(),
                        description: String::new(),
                        parts: vec![part],
                        metadata: ::buffa::MessageField::none(),
                        extensions: Vec::new(),
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }

        let task_builder = Task::builder()
            .id(task_id.to_string())
            .context_id(uuid::Uuid::new_v4().to_string())
            .status(TaskStatus::new(task_state, None))
            .history(vec![message.clone(), agent_message]);

        if !artifacts.is_empty() {
            task_builder.artifacts(artifacts).build()
        } else {
            task_builder.build()
        }
    }

    /// The task for a tool call the server paused: `InputRequired`, with the
    /// server's question as the status message. The next message on the
    /// task is the answer.
    fn paused_task(task_id: &str, message: &Message, paused: &PausedCall) -> Task {
        let question = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(render_questions(&paused.input_requests))])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();
        Task::builder()
            .id(task_id.to_string())
            .context_id(uuid::Uuid::new_v4().to_string())
            .status(TaskStatus::new(
                TaskState::InputRequired,
                Some(question.clone()),
            ))
            .history(vec![message.clone(), question])
            .build()
    }

    /// A tool call's outcome as the task for it, holding the call when the
    /// server paused it.
    fn task_for(
        &self,
        task_id: &str,
        message: &Message,
        tool: &str,
        arguments: &Value,
        outcome: ToolOutcome,
    ) -> Task {
        match outcome {
            ToolOutcome::Complete(result) => Self::completed_task(task_id, message, &result),
            ToolOutcome::InputRequired {
                input_requests,
                resume,
            } => {
                let paused = PausedCall {
                    tool: tool.to_string(),
                    arguments: arguments.clone(),
                    resume,
                    input_requests,
                };
                let task = Self::paused_task(task_id, message, &paused);
                self.paused
                    .lock()
                    .unwrap()
                    .insert(task_id.to_string(), paused);
                task
            }
            ToolOutcome::Cancelled => {
                let note = Message::builder()
                    .role(Role::Agent)
                    .parts(vec![Part::text(format!(
                        "tool '{tool}' was cancelled by the server"
                    ))])
                    .message_id(uuid::Uuid::new_v4().to_string())
                    .build();
                Task::builder()
                    .id(task_id.to_string())
                    .context_id(uuid::Uuid::new_v4().to_string())
                    .status(TaskStatus::new(TaskState::Canceled, Some(note.clone())))
                    .history(vec![message.clone(), note])
                    .build()
            }
        }
    }

    /// Answer a paused call with what the A2A side said, and carry on: a
    /// call the server paused in `tools/call` is retried with the answer,
    /// one it made a task of gets the answer through `tasks/update` and is
    /// watched again.
    async fn resume_call(
        &self,
        task_id: &str,
        paused: &PausedCall,
        responses: InputResponses,
    ) -> Result<ToolOutcome> {
        match &paused.resume {
            Resume::Retry { request_state } => {
                self.call_mcp_tool(
                    task_id,
                    &paused.tool,
                    paused.arguments.clone(),
                    (!responses.is_empty()).then_some(responses),
                    request_state.clone(),
                )
                .await
            }
            Resume::Task {
                mcp_task_id,
                poll_interval_ms,
            } => {
                self.mcp_peer
                    .update_task(UpdateTaskParams::new(mcp_task_id.clone(), responses))
                    .await
                    .map_err(|e| {
                        A2aMcpError::McpServer(format!(
                            "tasks/update for {mcp_task_id} failed: {e}"
                        ))
                    })?;
                self.watch_task(task_id, mcp_task_id.clone(), *poll_interval_ms)
                    .await
            }
        }
    }

    /// Call an MCP prompt
    async fn call_mcp_prompt(
        &self,
        prompt_name: &str,
        arguments: serde_json::Value,
    ) -> Result<GetPromptResult> {
        debug!(
            "Calling MCP prompt: {} with args: {}",
            prompt_name, arguments
        );

        // Verify prompt exists
        if !self
            .prompts
            .read()
            .await
            .iter()
            .any(|p| p.name == prompt_name)
        {
            return Err(A2aMcpError::PromptNotFound(prompt_name.to_string()));
        }

        // Call the MCP prompt via the peer
        let mut params = GetPromptRequestParams::new(prompt_name.to_string());
        if let serde_json::Value::Object(map) = arguments {
            params = params.with_arguments(map);
        }

        let handle = self
            .mcp_peer
            .send_request_with_option(
                ClientRequest::GetPromptRequest(GetPromptRequest::new(params)),
                PeerRequestOptions::no_options(),
            )
            .await
            .map_err(|e| A2aMcpError::McpServer(format!("Prompt call failed to send: {:?}", e)))?;

        let request_id = handle.id.clone();
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _guard = RequestCancelGuard {
            peer: self.mcp_peer.clone(),
            request_id,
            completed: completed.clone(),
        };

        // Await the response
        let response = handle.await_response().await;

        // Mark as completed so the guard doesn't send cancel on drop
        completed.store(true, std::sync::atomic::Ordering::SeqCst);

        let result = match response {
            Ok(ServerResult::GetPromptResult(r)) => r,
            Ok(_) => {
                return Err(A2aMcpError::McpServer(
                    "Unexpected response from MCP server".to_string(),
                ));
            }
            Err(e) => return Err(e.into()),
        };

        info!("MCP prompt '{}' returned result", prompt_name);

        Ok(result)
    }
}

impl<H: AsyncMessageHandler + Clone + Send + Sync + 'static> rmcp::ClientHandler
    for McpToA2ABridge<H>
{
    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.progress_dispatcher.handle_notification(params).await;
    }

    /// The server said a resource changed: remember which, for whoever holds
    /// it as context to re-read.
    async fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        debug!("MCP server reports resource updated: {}", params.uri);
        self.updated_resources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(params.uri);
    }

    /// The three lists are re-read when the server says they changed. Until
    /// this, the bridge served the list it read at startup for as long as it
    /// ran, and a tool added mid-conversation was `ToolNotFound` here while
    /// the server offered it.
    async fn on_resource_list_changed(&self, _context: NotificationContext<RoleClient>) {
        match self.mcp_peer.list_all_resources().await {
            Ok(resources) => *self.resources.write().await = resources,
            Err(e) => debug!("Re-listing resources after list_changed failed: {e:?}"),
        }
    }

    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        match self.mcp_peer.list_all_tools().await {
            Ok(tools) => *self.tools.write().await = tools,
            Err(e) => debug!("Re-listing tools after list_changed failed: {e:?}"),
        }
    }

    async fn on_prompt_list_changed(&self, _context: NotificationContext<RoleClient>) {
        match self.mcp_peer.list_all_prompts().await {
            Ok(prompts) => *self.prompts.write().await = prompts,
            Err(e) => debug!("Re-listing prompts after list_changed failed: {e:?}"),
        }
    }
}

/// What a server lists at `initialize`: read once for both constructors.
struct Listed {
    tools: Vec<Tool>,
    prompts: Vec<Prompt>,
    resources: Vec<Resource>,
}

impl Listed {
    /// Tools are required — a server with none is not one this bridge is for.
    /// Prompts and resources are optional capabilities, and a server that
    /// refuses to list them (or has not declared them) simply has none.
    async fn from_peer(peer: &Peer<RoleClient>) -> Result<Self> {
        let tools = peer
            .list_all_tools()
            .await
            .map_err(|e| A2aMcpError::McpServer(format!("Failed to list tools: {:?}", e)))?;
        let prompts = match peer.list_all_prompts().await {
            Ok(p) => p,
            Err(e) => {
                debug!("Failed to list prompts: {:?}", e);
                Vec::new()
            }
        };
        let resources = match peer.list_all_resources().await {
            Ok(r) => r,
            Err(e) => {
                debug!("Failed to list resources: {:?}", e);
                Vec::new()
            }
        };
        Ok(Self {
            tools,
            prompts,
            resources,
        })
    }
}

#[async_trait]
impl<H: AsyncMessageHandler + Clone + Send + Sync + 'static> AsyncMessageHandler
    for McpToA2ABridge<H>
{
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        ctx: &RequestContext,
    ) -> std::result::Result<Task, a2a_rs::domain::error::A2AError> {
        // Whatever this task was waiting on. A new tool call on the task
        // supersedes the question; any other message answers it.
        let paused = self.paused.lock().unwrap().remove(task_id);
        if let Some(McpToolCall {
            name: tool_name,
            arguments,
        }) = Self::extract_tool_call(message)
        {
            info!("Detected MCP tool call request for tool: {}", tool_name);
            let outcome = self
                .call_mcp_tool(task_id, &tool_name, arguments.clone(), None, None)
                .await
                .map_err(|e| {
                    error!("MCP tool call failed: {}", e);
                    e.to_a2a_error()
                })?;
            Ok(self.task_for(task_id, message, &tool_name, &arguments, outcome))
        } else if let Some(paused) = paused {
            // The answer to a question the server asked on this task.
            info!(
                "Answering MCP tool '{}' on task {task_id} with the message received",
                paused.tool
            );
            let responses = answers_for(&paused.input_requests, message);
            let outcome = self
                .resume_call(task_id, &paused, responses)
                .await
                .map_err(|e| {
                    error!("MCP tool call failed after its input was answered: {}", e);
                    e.to_a2a_error()
                })?;
            Ok(self.task_for(task_id, message, &paused.tool, &paused.arguments, outcome))
        } else if let Some(McpPromptCall {
            name: prompt_name,
            arguments,
        }) = Self::extract_prompt_call(message)
        {
            info!(
                "Detected MCP prompt call request for prompt: {}",
                prompt_name
            );

            // Call the MCP prompt
            match self.call_mcp_prompt(&prompt_name, arguments).await {
                Ok(result) => {
                    // Map the returned `PromptMessage`s to A2A messages
                    let mut history = vec![message.clone()];
                    for pm in &result.messages {
                        history.push(prompt_message_to_a2a_message(pm));
                    }

                    Ok(Task::builder()
                        .id(task_id.to_string())
                        .context_id(uuid::Uuid::new_v4().to_string())
                        .status(TaskStatus::new(TaskState::Completed, None))
                        .history(history)
                        .build())
                }
                Err(e) => {
                    error!("MCP prompt call failed: {}", e);
                    Err(e.to_a2a_error())
                }
            }
        } else if let Some(McpResourceRead { uri }) = Self::extract_resource_read(message) {
            info!("Detected MCP resource read request for: {}", uri);
            match self.read_resource_message(&uri, Role::Agent).await {
                Ok(contents) => Ok(Task::builder()
                    .id(task_id.to_string())
                    .context_id(uuid::Uuid::new_v4().to_string())
                    .status(TaskStatus::new(TaskState::Completed, None))
                    .history(vec![message.clone(), contents])
                    .build()),
                Err(e) => {
                    error!("MCP resource read failed: {}", e);
                    Err(e.to_a2a_error())
                }
            }
        } else {
            // Not a tool, prompt or resource request; delegate to inner handler
            debug!("Message is not an MCP request, delegating to inner handler");
            self.inner_handler
                .process_message(task_id, message, ctx)
                .await
        }
    }
}

/// The server's questions as text a person or a model can answer: each
/// elicitation's message, then what it wants filled in and how. Sampling has
/// no question to show; it is named so the reader knows why the task
/// cannot proceed.
fn render_questions(requests: &InputRequests) -> String {
    let mut lines = Vec::new();
    for request in requests.values() {
        match request {
            InputRequest::Elicitation(elicit) => match &elicit.params {
                ElicitRequestParams::FormElicitationParams {
                    message,
                    requested_schema,
                    ..
                } => {
                    lines.push(message.clone());
                    let required = requested_schema.required.clone().unwrap_or_default();
                    for (name, definition) in &requested_schema.properties {
                        let shape = serde_json::to_value(definition).unwrap_or(Value::Null);
                        let kind = shape
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("string")
                            .to_string();
                        let choices = shape.get("enum").and_then(Value::as_array).map(|values| {
                            values
                                .iter()
                                .map(|v| v.as_str().map(str::to_string).unwrap_or(v.to_string()))
                                .collect::<Vec<_>>()
                                .join(" | ")
                        });
                        let mut line = format!("- `{name}` ({kind}");
                        if let Some(choices) = choices {
                            line.push_str(&format!(": {choices}"));
                        }
                        line.push(')');
                        if required.iter().any(|r| r == name) {
                            line.push_str(", required");
                        }
                        lines.push(line);
                    }
                }
                ElicitRequestParams::UrlElicitationParams { message, url, .. } => {
                    lines.push(format!("{message}\nOpen {url}, then answer to continue."));
                }
                _ => lines.push(
                    "The server asked a question in a form this bridge cannot show.".to_string(),
                ),
            },
            InputRequest::CreateMessage(_) => lines.push(
                "The server asked for a model completion (sampling), which nobody here answers."
                    .to_string(),
            ),
            _ => lines.push("The server asked for something this bridge cannot show.".to_string()),
        }
    }
    if lines.is_empty() {
        "The server needs more input to continue.".to_string()
    } else {
        lines.join("\n")
    }
}

/// The message's answer as the server's `inputResponses`, one per request.
///
/// A data part is the answer as given: an object the form schema describes.
/// Text fills the form's one property, or its first required one, coerced
/// to the property's type; `decline` or `cancel` alone is that action. A
/// URL elicitation is accepted by any answer: the person has been told
/// where to go. Anything else, sampling included, gets a decline, since
/// nobody here answers it, and the server decides what that means.
fn answers_for(requests: &InputRequests, message: &Message) -> InputResponses {
    use a2a_rs::domain::generated::part;
    let text = message
        .parts
        .iter()
        .filter_map(|part| part.get_text())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let data: Option<Map<String, Value>> =
        message.parts.iter().find_map(|part| match &part.content {
            Some(part::Content::Data(value)) => serde_json::to_value(&**value)
                .ok()
                .and_then(|value| value.as_object().cloned()),
            _ => None,
        });
    let mut responses = InputResponses::new();
    for (key, request) in requests {
        let response = match request {
            InputRequest::Elicitation(elicit) => match &elicit.params {
                ElicitRequestParams::FormElicitationParams {
                    requested_schema, ..
                } => match action_of(&text) {
                    Some(action) => ElicitResult::new(action),
                    None => {
                        let content = match &data {
                            Some(object) => Value::Object(object.clone()),
                            None => fill_form(requested_schema, &text),
                        };
                        ElicitResult::new(ElicitationAction::Accept).with_content(content)
                    }
                },
                _ => ElicitResult::new(action_of(&text).unwrap_or(ElicitationAction::Accept)),
            },
            _ => ElicitResult::new(ElicitationAction::Decline),
        };
        responses.insert(
            key.clone(),
            serde_json::to_value(response).unwrap_or(Value::Null),
        );
    }
    responses
}

/// `decline` or `cancel` on its own, as the action it names.
fn action_of(text: &str) -> Option<ElicitationAction> {
    match text.to_ascii_lowercase().as_str() {
        "decline" => Some(ElicitationAction::Decline),
        "cancel" => Some(ElicitationAction::Cancel),
        _ => None,
    }
}

/// `text` as the form's content: under its one property, else its first
/// required property, else its first property, coerced to that property's
/// declared type. A form with nothing to fill accepts an empty object.
fn fill_form(schema: &ElicitationSchema, text: &str) -> Value {
    let required = schema.required.clone().unwrap_or_default();
    let target = if schema.properties.len() == 1 {
        schema.properties.keys().next()
    } else {
        required
            .iter()
            .find(|name| schema.properties.contains_key(*name))
            .or_else(|| schema.properties.keys().next())
    };
    let Some(name) = target else {
        return Value::Object(Map::new());
    };
    let shape = schema
        .properties
        .get(name)
        .and_then(|definition| serde_json::to_value(definition).ok())
        .unwrap_or(Value::Null);
    let value = match shape.get("type").and_then(Value::as_str) {
        Some("boolean") => Value::Bool(matches!(
            text.to_ascii_lowercase().as_str(),
            "true" | "yes" | "y" | "1" | "ok" | "confirm"
        )),
        Some("integer") => text
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(text.to_string())),
        Some("number") => text
            .parse::<f64>()
            .ok()
            .and_then(|n| serde_json::Number::from_f64(n).map(Value::Number))
            .unwrap_or_else(|| Value::String(text.to_string())),
        _ => Value::String(text.to_string()),
    };
    let mut object = Map::new();
    object.insert(name.clone(), value);
    Value::Object(object)
}

/// Helper to map `PromptMessage` to A2A `Message`.
fn prompt_message_to_a2a_message(pm: &PromptMessage) -> Message {
    let role = match pm.role {
        rmcp::model::Role::User => Role::User,
        rmcp::model::Role::Assistant => Role::Agent,
    };

    let mut parts = Vec::new();
    if let Ok(Some(part)) = MessageConverter::content_block_to_part(&pm.content) {
        parts.push(part);
    }

    if parts.is_empty() {
        parts.push(Part::text(String::new()));
    }

    Message::builder()
        .role(role)
        .parts(parts)
        .message_id(uuid::Uuid::new_v4().to_string())
        .build()
}

/// Build an A2A [`Message`] that carries an MCP tool-call envelope.
///
/// Produces a `User`-role message with no `parts` and a single metadata
/// entry under [`MCP_TOOL_CALL_METADATA_KEY`] holding `{name, arguments}`.
/// This is what [`McpToA2ABridge`] expects on the wire.
///
/// The returned message has a fresh UUIDv4 `message_id`. If you already
/// have a message and just want to attach a tool-call envelope, use
/// [`attach_tool_call`] instead.
pub fn create_tool_call_message(tool_name: impl Into<String>, arguments: Value) -> Message {
    let envelope = McpToolCall {
        name: tool_name.into(),
        arguments,
    };
    let mut map = Map::new();
    map.insert(
        MCP_TOOL_CALL_METADATA_KEY.to_string(),
        serde_json::to_value(&envelope).expect("McpToolCall always serialises"),
    );
    let metadata =
        serde_json::from_value::<::buffa_types::google::protobuf::Struct>(Value::Object(map))
            .expect("valid Struct");

    Message::builder()
        .role(Role::User)
        .metadata(metadata)
        .message_id(uuid::Uuid::new_v4().to_string())
        .build()
}

/// Attach an MCP tool-call envelope to an existing [`Message`] in place.
///
/// Overwrites any prior value at [`MCP_TOOL_CALL_METADATA_KEY`] and leaves
/// every other field (including `parts`) untouched.
pub fn attach_tool_call(message: &mut Message, tool_name: impl Into<String>, arguments: Value) {
    let envelope = McpToolCall {
        name: tool_name.into(),
        arguments,
    };
    let metadata_struct = message.metadata.get_or_insert_default();

    let mut map = serde_json::to_value(&*metadata_struct)
        .ok()
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();

    map.insert(
        MCP_TOOL_CALL_METADATA_KEY.to_string(),
        serde_json::to_value(&envelope).expect("McpToolCall always serialises"),
    );

    if let Ok(new_struct) =
        serde_json::from_value::<::buffa_types::google::protobuf::Struct>(Value::Object(map))
    {
        *metadata_struct = new_struct;
    }
}

/// Build an A2A [`Message`] that carries an MCP prompt-call envelope.
pub fn create_prompt_call_message(prompt_name: impl Into<String>, arguments: Value) -> Message {
    let envelope = McpPromptCall {
        name: prompt_name.into(),
        arguments,
    };
    let mut map = Map::new();
    map.insert(
        MCP_PROMPT_CALL_METADATA_KEY.to_string(),
        serde_json::to_value(&envelope).expect("McpPromptCall always serialises"),
    );
    let metadata =
        serde_json::from_value::<::buffa_types::google::protobuf::Struct>(Value::Object(map))
            .expect("valid Struct");

    Message::builder()
        .role(Role::User)
        .metadata(metadata)
        .message_id(uuid::Uuid::new_v4().to_string())
        .build()
}

/// Attach an MCP prompt-call envelope to an existing [`Message`] in place.
pub fn attach_prompt_call(message: &mut Message, prompt_name: impl Into<String>, arguments: Value) {
    let envelope = McpPromptCall {
        name: prompt_name.into(),
        arguments,
    };
    let metadata_struct = message.metadata.get_or_insert_default();

    let mut map = serde_json::to_value(&*metadata_struct)
        .ok()
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();

    map.insert(
        MCP_PROMPT_CALL_METADATA_KEY.to_string(),
        serde_json::to_value(&envelope).expect("McpPromptCall always serialises"),
    );

    if let Ok(new_struct) =
        serde_json::from_value::<::buffa_types::google::protobuf::Struct>(Value::Object(map))
    {
        *metadata_struct = new_struct;
    }
}

/// Build an A2A [`Message`] that carries an MCP resource-read envelope.
pub fn create_resource_read_message(uri: impl Into<String>) -> Message {
    let envelope = McpResourceRead { uri: uri.into() };
    let mut map = Map::new();
    map.insert(
        MCP_RESOURCE_READ_METADATA_KEY.to_string(),
        serde_json::to_value(&envelope).expect("McpResourceRead always serialises"),
    );
    let metadata =
        serde_json::from_value::<::buffa_types::google::protobuf::Struct>(Value::Object(map))
            .expect("valid Struct");
    Message::builder()
        .role(Role::User)
        .metadata(metadata)
        .message_id(uuid::Uuid::new_v4().to_string())
        .build()
}

/// Attach an MCP resource-read envelope to an existing [`Message`] in place.
pub fn attach_resource_read(message: &mut Message, uri: impl Into<String>) {
    let envelope = McpResourceRead { uri: uri.into() };
    let metadata_struct = message.metadata.get_or_insert_default();
    let mut map = serde_json::to_value(&*metadata_struct)
        .ok()
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();
    map.insert(
        MCP_RESOURCE_READ_METADATA_KEY.to_string(),
        serde_json::to_value(&envelope).expect("McpResourceRead always serialises"),
    );
    if let Ok(new_struct) =
        serde_json::from_value::<::buffa_types::google::protobuf::Struct>(Value::Object(map))
    {
        *metadata_struct = new_struct;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resource_read_envelope_is_detected_and_only_that() {
        let msg = create_resource_read_message("catalogue://views");
        let read = McpToA2ABridge::<NoOpHandler>::extract_resource_read(&msg)
            .expect("metadata envelope should be detected");
        assert_eq!(read.uri, "catalogue://views");
        assert!(McpToA2ABridge::<NoOpHandler>::extract_tool_call(&msg).is_none());
        assert!(McpToA2ABridge::<NoOpHandler>::extract_prompt_call(&msg).is_none());

        let mut with_parts = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text("read the catalogue".to_string())])
            .message_id("test".to_string())
            .build();
        attach_resource_read(&mut with_parts, "catalogue://views");
        assert_eq!(with_parts.parts.len(), 1);
        assert!(McpToA2ABridge::<NoOpHandler>::extract_resource_read(&with_parts).is_some());
    }

    #[test]
    fn test_extract_tool_call_detection() {
        let msg = create_tool_call_message("my_tool", serde_json::json!({"param": "value"}));

        let call = McpToA2ABridge::<NoOpHandler>::extract_tool_call(&msg)
            .expect("metadata envelope should be detected");
        assert_eq!(call.name, "my_tool");
        assert_eq!(call.arguments["param"], "value");
    }

    #[test]
    fn test_extract_tool_call_missing_metadata() {
        let normal_message = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text("Just a normal message".to_string())])
            .message_id("test".to_string())
            .build();

        assert!(McpToA2ABridge::<NoOpHandler>::extract_tool_call(&normal_message).is_none());
    }

    #[test]
    fn test_extract_tool_call_legacy_text_prefix_no_longer_routes() {
        // The pre-typed convention used a `TOOL_CALL: name` text part.
        // After the metadata refactor, such a message must NOT be treated
        // as a tool call — it should flow through to the inner handler.
        let legacy = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text("TOOL_CALL: my_tool".to_string())])
            .message_id("test".to_string())
            .build();

        assert!(McpToA2ABridge::<NoOpHandler>::extract_tool_call(&legacy).is_none());
    }

    #[test]
    fn test_extract_tool_call_malformed_metadata_falls_through() {
        // Wrong shape under the key — bridge should ignore it rather than fail
        // routing, so the inner handler still sees the message.
        let mut metadata = serde_json::Map::new();
        metadata.insert(
            MCP_TOOL_CALL_METADATA_KEY.to_string(),
            serde_json::json!("not an object"),
        );
        let metadata = serde_json::from_value::<::buffa_types::google::protobuf::Struct>(
            Value::Object(metadata),
        )
        .expect("valid Struct");
        let msg = Message::builder()
            .role(Role::User)
            .metadata(metadata)
            .message_id("test".to_string())
            .build();

        assert!(McpToA2ABridge::<NoOpHandler>::extract_tool_call(&msg).is_none());
    }

    #[test]
    fn test_create_tool_call_message_shape() {
        let msg = create_tool_call_message("test_tool", serde_json::json!({"x": 42}));
        assert_eq!(
            msg.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_USER)
        );
        // The envelope lives in metadata; parts is intentionally empty.
        assert!(msg.parts.is_empty());

        let metadata_struct = msg.metadata.as_option().expect("metadata present");
        let metadata_val = serde_json::to_value(metadata_struct).unwrap();
        let envelope = metadata_val
            .get(MCP_TOOL_CALL_METADATA_KEY)
            .expect("envelope present");
        assert_eq!(envelope["name"], "test_tool");
        assert_eq!(envelope["arguments"]["x"].as_f64(), Some(42.0));
    }

    #[test]
    fn test_attach_tool_call_preserves_parts() {
        let mut msg = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text("add 5 and 7".to_string())])
            .message_id("test".to_string())
            .build();

        attach_tool_call(&mut msg, "add", serde_json::json!({"a": 5, "b": 7}));

        assert_eq!(msg.parts.len(), 1, "display text part should be preserved");
        let metadata_struct = msg.metadata.as_option().expect("metadata present");
        let metadata_val = serde_json::to_value(metadata_struct).unwrap();
        let envelope = metadata_val
            .get(MCP_TOOL_CALL_METADATA_KEY)
            .expect("envelope present");
        assert_eq!(envelope["name"], "add");
    }

    #[test]
    fn test_extract_prompt_call_detection() {
        let msg = create_prompt_call_message("my_prompt", serde_json::json!({"param": "value"}));

        let call = McpToA2ABridge::<NoOpHandler>::extract_prompt_call(&msg)
            .expect("metadata envelope should be detected");
        assert_eq!(call.name, "my_prompt");
        assert_eq!(call.arguments["param"], "value");
    }

    #[test]
    fn test_extract_prompt_call_missing_metadata() {
        let normal_message = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text("Just a normal message".to_string())])
            .message_id("test".to_string())
            .build();

        assert!(McpToA2ABridge::<NoOpHandler>::extract_prompt_call(&normal_message).is_none());
    }

    #[test]
    fn test_create_prompt_call_message_shape() {
        let msg = create_prompt_call_message("test_prompt", serde_json::json!({"x": 42}));
        assert_eq!(
            msg.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_USER)
        );
        assert!(msg.parts.is_empty());

        let metadata_struct = msg.metadata.as_option().expect("metadata present");
        let metadata_val = serde_json::to_value(metadata_struct).unwrap();
        let envelope = metadata_val
            .get(MCP_PROMPT_CALL_METADATA_KEY)
            .expect("envelope present");
        assert_eq!(envelope["name"], "test_prompt");
        assert_eq!(envelope["arguments"]["x"].as_f64(), Some(42.0));
    }

    #[test]
    fn test_attach_prompt_call_preserves_parts() {
        let mut msg = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text("use prompt".to_string())])
            .message_id("test".to_string())
            .build();

        attach_prompt_call(&mut msg, "test_prompt", serde_json::json!({"a": 5}));

        assert_eq!(msg.parts.len(), 1, "display text part should be preserved");
        let metadata_struct = msg.metadata.as_option().expect("metadata present");
        let metadata_val = serde_json::to_value(metadata_struct).unwrap();
        let envelope = metadata_val
            .get(MCP_PROMPT_CALL_METADATA_KEY)
            .expect("envelope present");
        assert_eq!(envelope["name"], "test_prompt");
    }

    #[test]
    fn test_prompt_message_to_a2a_message_text() {
        let pm = PromptMessage::new_text(rmcp::model::Role::User, "Hello User");
        let msg = prompt_message_to_a2a_message(&pm);
        assert_eq!(
            msg.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_USER)
        );
        assert_eq!(msg.parts.len(), 1);
        use a2a_rs::domain::generated::part;
        if let Some(part::Content::Text(text)) = &msg.parts[0].content {
            assert_eq!(text, "Hello User");
        } else {
            panic!("Expected text part");
        }
    }

    #[test]
    fn test_prompt_message_to_a2a_message_image() {
        // An image is a file part holding the decoded bytes.
        let pm = PromptMessage::new(
            rmcp::model::Role::Assistant,
            ContentBlock::image("AQID", "image/png"),
        );
        let msg = prompt_message_to_a2a_message(&pm);
        assert_eq!(
            msg.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_AGENT)
        );
        assert_eq!(msg.parts.len(), 1);
        use a2a_rs::domain::generated::part;
        assert_eq!(
            msg.parts[0].content,
            Some(part::Content::Raw(vec![1, 2, 3]))
        );
        assert_eq!(msg.parts[0].media_type, "image/png");
    }

    #[test]
    fn test_prompt_message_to_a2a_message_resource() {
        let resource_contents = ResourceContents::text("Resource content", "file://test.txt")
            .with_mime_type("text/plain");
        let pm = PromptMessage::new(
            rmcp::model::Role::User,
            ContentBlock::resource(resource_contents),
        );
        let msg = prompt_message_to_a2a_message(&pm);
        assert_eq!(
            msg.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_USER)
        );
        assert_eq!(msg.parts.len(), 1);
        // An embedded resource in a prompt is carried as what it holds, the
        // same as one in a tool result — not as the address it came from.
        use a2a_rs::domain::generated::part;
        let part = &msg.parts[0];
        if let Some(part::Content::Text(text)) = &part.content {
            assert_eq!(text, "Resource content");
            assert_eq!(part.media_type, "text/plain");
        } else {
            panic!("Expected text part carrying the resource's text");
        }
    }

    #[test]
    fn test_prompt_message_to_a2a_message_resource_link() {
        let resource = Resource::new("http://example.com", "link").with_mime_type("text/html");
        let pm = PromptMessage::new(
            rmcp::model::Role::Assistant,
            ContentBlock::resource_link(resource),
        );
        let msg = prompt_message_to_a2a_message(&pm);
        assert_eq!(
            msg.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_AGENT)
        );
        assert_eq!(msg.parts.len(), 1);
        use a2a_rs::domain::generated::part;
        let part = &msg.parts[0];
        if let Some(part::Content::Url(uri)) = &part.content {
            assert_eq!(uri, "http://example.com");
            assert_eq!(part.filename, "link");
            assert_eq!(part.media_type, "text/html");
        } else {
            panic!("Expected file URL part");
        }
    }

    // Mock handler for testing
    #[derive(Clone)]
    struct NoOpHandler;

    #[async_trait]
    impl AsyncMessageHandler for NoOpHandler {
        async fn process_message(
            &self,
            task_id: &str,
            message: &Message,
            _ctx: &RequestContext,
        ) -> std::result::Result<Task, a2a_rs::domain::error::A2AError> {
            Ok(Task::builder()
                .id(task_id.to_string())
                .context_id(uuid::Uuid::new_v4().to_string())
                .status(TaskStatus::new(TaskState::Completed, None))
                .history(vec![message.clone()])
                .build())
        }
    }
}
