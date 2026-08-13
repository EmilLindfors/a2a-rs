use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod gemini;
pub mod openai;
pub mod provider;
pub mod tool_call;

pub use provider::{LlmSettings, provider_from_env, provider_from_settings};
pub use tool_call::{PartialToolCall, ToolCallAccumulator};

/// Represents an error returned by an LLM provider.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON Schema representation of arguments
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
/// Where the model does honour it, its thinking comes back on a separate channel
/// ([`LlmResponse::reasoning`] / [`LlmStreamEvent::Reasoning`]).
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

/// A response from an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Reasoning-model "thinking" text, when the provider exposes it separately
    /// from the answer (e.g. OpenRouter's `reasoning`, Zhipu/GLM's
    /// `reasoning_content`). `None` for providers that don't surface it.
    pub reasoning: Option<String>,
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
}
