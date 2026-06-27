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

/// Opt-in reasoning controls for reasoning-capable models (e.g. GLM via
/// OpenRouter). When set on an [`LlmRequest`], a supporting provider returns its
/// thinking on a separate channel (surfaced as [`LlmResponse::reasoning`] /
/// [`LlmStreamEvent::Reasoning`]). Providers that don't support it ignore this.
#[derive(Debug, Clone, Default)]
pub struct ReasoningConfig {
    /// Reasoning effort. Mutually exclusive with `max_tokens` on most providers.
    pub effort: Option<ReasoningEffort>,
    /// Hard cap on reasoning tokens.
    pub max_tokens: Option<u32>,
    /// Let the model reason internally but omit the reasoning from the response.
    pub exclude: bool,
}

impl ReasoningConfig {
    /// Request reasoning at the given effort level.
    pub fn effort(effort: ReasoningEffort) -> Self {
        Self {
            effort: Some(effort),
            ..Default::default()
        }
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
    /// Opt-in reasoning controls; `None` leaves provider defaults untouched.
    pub reasoning: Option<ReasoningConfig>,
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

    pub fn reasoning(mut self, config: ReasoningConfig) -> Self {
        self.reasoning = Some(config);
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
