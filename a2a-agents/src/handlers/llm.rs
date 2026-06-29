use std::sync::Arc;

use a2a_agents_common::llm::{
    ChatMessage, LlmProvider, LlmRequest, LlmStreamEvent, MessageRole, ReasoningConfig,
    ReasoningEffort, ToolCallAccumulator, ToolDefinition,
};
use a2a_rs::application::{HasPushNotifier, HasStreaming, HasTaskLifecycle, TaskStatusBroadcast};
use a2a_rs::domain::{
    A2AError, ContextId, Message, Part, Role, Task, TaskArtifactUpdateEvent, TaskId, TaskState,
    part,
};
use a2a_rs::port::{
    AsyncMessageHandler, AsyncPushNotifier, AsyncStreamingHandler, AsyncTaskLifecycle,
};
use async_trait::async_trait;
use buffa::MessageField;

use super::tools::{self, ToolSource};

#[derive(Clone)]
pub struct LlmHandler {
    system_prompt: String,
    max_tool_rounds: u32,
    lifecycle: Arc<dyn AsyncTaskLifecycle>,
    streaming: Arc<dyn AsyncStreamingHandler>,
    push: Arc<dyn AsyncPushNotifier>,
    tools: Arc<Vec<Arc<dyn ToolSource>>>,
    llm: Option<Arc<dyn LlmProvider>>,
}

impl HasTaskLifecycle for LlmHandler {
    fn lifecycle(&self) -> &dyn AsyncTaskLifecycle {
        self.lifecycle.as_ref()
    }
}
impl HasStreaming for LlmHandler {
    fn streaming(&self) -> &dyn AsyncStreamingHandler {
        self.streaming.as_ref()
    }
}
impl HasPushNotifier for LlmHandler {
    fn push_notifier(&self) -> &dyn AsyncPushNotifier {
        self.push.as_ref()
    }
}

impl LlmHandler {
    pub fn new(
        system_prompt: String,
        max_tool_rounds: u32,
        lifecycle: impl AsyncTaskLifecycle + 'static,
        streaming: impl AsyncStreamingHandler + 'static,
        push: Arc<dyn AsyncPushNotifier>,
        tools: Vec<Arc<dyn ToolSource>>,
        llm: Option<Arc<dyn LlmProvider>>,
    ) -> Self {
        Self {
            system_prompt,
            max_tool_rounds,
            lifecycle: Arc::new(lifecycle),
            streaming: Arc::new(streaming),
            push,
            tools: Arc::new(tools),
            llm,
        }
    }

    async fn stream_artifact(
        &self,
        task_id: &str,
        context_id: &str,
        artifact_id: &str,
        name: &str,
        text: &str,
    ) {
        let artifact = a2a_rs::Artifact {
            artifact_id: artifact_id.to_string(),
            name: name.to_string(),
            description: String::new(),
            parts: vec![Part::text(text.to_string())],
            metadata: MessageField::none(),
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
            tracing::warn!("failed to broadcast artifact: {e}");
        }
    }

    async fn stream_progress(&self, task_id: &str, context_id: &str, text: &str) {
        self.stream_artifact(
            task_id,
            context_id,
            &format!("progress-{task_id}"),
            "progress",
            text,
        )
        .await;
    }

    async fn run_with_llm(
        &self,
        llm: &dyn LlmProvider,
        task_id: &str,
        context_id: &str,
        user_text: &str,
    ) -> Result<String, A2AError> {
        use futures::StreamExt;

        let tools: Vec<ToolDefinition> = tools::collect_tool_defs(&self.tools);
        let mut messages = vec![
            ChatMessage::system(self.system_prompt.clone()),
            ChatMessage::user(user_text),
        ];
        let reasoning_enabled = llm.supports_reasoning();

        for round in 0..self.max_tool_rounds {
            let mut request = LlmRequest::new(messages.clone()).temperature(0.2);
            if !tools.is_empty() {
                request = request.tools(tools.clone());
            }
            if reasoning_enabled {
                request = request.reasoning(ReasoningConfig::effort(ReasoningEffort::High));
            }

            let mut stream = llm
                .chat_completion_stream(request)
                .await
                .map_err(|e| A2AError::Internal(format!("LLM error: {e}")))?;

            let thinking_id = format!("thinking-{task_id}-{round}");
            let answer_id = format!("answer-{task_id}-{round}");
            let mut content = String::new();
            let mut reasoning = String::new();
            let mut calls = ToolCallAccumulator::new();

            while let Some(event) = stream.next().await {
                match event.map_err(|e| A2AError::Internal(format!("LLM stream error: {e}")))? {
                    LlmStreamEvent::Reasoning(chunk) => {
                        reasoning.push_str(&chunk);
                        self.stream_artifact(task_id, context_id, &thinking_id, "AI Thinking...", &chunk).await;
                    }
                    LlmStreamEvent::ContentChunk(chunk) => {
                        content.push_str(&chunk);
                        self.stream_artifact(task_id, context_id, &answer_id, "AI Answer", &chunk).await;
                    }
                    LlmStreamEvent::ToolCallChunk { id, name, arguments } => {
                        calls.push(&id, name.as_deref(), &arguments);
                    }
                    LlmStreamEvent::ToolCall(call) => {
                        calls.finalize(call);
                    }
                }
            }

            if !reasoning.trim().is_empty() {
                let preview: String = reasoning.chars().take(280).collect();
                tracing::info!(has_reasoning = true, "reasoning: {preview}");
            }

            let calls = calls.drain_completed();
            if calls.is_empty() {
                return Ok(content);
            }

            messages.push(ChatMessage {
                role: MessageRole::Assistant,
                content: (!content.is_empty()).then_some(content),
                tool_calls: Some(calls.clone()),
                tool_call_id: None,
                name: None,
            });
            for call in &calls {
                self.stream_progress(task_id, context_id, &format!("calling {}({})", call.name, call.arguments)).await;
                let source = tools::resolve(&self.tools, &call.name).ok_or_else(|| {
                    A2AError::Internal(format!("model called unknown tool '{}'", call.name))
                })?;
                let result = source.invoke(task_id, call).await?;
                self.stream_progress(task_id, context_id, &format!("{} -> {result}", call.name)).await;
                messages.push(ChatMessage::tool_result(call.id.clone(), call.name.clone(), result));
            }
        }
        Ok("I could not converge on an answer within the tool-call budget.".to_string())
    }

    async fn run_fallback(
        &self,
        task_id: &str,
        context_id: &str,
        user_text: &str,
    ) -> Result<String, A2AError> {
        let names: Vec<String> = tools::collect_tool_defs(&self.tools)
            .into_iter()
            .map(|t| t.name)
            .collect();
        if names.is_empty() {
            return Ok(format!("No LLM key configured and no MCP tools available. You said: {user_text}"));
        }
        self.stream_progress(task_id, context_id, "no LLM key; routing deterministically").await;
        Ok(format!("No LLM key is configured, so I cannot reason over your message. This agent has MCP tools available ({}). Set an LLM key (OPENAI_API_KEY / GEMINI_API_KEY / OPENROUTER_API_KEY) to enable natural-language answers.", names.join(", ")))
    }
}

#[async_trait]
impl AsyncMessageHandler for LlmHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;

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

        let working = self
            .update_and_broadcast(&id, TaskState::Working, Some(message.clone()))
            .await?;

        let handler = self.clone();
        let task_id = task_id.to_string();
        let user_text = extract_text(message);
        tokio::spawn(async move {
            handler.stream_progress(&task_id, &context_id, "analyzing your request").await;

            let outcome = match &handler.llm {
                Some(llm) => handler.run_with_llm(llm.as_ref(), &task_id, &context_id, &user_text).await,
                None => handler.run_fallback(&task_id, &context_id, &user_text).await,
            };

            let (state, reply) = match outcome {
                Ok(text) => (TaskState::Completed, text),
                Err(e) => (TaskState::Failed, format!("Sorry, I hit an error: {e}")),
            };

            let response = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text(reply)])
                .message_id(uuid::Uuid::new_v4().to_string())
                .context_id(context_id.clone())
                .build();

            if let Err(e) = handler.update_and_broadcast(&id, state, Some(response)).await {
                tracing::warn!("failed to finalize task {task_id}: {e}");
            }
        });

        Ok(working)
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
