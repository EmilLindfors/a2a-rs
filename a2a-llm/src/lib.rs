//! Provider-neutral vocabulary for chat completions, plus the providers that
//! speak it.
//!
//! [`LlmProvider`] is the port: [`chat_completion`](LlmProvider::chat_completion)
//! and [`chat_completion_stream`](LlmProvider::chat_completion_stream) over
//! [`LlmRequest`] / [`LlmResponse`]. [`openai`] covers OpenAI and every
//! OpenAI-compatible endpoint (OpenRouter, vLLM, llama.cpp); [`gemini`] covers
//! Google's API. [`provider_from_env`] picks one from the environment.
//!
//! The types are deliberately not tied to A2A. [`ToolCall`], [`ToolDefinition`]
//! and [`ToolResult`] are the tool-calling vocabulary shared with the MCP
//! bridge, which is why they live in their own crate rather than inside an
//! agent framework.

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod gemini;
pub mod openai;
pub mod provider;
pub mod tool_call;

pub use provider::{
    LlmConfigError, LlmSettings, PROVIDER_ENV_VARS, ReasoningPlan, SUPPORTED_PROVIDERS,
    SelectedLlm, provider_from_env, provider_from_settings,
};
pub use tool_call::{PartialToolCall, ToolCallAccumulator};

/// The environment, as this crate reads it when building a provider.
///
/// Passed in rather than read directly so the selection rules can be tested
/// without mutating the process environment, which would race other tests.
#[derive(Clone, Copy)]
pub(crate) struct Env<'a>(&'a dyn Fn(&str) -> Option<String>);

impl<'a> Env<'a> {
    /// A stand-in environment. Only tests need one; production reads
    /// [`Env::os`].
    #[cfg(test)]
    pub(crate) fn new(lookup: &'a dyn Fn(&str) -> Option<String>) -> Self {
        Self(lookup)
    }

    /// The process environment.
    pub(crate) fn os() -> Env<'static> {
        const LOOKUP: &dyn Fn(&str) -> Option<String> = &os_lookup;
        Env(LOOKUP)
    }

    /// A variable set to whitespace reads as unset. `.env` files leave those
    /// behind, and an empty `OPENROUTER_API_KEY` would otherwise select a
    /// provider that cannot authenticate.
    pub(crate) fn get(&self, key: &str) -> Option<String> {
        (self.0)(key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

fn os_lookup(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Represents an error returned by an LLM provider.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("API error: {0}")]
    ApiError(String),
    /// The request was larger than the model's context window.
    ///
    /// Separate from [`LlmError::ApiError`] because it is the one API failure a
    /// caller can act on: drop history and try again. Folded into `ApiError` it
    /// reached the handler as `A2AError::Internal("LLM error: API error (400)
    /// …")` and simply failed the task.
    ///
    /// The numbers are carried where the provider's body names them, because
    /// they are the difference between an answer and a bound: a caller advising
    /// "set your ceiling below N" from its own token *estimate* hands out a
    /// number with the estimator's error on it, while `context_window` is the
    /// limit the provider actually enforced.
    #[error("context length exceeded: {detail}")]
    ContextLengthExceeded {
        /// The provider's message as received (label, status, body).
        detail: String,
        /// Prompt tokens as the provider counted them, where the body names
        /// them. The provider's count, never an estimate; `None` must not read
        /// as zero (the same rule as [`TokenUsage`]).
        prompt_tokens: Option<u32>,
        /// The context window the provider enforced, where the body names it.
        context_window: Option<u32>,
    },
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
}

/// An error and everything under it, as one line.
///
/// `reqwest::Error`'s `Display` omits its source chain, so a DNS failure, a
/// refused connection and an untrusted certificate all read as `error sending
/// request for url (…)` — which is what made a TLS-intercepting proxy
/// indistinguishable from the network being down, and cost a full investigation
/// (see `NOTES.md`). The certificate error was one `source()` away the whole
/// time. Takes `dyn Error` so the SSE stream's wrapper is covered by the same
/// rule.
/// The HTTP client every provider sends through, built rather than
/// `Client::new()`-ed: `new()` unwraps the builder, and since reqwest 0.13 the
/// builder fails on a machine with no CA bundle ("No CA certificates were
/// loaded from the system"). That is a deployment fault — ship
/// `ca-certificates`, or point `SSL_CERT_FILE` at a bundle — and a deployment
/// fault is an error to report, not a panic in a constructor.
pub(crate) fn http_client() -> Result<reqwest::Client, LlmError> {
    reqwest::Client::builder().build().map_err(|e| {
        LlmError::NetworkError(format!(
            "building the HTTP client: {}",
            describe_transport_error(&e)
        ))
    })
}

pub(crate) fn describe_transport_error(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// Substrings that identify an over-long request in a provider's error body.
///
/// Providers disagree on both the status code and the shape, and several return
/// a plain 400 with prose, so matching on text is the only thing that works
/// across all of them. Checked lowercase.
const CONTEXT_LENGTH_MARKERS: [&str; 8] = [
    // OpenAI (`"code": "context_length_exceeded"`), and OpenRouter passes it through.
    "context_length_exceeded",
    // OpenAI / OpenRouter prose, and most OpenAI-compatible servers.
    "maximum context length",
    "context length",
    // The same concept spelled "size", which is llama.cpp's word for it and
    // matched none of the above. Kept as a bare marker, symmetric with
    // "context length", so a build whose error `type` differs from the one
    // below is still recognized by its prose.
    "context size",
    // llama.cpp, vLLM.
    "too many tokens",
    "exceeds the maximum",
    // llama.cpp b10524's `type`, for the same overflow:
    // `{"type":"exceed_context_size_error","message":"request (40089 tokens)
    // exceeds the available context size (32768 tokens)"}`. Matched on the
    // `type` as well as the prose above, that being the half of the body least
    // likely to be reworded.
    "exceed_context_size_error",
    // Gemini: INVALID_ARGUMENT naming the input token count.
    "input token count",
];

/// Classify a provider's failure body, so an over-long request becomes
/// [`LlmError::ContextLengthExceeded`] rather than an opaque API error.
///
/// One function for both providers and both code paths (streaming and not) so
/// they classify identically — it owns the `{label} ({status}): {body}`
/// format too, which used to be written at every call site. Takes the **raw**
/// body because the numbers a context refusal carries are read from it before
/// anything is flattened into prose.
pub(crate) fn classify_api_error(label: &str, status: reqwest::StatusCode, body: &str) -> LlmError {
    let message = format!("{label} ({status}): {body}");
    let haystack = message.to_lowercase();
    if CONTEXT_LENGTH_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
    {
        let (prompt_tokens, context_window) = context_refusal_numbers(body);
        return LlmError::ContextLengthExceeded {
            detail: message,
            prompt_tokens,
            context_window,
        };
    }
    LlmError::ApiError(message)
}

/// What the provider's body says about a refused request: (prompt tokens as it
/// counted them, the window it enforced).
///
/// JSON fields first — llama.cpp returns `n_prompt_tokens` and `n_ctx` beside
/// the message, inside the `error` object — then the two prose shapes the
/// fixtures pin: OpenAI's "maximum context length is <N>" names the window,
/// Gemini's "input token count (<N>)" names the count. Nothing speculative: a
/// shape with no fixture is not parsed, because a wrong number is worse than
/// none — these values are what a caller repairs its config with.
fn context_refusal_numbers(body: &str) -> (Option<u32>, Option<u32>) {
    let json: Option<Value> = serde_json::from_str(body).ok();
    let field = |name: &str| {
        let json = json.as_ref()?;
        json.get(name)
            .or_else(|| json.get("error").and_then(|error| error.get(name)))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    let prose = body.to_lowercase();
    let prompt_tokens =
        field("n_prompt_tokens").or_else(|| number_after(&prose, "input token count ("));
    let context_window =
        field("n_ctx").or_else(|| number_after(&prose, "maximum context length is "));
    (prompt_tokens, context_window)
}

/// The unsigned number immediately following `pattern` in `haystack`, if any.
fn number_after(haystack: &str, pattern: &str) -> Option<u32> {
    let start = haystack.find(pattern)? + pattern.len();
    let digits: &str = haystack[start..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?;
    digits.parse().ok()
}

/// Whether the model behind an endpoint accepts the reasoning parameter its
/// adapter sends.
///
/// Support turns on the *model*, not the provider: `reasoning_effort` is a 400
/// on `gpt-4o-mini` and mandatory on `gpt-5-pro`, and Gemini's `thinkingLevel`
/// is accepted by some 2.5-generation models and refused by others. A table of
/// model names answers that for the models it lists and goes stale with every
/// release, so the parameter is sent instead, the refusal is read back off the
/// 400, and the request is retried once without it.
///
/// The answer is remembered here, so only the first refused call of a process
/// pays the extra round trip — and only once the retry has confirmed it, since a
/// 400 that names the field can still be about something else. Shared across
/// clones of a provider, since a provider is cloned per handler and they all
/// call the same model.
#[derive(Clone, Default)]
pub(crate) struct ReasoningSupport(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl ReasoningSupport {
    /// Whether a refusal has already been seen. Nothing is sent after one.
    pub(crate) fn refused(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Remember that this endpoint refused the parameter. `Relaxed` because a
    /// racing caller re-sending once is the cost of being wrong.
    pub(crate) fn record_refusal(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Whether a failed request is the endpoint refusing the reasoning parameter,
/// rather than the request failing on its own merits.
///
/// Both providers answer 400 and name the field: OpenAI with
/// `Unsupported parameter: 'reasoning_effort' is not supported with this model`,
/// Gemini with `Unknown name "thinkingLevel": Cannot find field`. A refused
/// *value* reads the same way (`Invalid value: 'none'. Supported values are …`)
/// and wants the same recovery, so the test is the field name rather than the
/// prose. `field` is the one word every such message contains — `reasoning`,
/// `thinking` — which is specific enough because this is only asked when that
/// field was actually sent.
pub(crate) fn refuses_reasoning(status: u16, body: &str, field: &str) -> bool {
    status == 400 && body.to_lowercase().contains(field)
}

/// Tokens a provider reported for one request.
///
/// Reported rather than estimated: a caller's own token estimate decides what to
/// send, and this says what it actually cost. Every field is optional because
/// providers disagree on which they return, and a missing count must not read as
/// zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// Tokens in the request, including the system prompt and tool definitions.
    pub prompt_tokens: Option<u32>,
    /// Tokens the model generated, excluding reasoning where a provider splits
    /// them out.
    pub completion_tokens: Option<u32>,
    /// Reasoning tokens, where the provider reports them separately. Billed, and
    /// invisible in `completion_tokens` on most providers.
    pub reasoning_tokens: Option<u32>,
    /// The provider's own total. Not derived from the fields above — a provider
    /// that reports only this one is common, and a total that disagrees with the
    /// parts is the provider's answer, not ours to correct.
    pub total_tokens: Option<u32>,
}

impl TokenUsage {
    /// Whether the provider reported anything at all. A response carrying no
    /// counts is `Some(TokenUsage::default())` nowhere — it is `None`.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl std::fmt::Display for TokenUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let field = |value: Option<u32>| match value {
            Some(count) => count.to_string(),
            None => "?".to_string(),
        };
        write!(
            f,
            "prompt={} completion={} reasoning={} total={}",
            field(self.prompt_tokens),
            field(self.completion_tokens),
            field(self.reasoning_tokens),
            field(self.total_tokens)
        )
    }
}

/// The role of the message sender.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Defines a tool (function) available for the LLM to call.
///
/// Only `name`, `description` and `parameters` go on the wire: that is all a
/// chat-completion API takes. The rest is what the tool's *source* said about
/// it — an MCP server's title, hints and output schema — kept so the consumer
/// holding the definition can act on it (refuse a destructive tool to an
/// analyst, validate a structured result), which it could not while the
/// bridge flattened everything to three fields.
///
/// `#[non_exhaustive]`: build one with [`ToolDefinition::new`] and the
/// `with_*` methods. Adding a field here broke every constructor downstream
/// once; it should not do so again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments.
    pub parameters: Value,
    /// A human-readable name, where the source distinguishes one from `name`.
    /// Not sent to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What the source says about the tool's effects. Not sent to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
    /// JSON Schema for a structured result, where the source declares one.
    /// Not sent to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
}

impl ToolDefinition {
    /// A tool with the three fields every provider needs.
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
            title: None,
            annotations: None,
            output_schema: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }

    pub fn with_output_schema(mut self, schema: Value) -> Self {
        self.output_schema = Some(schema);
        self
    }
}

/// What a tool's source says about its effects. Hints, not guarantees: MCP's
/// own wording, and a server can lie. `None` is "the source said nothing",
/// which is not the same as `Some(false)` — a tool with no `read_only` claim
/// is a tool the consumer knows nothing about, not one that writes.
///
/// Owned here rather than borrowed from `rmcp` so this crate stays free of
/// MCP; the bridge maps between the two.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ToolAnnotations {
    /// The tool does not modify its environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    /// The tool may perform destructive updates (meaningful when not read-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive: Option<bool>,
    /// Repeating a call with the same arguments has no further effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent: Option<bool>,
    /// The tool reaches an open world of external entities (a web search),
    /// as opposed to a closed one (a memory store).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world: Option<bool>,
}

impl ToolAnnotations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_only(mut self, value: bool) -> Self {
        self.read_only = Some(value);
        self
    }

    pub fn destructive(mut self, value: bool) -> Self {
        self.destructive = Some(value);
        self
    }

    pub fn idempotent(mut self, value: bool) -> Self {
        self.idempotent = Some(value);
        self
    }

    pub fn open_world(mut self, value: bool) -> Self {
        self.open_world = Some(value);
        self
    }

    /// Whether the source said nothing at all.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What a tool answered: the text the model is shown, and the structured
/// value beside it when the tool's source returned one (MCP's
/// `structuredContent`).
///
/// The model gets a string either way — [`into_model_text`] — because that is
/// what every chat-completion API takes for a tool message. The structured
/// value is for whoever holds the result: a consumer that wants to check it
/// against the tool's `output_schema` or render it, without re-parsing prose.
///
/// [`into_model_text`]: ToolResult::into_model_text
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolResult {
    /// The text content, joined. Empty when the tool returned only structure.
    pub text: String,
    /// The structured result, where the source gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<Value>,
}

impl ToolResult {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
        }
    }

    pub fn with_structured(mut self, value: Value) -> Self {
        self.structured = Some(value);
        self
    }

    /// The string the model is told. The text when there is any; otherwise
    /// the structured value serialized, so a tool that answered only in
    /// structure is not reported as having said nothing.
    pub fn into_model_text(self) -> String {
        if !self.text.is_empty() {
            return self.text;
        }
        match self.structured {
            Some(value) => value.to_string(),
            None => self.text,
        }
    }
}

/// Represents a specific tool invocation requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String, // ID of the tool call
    pub name: String,
    pub arguments: String, // Stringified JSON arguments
}

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: MessageRole::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }
}

/// How hard a reasoning model should think, when reasoning is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    /// The wire token used by OpenRouter's `reasoning.effort`.
    pub fn as_str(self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }
}

/// What to ask a reasoning-capable model to do with its thinking.
///
/// This is a *request*, not a capability: `Some(_)` says what the caller wants
/// and `None` says nothing at all, leaving the model's own default alone.
/// Whether the endpoint can carry it is the provider's business — a provider
/// that cannot say this on the wire drops it rather than making every caller
/// ask first.
///
/// Where the model does honour it, its thinking *may* come back on a separate
/// channel ([`LlmResponse::reasoning`] / [`LlmStreamEvent::Reasoning`]). The
/// text is the model's to give, and often it does not: Gemini via OpenRouter
/// bills `reasoning_tokens` on every request and put a summary in
/// `delta.reasoning` on about one request in nine, with no request parameter
/// changing that. Do not build on the text arriving. What can be relied on is
/// [`TokenUsage::reasoning_tokens`], which says that thinking happened and
/// what it cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reasoning {
    /// Don't think — for models that let reasoning be turned off. The one
    /// setting a small, fast model usually wants, and the one an effort-only
    /// knob cannot express.
    Off,
    /// Think at one of the provider's named effort levels.
    Effort(ReasoningEffort),
    /// Think within a hard budget of reasoning tokens.
    Budget(u32),
}

/// What a host's config or environment may spell, listed once so the parser,
/// the error message, and the docs cannot drift apart.
const REASONING_EXPECTED: &str =
    r#""off", "low", "medium", "high", or a number of reasoning tokens"#;

impl std::str::FromStr for Reasoning {
    type Err = String;

    /// Parses the tokens a host config or `OPENROUTER_REASONING` accepts:
    /// `off`, `low`, `medium`, `high`, or a plain token budget (`2000`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "off" => Ok(Reasoning::Off),
            "low" => Ok(Reasoning::Effort(ReasoningEffort::Low)),
            "medium" => Ok(Reasoning::Effort(ReasoningEffort::Medium)),
            "high" => Ok(Reasoning::Effort(ReasoningEffort::High)),
            budget => budget
                .parse()
                .map(Reasoning::Budget)
                .map_err(|_| format!("expected {REASONING_EXPECTED}; got {s:?}")),
        }
    }
}

impl std::fmt::Display for Reasoning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reasoning::Off => f.write_str("off"),
            Reasoning::Effort(effort) => f.write_str(effort.as_str()),
            Reasoning::Budget(tokens) => write!(f, "{tokens}"),
        }
    }
}

impl Serialize for Reasoning {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // A budget round-trips as the number it was written as; the levels
            // as their token. Both are what a host config spells.
            Reasoning::Budget(tokens) => serializer.serialize_u32(*tokens),
            level => serializer.serialize_str(&level.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for Reasoning {
    /// Accepts a level (`"high"`) or a token budget (`2000`) — one parser for
    /// every host, so a bad value is refused the same way with the same message
    /// wherever it was written.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, Unexpected, Visitor};

        struct ReasoningVisitor;

        impl Visitor<'_> for ReasoningVisitor {
            type Value = Reasoning;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(REASONING_EXPECTED)
            }

            fn visit_str<E: Error>(self, value: &str) -> Result<Reasoning, E> {
                value
                    .parse()
                    .map_err(|_| E::invalid_value(Unexpected::Str(value), &self))
            }

            fn visit_u64<E: Error>(self, value: u64) -> Result<Reasoning, E> {
                u32::try_from(value)
                    .map(Reasoning::Budget)
                    .map_err(|_| E::invalid_value(Unexpected::Unsigned(value), &self))
            }

            fn visit_i64<E: Error>(self, value: i64) -> Result<Reasoning, E> {
                u32::try_from(value)
                    .map(Reasoning::Budget)
                    .map_err(|_| E::invalid_value(Unexpected::Signed(value), &self))
            }
        }

        deserializer.deserialize_any(ReasoningVisitor)
    }
}

/// A request to an LLM provider for chat completion.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub force_json: bool,
    /// What this request asks of a reasoning model; `None` defers to whatever
    /// default the provider was configured with, and then to the model's own.
    pub reasoning: Option<Reasoning>,
}

impl LlmRequest {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            tools: None,
            temperature: None,
            max_tokens: None,
            force_json: false,
            reasoning: None,
        }
    }

    pub fn reasoning(mut self, reasoning: Reasoning) -> Self {
        self.reasoning = Some(reasoning);
        self
    }

    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn force_json(mut self, force: bool) -> Self {
        self.force_json = force;
        self
    }
}

/// Why the model stopped generating, normalized across providers.
///
/// The variant a caller acts on is [`FinishReason::Length`]: the output hit a
/// token limit — the request's own `max_tokens`, or whatever the serving stack
/// enforces on its own — and the content is cut mid-thought. That arrives as
/// HTTP 200 with no refusal, so this field is the *only* thing in the response
/// that says the text is incomplete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    /// The model finished on its own.
    Stop,
    /// Generation was cut at an output-token limit; the content is incomplete.
    Length,
    /// The model stopped to call tools.
    ToolCalls,
    /// The provider suppressed output (safety filter, recitation).
    ContentFilter,
    /// A reason this crate does not recognize, kept verbatim.
    Other(String),
}

impl FinishReason {
    /// Reads a provider's wire spelling. OpenAI-compatible endpoints (OpenAI,
    /// OpenRouter, llama.cpp, vLLM) say `stop` / `length` / `tool_calls` /
    /// `function_call` / `content_filter`; Gemini says `STOP` / `MAX_TOKENS` /
    /// `SAFETY` / … — one reader for both, since the spellings do not collide.
    pub fn from_wire(value: &str) -> Self {
        match value {
            "stop" | "STOP" => Self::Stop,
            "length" | "MAX_TOKENS" => Self::Length,
            "tool_calls" | "function_call" => Self::ToolCalls,
            "content_filter" | "SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" | "BLOCKLIST"
            | "SPII" => Self::ContentFilter,
            other => Self::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for FinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FinishReason::Stop => f.write_str("stop"),
            FinishReason::Length => f.write_str("length"),
            FinishReason::ToolCalls => f.write_str("tool_calls"),
            FinishReason::ContentFilter => f.write_str("content_filter"),
            FinishReason::Other(reason) => f.write_str(reason),
        }
    }
}

/// A response from an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Reasoning-model "thinking" text, when the provider exposes it separately
    /// from the answer (e.g. OpenRouter's `reasoning`, Zhipu/GLM's
    /// `reasoning_content`). `None` for providers that don't surface it — and
    /// often `None` from ones that do, since the text is the model's to give
    /// (see [`Reasoning`]); `usage.reasoning_tokens` is the reliable signal.
    pub reasoning: Option<String>,
    /// What the provider says the request cost. `None` when it reported nothing.
    pub usage: Option<TokenUsage>,
    /// Why generation stopped. `None` when the provider named no reason.
    pub finish: Option<FinishReason>,
}

/// An event emitted during a streaming LLM response.
#[derive(Debug, Clone)]
pub enum LlmStreamEvent {
    ContentChunk(String),
    /// A chunk of reasoning-model "thinking" text, distinct from the answer
    /// content (e.g. OpenRouter's `reasoning` / Zhipu's `reasoning_content`).
    Reasoning(String),
    ToolCallChunk {
        id: String,
        name: Option<String>,
        arguments: String,
    },
    ToolCall(ToolCall),
    /// What the request cost, as reported by the provider. Terminal: it arrives
    /// in the final chunk, after the content. Absent on endpoints that do not
    /// report usage while streaming — see `OpenAiConfig::stream_usage`.
    Usage(TokenUsage),
    /// Why generation stopped. Arrives near the end of the stream but not
    /// necessarily last — OpenAI-style endpoints put it on the final content
    /// chunk, before the usage-only chunk and before this crate flushes
    /// accumulated tool calls — so a caller should record it, not break on it.
    /// A fact, not a terminator: the stream ends when it ends.
    ///
    /// Emitted at most once per reason. OpenRouter repeats `finish_reason` on
    /// its usage-only chunk, and this crate drops the repeat, so a consumer
    /// that counts finishes counts one. Absent when the provider named no
    /// reason.
    Finish(FinishReason),
}

/// Trait defining a generic LLM provider for standardizing AI integration across agents.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generates a chat completion based on the provided request.
    async fn chat_completion(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;

    /// Generates a streaming chat completion.
    async fn chat_completion_stream(
        &self,
        request: LlmRequest,
    ) -> Result<BoxStream<'static, Result<LlmStreamEvent, LlmError>>, LlmError>;

    /// Whether the model behind this provider has refused the reasoning
    /// parameter the adapter sends, so requests now go without it.
    ///
    /// `Some(true)` is a fact about the process: the parameter was sent, the
    /// model answered 400 naming it, the retry without it succeeded, and
    /// nothing will carry it again — a configured `reasoning` is being paid
    /// for and not delivered, which is what an agent's operator wants to see
    /// somewhere other than one log line. `Some(false)` says it has not
    /// happened (yet).
    ///
    /// `None` is the default and means this adapter has nothing to report:
    /// it sends no such parameter, or has no memory of a refusal. An adapter
    /// that does retry without the parameter must override this, or the
    /// refusal it remembers is one nobody outside it can see.
    fn reasoning_refused(&self) -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;

    fn status(code: u16) -> reqwest::StatusCode {
        reqwest::StatusCode::from_u16(code).unwrap()
    }

    /// The one API failure a caller can act on has to be tellable from the rest,
    /// across the shapes the providers actually return — and where a body names
    /// the numbers, they come through, because they are what a caller repairs
    /// its config with (an estimate-derived bound has the estimator's error on
    /// it; `n_ctx` is the limit the provider actually enforced).
    #[test]
    fn an_over_long_request_is_classified_as_a_context_length_failure() {
        // (label, body, provider-counted prompt tokens, enforced window)
        let cases = [
            (
                "OpenAI API error",
                r#"{"error":{"message":"This model's maximum context length is 128000 tokens","code":"context_length_exceeded"}}"#,
                None,
                Some(128000),
            ),
            (
                "OpenAI stream error",
                "Requested 200000 tokens, exceeds the maximum for this model",
                // No fixture-pinned shape names these numbers, and a shape
                // without a fixture is not parsed.
                None,
                None,
            ),
            (
                "Gemini API error",
                r#"{"error":{"status":"INVALID_ARGUMENT","message":"The input token count (1200000) exceeds the maximum"}}"#,
                Some(1200000),
                None,
            ),
            ("OpenAI API error", "too many tokens in prompt", None, None),
            // llama.cpp b10524, verbatim. None of the markers above match it
            // (it says "context size", not "context length", and "exceeds the
            // available", not "exceeds the maximum") — and it is the one body
            // carrying both numbers as JSON fields.
            (
                "OpenAI stream error",
                r#"{"error":{"code":400,"message":"request (40089 tokens) exceeds the available context size (32768 tokens), try increasing it","type":"exceed_context_size_error","n_prompt_tokens":40089,"n_ctx":32768}}"#,
                Some(40089),
                Some(32768),
            ),
        ];
        for (label, body, prompt, window) in cases {
            match classify_api_error(label, status(400), body) {
                LlmError::ContextLengthExceeded {
                    detail,
                    prompt_tokens,
                    context_window,
                } => {
                    assert!(detail.contains(label) && detail.contains(body), "{detail}");
                    assert_eq!(prompt_tokens, prompt, "prompt tokens for: {body}");
                    assert_eq!(context_window, window, "window for: {body}");
                }
                other => panic!("should classify as context length: {body} → {other:?}"),
            }
        }
    }

    /// Everything else stays an ordinary API error. Classifying a bad key as
    /// "too long" would send the handler into a compaction loop it can never win.
    #[test]
    fn other_failures_stay_api_errors() {
        let cases = [
            (
                "OpenAI API error",
                401,
                r#"{"error":{"message":"Incorrect API key provided"}}"#,
            ),
            ("OpenAI API error", 429, "Rate limit reached for requests"),
            ("Gemini API error", 503, "The model is overloaded"),
        ];
        for (label, code, body) in cases {
            assert!(
                matches!(
                    classify_api_error(label, status(code), body),
                    LlmError::ApiError(_)
                ),
                "should stay an API error: {body}"
            );
        }
    }

    /// The recovery turns on recognizing this one failure, so it is checked
    /// against the bodies the two APIs actually answer with — an unsupported
    /// parameter, an unknown field, and a value the model does not offer, which
    /// wants the same retry as the other two.
    #[test]
    fn a_refused_reasoning_parameter_is_recognized_from_the_body() {
        let openai = [
            r#"{"error":{"message":"Unsupported parameter: 'reasoning_effort' is not supported with this model.","type":"invalid_request_error","param":"reasoning_effort","code":"unsupported_parameter"}}"#,
            r#"{"error":{"message":"Invalid value: 'none'. Supported values are: 'low', 'medium' and 'high'.","type":"invalid_request_error","param":"reasoning_effort","code":"invalid_value"}}"#,
            r#"{"error":{"message":"Unrecognized request argument supplied: reasoning_effort"}}"#,
        ];
        for body in openai {
            assert!(refuses_reasoning(400, body, "reasoning"), "{body}");
        }

        let gemini = [
            r#"{"error":{"code":400,"message":"Invalid JSON payload received. Unknown name \"thinkingLevel\" at 'generation_config': Cannot find field.","status":"INVALID_ARGUMENT"}}"#,
            r#"{"error":{"code":400,"message":"Budget 128 is invalid. thinkingBudget must be 0 or in the range [128, 32768]","status":"INVALID_ARGUMENT"}}"#,
        ];
        for body in gemini {
            assert!(refuses_reasoning(400, body, "thinking"), "{body}");
        }
    }

    /// Everything else is the request failing on its own merits, and retrying it
    /// without reasoning would waste a round trip and hide the real cause. A 5xx
    /// that happens to name the field is the same: dropping the setting on an
    /// outage would leave the model thinking at its default long after.
    #[test]
    fn other_failures_are_not_read_as_a_refusal() {
        assert!(!refuses_reasoning(
            400,
            r#"{"error":{"message":"Incorrect API key provided"}}"#,
            "reasoning"
        ));
        assert!(!refuses_reasoning(
            429,
            r#"{"error":{"message":"Rate limit reached"}}"#,
            "reasoning"
        ));
        assert!(!refuses_reasoning(
            503,
            r#"{"error":{"message":"reasoning_effort backend unavailable"}}"#,
            "reasoning"
        ));
    }

    /// A refusal is remembered once, and every clone of the provider sees it:
    /// they all call the same model.
    #[test]
    fn a_recorded_refusal_is_shared() {
        let support = ReasoningSupport::default();
        let clone = support.clone();
        assert!(!support.refused());
        clone.record_refusal();
        assert!(support.refused());
    }

    #[test]
    fn usage_with_nothing_reported_reads_as_empty() {
        assert!(TokenUsage::default().is_empty());
        assert!(
            !TokenUsage {
                prompt_tokens: Some(0),
                ..Default::default()
            }
            .is_empty(),
            "a reported zero is a report, not an absence"
        );
    }

    /// One reader for both dialects, because the spellings do not collide —
    /// and a reason it does not recognize survives verbatim rather than being
    /// flattened into a guess.
    #[test]
    fn finish_reasons_normalize_across_provider_spellings() {
        let cases = [
            ("stop", FinishReason::Stop),
            ("STOP", FinishReason::Stop),
            ("length", FinishReason::Length),
            ("MAX_TOKENS", FinishReason::Length),
            ("tool_calls", FinishReason::ToolCalls),
            ("function_call", FinishReason::ToolCalls),
            ("content_filter", FinishReason::ContentFilter),
            ("SAFETY", FinishReason::ContentFilter),
            ("RECITATION", FinishReason::ContentFilter),
        ];
        for (wire, expected) in cases {
            assert_eq!(FinishReason::from_wire(wire), expected, "for {wire:?}");
        }
        assert_eq!(
            FinishReason::from_wire("MALFORMED_FUNCTION_CALL"),
            FinishReason::Other("MALFORMED_FUNCTION_CALL".to_string())
        );
    }
}

#[cfg(test)]
mod tool_vocabulary_tests {
    use super::*;
    use serde_json::json;

    /// A definition written before the source's title, hints and output
    /// schema were carried still reads: the new fields are all optional, and
    /// a definition that has none of them serializes as it always did.
    #[test]
    fn a_three_field_definition_still_round_trips() {
        let old = json!({
            "name": "add",
            "description": "Adds two numbers",
            "parameters": {"type": "object"}
        });
        let def: ToolDefinition = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(def.title, None);
        assert_eq!(def.annotations, None);
        assert_eq!(def.output_schema, None);
        assert_eq!(serde_json::to_value(&def).unwrap(), old);
    }

    #[test]
    fn what_the_source_said_survives_serde() {
        let def = ToolDefinition::new("query", "Runs a query", json!({"type": "object"}))
            .with_title("Query")
            .with_annotations(ToolAnnotations::new().read_only(true).open_world(false))
            .with_output_schema(json!({"type": "object", "properties": {"rows": {}}}));
        let wire = serde_json::to_value(&def).unwrap();
        assert_eq!(wire["title"], "Query");
        assert_eq!(
            wire["annotations"],
            json!({"readOnly": true, "openWorld": false})
        );
        assert_eq!(wire["output_schema"]["properties"]["rows"], json!({}));
        let back: ToolDefinition = serde_json::from_value(wire).unwrap();
        assert_eq!(back, def);
    }

    /// `None` on a hint is "the source said nothing", so an annotations value
    /// with nothing set says so too.
    #[test]
    fn empty_annotations_are_distinguishable_from_a_false_claim() {
        assert!(ToolAnnotations::new().is_empty());
        assert!(!ToolAnnotations::new().read_only(false).is_empty());
    }

    /// The model is told the text when there is any, and the structure when
    /// that is all the tool returned — never nothing for a tool that answered.
    #[test]
    fn a_result_reaches_the_model_as_text_or_as_its_structure() {
        let both = ToolResult::new("3 rows").with_structured(json!({"rows": 3}));
        assert_eq!(both.into_model_text(), "3 rows");

        let only_structure = ToolResult::new("").with_structured(json!({"rows": 3}));
        assert_eq!(only_structure.into_model_text(), r#"{"rows":3}"#);

        assert_eq!(ToolResult::new("").into_model_text(), "");
    }
}
