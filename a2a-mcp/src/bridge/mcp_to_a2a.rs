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
use std::collections::BTreeSet;
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
                    .notify_cancelled(CancelledNotificationParam {
                        request_id,
                        reason: Some("Task canceled or handler dropped".to_string()),
                    })
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
        }
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

        // Reuse the internal `call_mcp_tool` logic
        let mcp_result = self.call_mcp_tool(task_id, &params.name, args).await?;

        Ok(LlmToolConverter::mcp_result_to_llm(&mcp_result))
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
    async fn call_mcp_tool(
        &self,
        task_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult> {
        debug!("Calling MCP tool: {} with args: {}", tool_name, arguments);

        // Verify tool exists
        if !self.tools.read().await.iter().any(|t| t.name == tool_name) {
            return Err(A2aMcpError::ToolNotFound(tool_name.to_string()));
        }

        // Call the MCP tool via the peer
        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let serde_json::Value::Object(map) = arguments {
            params = params.with_arguments(map);
        }

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

        let result = match response {
            Ok(ServerResult::CallToolResult(r)) => r,
            Ok(_) => {
                return Err(A2aMcpError::McpServer(
                    "Unexpected response from MCP server".to_string(),
                ));
            }
            Err(e) => return Err(e.into()),
        };

        info!("MCP tool '{}' returned result", tool_name);

        Ok(result)
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
        // Check if this is a tool call request
        if let Some(McpToolCall {
            name: tool_name,
            arguments,
        }) = Self::extract_tool_call(message)
        {
            info!("Detected MCP tool call request for tool: {}", tool_name);

            // Call the MCP tool
            match self.call_mcp_tool(task_id, &tool_name, arguments).await {
                Ok(result) => {
                    // Convert MCP result to A2A task
                    let task_state = if result.is_error.unwrap_or(false) {
                        TaskState::Failed
                    } else {
                        TaskState::Completed
                    };

                    let message_text = MessageConverter::extract_text_from_content(&result.content);

                    // Create agent response message
                    let agent_message = Message::builder()
                        .role(Role::Agent)
                        .parts(vec![Part::text(message_text)])
                        .message_id(uuid::Uuid::new_v4().to_string())
                        .build();

                    // Extract resources into artifacts
                    let mut artifacts = Vec::new();
                    for content_item in &result.content {
                        match &**content_item {
                            rmcp::model::RawContent::Resource(res) => {
                                let (uri, mime_type) = match &res.resource {
                                    rmcp::model::ResourceContents::TextResourceContents {
                                        uri,
                                        mime_type,
                                        ..
                                    } => (uri.clone(), mime_type.clone()),
                                    rmcp::model::ResourceContents::BlobResourceContents {
                                        uri,
                                        mime_type,
                                        ..
                                    } => (uri.clone(), mime_type.clone()),
                                };
                                let part = Part::file_from_uri(uri, None, mime_type);
                                artifacts.push(a2a_rs::domain::Artifact {
                                    artifact_id: uuid::Uuid::new_v4().to_string(),
                                    name: String::new(),
                                    description: String::new(),
                                    parts: vec![part],
                                    metadata: ::buffa::MessageField::none(),
                                    extensions: Vec::new(),
                                    ..Default::default()
                                });
                            }
                            rmcp::model::RawContent::ResourceLink(link) => {
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
                        Ok(task_builder.artifacts(artifacts).build())
                    } else {
                        Ok(task_builder.build())
                    }
                }
                Err(e) => {
                    error!("MCP tool call failed: {}", e);
                    Err(e.to_a2a_error())
                }
            }
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

/// Helper to map `PromptMessage` to A2A `Message`.
fn prompt_message_to_a2a_message(pm: &PromptMessage) -> Message {
    let role = match pm.role {
        PromptMessageRole::User => Role::User,
        PromptMessageRole::Assistant => Role::Agent,
    };

    let mut parts = Vec::new();
    match &pm.content {
        PromptMessageContent::Text { text } => {
            parts.push(Part::text(text.clone()));
        }
        PromptMessageContent::Image { image } => {
            let mut data_map = serde_json::Map::new();
            data_map.insert(
                "type".to_string(),
                serde_json::Value::String("image".to_string()),
            );
            data_map.insert(
                "data".to_string(),
                serde_json::Value::String(image.data.clone()),
            );
            data_map.insert(
                "mimeType".to_string(),
                serde_json::Value::String(image.mime_type.clone()),
            );

            let val: ::buffa_types::google::protobuf::Value =
                serde_json::from_value(serde_json::Value::Object(data_map))
                    .expect("valid JSON Value");
            parts.push(Part::data(val));
        }
        PromptMessageContent::Resource { resource } => match &resource.resource {
            rmcp::model::ResourceContents::TextResourceContents { uri, mime_type, .. } => {
                parts.push(Part::file_from_uri(uri.clone(), None, mime_type.clone()));
            }
            rmcp::model::ResourceContents::BlobResourceContents { uri, mime_type, .. } => {
                parts.push(Part::file_from_uri(uri.clone(), None, mime_type.clone()));
            }
        },
        PromptMessageContent::ResourceLink { link } => {
            parts.push(Part::file_from_uri(
                link.uri.clone(),
                Some(link.name.clone()),
                link.mime_type.clone(),
            ));
        }
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
        let pm = PromptMessage::new_text(PromptMessageRole::User, "Hello User");
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
        let raw_image = RawImageContent {
            data: "imagedata".to_string(),
            mime_type: "image/png".to_string(),
            meta: None,
        };
        let image = raw_image.no_annotation();
        let pm = PromptMessage::new(
            PromptMessageRole::Assistant,
            PromptMessageContent::Image { image },
        );
        let msg = prompt_message_to_a2a_message(&pm);
        assert_eq!(
            msg.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_AGENT)
        );
        assert_eq!(msg.parts.len(), 1);
        use a2a_rs::domain::generated::part;
        if let Some(part::Content::Data(val)) = &msg.parts[0].content {
            let json_val = serde_json::to_value(&**val).unwrap();
            assert_eq!(json_val["type"], "image");
            assert_eq!(json_val["data"], "imagedata");
            assert_eq!(json_val["mimeType"], "image/png");
        } else {
            panic!("Expected data part");
        }
    }

    #[test]
    fn test_prompt_message_to_a2a_message_resource() {
        let resource_contents = ResourceContents::TextResourceContents {
            uri: "file://test.txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            text: "Resource content".to_string(),
            meta: None,
        };
        let embedded_resource = RawEmbeddedResource::new(resource_contents).no_annotation();
        let pm = PromptMessage::new(
            PromptMessageRole::User,
            PromptMessageContent::Resource {
                resource: embedded_resource,
            },
        );
        let msg = prompt_message_to_a2a_message(&pm);
        assert_eq!(
            msg.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_USER)
        );
        assert_eq!(msg.parts.len(), 1);
        use a2a_rs::domain::generated::part;
        let part = &msg.parts[0];
        if let Some(part::Content::Url(uri)) = &part.content {
            assert_eq!(uri, "file://test.txt");
            assert_eq!(part.media_type, "text/plain");
        } else {
            panic!("Expected file URL part");
        }
    }

    #[test]
    fn test_prompt_message_to_a2a_message_resource_link() {
        let raw_resource = RawResource {
            uri: "http://example.com".to_string(),
            name: "link".to_string(),
            title: None,
            description: None,
            mime_type: Some("text/html".to_string()),
            size: None,
            icons: None,
            meta: None,
        };
        let resource = raw_resource.no_annotation();
        let pm = PromptMessage::new(
            PromptMessageRole::Assistant,
            PromptMessageContent::ResourceLink { link: resource },
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
