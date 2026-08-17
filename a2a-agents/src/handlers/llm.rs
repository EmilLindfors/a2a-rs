use std::sync::Arc;

use crate::context::{
    CharEstimate, ContextBudget, DriftWatch, Fit, Prompt, TokenEstimate, cap_tool_result, fit,
    project,
};
use a2a_llm::{
    ChatMessage, LlmError, LlmProvider, LlmRequest, LlmStreamEvent, MessageRole,
    ToolCallAccumulator, ToolDefinition,
};
use a2a_rs::application::{HasPushNotifier, HasStreaming, HasTaskLifecycle, TaskStatusBroadcast};
use a2a_rs::domain::{
    A2AError, ContextId, ContextState, Conversation, Message, Part, Role, Seq, SequencedMessage,
    Task, TaskArtifactUpdateEvent, TaskId, TaskState, part,
};
use a2a_rs::port::{
    AsyncContextStateStore, AsyncConversationStore, AsyncConversationStoreExt, AsyncMessageHandler,
    AsyncPushNotifier, AsyncStreamingHandler, AsyncTaskLifecycle, RequestContext,
};
use async_trait::async_trait;
use buffa::MessageField;

use super::context::{SUMMARY_INSTRUCTION, budget_from, turns_from};
use super::memory::{MemoryToolSource, is_memory_tool, render_state};
use super::tools::{self, ToolSource};
use crate::core::config::{ContextConfig, ContextMode};

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

/// How many model responses spent only on `remember`/`forget` are not charged
/// against `max_tool_rounds`.
///
/// A tool round is a model response, so an agent with the state bag on used to
/// pay for its bookkeeping out of the budget for the work: with the default of
/// 4, a model that wrote one fact had three rounds left to answer, and the
/// failure it produced named `max_tool_rounds` without saying where they went.
/// Bookkeeping is not the work, so it is free — but it still has to be bounded,
/// since nothing stops a model calling `remember` forever. Two covers the shape
/// this takes in practice: one response for what it learned on the way in, one
/// for what it concluded. Past that the rounds are charged again, so a model
/// looping on the bag ends the same way it did before.
const FREE_MEMORY_ROUNDS: u32 = 2;

/// What the agent recalled for one turn, read before the work starts.
///
/// The two halves travel together because they are read together, refuse
/// together (a context the caller does not own denies both), and are projected
/// into the same request. Loaded in `process_message` rather than in the run:
/// the refusal has to reach the transport as a refusal, not settle a task as
/// failed with the reason recorded as something the agent said.
struct Recalled {
    /// What was said in this context, as far back as the budget allows.
    conversation: Conversation,
    /// What the agent was asked to remember, empty when `remember = false`.
    state: ContextState,
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
    /// Where the conversation for a context is read from and summarized back to.
    /// `NoConversationMemory` when `[handler.llm.context] mode = "none"`, so the
    /// run loop needs no branch on whether history exists.
    conversations: Arc<dyn AsyncConversationStore>,
    /// Where the state bag lives. `NoContextState` when
    /// `[handler.llm.context] remember = false`, for the same reason.
    state: Arc<dyn AsyncContextStateStore>,
    context: ContextConfig,
    budget: ContextBudget,
    estimator: Arc<dyn TokenEstimate>,
    /// Reconciles the estimator against what the provider charges. Shared across
    /// clones of this handler, since one agent's ratio is one measurement.
    drift: Arc<DriftWatch>,
    /// Which model this handler is pointed at, recorded on every summary it
    /// writes. `LlmProvider` does not expose it, so it is passed in at the
    /// composition edge where `SelectedLlm` resolved it.
    model: String,
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

#[bon::bon]
impl LlmHandler {
    /// Assemble a handler from its collaborators.
    ///
    /// A builder rather than positional arguments: five of these are ports, and
    /// at the call site `Arc::new(NoopPushNotifier)` next to `Arc::new(storage)`
    /// says nothing about which is which.
    #[builder]
    pub fn new(
        system_prompt: String,
        max_tool_rounds: u32,
        lifecycle: impl AsyncTaskLifecycle + 'static,
        streaming: impl AsyncStreamingHandler + 'static,
        push: Arc<dyn AsyncPushNotifier>,
        tools: Vec<Arc<dyn ToolSource>>,
        llm: Option<Arc<dyn LlmProvider>>,
        /// Which model this handler is pointed at, recorded against every
        /// summary it writes. Without it a digest cannot be told apart from one
        /// a weaker model wrote.
        #[builder(into, default = "unknown".to_string())]
        model: String,
    ) -> Self {
        let context = ContextConfig::default();
        Self {
            system_prompt,
            max_tool_rounds,
            lifecycle: Arc::new(lifecycle),
            streaming: Arc::new(streaming),
            push,
            tools: Arc::new(tools),
            llm,
            conversations: Arc::new(a2a_rs::port::NoConversationMemory),
            state: Arc::new(a2a_rs::port::NoContextState),
            budget: budget_from(&context),
            estimator: Arc::new(CharEstimate::with_chars_per_token(
                context.chars_per_token.as_f32(),
            )),
            drift: Arc::new(DriftWatch::new(context.chars_per_token.as_f32())),
            context,
            model,
        }
    }

    /// Give this handler somewhere to remember, and the settings that say what
    /// it keeps.
    ///
    /// One method taking all three, rather than builder fields: a
    /// `ContextConfig` asking for `mode = "context"` or `remember = true` with
    /// no store behind it would compile, build, and remember nothing. The two
    /// stores are separate arguments because they are separate capabilities —
    /// usually one object implements both, and nothing requires it to.
    pub fn with_context_memory(
        mut self,
        conversations: Arc<dyn AsyncConversationStore>,
        state: Arc<dyn AsyncContextStateStore>,
        context: ContextConfig,
    ) -> Self {
        self.state = state;
        self.budget = budget_from(&context);
        self.estimator = Arc::new(CharEstimate::with_chars_per_token(
            context.chars_per_token.as_f32(),
        ));
        self.drift = Arc::new(DriftWatch::new(context.chars_per_token.as_f32()));
        self.context = context;
        self.conversations = conversations;
        self
    }

    /// Load whatever this agent is configured to remember about `context_id`.
    ///
    /// `mode = "none"` never reads, so an agent that carries no history costs no
    /// query. Failure to load is logged and answered with an empty conversation
    /// rather than failing the turn — except for
    /// [`ContextAccessDenied`](A2AError::ContextAccessDenied), which is a
    /// refusal and has to stay one.
    async fn load_conversation(
        &self,
        task_id: &TaskId,
        context_id: &str,
        caller: Option<&str>,
    ) -> Result<Conversation, A2AError> {
        let limit = load_limit(self.context.keep_recent_turns);

        let loaded = match self.context.mode {
            ContextMode::None => return Ok(Conversation::default()),
            // One task's own messages. No conversation store involved, so no
            // ownership question: a caller that can address the task can already
            // read its history through `tasks/get`.
            ContextMode::Task => {
                return Ok(Conversation {
                    digest: None,
                    tail: self
                        .lifecycle
                        .get(task_id, Some(limit))
                        .await?
                        .history
                        .into_iter()
                        .enumerate()
                        .map(|(index, message)| SequencedMessage {
                            seq: Seq::new(index as u64 + 1),
                            message,
                        })
                        .collect(),
                });
            }
            ContextMode::Context => {
                let id: ContextId = context_id.parse()?;
                self.conversations.load(&id, caller, Some(limit)).await
            }
        };

        match loaded {
            Ok(conversation) => Ok(conversation),
            // A refusal has to stay a refusal. Everything else degrades to "no
            // history", because answering without context beats not answering.
            Err(denied @ A2AError::ContextAccessDenied { .. }) => Err(denied),
            Err(e) => {
                tracing::warn!("could not load conversation for {context_id}: {e}");
                Ok(Conversation::default())
            }
        }
    }

    /// Load the state bag for this context and caller.
    ///
    /// Degrades the same way [`load_conversation`](Self::load_conversation)
    /// does, and for the same reason: answering without what was remembered
    /// beats not answering, while a refusal stays a refusal.
    async fn load_state(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
    ) -> Result<ContextState, A2AError> {
        match self.state.load_state(context_id, caller).await {
            Ok(state) => Ok(state),
            Err(denied @ A2AError::ContextAccessDenied { .. }) => Err(denied),
            Err(e) => {
                tracing::warn!("could not load remembered state for {context_id}: {e}");
                Ok(ContextState::new())
            }
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

    /// Summarize `conversation` and record the digest, so the next turn starts
    /// from the summary instead of the transcript.
    ///
    /// Runs inline, before the answer: the caller is already waiting on a model,
    /// and a background pass would need a lock across an LLM call to stop two
    /// turns of one conversation compacting at once. Appending with a watermark
    /// makes that race merely wasteful, so there is nothing to lock.
    ///
    /// A failure here is logged and swallowed. Not summarizing costs tokens on
    /// the next turn; failing the task costs the user their answer.
    async fn compact_conversation(
        &self,
        llm: &dyn LlmProvider,
        task_id: &str,
        context_id: &str,
        caller: Option<&str>,
        conversation: &Conversation,
    ) {
        // Only the part before the recent window is folded. Summarizing the
        // whole tail would leave the next turn with nothing verbatim, which is
        // the opposite of what `keep_recent_turns` promises — and the recent
        // turns are the ones a model most needs word for word.
        let Some(older) = older_than_recent(conversation, self.context.keep_recent_turns) else {
            return;
        };

        self.stream_progress(task_id, context_id, "compacting conversation")
            .await;

        let turns = turns_from(&older);
        let mut messages = project(Prompt {
            system: "You are summarizing a conversation.",
            // No state block: the bag is not part of the conversation and
            // survives compaction by itself, so summarizing it would only
            // duplicate it into the digest.
            state: None,
            // The previous summary is folded in rather than kept alongside, so
            // digests chain instead of accumulating.
            summary: conversation.summary(),
            turns: &turns,
            current: SUMMARY_INSTRUCTION,
        });
        // The summary request is itself subject to the budget: a conversation
        // too long to send is exactly the one being compacted.
        if let Err(e) = fit(&mut messages, &self.budget, self.estimator.as_ref()) {
            tracing::warn!("cannot summarize conversation for {context_id}: {e}");
            return;
        }

        // What the digest will stand in for: the turns being folded, plus the
        // previous summary, which is folded in rather than kept alongside.
        let replaced_tokens: usize = turns
            .iter()
            .map(|turn| self.estimator.estimate_text(&turn.text))
            .sum::<usize>()
            + conversation
                .summary()
                .map_or(0, |summary| self.estimator.estimate_text(summary));

        let request = LlmRequest::new(messages).temperature(0.0).max_tokens(
            self.budget
                .summary_tokens(replaced_tokens)
                .try_into()
                .unwrap_or(u32::MAX),
        );
        let summary = match llm.chat_completion(request).await {
            Ok(response) => response.content.unwrap_or_default(),
            Err(e) => {
                tracing::warn!("summarizing conversation for {context_id} failed: {e}");
                return;
            }
        };
        if summary.trim().is_empty() {
            tracing::warn!("summary for {context_id} came back empty; keeping the transcript");
            return;
        }
        // The backstop for a provider that ignored `max_tokens`. A digest is
        // re-sent on every later turn, so one that is not smaller than the turns
        // it replaces makes every following turn more expensive, not less.
        let summary_tokens = self.estimator.estimate_text(&summary);
        if summary_tokens >= replaced_tokens {
            tracing::warn!(
                summary_tokens,
                replaced_tokens,
                "summary for {context_id} is no shorter than what it replaces; keeping the \
                 transcript"
            );
            return;
        }

        let Ok(id) = context_id.parse::<ContextId>() else {
            return;
        };
        // `older`, not `conversation`: the watermark has to be the last message
        // actually summarized, or the recent turns are hidden behind a digest
        // that does not describe them.
        if let Err(e) = self
            .conversations
            .compact_through(&id, caller, &older, summary, self.model.clone())
            .await
        {
            tracing::warn!("recording the summary for {context_id} failed: {e}");
        }
    }

    /// Compact at most once per turn, and only when there is history to compact.
    ///
    /// A second pass would summarize a summary, and a request that still does not
    /// fit after one is not going to fit after two. `done` carries that across
    /// the tool rounds of one turn.
    async fn compact_once(
        &self,
        done: &mut bool,
        llm: &dyn LlmProvider,
        task_id: &str,
        context_id: &str,
        caller: Option<&str>,
        conversation: &Conversation,
    ) {
        if *done || !self.context.mode.reads_history() {
            return;
        }
        *done = true;
        self.compact_conversation(llm, task_id, context_id, caller, conversation)
            .await;
    }

    async fn run_with_llm(
        &self,
        llm: &dyn LlmProvider,
        task_id: &str,
        context_id: &str,
        caller: Option<&str>,
        user_text: &str,
        recalled: Recalled,
    ) -> Result<Answer, A2AError> {
        use futures::StreamExt;

        let Recalled {
            conversation,
            state: remembered,
        } = recalled;

        // The sources for this turn: the handler's own, plus the two memory
        // tools when the agent keeps state. Built here rather than held on the
        // handler because they are bound to this context and this caller, which
        // is what stops a write landing in another conversation.
        let mut sources: Vec<Arc<dyn ToolSource>> = self.tools.as_ref().clone();
        let state_block = if self.context.remember {
            // First, so the built-ins win `resolve` against a tool server that
            // happens to advertise the same name.
            sources.insert(
                0,
                Arc::new(MemoryToolSource::new(
                    self.state.clone(),
                    context_id.parse()?,
                    caller.map(str::to_string),
                    self.context.max_state_chars,
                )),
            );
            render_state(&remembered, self.context.max_state_chars)
        } else {
            None
        };

        let tools: Vec<ToolDefinition> = tools::collect_tool_defs(&sources);
        let turns = turns_from(&conversation);
        let mut messages = project(Prompt {
            system: &self.system_prompt,
            state: state_block.as_deref(),
            summary: conversation.summary(),
            turns: &turns,
            current: user_text,
        });

        // Tool schemas ride on every request and are easy to leave out of a
        // budget. Charged once here rather than per round, since they do not
        // change between rounds.
        let tool_tokens = self.estimator.estimate_tools(&tools);
        let mut budget = self.budget;
        budget.max_input_tokens = budget.max_input_tokens.saturating_sub(tool_tokens);
        // What the model said on its way through the tool rounds. Kept outside
        // the loop because it is the only thing left to show if the budget runs
        // out before it commits to an answer.
        let mut said_along_the_way = String::new();

        let mut compacted = false;
        // `round` counts passes through the loop and `charged` counts the ones
        // that went on the task, which differ once the state bag is on. Both
        // exist because the artifact ids are keyed on the pass: two passes
        // sharing a suffix would stream into one artifact.
        let mut round = 0u32;
        let mut charged = 0u32;
        let mut memory_rounds = 0u32;
        while charged < self.max_tool_rounds {
            // Trim before every round, not just the first: tool results are what
            // grows, and they arrive between rounds.
            match fit(&mut messages, &budget, self.estimator.as_ref()) {
                Ok(Fit::AsIs) => {}
                Ok(Fit::Trimmed) => {
                    tracing::debug!(round, "trimmed the request to fit the context budget");
                }
                // Under the ceiling, over the threshold: this request is fine
                // and the next one might not be.
                Ok(Fit::ShouldCompact) => {
                    self.compact_once(
                        &mut compacted,
                        llm,
                        task_id,
                        context_id,
                        caller,
                        &conversation,
                    )
                    .await;
                }
                Ok(Fit::OverBudget) => {
                    // Compaction writes a digest for the next turn; this request
                    // goes out as it is. Which makes it the one case worth
                    // saying out loud — the provider may refuse it, and the
                    // retry that follows drops every tool result to get under
                    // the window.
                    tracing::warn!(
                        round,
                        max_input_tokens = self.context.max_input_tokens,
                        "the request is over the context budget with nothing left to trim; \
                         sending it anyway"
                    );
                    self.compact_once(
                        &mut compacted,
                        llm,
                        task_id,
                        context_id,
                        caller,
                        &conversation,
                    )
                    .await;
                }
                Err(e) => return Err(A2AError::Internal(e.to_string())),
            }

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

            let mut stream = match llm.chat_completion_stream(request).await {
                Ok(stream) => stream,
                // The estimate was wrong and the model says so. Drop every tool
                // result to their smallest useful size and try once more; a
                // second refusal is a real failure.
                Err(LlmError::ContextLengthExceeded(detail)) => {
                    tracing::warn!(
                        round,
                        "the model rejected the request as too long: {detail}"
                    );
                    if !shrink_hard(&mut messages) {
                        return Err(A2AError::Internal(format!(
                            "the request exceeds the model's context window and there is nothing \
                             left to drop — lower `[handler.llm.context] max_input_tokens` to \
                             match the model, or shorten the system prompt ({detail})"
                        )));
                    }
                    let mut retry = LlmRequest::new(messages.clone()).temperature(0.2);
                    if !tools.is_empty() {
                        retry = retry.tools(tools.clone());
                    }
                    llm.chat_completion_stream(retry)
                        .await
                        .map_err(|e| A2AError::Internal(format!("LLM error: {e}")))?
                }
                Err(e) => return Err(A2AError::Internal(format!("LLM error: {e}"))),
            };

            let estimated = {
                let refs: Vec<&ChatMessage> = messages.iter().collect();
                self.estimator.estimate_messages(&refs) + tool_tokens
            };
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
                    LlmStreamEvent::Usage(usage) => {
                        // What it actually cost, against what was estimated.
                        tracing::info!(
                            round,
                            estimated_prompt_tokens = estimated,
                            "llm usage: {usage}"
                        );
                        // And once the gap is wide enough to change what this
                        // agent sends, the ratio that closes it. Nothing else in
                        // the loop can tell an over-eager compaction from a
                        // window the estimate never reached.
                        if let Some(drift) = self
                            .drift
                            .record(estimated, usage.prompt_tokens.unwrap_or(0))
                        {
                            tracing::warn!(
                                observed_ratio = drift.ratio,
                                samples = drift.samples,
                                "the token estimate is off by more than a third against what \
                                 {} charges — set `[handler.llm.context] chars_per_token = {:.1}`",
                                self.model,
                                drift.chars_per_token
                            );
                        }
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
                let source = tools::resolve(&sources, &call.name).ok_or_else(|| {
                    A2AError::Internal(format!("model called unknown tool '{}'", call.name))
                })?;
                let result = source.invoke(task_id, call).await?;
                // The progress artifact carries the whole result; only the copy
                // going back to the model is capped. A caller watching the
                // stream sees what the tool actually said.
                self.stream_progress(task_id, context_id, &format!("{} -> {result}", call.name))
                    .await;
                let for_model =
                    cap_tool_result(&result, self.context.max_tool_result_chars).unwrap_or(result);
                messages.push(ChatMessage::tool_result(
                    call.id.clone(),
                    call.name.clone(),
                    for_model,
                ));
            }

            // A response that only wrote to the state bag did not advance the
            // task, so it is not charged — up to the allowance, past which the
            // model is looping on the bag rather than using it. `calls` is
            // non-empty here; the answer path returned above.
            if self.context.remember && calls.iter().all(|call| is_memory_tool(&call.name)) {
                memory_rounds += 1;
                if memory_rounds > FREE_MEMORY_ROUNDS {
                    charged += 1;
                }
            } else {
                charged += 1;
            }
            round += 1;
        }
        // The budget ran out with the model still calling tools. That is a task
        // that did not get done, so it settles `Failed` — and it keeps whatever
        // the model said on the way, because an apology in place of the work is
        // strictly less than the work.
        let mut why = format!(
            "gave up after {} tool-call rounds without reaching an answer — raise `max_tool_rounds` if the work genuinely needs more",
            self.max_tool_rounds
        );
        // Where the rounds went, when some of them went on bookkeeping. Raising
        // `max_tool_rounds` is the wrong fix for a model stuck writing to the
        // bag, and the number is the only thing that says which case this is.
        if memory_rounds > 0 {
            why.push_str(&format!(
                "; {memory_rounds} responses went on `remember`/`forget`, {} of them charged \
                 against that budget",
                memory_rounds.saturating_sub(FREE_MEMORY_ROUNDS)
            ));
        }
        Ok(Answer::GaveUp {
            why,
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
        ctx: &RequestContext,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse()?;

        if !self.lifecycle.exists(&id).await? {
            let raw_context = if message.context_id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                message.context_id.clone()
            };
            let context: ContextId = raw_context.parse()?;
            self.lifecycle.create(&id, &context).await?;
        }
        let context_id = self.lifecycle.get(&id, Some(1)).await?.context_id.clone();

        // Owned because the answer is produced on a spawned task, which outlives
        // this borrow of the request.
        let caller = ctx.caller().map(str::to_string);

        // Loaded *before* this message is recorded. `update_and_broadcast` puts
        // it on the conversation log, so a load afterwards returns it as history
        // too and the model is handed the same question twice — once as the last
        // turn and once as the thing to answer.
        let conversation = self
            .load_conversation(&id, &context_id, caller.as_deref())
            .await?;

        // Read here rather than in the run, and for the same reason the
        // conversation is: refusing a context the caller does not own has to
        // reach the transport as a refusal (403), not settle a task as failed
        // and record the refusal in the conversation as something the agent
        // said. With `mode = "none"` this is the only read that checks.
        let recalled = Recalled {
            conversation,
            state: if self.context.remember {
                let context: ContextId = context_id.parse()?;
                self.load_state(&context, caller.as_deref()).await?
            } else {
                ContextState::new()
            },
        };

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
                        .run_with_llm(
                            llm.as_ref(),
                            &task_id,
                            &context_id,
                            caller.as_deref(),
                            &user_text,
                            recalled,
                        )
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

/// Messages per turn — a question and its answer.
const MESSAGES_PER_TURN: usize = 2;

/// How many recent windows a load reaches back over.
///
/// Strictly more than one, or the load would return exactly the window
/// compaction is forbidden to touch and there would never be anything to
/// summarize. In steady state the digest watermark bounds the read long before
/// this does; the limit is the backstop for a conversation that has not
/// compacted yet.
const LOAD_WINDOWS: usize = 4;

/// How many messages to read back for a prompt.
///
/// Bounded because loading an unbounded conversation only to drop most of it is
/// the one cost trimming cannot recover.
fn load_limit(keep_recent_turns: usize) -> u32 {
    keep_recent_turns
        .saturating_mul(MESSAGES_PER_TURN)
        .saturating_mul(LOAD_WINDOWS)
        .max(MESSAGES_PER_TURN)
        .try_into()
        .unwrap_or(u32::MAX)
}

/// The part of `conversation` that compaction may fold: everything before the
/// last `keep_recent` turns.
///
/// `None` when there is nothing to summarize — a conversation shorter than the
/// recent window, where compacting would replace turns the next prompt is
/// supposed to carry verbatim.
///
/// A turn is a pair of messages (the question and the answer), so the window is
/// counted in messages as twice that.
fn older_than_recent(conversation: &Conversation, keep_recent: usize) -> Option<Conversation> {
    let keep = keep_recent.saturating_mul(MESSAGES_PER_TURN);
    if conversation.tail.len() <= keep {
        return None;
    }
    let older = conversation.tail[..conversation.tail.len() - keep].to_vec();
    Some(Conversation {
        digest: conversation.digest.clone(),
        tail: older,
    })
}

/// How much of a tool result survives the last-resort shrink. Small enough to
/// make a real difference against a model that just refused the request, large
/// enough that an error message or a short answer still comes through whole.
const HARD_TOOL_RESULT_CHARS: usize = 500;

/// Cut every tool result down to [`HARD_TOOL_RESULT_CHARS`], reporting whether
/// anything changed.
///
/// The backstop for a model that rejects a request the estimator thought would
/// fit. `false` means there was nothing left to give up, and the caller should
/// say so rather than retry the identical request.
fn shrink_hard(messages: &mut [ChatMessage]) -> bool {
    let mut changed = false;
    for message in messages.iter_mut() {
        if !matches!(message.role, MessageRole::Tool) {
            continue;
        }
        if let Some(content) = message.content.as_ref()
            && let Some(capped) = cap_tool_result(content, HARD_TOOL_RESULT_CHARS)
        {
            message.content = Some(capped);
            changed = true;
        }
    }
    changed
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
    use a2a_llm::{LlmError, LlmResponse, ToolCall};
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

        fn label(&self) -> String {
            "the lookup tool".to_string()
        }

        async fn invoke(&self, _task_id: &str, _call: &ToolCall) -> Result<String, A2AError> {
            Ok("a fact".to_string())
        }
    }

    /// A tool server that happens to advertise `remember`. Both built-in names
    /// are ordinary enough words for one to use, which is why the memory source
    /// is inserted ahead of the handler's own sources.
    struct NamesakeTool;

    #[async_trait]
    impl ToolSource for NamesakeTool {
        fn tool_defs(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "remember".to_string(),
                description: "someone else's remember".to_string(),
                parameters: serde_json::json!({ "type": "object", "properties": {} }),
            }]
        }

        fn has_tool(&self, name: &str) -> bool {
            name == "remember"
        }

        fn label(&self) -> String {
            "a tool server".to_string()
        }

        async fn invoke(&self, _task_id: &str, _call: &ToolCall) -> Result<String, A2AError> {
            Ok("noted".to_string())
        }
    }

    /// A tool whose result is far too long for any budget, so a test can drive
    /// the trimming path without an MCP server that happens to be chatty.
    struct VerboseTool;

    #[async_trait]
    impl ToolSource for VerboseTool {
        fn tool_defs(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "dump_everything".to_string(),
                description: "returns far too much".to_string(),
                parameters: serde_json::json!({ "type": "object", "properties": {} }),
            }]
        }

        fn has_tool(&self, name: &str) -> bool {
            name == "dump_everything"
        }

        fn label(&self) -> String {
            "the verbose tool".to_string()
        }

        async fn invoke(&self, _task_id: &str, _call: &ToolCall) -> Result<String, A2AError> {
            Ok(format!("HEAD{}TAIL", "x".repeat(200_000)))
        }
    }

    /// Replays a fixed event sequence, so a test can pin what the handler does
    /// with a *shape* of response rather than with a particular model's mood.
    ///
    /// Records the requests it was given, because for context management the
    /// thing under test *is* what reached the model.
    #[derive(Clone)]
    struct ScriptedProvider {
        /// One script per request; the last one repeats. Lets a test give the
        /// model a tool call first and an answer after, which is what the
        /// interesting paths need.
        scripts: Vec<Vec<LlmStreamEvent>>,
        seen: Arc<std::sync::Mutex<Vec<LlmRequest>>>,
        /// Which request index to refuse as over-long, if any.
        refuse_at: Option<usize>,
        /// What the non-streaming call answers with — the summary, since that
        /// is the only thing this handler asks for without streaming.
        summary: String,
    }

    impl ScriptedProvider {
        fn new(events: Vec<LlmStreamEvent>) -> Self {
            Self::scripted(vec![events])
        }

        fn scripted(scripts: Vec<Vec<LlmStreamEvent>>) -> Self {
            Self {
                scripts,
                seen: Arc::new(std::sync::Mutex::new(Vec::new())),
                refuse_at: None,
                summary: "they talked about Bergen".to_string(),
            }
        }

        fn refusing_at(mut self, index: usize) -> Self {
            self.refuse_at = Some(index);
            self
        }

        /// A model that ignores `max_tokens` and hands back a "summary" the size
        /// of the transcript.
        fn summarizing_with(mut self, summary: impl Into<String>) -> Self {
            self.summary = summary.into();
            self
        }

        fn requests(&self) -> Vec<LlmRequest> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn chat_completion(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
            self.seen.lock().unwrap().push(request);
            Ok(LlmResponse {
                content: Some(self.summary.clone()),
                tool_calls: None,
                reasoning: None,
                usage: None,
            })
        }

        async fn chat_completion_stream(
            &self,
            request: LlmRequest,
        ) -> Result<BoxStream<'static, Result<LlmStreamEvent, LlmError>>, LlmError> {
            let index = {
                let mut seen = self.seen.lock().unwrap();
                seen.push(request);
                seen.len() - 1
            };
            if self.refuse_at == Some(index) {
                return Err(LlmError::ContextLengthExceeded(
                    "maximum context length is 8192 tokens".to_string(),
                ));
            }
            let script = self.scripts[index.min(self.scripts.len() - 1)].clone();
            let events: Vec<_> = script.into_iter().map(Ok).collect();
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
        handler_from(
            ScriptedProvider::new(events),
            tools,
            ContextConfig::default(),
        )
        .0
    }

    /// A handler over one shared `InMemoryTaskStorage`, which is both its task
    /// store and its conversation store — the same wiring `a2a run` uses, and
    /// the reason "the conversation" and "the task history" cannot disagree.
    fn handler_from(
        provider: ScriptedProvider,
        tools: Vec<Arc<dyn ToolSource>>,
        context: ContextConfig,
    ) -> (LlmHandler, ScriptedProvider) {
        let storage = InMemoryTaskStorage::new();
        let handler = LlmHandler::builder()
            .system_prompt("test".to_string())
            .max_tool_rounds(2)
            .lifecycle(storage.clone())
            .streaming(InMemoryStreamingHandler::new())
            .push(Arc::new(NoopPushNotifier))
            .tools(tools)
            .llm(Arc::new(provider.clone()) as Arc<dyn LlmProvider>)
            .build()
            .with_context_memory(Arc::new(storage.clone()), Arc::new(storage), context);
        (handler, provider)
    }

    /// Every user/assistant message that reached the model on request `index`.
    fn sent_texts(provider: &ScriptedProvider, index: usize) -> Vec<String> {
        provider.requests()[index]
            .messages
            .iter()
            .filter(|m| matches!(m.role, MessageRole::User | MessageRole::Assistant))
            .filter_map(|m| m.content.clone())
            .collect()
    }

    async fn say(handler: &LlmHandler, task_id: &str, context_id: &str, text: &str) -> Task {
        say_as(handler, task_id, context_id, text, None)
            .await
            .unwrap()
    }

    /// [`say`], as a named principal — the shape an authenticated agent sees.
    ///
    /// Returns the error rather than unwrapping, because a refusal is one of the
    /// outcomes under test.
    async fn say_as(
        handler: &LlmHandler,
        task_id: &str,
        context_id: &str,
        text: &str,
        caller: Option<&str>,
    ) -> Result<Task, A2AError> {
        let id: TaskId = task_id.parse().unwrap();
        let message = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text(text.to_string())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .context_id(context_id.to_string())
            .build();
        let ctx = RequestContext::anonymous()
            .with_session(context_id)
            .with_principal(
                caller
                    .map(|id| a2a_rs::port::AuthPrincipal::new(id.to_string(), "test".to_string())),
            );
        handler.process_message(task_id, &message, &ctx).await?;
        for _ in 0..200 {
            let task = handler.lifecycle.get(&id, None).await.unwrap();
            if state_of(&task).is_terminal() {
                return Ok(task);
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("task never settled");
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
            .process_message(task_id, &message, &RequestContext::anonymous())
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

    fn context_config(mode: ContextMode) -> ContextConfig {
        ContextConfig {
            mode,
            ..ContextConfig::default()
        }
    }

    /// The default, and what every existing agent keeps doing: each message is
    /// answered on its own, with no memory of the last one.
    #[tokio::test]
    async fn without_context_memory_each_turn_starts_fresh() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("ok".to_string())]),
            Vec::new(),
            context_config(ContextMode::None),
        );

        say(
            &handler,
            "11111111-1111-1111-1111-111111111111",
            "ctx",
            "first",
        )
        .await;
        say(
            &handler,
            "22222222-2222-2222-2222-222222222222",
            "ctx",
            "second",
        )
        .await;

        assert_eq!(
            sent_texts(&provider, 1),
            vec!["second"],
            "the second turn must not see the first"
        );
    }

    /// With `mode = "context"`, a second task in the same context is answered
    /// with what was already said — the whole point of the feature.
    #[tokio::test]
    async fn a_second_turn_in_one_context_sees_the_first() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("Oslo".to_string())]),
            Vec::new(),
            context_config(ContextMode::Context),
        );

        say(
            &handler,
            "11111111-1111-1111-1111-111111111111",
            "ctx",
            "what is the capital",
        )
        .await;
        say(
            &handler,
            "22222222-2222-2222-2222-222222222222",
            "ctx",
            "and the population",
        )
        .await;

        let second = sent_texts(&provider, 1);
        assert!(
            second.contains(&"what is the capital".to_string()),
            "the earlier question must carry across: {second:?}"
        );
        assert!(
            second.contains(&"Oslo".to_string()),
            "the earlier answer must carry across: {second:?}"
        );
        assert_eq!(
            second.last().map(String::as_str),
            Some("and the population")
        );
    }

    /// Conversations are keyed by context. A different context is a different
    /// conversation, even on the same agent.
    #[tokio::test]
    async fn a_different_context_is_a_different_conversation() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("ok".to_string())]),
            Vec::new(),
            context_config(ContextMode::Context),
        );

        say(
            &handler,
            "11111111-1111-1111-1111-111111111111",
            "one",
            "secret",
        )
        .await;
        say(
            &handler,
            "22222222-2222-2222-2222-222222222222",
            "two",
            "hei",
        )
        .await;

        assert_eq!(
            sent_texts(&provider, 1),
            vec!["hei"],
            "nothing from context 'one' may appear in context 'two'"
        );
    }

    /// A `contextId` is supplied by the caller, so on an authenticated agent it
    /// decides which conversation you get. The store claims the context for
    /// whoever first read it; this is the end of that check that only works once
    /// the principal reaches the handler.
    #[tokio::test]
    async fn a_second_caller_is_refused_the_first_ones_conversation() {
        let (handler, _) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("ok".to_string())]),
            Vec::new(),
            context_config(ContextMode::Context),
        );

        say_as(
            &handler,
            "11111111-1111-1111-1111-111111111111",
            "shared",
            "my bank balance is 12",
            Some("alice"),
        )
        .await
        .expect("the first caller claims the context");

        let denied = say_as(
            &handler,
            "22222222-2222-2222-2222-222222222222",
            "shared",
            "what did I just say",
            Some("bob"),
        )
        .await
        .expect_err("a context owned by alice must not answer bob");

        assert!(
            matches!(denied, A2AError::ContextAccessDenied { .. }),
            "expected a refusal, got {denied:?}"
        );
    }

    /// The same caller coming back is the ordinary case and must still work —
    /// the check is on identity, not on having been seen before.
    #[tokio::test]
    async fn the_same_caller_keeps_reading_their_own_conversation() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("Oslo".to_string())]),
            Vec::new(),
            context_config(ContextMode::Context),
        );

        for (task_id, text) in [
            (
                "11111111-1111-1111-1111-111111111111",
                "what is the capital",
            ),
            ("22222222-2222-2222-2222-222222222222", "and the population"),
        ] {
            say_as(&handler, task_id, "alices-ctx", text, Some("alice"))
                .await
                .expect("alice owns this context");
        }

        let second = sent_texts(&provider, 1);
        assert!(
            second.contains(&"what is the capital".to_string()),
            "alice's own history must carry across: {second:?}"
        );
    }

    /// A tool that returns 200k characters used to go to the model whole. The
    /// caller still sees all of it on the progress stream; only the model's copy
    /// is capped.
    #[tokio::test]
    async fn a_huge_tool_result_is_capped_before_it_reaches_the_model() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(always_calls("dump_everything", None)),
            vec![Arc::new(VerboseTool) as Arc<dyn ToolSource>],
            context_config(ContextMode::None),
        );

        say(
            &handler,
            "11111111-1111-1111-1111-111111111111",
            "ctx",
            "dump it",
        )
        .await;

        // The second request is the one carrying the first round's tool result.
        let tool_message = provider.requests()[1]
            .messages
            .iter()
            .find(|m| matches!(m.role, MessageRole::Tool))
            .and_then(|m| m.content.clone())
            .expect("a tool result reached the model");

        assert!(
            tool_message.chars().count() < 20_000,
            "200k characters must not reach the model, got {}",
            tool_message.chars().count()
        );
        assert!(tool_message.starts_with("HEAD"), "the head survives");
        assert!(tool_message.ends_with("TAIL"), "the tail survives");
        assert!(tool_message.contains("elided"), "the cut is marked");
    }

    /// A model that rejects the request as too long gets one more chance with
    /// tool results cut to the bone. This used to arrive as
    /// `A2AError::Internal("LLM error: API error (400) ...")` and simply fail.
    ///
    /// The refusal is aimed at the *second* request, the one carrying a tool
    /// result: that is the only point where there is anything left to shrink.
    #[tokio::test]
    async fn a_context_length_refusal_is_retried_smaller() {
        let (handler, provider) = handler_from(
            ScriptedProvider::scripted(vec![
                always_calls("dump_everything", None),
                vec![LlmStreamEvent::ContentChunk("fits now".to_string())],
            ])
            .refusing_at(1),
            vec![Arc::new(VerboseTool) as Arc<dyn ToolSource>],
            context_config(ContextMode::None),
        );

        let task = say(
            &handler,
            "11111111-1111-1111-1111-111111111111",
            "ctx",
            "go",
        )
        .await;

        assert_eq!(state_of(&task), TaskState::Completed);
        assert_eq!(reply_text(&task), "fits now");
        assert_eq!(
            provider.requests().len(),
            3,
            "one refused request plus exactly one retry"
        );

        let retried = provider.requests()[2]
            .messages
            .iter()
            .find(|m| matches!(m.role, MessageRole::Tool))
            .and_then(|m| m.content.clone())
            .expect("the retry still carries the tool result");
        assert!(
            retried.chars().count() <= 600,
            "the retry has to be materially smaller, got {}",
            retried.chars().count()
        );
    }

    /// ...and when the very first request is refused there are no tool results
    /// to give up, so retrying would send the identical request. It fails
    /// instead, naming the setting that fixes it.
    #[tokio::test]
    async fn a_refusal_with_nothing_left_to_drop_fails_and_names_the_knob() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("never sent".to_string())])
                .refusing_at(0),
            Vec::new(),
            context_config(ContextMode::None),
        );

        let task = say(
            &handler,
            "11111111-1111-1111-1111-111111111111",
            "ctx",
            "go",
        )
        .await;

        assert_eq!(state_of(&task), TaskState::Failed);
        let reply = reply_text(&task);
        assert!(reply.contains("max_input_tokens"), "{reply}");
        assert_eq!(
            provider.requests().len(),
            1,
            "an identical retry is not worth a round trip"
        );
    }

    /// `mode = "task"` carries one task's own messages and nothing from the
    /// sibling tasks that share its context.
    #[tokio::test]
    async fn task_mode_does_not_reach_across_tasks() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("ok".to_string())]),
            Vec::new(),
            context_config(ContextMode::Task),
        );

        say(
            &handler,
            "11111111-1111-1111-1111-111111111111",
            "ctx",
            "first",
        )
        .await;
        say(
            &handler,
            "22222222-2222-2222-2222-222222222222",
            "ctx",
            "second",
        )
        .await;

        assert!(
            !sent_texts(&provider, 1).contains(&"first".to_string()),
            "task mode must not see a sibling task: {:?}",
            sent_texts(&provider, 1)
        );
    }

    /// A budget small enough that a few short turns trip compaction; in
    /// production the same path runs at 80% of a real window.
    fn compacting_context() -> ContextConfig {
        ContextConfig {
            mode: ContextMode::Context,
            // Small, but well clear of the summary prompt itself: `fit` refuses
            // a budget that cannot hold the request it is being asked to make.
            max_input_tokens: 400,
            reserve_for_output: 0,
            compact_at_percent: 50,
            // One turn kept verbatim, so the second turn already has something
            // older to fold. The default of 4 would need nine turns.
            keep_recent_turns: 1,
            ..ContextConfig::default()
        }
    }

    /// Four turns of one context. The fourth is there because a turn compacts
    /// the conversation it *loaded*: a digest written during turn three first
    /// reaches a prompt on turn four.
    async fn four_turns(handler: &LlmHandler) {
        for (task, text) in [
            ("11111111-1111-1111-1111-111111111111", "first question"),
            ("22222222-2222-2222-2222-222222222222", "second question"),
            ("33333333-3333-3333-3333-333333333333", "third question"),
            ("44444444-4444-4444-4444-444444444444", "fourth question"),
        ] {
            say(handler, task, "ctx", text).await;
        }
    }

    /// `chat_completion` (non-streaming) is only used for summarizing, so a
    /// recorded non-streaming request is what proves compaction ran.
    fn summary_requests(provider: &ScriptedProvider) -> Vec<LlmRequest> {
        provider
            .requests()
            .into_iter()
            .filter(|request| {
                request.messages.iter().any(|m| {
                    m.content
                        .as_deref()
                        .is_some_and(|c| c.contains("Summarize the conversation"))
                })
            })
            .collect()
    }

    /// A conversation that crosses the compaction threshold is summarized, and
    /// the next turn is handed the summary instead of the transcript.
    #[tokio::test]
    async fn a_long_conversation_is_summarized_and_the_summary_carries_forward() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("x".repeat(1000))]),
            Vec::new(),
            compacting_context(),
        );

        four_turns(&handler).await;

        assert!(
            !summary_requests(&provider).is_empty(),
            "the conversation should have been compacted"
        );

        // And the digest reaches the next prompt as a system message.
        let requests = provider.requests();
        let last = requests.last().unwrap();
        let has_summary_block = last.messages.iter().any(|m| {
            matches!(m.role, MessageRole::System)
                && m.content
                    .as_deref()
                    .is_some_and(|c| c.contains("Summary of the earlier part"))
        });
        assert!(
            has_summary_block,
            "the summary must be carried into the next turn: {:?}",
            last.messages
                .iter()
                .map(|m| (&m.role, m.content.as_deref().map(|c| &c[..c.len().min(60)])))
                .collect::<Vec<_>>()
        );
    }

    /// Without a ceiling a verbose model can return a "summary" about as long
    /// as the transcript it replaces, and the digest is re-sent every turn
    /// after.
    #[tokio::test]
    async fn a_summary_is_asked_for_with_a_ceiling() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("x".repeat(1000))]),
            Vec::new(),
            compacting_context(),
        );

        four_turns(&handler).await;

        let requests = summary_requests(&provider);
        assert!(!requests.is_empty(), "nothing was summarized");
        assert!(
            requests.iter().all(|request| request.max_tokens.is_some()),
            "every summary request must carry a max_tokens"
        );
    }

    /// The backstop for a provider that ignores `max_tokens`: a digest that is
    /// no smaller than the turns it stands in for makes every later turn more
    /// expensive rather than less, so it is thrown away and the transcript kept.
    #[tokio::test]
    async fn a_summary_no_shorter_than_the_transcript_is_discarded() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("x".repeat(1000))])
                .summarizing_with("y".repeat(100_000)),
            Vec::new(),
            compacting_context(),
        );

        four_turns(&handler).await;

        assert!(
            !summary_requests(&provider).is_empty(),
            "compaction should still have been attempted"
        );
        let requests = provider.requests();
        let carried_a_digest = requests.iter().any(|request| {
            request.messages.iter().any(|m| {
                matches!(m.role, MessageRole::System)
                    && m.content
                        .as_deref()
                        .is_some_and(|c| c.contains("Summary of the earlier part"))
            })
        });
        assert!(
            !carried_a_digest,
            "a summary that saves nothing must not reach a later prompt"
        );
    }

    /// The recent window is kept verbatim; only what precedes it is summarized.
    /// Folding the whole tail would leave the next turn nothing word for word,
    /// which is the opposite of what `keep_recent_turns` promises.
    #[test]
    fn compaction_leaves_the_recent_window_alone() {
        use a2a_rs::domain::{Seq, SequencedMessage};

        let conversation = Conversation {
            digest: None,
            tail: (1..=6)
                .map(|seq| SequencedMessage {
                    seq: Seq::new(seq),
                    message: Message::default(),
                })
                .collect(),
        };

        // Two turns kept is four messages, leaving the first two foldable.
        let older = older_than_recent(&conversation, 2).expect("six messages, two foldable");
        assert_eq!(older.tail.len(), 2);
        assert_eq!(
            older.tail.last().unwrap().seq,
            Seq::new(2),
            "the watermark must be the last message actually summarized"
        );

        // …and a conversation no longer than the window has nothing to fold.
        assert!(older_than_recent(&conversation, 3).is_none());
        assert!(older_than_recent(&Conversation::default(), 1).is_none());
    }

    /// The load has to reach further back than the window compaction may not
    /// touch, or there would never be anything to summarize.
    #[test]
    fn a_load_reaches_past_the_window_it_may_not_compact() {
        for keep in [1usize, 4, 100] {
            assert!(
                load_limit(keep) as usize > keep * MESSAGES_PER_TURN,
                "load_limit({keep}) must exceed the protected window"
            );
        }
        assert!(load_limit(0) >= MESSAGES_PER_TURN as u32);
    }

    /// A model that only ever calls `name`, so the loop always runs out of
    /// rounds and every round appends a tool result.
    fn always_calls(name: &str, said: Option<&str>) -> Vec<LlmStreamEvent> {
        let mut events: Vec<_> = said
            .map(|s| LlmStreamEvent::ContentChunk(s.to_string()))
            .into_iter()
            .collect();
        events.push(LlmStreamEvent::ToolCall(ToolCall {
            id: "call-1".to_string(),
            name: name.to_string(),
            arguments: "{}".to_string(),
        }));
        events
    }

    // --- the state bag -------------------------------------------------------

    fn remembering_context() -> ContextConfig {
        ContextConfig {
            remember: true,
            ..ContextConfig::default()
        }
    }

    /// One tool call with arguments, then an answer — the two requests a turn
    /// that writes to memory produces.
    fn calls_then_answers(name: &str, arguments: serde_json::Value) -> Vec<Vec<LlmStreamEvent>> {
        vec![
            vec![LlmStreamEvent::ToolCall(ToolCall {
                id: "call-1".to_string(),
                name: name.to_string(),
                arguments: arguments.to_string(),
            })],
            vec![LlmStreamEvent::ContentChunk("noted".to_string())],
        ]
    }

    /// Every system message that reached the model on request `index`. The state
    /// block is one, which is why `sent_texts` cannot see it.
    fn sent_system(provider: &ScriptedProvider, index: usize) -> Vec<String> {
        provider.requests()[index]
            .messages
            .iter()
            .filter(|m| matches!(m.role, MessageRole::System))
            .filter_map(|m| m.content.clone())
            .collect()
    }

    fn tool_names(provider: &ScriptedProvider, index: usize) -> Vec<String> {
        provider.requests()[index]
            .tools
            .iter()
            .flatten()
            .map(|t| t.name.clone())
            .collect()
    }

    /// The point of the whole feature: a fact written on one turn is in the
    /// prompt on the next, without having been said again.
    #[tokio::test]
    async fn a_remembered_fact_reaches_the_next_turns_prompt() {
        let (handler, provider) = handler_from(
            ScriptedProvider::scripted(calls_then_answers(
                "remember",
                serde_json::json!({"key": "project", "value": "a2a-rs"}),
            )),
            Vec::new(),
            remembering_context(),
        );

        say(&handler, "task-1", "ctx-1", "I work on a2a-rs").await;
        // A second task in the same context: a new turn, nothing carried in
        // process.
        say(&handler, "task-2", "ctx-1", "what am I working on").await;

        let system = sent_system(&provider, 2).join("\n");
        assert!(system.contains("project = a2a-rs"), "{system}");
    }

    /// The tools are advertised only when the agent keeps state, so an existing
    /// agent's prompt and tool list are unchanged by this shipping.
    #[tokio::test]
    async fn the_memory_tools_appear_only_when_the_agent_remembers() {
        let (off, off_provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("ok".to_string())]),
            Vec::new(),
            ContextConfig::default(),
        );
        say(&off, "task-1", "ctx-1", "hei").await;
        assert!(tool_names(&off_provider, 0).is_empty());
        assert!(
            sent_system(&off_provider, 0) == vec!["test".to_string()],
            "{:?}",
            sent_system(&off_provider, 0)
        );

        let (on, on_provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("ok".to_string())]),
            Vec::new(),
            remembering_context(),
        );
        say(&on, "task-1", "ctx-1", "hei").await;
        assert_eq!(tool_names(&on_provider, 0), ["remember", "forget"]);
        // And nothing remembered yet means no block, rather than an empty one.
        assert_eq!(sent_system(&on_provider, 0), vec!["test".to_string()]);
    }

    /// What `user:` buys over a bare key: it is filed under the principal, so a
    /// conversation that has never seen the fact still gets it.
    #[tokio::test]
    async fn a_user_scoped_fact_crosses_into_another_conversation() {
        let (handler, provider) = handler_from(
            ScriptedProvider::scripted(calls_then_answers(
                "remember",
                serde_json::json!({"key": "user:tone", "value": "brief"}),
            )),
            Vec::new(),
            remembering_context(),
        );

        say_as(&handler, "task-1", "ctx-1", "be brief", Some("alice"))
            .await
            .unwrap();
        say_as(&handler, "task-2", "ctx-2", "hei", Some("alice"))
            .await
            .unwrap();

        let system = sent_system(&provider, 2).join("\n");
        assert!(system.contains("user:tone = brief"), "{system}");
    }

    /// The other half of that: it is the *principal's*, so another caller's
    /// conversation does not read it.
    #[tokio::test]
    async fn a_user_scoped_fact_belongs_to_the_caller_it_was_written_for() {
        let (handler, provider) = handler_from(
            ScriptedProvider::scripted(calls_then_answers(
                "remember",
                serde_json::json!({"key": "user:tone", "value": "brief"}),
            )),
            Vec::new(),
            remembering_context(),
        );

        say_as(&handler, "task-1", "ctx-1", "be brief", Some("alice"))
            .await
            .unwrap();
        say_as(&handler, "task-2", "ctx-2", "hei", Some("bob"))
            .await
            .unwrap();

        let system = sent_system(&provider, 2).join("\n");
        assert!(!system.contains("brief"), "{system}");
    }

    /// A context belongs to whoever started it, and the state bag is behind the
    /// same check as the transcript — reached here with `mode = "none"`, where
    /// nothing else would have consulted the store.
    #[tokio::test]
    async fn another_caller_is_refused_the_state_of_a_context_they_do_not_own() {
        let (handler, _) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ContentChunk("ok".to_string())]),
            Vec::new(),
            remembering_context(),
        );

        say_as(&handler, "task-1", "ctx-1", "hei", Some("alice"))
            .await
            .unwrap();
        let refused = say_as(&handler, "task-2", "ctx-1", "hei", Some("bob")).await;
        assert!(
            matches!(refused, Err(A2AError::ContextAccessDenied { .. })),
            "{refused:?}"
        );
    }

    /// A `forget` has to reach the same store the block was rendered from, or
    /// the model would keep being shown something it has dropped.
    #[tokio::test]
    async fn a_forgotten_fact_leaves_the_prompt() {
        let mut scripts = calls_then_answers(
            "remember",
            serde_json::json!({"key": "project", "value": "a2a-rs"}),
        );
        scripts.extend(calls_then_answers(
            "forget",
            serde_json::json!({"key": "project"}),
        ));
        let (handler, provider) = handler_from(
            ScriptedProvider::scripted(scripts),
            Vec::new(),
            remembering_context(),
        );

        say(&handler, "task-1", "ctx-1", "I work on a2a-rs").await;
        // Requests 2 and 3: the forget call and the answer after it.
        say(&handler, "task-2", "ctx-1", "forget that").await;
        assert!(sent_system(&provider, 2).join("\n").contains("project"));

        say(&handler, "task-3", "ctx-1", "what am I working on").await;
        let system = sent_system(&provider, 4).join("\n");
        assert!(!system.contains("project"), "{system}");
    }

    /// One model response per name, then an answer. The arguments suit
    /// `remember` and are ignored by everything else.
    fn calls_in_turn(names: &[&str]) -> Vec<Vec<LlmStreamEvent>> {
        let mut scripts: Vec<Vec<LlmStreamEvent>> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                vec![LlmStreamEvent::ToolCall(ToolCall {
                    id: format!("call-{i}"),
                    name: name.to_string(),
                    arguments: serde_json::json!({"key": "project", "value": "a2a-rs"}).to_string(),
                })]
            })
            .collect();
        scripts.push(vec![LlmStreamEvent::ContentChunk("42".to_string())]);
        scripts
    }

    /// A tool round is a model response, so writing to the bag came out of the
    /// budget for the work: with `max_tool_rounds` at 2, remembering one fact
    /// and looking one up left nothing to answer with.
    #[tokio::test]
    async fn remembering_something_does_not_spend_a_tool_round() {
        let (handler, _) = handler_from(
            ScriptedProvider::scripted(calls_in_turn(&["remember", "look_it_up"])),
            vec![Arc::new(AlwaysCallableTool) as Arc<dyn ToolSource>],
            remembering_context(),
        );

        let task = say(&handler, "task-1", "ctx-1", "I work on a2a-rs").await;
        assert_eq!(state_of(&task), TaskState::Completed);
        assert_eq!(reply_text(&task), "42");
    }

    /// The other half: free is not unbounded, since nothing stops a model
    /// calling `remember` forever. Past the allowance the rounds are charged
    /// again, and the failure says where they went — raising `max_tool_rounds`
    /// is the wrong fix for a model stuck writing to the bag.
    #[tokio::test]
    async fn a_model_that_only_ever_remembers_still_gives_up() {
        let (handler, provider) = handler_from(
            ScriptedProvider::new(vec![LlmStreamEvent::ToolCall(ToolCall {
                id: "call-1".to_string(),
                name: "remember".to_string(),
                arguments: serde_json::json!({"key": "project", "value": "a2a-rs"}).to_string(),
            })]),
            Vec::new(),
            remembering_context(),
        );

        let task = say(&handler, "task-1", "ctx-1", "I work on a2a-rs").await;
        assert_eq!(state_of(&task), TaskState::Failed);
        // Two free responses, then the budget of two.
        assert_eq!(provider.requests().len(), 4);
        let reply = reply_text(&task);
        assert!(
            reply.contains("`remember`/`forget`"),
            "the failure has to say where the rounds went, got: {reply:?}"
        );
    }

    /// The exemption is for the handler's own memory tools, which exist only
    /// when the bag is on. With `remember = false` the same name belongs to
    /// whatever tool server advertises it, and calling one is work.
    #[tokio::test]
    async fn a_tool_named_remember_is_charged_when_the_bag_is_off() {
        let (handler, _) = handler_from(
            ScriptedProvider::scripted(calls_in_turn(&["remember", "remember"])),
            vec![Arc::new(NamesakeTool) as Arc<dyn ToolSource>],
            ContextConfig::default(),
        );

        let task = say(&handler, "task-1", "ctx-1", "hei").await;
        assert_eq!(state_of(&task), TaskState::Failed);
    }
}
