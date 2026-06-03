//! Kitchen-sink complex agent — every major a2a-rs building block in one binary.
//!
//! Run with (the `mcp-server` feature pulls in `a2a-mcp` + `rmcp`):
//!
//! ```bash
//! cargo run -p a2a-agents --example complex_agent --features mcp-server
//! ```
//!
//! Optionally export `OPENAI_API_KEY` or `GEMINI_API_KEY` first to let an LLM
//! drive tool selection and answer in natural language. With no key set the
//! agent still works — it falls back to a deterministic rule-based router so the
//! example runs end-to-end in CI without secrets.
//!
//! What it wires together:
//!
//! * **Declarative TOML config** (`complex_agent.toml`) — identity, skills,
//!   transport port, storage, and the `streaming` feature flag.
//! * **An in-process MCP tool server** exposing `add`, `multiply`, and
//!   `word_count`, reached over an in-memory `tokio::io::duplex` pipe (no
//!   external process).
//! * **`McpToA2ABridge`** — the agent discovers those MCP tools
//!   (`get_llm_tools`) and executes them (`execute_llm_tool_call`).
//! * **Optional LLM tool-calling** via `a2a-agents-common`'s `LlmProvider`.
//! * **Live streaming to web clients** — the handler broadcasts progress
//!   artifacts through the `TaskStatusBroadcast` mixin and a shared
//!   `InMemoryStreamingHandler`, which the runtime now injects into the
//!   transport so `tasks/subscribe` SSE streams actually observe them.
//! * **A2A native tasks & progress** — every request creates/advances a task
//!   through `Working` → `Completed`/`Failed`.
//!
//! Talk to it once running (separate shell):
//!
//! ```bash
//! # Agent card
//! curl -s http://127.0.0.1:8080/.well-known/agent-card.json | jq .
//! ```
//!
//! …or point any A2A client (e.g. the `a2a-web-client`) at the same URL and
//! subscribe to a task to watch the progress artifacts stream in.

use std::sync::Arc;

use a2a_agents::core::AgentBuilder;
use a2a_agents_common::llm::{
    ChatMessage, LlmProvider, LlmRequest, MessageRole, ToolCall, ToolDefinition,
};
use a2a_mcp::McpToA2ABridge;
use a2a_rs::Artifact;
use a2a_rs::application::{HasPushNotifier, HasStreaming, HasTaskLifecycle, TaskStatusBroadcast};
use a2a_rs::domain::{
    A2AError, ContextId, Message, Part, Role, Task, TaskArtifactUpdateEvent, TaskId, TaskState,
    part,
};
use a2a_rs::port::{
    AsyncMessageHandler, AsyncPushNotifier, AsyncStreamingHandler, AsyncTaskLifecycle,
};
use a2a_rs::{InMemoryStreamingHandler, InMemoryTaskStorage};
use async_trait::async_trait;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, model::*,
    service::RequestContext,
};
use serde_json::json;
use tracing_subscriber::EnvFilter;

/// How many LLM ↔ tool round-trips to allow before giving up.
const MAX_TOOL_ROUNDS: usize = 4;

const SYSTEM_PROMPT: &str = "You are a concise research assistant. You have tools \
for arithmetic (add, multiply) and text analysis (word_count). Use a tool whenever \
it gives an exact answer instead of guessing, then reply in one short sentence.";

// ---------------------------------------------------------------------------
// 1. Downstream MCP tool server (in-process).
// ---------------------------------------------------------------------------

/// A tiny MCP server exposing three tools. Mirrors the shape an external MCP
/// server (spawned as a child process) would have — here it just runs over an
/// in-memory duplex pipe so the example needs no external setup.
#[derive(Clone)]
struct ToolServer {
    tools: Arc<Vec<Tool>>,
}

impl ToolServer {
    fn new() -> Self {
        let number_pair: Arc<JsonObject> = Arc::new(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                },
                "required": ["a", "b"]
            }))
            .expect("valid JSON schema"),
        );
        let text_arg: Arc<JsonObject> = Arc::new(
            serde_json::from_value(json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }))
            .expect("valid JSON schema"),
        );

        let tools = vec![
            Tool::new("add", "Add two numbers a + b", number_pair.clone()),
            Tool::new("multiply", "Multiply two numbers a * b", number_pair),
            Tool::new("word_count", "Count the words in a piece of text", text_arg),
        ];
        Self {
            tools: Arc::new(tools),
        }
    }
}

// The rmcp `ServerHandler` methods are declared with explicit `impl Future`
// return types (RPITIT), so they're written in the same manual form here.
#[allow(clippy::manual_async_fn)]
impl ServerHandler for ToolServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_server_info(Implementation::new("kitchen-sink-tools", "0.1.0"))
            .with_instructions("Arithmetic and text-analysis tools for the complex agent example")
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            Ok(ListToolsResult {
                tools: (*self.tools).clone(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn call_tool(
        &self,
        CallToolRequestParams { name, arguments, .. }: CallToolRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            let args = arguments.unwrap_or_default();
            let result = match name.as_ref() {
                "add" | "multiply" => {
                    let a = number_arg(&args, "a")?;
                    let b = number_arg(&args, "b")?;
                    if name == "add" { a + b } else { a * b }.to_string()
                }
                "word_count" => {
                    let text = args
                        .get("text")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| McpError::invalid_params("missing 'text'", None))?;
                    text.split_whitespace().count().to_string()
                }
                other => {
                    return Err(McpError::invalid_params(format!("unknown tool: {other}"), None));
                }
            };
            Ok(CallToolResult::success(vec![Content::text(result)]))
        }
    }
}

fn number_arg(args: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<f64, McpError> {
    args.get(key)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| McpError::invalid_params(format!("missing or non-numeric '{key}'"), None))
}

// ---------------------------------------------------------------------------
// 2. The agent handler.
// ---------------------------------------------------------------------------

/// Inner handler the bridge requires but this example never routes through —
/// we call `execute_llm_tool_call`/`get_llm_tools` on the bridge directly and
/// keep task-lifecycle ownership in [`ResearchAssistantHandler`]. It only
/// fires if a raw `a2a_rs_tool_call` envelope arrives, which we never send.
#[derive(Clone)]
struct UnusedInner;

#[async_trait]
impl AsyncMessageHandler for UnusedInner {
    async fn process_message(
        &self,
        _task_id: &str,
        _message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "inner handler is not used in the complex_agent example".to_string(),
        ))
    }
}

/// The agent. Owns the task-lifecycle, streaming, and push ports (so it hosts
/// the `TaskStatusBroadcast` mixin), the MCP bridge, and an optional LLM.
#[derive(Clone)]
struct ResearchAssistantHandler {
    lifecycle: Arc<dyn AsyncTaskLifecycle>,
    streaming: Arc<dyn AsyncStreamingHandler>,
    push: Arc<dyn AsyncPushNotifier>,
    bridge: Arc<McpToA2ABridge<UnusedInner>>,
    llm: Option<Arc<dyn LlmProvider>>,
}

// Accessors that surface the `TaskStatusBroadcast` mixin on this handler. Every
// status transition routed through `update_and_broadcast` reaches streaming
// subscribers and push targets — see `.claude/rules/hexagonal_architecture.md` §9.
impl HasTaskLifecycle for ResearchAssistantHandler {
    fn lifecycle(&self) -> &dyn AsyncTaskLifecycle {
        self.lifecycle.as_ref()
    }
}
impl HasStreaming for ResearchAssistantHandler {
    fn streaming(&self) -> &dyn AsyncStreamingHandler {
        self.streaming.as_ref()
    }
}
impl HasPushNotifier for ResearchAssistantHandler {
    fn push_notifier(&self) -> &dyn AsyncPushNotifier {
        self.push.as_ref()
    }
}

impl ResearchAssistantHandler {
    fn new(
        lifecycle: impl AsyncTaskLifecycle + 'static,
        streaming: impl AsyncStreamingHandler + 'static,
        push: Arc<dyn AsyncPushNotifier>,
        bridge: Arc<McpToA2ABridge<UnusedInner>>,
        llm: Option<Arc<dyn LlmProvider>>,
    ) -> Self {
        Self {
            lifecycle: Arc::new(lifecycle),
            streaming: Arc::new(streaming),
            push,
            bridge,
            llm,
        }
    }

    /// Push an incremental progress artifact to any SSE subscriber.
    async fn stream_progress(&self, task_id: &str, context_id: &str, text: &str) {
        let artifact = Artifact {
            artifact_id: format!("progress-{task_id}"),
            name: "progress".to_string(),
            description: String::new(),
            parts: vec![Part::text(text.to_string())],
            metadata: ::buffa::MessageField::none(),
            extensions: Vec::new(),
            ..Default::default()
        };
        let event = TaskArtifactUpdateEvent {
            task_id: task_id.to_string(),
            context_id: context_id.to_string(),
            kind: "artifact-update".to_string(),
            artifact,
            append: Some(true),
            last_chunk: Some(false),
            metadata: None,
        };
        if let Err(e) = self.streaming.broadcast_artifact_update(task_id, event).await {
            tracing::warn!("failed to broadcast progress: {e}");
        }
    }

    /// LLM path: let the model pick tools, execute them via the bridge, loop
    /// until it answers in prose.
    async fn run_with_llm(
        &self,
        llm: &dyn LlmProvider,
        task_id: &str,
        context_id: &str,
        user_text: &str,
    ) -> Result<String, A2AError> {
        let tools: Vec<ToolDefinition> = self.bridge.get_llm_tools();
        let mut messages = vec![
            ChatMessage::system(SYSTEM_PROMPT),
            ChatMessage::user(user_text),
        ];

        for _round in 0..MAX_TOOL_ROUNDS {
            let mut request = LlmRequest::new(messages.clone()).temperature(0.2);
            if !tools.is_empty() {
                request = request.tools(tools.clone());
            }

            let response = llm
                .chat_completion(request)
                .await
                .map_err(|e| A2AError::Internal(format!("LLM error: {e}")))?;

            match response.tool_calls {
                Some(calls) if !calls.is_empty() => {
                    // Record the assistant turn that requested the tools…
                    messages.push(ChatMessage {
                        role: MessageRole::Assistant,
                        content: response.content.clone(),
                        tool_calls: Some(calls.clone()),
                        tool_call_id: None,
                        name: None,
                    });
                    // …then execute each tool against MCP and feed results back.
                    for call in &calls {
                        self.stream_progress(
                            task_id,
                            context_id,
                            &format!("🛠️ calling `{}`({})", call.name, call.arguments),
                        )
                        .await;
                        let result = self
                            .bridge
                            .execute_llm_tool_call(task_id, call)
                            .await
                            .map_err(|e| e.to_a2a_error())?;
                        self.stream_progress(
                            task_id,
                            context_id,
                            &format!("✅ `{}` → {result}", call.name),
                        )
                        .await;
                        messages.push(ChatMessage::tool_result(
                            call.id.clone(),
                            call.name.clone(),
                            result,
                        ));
                    }
                }
                _ => return Ok(response.content.unwrap_or_default()),
            }
        }
        Ok("I couldn't converge on an answer within the tool-call budget.".to_string())
    }

    /// No-LLM fallback: a deterministic router so the example runs without keys.
    async fn run_rule_based(
        &self,
        task_id: &str,
        context_id: &str,
        user_text: &str,
    ) -> Result<String, A2AError> {
        let lower = user_text.to_lowercase();
        let make = |name: &str, args: serde_json::Value| ToolCall {
            id: format!("rule-{name}"),
            name: name.to_string(),
            arguments: args.to_string(),
        };

        let tool_call = if lower.contains("multipl") || lower.contains("times") || lower.contains('*')
        {
            parse_two_numbers(user_text).map(|(a, b)| make("multiply", json!({ "a": a, "b": b })))
        } else if lower.contains("add") || lower.contains("plus") || lower.contains("sum") {
            parse_two_numbers(user_text).map(|(a, b)| make("add", json!({ "a": a, "b": b })))
        } else if lower.contains("word") || lower.contains("count") {
            let text = user_text
                .split_once(':')
                .map(|(_, t)| t.trim().to_string())
                .unwrap_or_else(|| user_text.to_string());
            Some(make("word_count", json!({ "text": text })))
        } else {
            None
        };

        match tool_call {
            Some(call) => {
                self.stream_progress(
                    task_id,
                    context_id,
                    &format!("🛠️ (rule-based) calling `{}`", call.name),
                )
                .await;
                let result = self
                    .bridge
                    .execute_llm_tool_call(task_id, &call)
                    .await
                    .map_err(|e| e.to_a2a_error())?;
                Ok(format!(
                    "The `{}` tool returned **{result}**.\n\n(Set OPENAI_API_KEY or \
                     GEMINI_API_KEY to let an LLM choose tools and answer in natural language.)",
                    call.name
                ))
            }
            None => {
                let names: Vec<String> =
                    self.bridge.tools().iter().map(|t| t.name.to_string()).collect();
                Ok(format!(
                    "I can do simple math and text stats via MCP tools ({}).\n\
                     Try: `add 21 21`, `multiply 6 7`, or `count words: the quick brown fox`.",
                    names.join(", ")
                ))
            }
        }
    }
}

#[async_trait]
impl AsyncMessageHandler for ResearchAssistantHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;

        // Create the task on first contact.
        if !self.lifecycle.exists(&id).await? {
            let raw_ctx = if message.context_id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                message.context_id.clone()
            };
            let ctx: ContextId = raw_ctx.parse()?;
            self.lifecycle.create(&id, &ctx).await?;
        }
        let context_id = self.lifecycle.get(&id, Some(1)).await?.context_id.clone();

        // Record the user's message and move to Working — broadcast both.
        self.update_and_broadcast(&id, TaskState::Working, Some(message.clone()))
            .await?;
        self.stream_progress(task_id, &context_id, "🔎 Analyzing your request…")
            .await;

        let user_text = extract_text(message);
        let outcome = match &self.llm {
            Some(llm) => {
                self.run_with_llm(llm.as_ref(), task_id, &context_id, &user_text)
                    .await
            }
            None => {
                self.run_rule_based(task_id, &context_id, &user_text)
                    .await
            }
        };

        let (state, reply) = match outcome {
            Ok(text) => (TaskState::Completed, text),
            Err(e) => (TaskState::Failed, format!("Sorry — I hit an error: {e}")),
        };

        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(reply)])
            .message_id(uuid::Uuid::new_v4().to_string())
            .context_id(context_id)
            .build();

        let final_task = self.update_and_broadcast(&id, state, Some(response)).await?;
        Ok(final_task)
    }

    async fn validate_message(&self, message: &Message) -> Result<(), A2AError> {
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "message.parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 3. Helpers.
// ---------------------------------------------------------------------------

fn extract_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|p| match &p.content {
            Some(part::Content::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pull the first two numbers out of free text (e.g. "what is 6 times 7" → 6,7).
fn parse_two_numbers(text: &str) -> Option<(f64, f64)> {
    let nums: Vec<f64> = text
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    match nums.as_slice() {
        [a, b, ..] => Some((*a, *b)),
        _ => None,
    }
}

fn load_llm() -> Option<Arc<dyn LlmProvider>> {
    use a2a_agents_common::llm::{gemini::GeminiProvider, openai::OpenAiProvider};
    if let Ok(gemini) = GeminiProvider::from_env() {
        tracing::info!("🤖 LLM: Gemini (tool-calling enabled)");
        return Some(Arc::new(gemini));
    }
    if let Ok(openai) = OpenAiProvider::from_env() {
        tracing::info!("🤖 LLM: OpenAI (tool-calling enabled)");
        return Some(Arc::new(openai));
    }
    tracing::info!("🤖 LLM: none configured — using rule-based fallback");
    None
}

// ---------------------------------------------------------------------------
// 4. Composition root.
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // (a) Start the in-process MCP tool server over a duplex pipe.
    let (server_io, client_io) = tokio::io::duplex(8192);
    let _tool_server = tokio::spawn(async move {
        let running = ToolServer::new().serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });

    // (b) Connect an MCP client and hand its peer to the bridge.
    //     `mcp_client` is kept alive for the whole process (the run loop below
    //     never returns) so the peer stays connected.
    let mcp_client = ().serve(client_io).await?;
    let bridge = Arc::new(McpToA2ABridge::new(mcp_client.peer().clone(), UnusedInner).await?);
    tracing::info!(
        "🔧 MCP tools available: {}",
        bridge.tools().iter().map(|t| t.name.to_string()).collect::<Vec<_>>().join(", ")
    );

    // (c) Shared storage + streaming. The SAME streaming instance goes to the
    //     handler (it broadcasts) and to the builder (the transport subscribes),
    //     so SSE clients see the handler's progress. Clones share the registry.
    let storage = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();

    // (d) Build the handler with optional LLM.
    let handler = ResearchAssistantHandler::new(
        storage.clone(),
        streaming.clone(),
        storage.push_notifier(),
        bridge,
        load_llm(),
    );

    // (e) Assemble from TOML and run. `with_streaming` is the new builder hook
    //     that bridges the handler's broadcasts to the transport's SSE streams.
    println!("🚀 Complex agent listening on http://127.0.0.1:8080");
    println!("   Agent card: http://127.0.0.1:8080/.well-known/agent-card.json");
    AgentBuilder::from_file("examples/complex_agent.toml")?
        .with_handler(handler)
        .with_storage(storage)
        .with_streaming(streaming)
        .build()?
        .run()
        .await?;

    drop(mcp_client);
    Ok(())
}
