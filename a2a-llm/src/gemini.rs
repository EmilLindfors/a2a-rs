use super::{
    Env, LlmError, LlmProvider, LlmRequest, LlmResponse, MessageRole, Reasoning, ReasoningSupport,
    describe_transport_error, refuses_reasoning,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

/// Configuration for the Gemini AI client
#[derive(Debug, Clone)]
pub struct GeminiConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    /// Reasoning applied to requests that don't ask for their own — the model's
    /// setting, configured where the model is. `None` sends nothing.
    pub reasoning: Option<Reasoning>,
}

/// Default base URL for the Gemini generative-language API.
pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

// There is deliberately no `GEMINI_DEFAULT_MODEL`. It was `gemini-1.5-pro`
// until 2026-08-25, by which point Google had stopped listing that model at
// all — so every config naming `gemini` without a model ran on something the
// vendor's own documentation no longer described, and nothing said so. A
// default model is a one-entry table of somebody else's product line: it goes
// stale in place, and it goes stale silently, because the value that is wrong
// is the one nobody wrote down. The model is required instead.

impl GeminiConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(Env::os())
    }

    /// Read a Gemini config from `env`. The key and the model are both
    /// required: there is no keyless Gemini endpoint, and no model this crate
    /// is willing to choose on a caller's behalf.
    pub(crate) fn from_lookup(env: Env<'_>) -> Result<Self, String> {
        Ok(Self {
            base_url: env
                .get("GEMINI_API_BASE_URL")
                .unwrap_or_else(|| GEMINI_BASE_URL.to_string()),
            model: env
                .get("GEMINI_MODEL")
                .ok_or_else(|| "GEMINI_MODEL environment variable is required".to_string())?,
            api_key: env
                .get("GEMINI_API_KEY")
                .ok_or_else(|| "GEMINI_API_KEY environment variable is required".to_string())?,
            reasoning: None,
        })
    }
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    #[serde(rename = "temperature", skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(rename = "responseMimeType", skip_serializing_if = "Option::is_none")]
    response_mime_type: Option<String>,
    /// What to ask the model to do with its thinking. Only sent when something
    /// asked for it — the field is refused outright by models that do not think.
    #[serde(rename = "thinkingConfig", skip_serializing_if = "Option::is_none")]
    thinking_config: Option<ThinkingConfig>,
}

/// Gemini's `generationConfig.thinkingConfig`.
///
/// `thinkingLevel` and `thinkingBudget` are mutually exclusive — sending both is
/// an error — and which of them a model takes differs by model generation. The
/// mapping in [`thinking_config_for`] sets exactly one, and a model that refuses
/// it says so on a 400.
#[derive(Debug, Clone, Serialize)]
struct ThinkingConfig {
    #[serde(rename = "thinkingLevel", skip_serializing_if = "Option::is_none")]
    thinking_level: Option<&'static str>,
    #[serde(rename = "thinkingBudget", skip_serializing_if = "Option::is_none")]
    thinking_budget: Option<u32>,
}

/// Gemini's thinking control for a requested [`Reasoning`].
///
/// Every variant has a spelling here, unlike OpenAI's effort-only parameter: a
/// level for each named effort, a budget for a token cap, and a budget of zero
/// for off, which is the documented way to turn thinking off where a model
/// allows it at all.
fn thinking_config_for(reasoning: Reasoning) -> ThinkingConfig {
    let (thinking_level, thinking_budget) = match reasoning {
        Reasoning::Off => (None, Some(0)),
        Reasoning::Effort(effort) => (Some(effort.as_str()), None),
        Reasoning::Budget(tokens) => (None, Some(tokens)),
    };
    ThinkingConfig {
        thinking_level,
        thinking_budget,
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    function_response: Option<GeminiFunctionResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct Content {
    role: String,
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
struct SystemInstruction {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
struct GeminiGenerateContentRequest {
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction>,
    contents: Vec<Content>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
}

impl GeminiGenerateContentRequest {
    /// Whether this request asks for thinking at all.
    fn asks_for_thinking(&self) -> bool {
        self.generation_config
            .as_ref()
            .is_some_and(|config| config.thinking_config.is_some())
    }

    /// Drop the thinking configuration, for the retry after a model refuses it.
    fn clear_thinking(&mut self) {
        if let Some(config) = self.generation_config.as_mut() {
            config.thinking_config = None;
        }
    }
}

#[derive(Debug, Deserialize)]
struct GeminiGenerateContentResponse {
    candidates: Option<Vec<Candidate>>,
    #[serde(rename = "promptFeedback")]
    _prompt_feedback: Option<serde_json::Value>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

/// Gemini's `usageMetadata`. Present on the response and, while streaming, on
/// every chunk — the last one carries the final counts, so a caller that keeps
/// the newest wins.
#[derive(Debug, Deserialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
    #[serde(rename = "thoughtsTokenCount")]
    thoughts_token_count: Option<u32>,
    #[serde(rename = "totalTokenCount")]
    total_token_count: Option<u32>,
}

impl From<GeminiUsageMetadata> for super::TokenUsage {
    fn from(usage: GeminiUsageMetadata) -> Self {
        Self {
            prompt_tokens: usage.prompt_token_count,
            completion_tokens: usage.candidates_token_count,
            reasoning_tokens: usage.thoughts_token_count,
            total_tokens: usage.total_token_count,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Option<ResponseContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseContent {
    parts: Option<Vec<ResponsePart>>,
    #[allow(dead_code)]
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsePart {
    text: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Clone)]
pub struct GeminiProvider {
    config: GeminiConfig,
    client: reqwest::Client,
    /// Whether this model has already refused `thinkingConfig`. Shared across
    /// clones.
    reasoning_support: ReasoningSupport,
}

/// The one word every message about a refused `thinkingConfig` contains,
/// whichever of its two fields was sent.
const THINKING_FIELD: &str = "thinking";

impl GeminiProvider {
    pub fn new(config: GeminiConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            reasoning_support: ReasoningSupport::default(),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        let config = GeminiConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// What thinking configuration this request carries, if any.
    ///
    /// The request's own setting wins over the model's configured default, and
    /// nothing is sent once the model has refused the field.
    fn thinking_for(&self, request: &LlmRequest) -> Option<ThinkingConfig> {
        let reasoning = request.reasoning.or(self.config.reasoning)?;
        if self.reasoning_support.refused() {
            return None;
        }
        Some(thinking_config_for(reasoning))
    }

    /// POST `body` to `url`, and if the model refuses the `thinkingConfig` it
    /// carries, send it once more without one.
    ///
    /// Which models take `thinkingLevel` and which take only `thinkingBudget` is
    /// what Google's own docs and the field reports disagree about, so the
    /// endpoint is asked rather than a table consulted. Nothing was generated on
    /// a 400, so the retry costs a round trip and no tokens, and only the first
    /// refused call pays it.
    async fn send_generate_request(
        &self,
        url: &str,
        mut body: GeminiGenerateContentRequest,
        label: &str,
    ) -> Result<reqwest::Response, LlmError> {
        let response = self.post(url, &body).await?;
        if response.status().is_success() {
            return Ok(response);
        }

        // Read here rather than in `api_error`, because the decision to retry is
        // made from this body.
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());

        if !(body.asks_for_thinking()
            && refuses_reasoning(status.as_u16(), &error_text, THINKING_FIELD))
        {
            error!(status = %status, error = %error_text, "Gemini API returned error");
            return Err(super::classify_api_error(label, status, &error_text));
        }

        warn!(
            model = %self.config.model,
            %status,
            error = %error_text,
            "model refused `thinkingConfig`; retrying without it"
        );
        body.clear_thinking();

        let retried = self.post(url, &body).await?;
        if retried.status().is_success() {
            // Dropping it fixed the call, so the field was the problem and this
            // model gets no more of them. A retry that fails the same way says
            // the 400 was about something else, and remembering it would
            // disable thinking for the rest of the process over an unrelated
            // failure.
            self.reasoning_support.record_refusal();
            Ok(retried)
        } else {
            Err(self.api_error(label, retried).await)
        }
    }

    async fn post(
        &self,
        url: &str,
        body: &GeminiGenerateContentRequest,
    ) -> Result<reqwest::Response, LlmError> {
        self.client.post(url).json(body).send().await.map_err(|e| {
            // The URL names which call this was: `:generateContent` or
            // `:streamGenerateContent`.
            error!(url = %url, error = %e, "Failed to send request to Gemini API");
            LlmError::NetworkError(describe_transport_error(&e))
        })
    }

    /// Read a failed response's body and turn it into the error it becomes.
    async fn api_error(&self, label: &str, response: reqwest::Response) -> LlmError {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        error!(status = %status, error = %error_text, "Gemini API returned error");
        super::classify_api_error(label, status, &error_text)
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn chat_completion(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!(
            "{}/{}:generateContent?key={}",
            self.config.base_url, self.config.model, self.config.api_key
        );
        // Resolved before the messages are moved out of `request`.
        let thinking_config = self.thinking_for(&request);

        let mut system_instruction_parts = Vec::new();
        let mut contents = Vec::new();

        // Gemini only supports "user" and "model" roles in contents.
        // System prompt goes into `systemInstruction`.
        for msg in request.messages {
            match msg.role {
                MessageRole::System => {
                    if let Some(text) = msg.content {
                        system_instruction_parts.push(Part {
                            text: Some(text),
                            function_call: None,
                            function_response: None,
                        });
                    }
                }
                MessageRole::User => {
                    if let Some(text) = msg.content {
                        contents.push(Content {
                            role: "user".to_string(),
                            parts: vec![Part {
                                text: Some(text),
                                function_call: None,
                                function_response: None,
                            }],
                        });
                    }
                }
                MessageRole::Assistant => {
                    let mut parts = Vec::new();
                    if let Some(text) = msg.content {
                        parts.push(Part {
                            text: Some(text),
                            function_call: None,
                            function_response: None,
                        });
                    }
                    if let Some(tool_calls) = msg.tool_calls {
                        for call in tool_calls {
                            parts.push(Part {
                                text: None,
                                function_call: Some(GeminiFunctionCall {
                                    name: call.name,
                                    args: serde_json::from_str(&call.arguments)
                                        .unwrap_or(serde_json::Value::Null),
                                }),
                                function_response: None,
                            });
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(Content {
                            role: "model".to_string(),
                            parts,
                        });
                    }
                }
                MessageRole::Tool => {
                    if let Some(name) = msg.name {
                        let response_val: serde_json::Value = if let Some(content) = msg.content {
                            serde_json::from_str(&content)
                                .unwrap_or(serde_json::Value::String(content))
                        } else {
                            serde_json::Value::Null
                        };
                        contents.push(Content {
                            role: "function".to_string(),
                            parts: vec![Part {
                                text: None,
                                function_call: None,
                                function_response: Some(GeminiFunctionResponse {
                                    name,
                                    response: response_val,
                                }),
                            }],
                        });
                    }
                }
            }
        }

        let system_instruction = if !system_instruction_parts.is_empty() {
            Some(SystemInstruction {
                parts: system_instruction_parts,
            })
        } else {
            None
        };

        let generation_config = GenerationConfig {
            temperature: request.temperature,
            max_output_tokens: request.max_tokens,
            response_mime_type: if request.force_json {
                Some("application/json".to_string())
            } else {
                None
            },
            thinking_config,
        };

        let tools = request.tools.map(|tools| {
            vec![GeminiTool {
                function_declarations: tools
                    .into_iter()
                    .map(|t| GeminiFunctionDeclaration {
                        name: t.name,
                        description: t.description,
                        parameters: t.parameters,
                    })
                    .collect(),
            }]
        });

        let api_request = GeminiGenerateContentRequest {
            system_instruction,
            contents,
            generation_config: Some(generation_config),
            tools,
        };

        debug!(
            model = %self.config.model,
            message_count = api_request.contents.len(),
            "Sending chat completion request to Gemini"
        );

        let response = self
            .send_generate_request(&url, api_request, "Gemini API error")
            .await?;

        let completion: GeminiGenerateContentResponse = response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse Gemini API response");
            LlmError::SerializationError(e.to_string())
        })?;

        let usage = completion.usage_metadata.map(super::TokenUsage::from);

        let candidates = completion.candidates.ok_or_else(|| {
            warn!("No candidates in Gemini API response");
            LlmError::ProviderError("No candidates in response".to_string())
        })?;

        let candidate = candidates.into_iter().next().ok_or_else(|| {
            warn!("Empty candidates array");
            LlmError::ProviderError("Empty candidates array".to_string())
        })?;

        let finish = candidate
            .finish_reason
            .as_deref()
            .map(super::FinishReason::from_wire);

        let response_content = candidate.content.ok_or_else(|| {
            warn!("No content in Gemini API candidate");
            LlmError::ProviderError("No content in response".to_string())
        })?;

        let parts = response_content.parts.unwrap_or_default();

        let mut message_content = None;
        let mut tool_calls = Vec::new();

        for part in parts {
            if let Some(text) = part.text {
                message_content = Some(text);
            }
            if let Some(call) = part.function_call {
                tool_calls.push(super::ToolCall {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: call.name,
                    arguments: serde_json::to_string(&call.args).unwrap_or_default(),
                });
            }
        }

        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        info!(
            has_content = message_content.is_some(),
            has_tools = tool_calls.is_some(),
            "Received chat completion response from Gemini"
        );

        Ok(LlmResponse {
            content: message_content,
            tool_calls,
            reasoning: None,
            usage,
            finish,
        })
    }

    async fn chat_completion_stream(
        &self,
        request: LlmRequest,
    ) -> Result<BoxStream<'static, Result<super::LlmStreamEvent, LlmError>>, LlmError> {
        let url = format!(
            "{}/{}:streamGenerateContent?alt=sse&key={}",
            self.config.base_url, self.config.model, self.config.api_key
        );
        // Resolved before the messages are moved out of `request`.
        let thinking_config = self.thinking_for(&request);

        let mut system_instruction_parts = Vec::new();
        let mut contents = Vec::new();

        for msg in request.messages {
            match msg.role {
                MessageRole::System => {
                    if let Some(text) = msg.content {
                        system_instruction_parts.push(Part {
                            text: Some(text),
                            function_call: None,
                            function_response: None,
                        });
                    }
                }
                MessageRole::User => {
                    if let Some(text) = msg.content {
                        contents.push(Content {
                            role: "user".to_string(),
                            parts: vec![Part {
                                text: Some(text),
                                function_call: None,
                                function_response: None,
                            }],
                        });
                    }
                }
                MessageRole::Assistant => {
                    let mut parts = Vec::new();
                    if let Some(text) = msg.content {
                        parts.push(Part {
                            text: Some(text),
                            function_call: None,
                            function_response: None,
                        });
                    }
                    if let Some(tool_calls) = msg.tool_calls {
                        for call in tool_calls {
                            parts.push(Part {
                                text: None,
                                function_call: Some(GeminiFunctionCall {
                                    name: call.name,
                                    args: serde_json::from_str(&call.arguments)
                                        .unwrap_or(serde_json::Value::Null),
                                }),
                                function_response: None,
                            });
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(Content {
                            role: "model".to_string(),
                            parts,
                        });
                    }
                }
                MessageRole::Tool => {
                    if let Some(name) = msg.name {
                        let response_val: serde_json::Value = if let Some(content) = msg.content {
                            serde_json::from_str(&content)
                                .unwrap_or(serde_json::Value::String(content))
                        } else {
                            serde_json::Value::Null
                        };
                        contents.push(Content {
                            role: "function".to_string(),
                            parts: vec![Part {
                                text: None,
                                function_call: None,
                                function_response: Some(GeminiFunctionResponse {
                                    name,
                                    response: response_val,
                                }),
                            }],
                        });
                    }
                }
            }
        }

        let system_instruction = if !system_instruction_parts.is_empty() {
            Some(SystemInstruction {
                parts: system_instruction_parts,
            })
        } else {
            None
        };

        let generation_config = GenerationConfig {
            temperature: request.temperature,
            max_output_tokens: request.max_tokens,
            response_mime_type: if request.force_json {
                Some("application/json".to_string())
            } else {
                None
            },
            thinking_config,
        };

        let tools = request.tools.map(|tools| {
            vec![GeminiTool {
                function_declarations: tools
                    .into_iter()
                    .map(|t| GeminiFunctionDeclaration {
                        name: t.name,
                        description: t.description,
                        parameters: t.parameters,
                    })
                    .collect(),
            }]
        });

        let api_request = GeminiGenerateContentRequest {
            system_instruction,
            contents,
            generation_config: Some(generation_config),
            tools,
        };

        debug!(
            model = %self.config.model,
            "Sending streaming chat completion request to Gemini"
        );

        let response = self
            .send_generate_request(&url, api_request, "Gemini stream error")
            .await?;

        let mut event_stream = response.bytes_stream().eventsource();

        let stream = async_stream::try_stream! {
            // Gemini repeats `usageMetadata` on every chunk with running totals,
            // so the last one seen is the answer. Held back and emitted once at
            // the end rather than yielded per chunk.
            let mut latest_usage: Option<super::TokenUsage> = None;

            while let Some(event_res) = event_stream.next().await {
                let event = match event_res {
                    Ok(e) => e,
                    Err(e) => {
                        yield Err(LlmError::NetworkError(format!("SSE error: {}", describe_transport_error(&e))))?;
                        continue;
                    }
                };

                let data = event.data;
                if data == "[DONE]" || data.is_empty() {
                    continue; // Skip empty keep-alive pings or done markers
                }

                let chunk: GeminiGenerateContentResponse = match serde_json::from_str(&data) {
                    Ok(c) => c,
                    Err(_e) => {
                        debug!("Skipping unparseable SSE data chunk: {}", data);
                        continue;
                    }
                };

                if let Some(usage) = chunk.usage_metadata {
                    let usage = super::TokenUsage::from(usage);
                    if !usage.is_empty() {
                        latest_usage = Some(usage);
                    }
                }

                if let Some(candidates) = chunk.candidates {
                    for candidate in candidates {
                        // Read before `content` is moved out of the candidate;
                        // yielded after the parts so the cut text still arrives
                        // ahead of the reason it was cut.
                        let finish = candidate
                            .finish_reason
                            .as_deref()
                            .map(super::FinishReason::from_wire);

                        if let Some(content) = candidate.content
                            && let Some(parts) = content.parts
                        {
                            for part in parts {
                                if let Some(text) = part.text
                                    && !text.is_empty()
                                {
                                    yield super::LlmStreamEvent::ContentChunk(text);
                                }

                                if let Some(call) = part.function_call {
                                    let id = uuid::Uuid::new_v4().to_string();
                                    let arguments = serde_json::to_string(&call.args).unwrap_or_default();

                                    yield super::LlmStreamEvent::ToolCallChunk {
                                        id: id.clone(),
                                        name: Some(call.name.clone()),
                                        arguments: arguments.clone(),
                                    };

                                    yield super::LlmStreamEvent::ToolCall(super::ToolCall {
                                        id,
                                        name: call.name,
                                        arguments,
                                    });
                                }
                            }
                        }

                        if let Some(reason) = finish {
                            yield super::LlmStreamEvent::Finish(reason);
                        }
                    }
                }
            }

            if let Some(usage) = latest_usage {
                yield super::LlmStreamEvent::Usage(usage);
            }
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatMessage, ReasoningEffort};

    fn provider(reasoning: Option<Reasoning>) -> GeminiProvider {
        GeminiProvider::new(GeminiConfig {
            base_url: GEMINI_BASE_URL.to_string(),
            model: "gemini-3-pro-preview".to_string(),
            api_key: "key".to_string(),
            reasoning,
        })
    }

    fn request(reasoning: Option<Reasoning>) -> LlmRequest {
        let request = LlmRequest::new(vec![ChatMessage::user("hi")]);
        match reasoning {
            Some(reasoning) => request.reasoning(reasoning),
            None => request,
        }
    }

    /// What `generationConfig` carries, serialized — the camelCase field names
    /// and the "send nothing" case are the wire's, not a restatement of the
    /// mapping.
    fn wire(provider: &GeminiProvider, request: &LlmRequest) -> serde_json::Value {
        let config = GenerationConfig {
            temperature: None,
            max_output_tokens: None,
            response_mime_type: None,
            thinking_config: provider.thinking_for(request),
        };
        serde_json::to_value(&config).expect("serializes")
    }

    /// Gemini's control lives one level down from `generationConfig`, and a level
    /// is what the 3.x models take.
    #[test]
    fn a_level_is_sent_as_a_thinking_level() {
        assert_eq!(
            wire(
                &provider(None),
                &request(Some(Reasoning::Effort(ReasoningEffort::Low)))
            ),
            serde_json::json!({ "thinkingConfig": { "thinkingLevel": "low" } })
        );
    }

    /// A budget has its own field, which is the one the 2.5-generation models
    /// take — and which the level is mutually exclusive with, so exactly one is
    /// ever set.
    #[test]
    fn a_budget_is_sent_as_a_thinking_budget() {
        assert_eq!(
            wire(&provider(None), &request(Some(Reasoning::Budget(2048)))),
            serde_json::json!({ "thinkingConfig": { "thinkingBudget": 2048 } })
        );
    }

    /// Zero is how the API spells "do not think"; there is no level for it.
    #[test]
    fn off_is_sent_as_a_budget_of_zero() {
        assert_eq!(
            wire(&provider(Some(Reasoning::Off)), &request(None)),
            serde_json::json!({ "thinkingConfig": { "thinkingBudget": 0 } })
        );
    }

    /// The configured model's setting covers requests that don't speak for
    /// themselves, and a request that does wins.
    #[test]
    fn a_request_overrides_the_configured_reasoning() {
        assert_eq!(
            wire(
                &provider(Some(Reasoning::Off)),
                &request(Some(Reasoning::Effort(ReasoningEffort::High)))
            ),
            serde_json::json!({ "thinkingConfig": { "thinkingLevel": "high" } })
        );
    }

    /// Which models take `thinkingLevel` is what Google's docs and the field
    /// reports disagree about, so a refusal is read back off the 400 — and then
    /// not paid for again.
    #[test]
    fn a_model_that_refused_the_field_is_not_asked_again() {
        let provider = provider(Some(Reasoning::Effort(ReasoningEffort::High)));
        assert!(provider.thinking_for(&request(None)).is_some());

        provider.reasoning_support.record_refusal();
        assert_eq!(
            wire(&provider, &request(None)),
            serde_json::json!({}),
            "a refusal is remembered, not rediscovered per call"
        );
    }

    #[test]
    fn nothing_is_sent_when_nobody_asked() {
        assert_eq!(wire(&provider(None), &request(None)), serde_json::json!({}));
    }

    /// The retry drops the field and keeps the request: the messages, tools and
    /// the rest of `generationConfig` are what the model was asked for.
    #[test]
    fn clearing_the_thinking_config_leaves_the_rest_of_the_request() {
        let mut body = GeminiGenerateContentRequest {
            system_instruction: None,
            contents: vec![Content {
                role: "user".to_string(),
                parts: vec![Part {
                    text: Some("hi".to_string()),
                    function_call: None,
                    function_response: None,
                }],
            }],
            generation_config: Some(GenerationConfig {
                temperature: Some(0.25),
                max_output_tokens: Some(64),
                response_mime_type: None,
                thinking_config: Some(thinking_config_for(Reasoning::Effort(
                    ReasoningEffort::High,
                ))),
            }),
            tools: None,
        };
        assert!(body.asks_for_thinking());

        body.clear_thinking();

        assert!(!body.asks_for_thinking());
        assert_eq!(
            serde_json::to_value(&body).expect("serializes"),
            serde_json::json!({
                "contents": [{ "role": "user", "parts": [{ "text": "hi" }] }],
                "generationConfig": { "temperature": 0.25, "maxOutputTokens": 64 },
            })
        );
    }

    /// Gemini spells the field `finishReason` and the values UPPER_SNAKE; a
    /// candidate cut at `maxOutputTokens` says `MAX_TOKENS` on an otherwise
    /// successful body, and may arrive with no content at all.
    #[test]
    fn a_candidate_carries_its_finish_reason() {
        let candidate: Candidate = serde_json::from_str(
            r#"{"content":{"parts":[{"text":"cut mid-"}],"role":"model"},
                "finishReason":"MAX_TOKENS"}"#,
        )
        .expect("parses");
        assert_eq!(candidate.finish_reason.as_deref(), Some("MAX_TOKENS"));
        assert_eq!(
            crate::FinishReason::from_wire("MAX_TOKENS"),
            crate::FinishReason::Length,
            "Gemini's spelling normalizes to the same variant as OpenAI's `length`"
        );
    }
}
