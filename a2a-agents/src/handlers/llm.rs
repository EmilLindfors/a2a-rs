use std::sync::Arc;

use a2a_agents_common::llm::{
    ChatMessage, LlmProvider, LlmRequest, LlmStreamEvent, MessageRole, ToolCallAccumulator,
    ToolDefinition,
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

/// What a run produced — as distinct from whether it *broke*.
///
/// A model that answers and a model that gives up are both a run that finished;
/// only one is a task that succeeded. Keeping them apart from `Err` is what lets
/// giving up settle the task as failed while still showing whatever the model
/// did manage, instead of an apology in place of the work.
enum Answer {
    /// The model produced a final answer.
    Given(String),
    /// The model stopped without one. `why` names the cause and the knob that
    /// changes it; `partial` is whatever it produced on the way, which on a
    /// failed task is the most useful thing there is to show.
    GaveUp { why: String, partial: String },
}

impl Answer {
    /// The state a task carrying this outcome settles in, and the text it shows.
    ///
    /// Giving up is `Failed` rather than `InputRequired`: the model asked the
    /// caller no question, so there is nothing for a client to usefully supply —
    /// prompting for input would only restart the same run against the same
    /// budget. The knob that unblocks it (`max_tool_rounds`, `[llm] reasoning`)
    /// belongs to whoever configured the agent, not to whoever is talking to it.
    fn settle(self) -> (TaskState, String) {
        match self {
            Answer::Given(text) => (TaskState::Completed, text),
            Answer::GaveUp { why, partial } if partial.trim().is_empty() => {
                (TaskState::Failed, why)
            }
            Answer::GaveUp { why, partial } => (TaskState::Failed, format!("{partial}\n\n({why})")),
        }
    }
}

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
        if let Err(e) = self
            .streaming
            .broadcast_artifact_update(task_id, event)
            .await
        {
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
    ) -> Result<Answer, A2AError> {
        use futures::StreamExt;

        let tools: Vec<ToolDefinition> = tools::collect_tool_defs(&self.tools);
        let mut messages = vec![
            ChatMessage::system(self.system_prompt.clone()),
            ChatMessage::user(user_text),
        ];
        // What the model said on its way through the tool rounds. Kept outside
        // the loop because it is the only thing left to show if the budget runs
        // out before it commits to an answer.
        let mut said_along_the_way = String::new();

        for round in 0..self.max_tool_rounds {
            let mut request = LlmRequest::new(messages.clone()).temperature(0.2);
            if !tools.is_empty() {
                request = request.tools(tools.clone());
            }
            // Reasoning is deliberately not set here: how hard to think is a
            // property of the model the agent was pointed at (`[llm] reasoning`,
            // carried by the provider), and this handler serves whichever model
            // it is handed. It used to ask for *high* effort whenever the
            // provider said it could reason at all — a fact about the endpoint,
            // not the model — so a flash agent answering in one line paid for
            // frontier thinking on every request.

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
                        self.stream_artifact(
                            task_id,
                            context_id,
                            &thinking_id,
                            "AI Thinking...",
                            &chunk,
                        )
                        .await;
                    }
                    LlmStreamEvent::ContentChunk(chunk) => {
                        content.push_str(&chunk);
                        self.stream_artifact(task_id, context_id, &answer_id, "AI Answer", &chunk)
                            .await;
                    }
                    LlmStreamEvent::ToolCallChunk {
                        id,
                        name,
                        arguments,
                    } => {
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
                // An empty completion is not an answer, and it cannot be
                // reported as one: proto3 omits a default value, so an empty
                // text part goes on the wire as `{}` — indistinguishable from a
                // file or data part, which is how a client renders it. So a
                // model that says nothing used to arrive as a *completed* task
                // showing `[non-text content]`, and the agent looked broken in a
                // way that named neither the cause nor the model.
                if content.trim().is_empty() {
                    return Ok(Answer::GaveUp {
                        why: if reasoning.trim().is_empty() {
                            "the model returned an empty response".to_string()
                        } else {
                            // The common one: a small model can spend its whole
                            // response budget thinking and emit no answer. Name
                            // the knob that stops it — the reader is looking at
                            // a failed task, not at this crate's docs.
                            "the model returned reasoning but no answer — lower `[llm] reasoning` (\"off\" or a token budget) or use a larger model".to_string()
                        },
                        // The thinking is the only output there was, and it was
                        // billed for; a non-streaming caller has seen none of it.
                        partial: reasoning,
                    });
                }
                return Ok(Answer::Given(content));
            }

            if !content.trim().is_empty() {
                if !said_along_the_way.is_empty() {
                    said_along_the_way.push_str("\n\n");
                }
                said_along_the_way.push_str(content.trim());
            }

            messages.push(ChatMessage {
                role: MessageRole::Assistant,
                content: (!content.is_empty()).then_some(content),
                tool_calls: Some(calls.clone()),
                tool_call_id: None,
                name: None,
            });
            for call in &calls {
                self.stream_progress(
                    task_id,
                    context_id,
                    &format!("calling {}({})", call.name, call.arguments),
                )
                .await;
                let source = tools::resolve(&self.tools, &call.name).ok_or_else(|| {
                    A2AError::Internal(format!("model called unknown tool '{}'", call.name))
                })?;
                let result = source.invoke(task_id, call).await?;
                self.stream_progress(task_id, context_id, &format!("{} -> {result}", call.name))
                    .await;
                messages.push(ChatMessage::tool_result(
                    call.id.clone(),
                    call.name.clone(),
                    result,
                ));
            }
        }
        // The budget ran out with the model still calling tools. That is a task
        // that did not get done, so it settles `Failed` — and it keeps whatever
        // the model said on the way, because an apology in place of the work is
        // strictly less than the work.
        Ok(Answer::GaveUp {
            why: format!(
                "gave up after {} tool-call rounds without reaching an answer — raise `max_tool_rounds` if the work genuinely needs more",
                self.max_tool_rounds
            ),
            partial: said_along_the_way,
        })
    }

    async fn run_fallback(
        &self,
        task_id: &str,
        context_id: &str,
        user_text: &str,
    ) -> Result<Answer, A2AError> {
        let names: Vec<String> = tools::collect_tool_defs(&self.tools)
            .into_iter()
            .map(|t| t.name)
            .collect();
        if names.is_empty() {
            return Ok(Answer::Given(format!(
                "No LLM key configured and no MCP tools available. You said: {user_text}"
            )));
        }
        self.stream_progress(task_id, context_id, "no LLM key; routing deterministically")
            .await;
        Ok(Answer::Given(format!(
            "No LLM key is configured, so I cannot reason over your message. This agent has MCP tools available ({}). Set an LLM key (OPENAI_API_KEY / GEMINI_API_KEY / OPENROUTER_API_KEY) to enable natural-language answers.",
            names.join(", ")
        )))
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
            handler
                .stream_progress(&task_id, &context_id, "analyzing your request")
                .await;

            let outcome = match &handler.llm {
                Some(llm) => {
                    handler
                        .run_with_llm(llm.as_ref(), &task_id, &context_id, &user_text)
                        .await
                }
                None => {
                    handler
                        .run_fallback(&task_id, &context_id, &user_text)
                        .await
                }
            };

            let (state, reply) = match outcome {
                Ok(answer) => answer.settle(),
                Err(e) => (TaskState::Failed, format!("Sorry, I hit an error: {e}")),
            };

            let response = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text(reply)])
                .message_id(uuid::Uuid::new_v4().to_string())
                .context_id(context_id.clone())
                .build();

            if let Err(e) = handler
                .update_and_broadcast(&id, state, Some(response))
                .await
            {
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

#[cfg(test)]
mod tests {
    use a2a_agents_common::llm::{LlmError, LlmResponse, ToolCall};
    use a2a_rs::adapter::{InMemoryStreamingHandler, InMemoryTaskStorage};
    use a2a_rs::port::NoopPushNotifier;
    use futures::stream::{self, BoxStream, StreamExt};

    use super::*;

    /// A tool that always resolves and always answers, so a test can drive the
    /// tool-calling loop to its budget without an MCP server or a peer agent.
    struct AlwaysCallableTool;

    #[async_trait]
    impl ToolSource for AlwaysCallableTool {
        fn tool_defs(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "look_it_up".to_string(),
                description: "looks something up".to_string(),
                parameters: serde_json::json!({ "type": "object", "properties": {} }),
            }]
        }

        fn has_tool(&self, name: &str) -> bool {
            name == "look_it_up"
        }

        async fn invoke(&self, _task_id: &str, _call: &ToolCall) -> Result<String, A2AError> {
            Ok("a fact".to_string())
        }
    }

    /// Replays a fixed event sequence, so a test can pin what the handler does
    /// with a *shape* of response rather than with a particular model's mood.
    struct ScriptedProvider(Vec<LlmStreamEvent>);

    #[async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn chat_completion(&self, _request: LlmRequest) -> Result<LlmResponse, LlmError> {
            Err(LlmError::ProviderError("unused in this test".to_string()))
        }

        async fn chat_completion_stream(
            &self,
            _request: LlmRequest,
        ) -> Result<BoxStream<'static, Result<LlmStreamEvent, LlmError>>, LlmError> {
            let events: Vec<_> = self.0.clone().into_iter().map(Ok).collect();
            Ok(stream::iter(events).boxed())
        }
    }

    fn handler_with(events: Vec<LlmStreamEvent>) -> LlmHandler {
        handler_with_tools(events, Vec::new())
    }

    fn handler_with_tools(
        events: Vec<LlmStreamEvent>,
        tools: Vec<Arc<dyn ToolSource>>,
    ) -> LlmHandler {
        LlmHandler::new(
            "test".to_string(),
            2,
            InMemoryTaskStorage::new(),
            InMemoryStreamingHandler::new(),
            Arc::new(NoopPushNotifier),
            tools,
            Some(Arc::new(ScriptedProvider(events)) as Arc<dyn LlmProvider>),
        )
    }

    /// A model that only ever calls tools, so the loop always runs out of rounds.
    fn always_calls_a_tool(said: Option<&str>) -> Vec<LlmStreamEvent> {
        let mut events: Vec<_> = said
            .map(|s| LlmStreamEvent::ContentChunk(s.to_string()))
            .into_iter()
            .collect();
        events.push(LlmStreamEvent::ToolCall(ToolCall {
            id: "call-1".to_string(),
            name: "look_it_up".to_string(),
            arguments: "{}".to_string(),
        }));
        events
    }

    /// The text a settled task shows a client, which is the whole point: an
    /// empty text part serializes as `{}` and renders as non-text content.
    fn reply_text(task: &Task) -> String {
        task.status
            .as_option()
            .and_then(|s| s.message.as_option())
            .map(extract_text)
            .unwrap_or_default()
    }

    fn state_of(task: &Task) -> TaskState {
        match task.status.as_option().map(|s| s.state) {
            Some(buffa::EnumValue::Known(state)) => state,
            other => panic!("expected a known task state, got {other:?}"),
        }
    }

    async fn settled(handler: &LlmHandler, task_id: &str) -> Task {
        let id: TaskId = task_id.parse().unwrap();
        let message = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text("hello".to_string())])
            .message_id("m1".to_string())
            .build();
        handler
            .process_message(task_id, &message, None)
            .await
            .unwrap();

        // `process_message` acknowledges and finishes the work on a spawned
        // task, so the settled state arrives after it returns.
        for _ in 0..200 {
            let task = handler.lifecycle.get(&id, None).await.unwrap();
            if state_of(&task).is_terminal() {
                return task;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("task never settled");
    }

    #[tokio::test]
    async fn an_answer_completes_the_task() {
        let handler = handler_with(vec![LlmStreamEvent::ContentChunk("42".to_string())]);
        let task = settled(&handler, "11111111-1111-1111-1111-111111111111").await;

        assert_eq!(state_of(&task), TaskState::Completed);
        assert_eq!(reply_text(&task), "42");
    }

    /// A reasoning-capable model asked for high effort can spend its whole
    /// response there and emit no content. Completing on that reports success
    /// and says nothing — and because proto3 drops the empty string, the client
    /// cannot even tell it apart from a file part.
    #[tokio::test]
    async fn reasoning_with_no_answer_fails_rather_than_completing_empty() {
        let handler = handler_with(vec![LlmStreamEvent::Reasoning("thinking…".to_string())]);
        let task = settled(&handler, "22222222-2222-2222-2222-222222222222").await;

        assert_eq!(state_of(&task), TaskState::Failed);
        let reply = reply_text(&task);
        assert!(
            reply.contains("reasoning"),
            "the failure has to name the cause, got: {reply:?}"
        );
    }

    /// The same trap without reasoning: a provider that streams nothing at all.
    #[tokio::test]
    async fn an_empty_response_fails_rather_than_completing_empty() {
        let handler = handler_with(Vec::new());
        let task = settled(&handler, "33333333-3333-3333-3333-333333333333").await;

        assert_eq!(state_of(&task), TaskState::Failed);
        assert!(!reply_text(&task).trim().is_empty());
    }

    /// Running out of tool-call rounds is work that did not get done, so it must
    /// settle `Failed`. It used to return `Ok(<apology>)` — a **completed** task
    /// whose text said it had failed, which every caller branching on state
    /// (`a2acli` exits 2 on `failed`; delegation relays whatever comes back)
    /// read as success.
    #[tokio::test]
    async fn exhausting_the_tool_budget_fails_rather_than_completing_with_an_apology() {
        let handler = handler_with_tools(
            always_calls_a_tool(None),
            vec![Arc::new(AlwaysCallableTool) as Arc<dyn ToolSource>],
        );
        let task = settled(&handler, "44444444-4444-4444-4444-444444444444").await;

        assert_eq!(state_of(&task), TaskState::Failed);
        let reply = reply_text(&task);
        assert!(
            reply.contains("max_tool_rounds"),
            "the failure has to name the knob that changes it, got: {reply:?}"
        );
    }

    /// …and whatever the model said on the way survives into the message. The
    /// partial work is the most useful thing a failed task can show, and it used
    /// to be replaced wholesale by the apology.
    #[tokio::test]
    async fn giving_up_keeps_the_partial_work() {
        let handler = handler_with_tools(
            always_calls_a_tool(Some("Bergen is on the west coast.")),
            vec![Arc::new(AlwaysCallableTool) as Arc<dyn ToolSource>],
        );
        let task = settled(&handler, "55555555-5555-5555-5555-555555555555").await;

        assert_eq!(state_of(&task), TaskState::Failed);
        let reply = reply_text(&task);
        assert!(
            reply.contains("Bergen is on the west coast."),
            "the partial answer must survive, got: {reply:?}"
        );
    }
}
