use super::{LlmError, LlmProvider, LlmRequest, LlmResponse, MessageRole, Reasoning};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use serde::{Deserialize, Serialize};
use std::env;
use tracing::{debug, error, info, warn};

/// Configuration for the OpenAI-compatible AI client
#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    /// Extra HTTP headers attached to every request. Kept provider-agnostic so
    /// the OpenAI-compatible adapter carries e.g. OpenRouter's `HTTP-Referer` /
    /// `X-Title` attribution headers without knowing what they mean.
    pub extra_headers: Vec<(String, String)>,
    /// Whether this endpoint speaks OpenRouter's `reasoning` request object
    /// (true for OpenRouter, false for plain OpenAI / local servers, which
    /// reject unknown parameters). Requests that ask for reasoning are sent
    /// without it elsewhere — the wire dialect is the adapter's business, not
    /// something every caller should have to ask about first.
    pub supports_reasoning: bool,
    /// Reasoning applied to requests that don't ask for their own — the model's
    /// setting, configured where the model is. `None` sends nothing.
    pub reasoning: Option<Reasoning>,
}

/// Default base URL for the OpenRouter API (OpenAI-compatible surface).
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

impl OpenAiConfig {
    pub fn from_env() -> Result<Self, String> {
        let base_url = env::var("OPENAI_API_BASE_URL")
            .or_else(|_| env::var("AI_API_BASE_URL"))
            .unwrap_or_else(|_| "http://localhost:11434/v1".to_string());

        let model = env::var("OPENAI_MODEL")
            .or_else(|_| env::var("AI_MODEL"))
            .unwrap_or_else(|_| "ministral".to_string());

        let api_key = env::var("OPENAI_API_KEY")
            .or_else(|_| env::var("AI_API_KEY"))
            .ok()
            .and_then(|key| {
                let trimmed = key.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });

        Ok(Self {
            base_url,
            model,
            api_key,
            extra_headers: Vec::new(),
            supports_reasoning: false,
            reasoning: None,
        })
    }

    /// Build an OpenRouter config (OpenAI-compatible) from explicit values.
    ///
    /// `base_url` defaults to [`OPENROUTER_BASE_URL`] when `None`. The optional
    /// `http_referer` / `x_title` become OpenRouter attribution headers and are
    /// only sent when provided.
    pub fn openrouter(
        api_key: String,
        model: String,
        base_url: Option<String>,
        http_referer: Option<String>,
        x_title: Option<String>,
    ) -> Self {
        let mut extra_headers = Vec::new();
        if let Some(referer) = http_referer {
            extra_headers.push(("HTTP-Referer".to_string(), referer));
        }
        if let Some(title) = x_title {
            extra_headers.push(("X-Title".to_string(), title));
        }
        Self {
            base_url: base_url.unwrap_or_else(|| OPENROUTER_BASE_URL.to_string()),
            model,
            api_key: Some(api_key),
            extra_headers,
            supports_reasoning: true,
            reasoning: None,
        }
    }

    /// Read OpenRouter config from the environment.
    ///
    /// `OPENROUTER_API_KEY` is required; the rest fall back to defaults:
    /// `OPENROUTER_MODEL` (`z-ai/glm-4.6`), `OPENROUTER_API_BASE_URL`
    /// ([`OPENROUTER_BASE_URL`]), plus optional `OPENROUTER_HTTP_REFERER` /
    /// `OPENROUTER_X_TITLE` attribution headers and `OPENROUTER_REASONING`
    /// (`off`, `low`, `medium`, `high`, or a token budget — see [`Reasoning`]).
    /// An unreadable `OPENROUTER_REASONING` is an error rather than a default,
    /// because the value costs money in both directions.
    pub fn openrouter_from_env() -> Result<Self, String> {
        let api_key = env::var("OPENROUTER_API_KEY")
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| "OPENROUTER_API_KEY environment variable is required".to_string())?;

        let model = env::var("OPENROUTER_MODEL").unwrap_or_else(|_| "z-ai/glm-4.6".to_string());
        let base_url = env::var("OPENROUTER_API_BASE_URL").ok();
        let http_referer = env::var("OPENROUTER_HTTP_REFERER").ok();
        let x_title = env::var("OPENROUTER_X_TITLE").ok();
        let reasoning = match env::var("OPENROUTER_REASONING") {
            Ok(value) => Some(
                value
                    .parse::<Reasoning>()
                    .map_err(|e| format!("OPENROUTER_REASONING: {e}"))?,
            ),
            Err(_) => None,
        };

        Ok(Self {
            reasoning,
            ..Self::openrouter(api_key, model, base_url, http_referer, x_title)
        })
    }
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    /// OpenRouter's unified reasoning control, as resolved by
    /// [`OpenAiProvider::reasoning_for`] — the request's own setting, else the
    /// configured model default, and never for an endpoint that has no such
    /// field.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<OpenRouterReasoning>,
}

/// OpenRouter's `reasoning` request object (see
/// <https://openrouter.ai/docs/use-cases/reasoning-tokens>).
#[derive(Debug, Serialize)]
struct OpenRouterReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    enabled: bool,
}

impl From<Reasoning> for OpenRouterReasoning {
    fn from(reasoning: Reasoning) -> Self {
        let (effort, max_tokens, enabled) = match reasoning {
            Reasoning::Off => (None, None, false),
            Reasoning::Effort(effort) => (Some(effort.as_str()), None, true),
            Reasoning::Budget(max_tokens) => (None, Some(max_tokens), true),
        };
        Self {
            effort,
            max_tokens,
            enabled,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Reasoning-model thinking, as normalized by OpenRouter. Response-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning: Option<String>,
    /// Raw Zhipu/GLM reasoning field (used when not going through OpenRouter's
    /// normalization). Response-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: OpenAiChatMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    index: u32,
    id: Option<String>,
    function: Option<StreamFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct StreamFunctionCall {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Clone)]
pub struct OpenAiProvider {
    config: OpenAiConfig,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(config: OpenAiConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        let config = OpenAiConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// What to put in the request's `reasoning` field, if anything.
    ///
    /// The request wins over the configured default, and an endpoint that does
    /// not speak the parameter carries neither — sending it to plain OpenAI
    /// fails the whole call, so a caller asking for reasoning it cannot have
    /// gets an answer, not an error.
    fn reasoning_for(&self, request: &LlmRequest) -> Option<OpenRouterReasoning> {
        let reasoning = request.reasoning.or(self.config.reasoning)?;
        if !self.config.supports_reasoning {
            debug!(
                base_url = %self.config.base_url,
                %reasoning,
                "endpoint does not accept a reasoning parameter; sending the request without it"
            );
            return None;
        }
        Some(reasoning.into())
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat_completion(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let reasoning = self.reasoning_for(&request);

        let response_format = if request.force_json {
            Some(ResponseFormat {
                format_type: "json_object".to_string(),
            })
        } else {
            None
        };

        let messages = request
            .messages
            .into_iter()
            .map(|msg| OpenAiChatMessage {
                role: match msg.role {
                    MessageRole::System => "system".to_string(),
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                    MessageRole::Tool => "tool".to_string(),
                },
                content: msg.content,
                tool_calls: msg.tool_calls.map(|calls| {
                    calls
                        .into_iter()
                        .map(|c| OpenAiToolCall {
                            id: c.id,
                            tool_type: "function".to_string(),
                            function: OpenAiFunctionCall {
                                name: c.name,
                                arguments: c.arguments,
                            },
                        })
                        .collect()
                }),
                tool_call_id: msg.tool_call_id,
                name: msg.name,
                reasoning: None,
                reasoning_content: None,
            })
            .collect();

        let tools = request.tools.map(|tools| {
            tools
                .into_iter()
                .map(|t| OpenAiTool {
                    tool_type: "function".to_string(),
                    function: OpenAiFunction {
                        name: t.name,
                        description: t.description,
                        parameters: t.parameters,
                    },
                })
                .collect()
        });

        let api_request = OpenAiChatRequest {
            model: self.config.model.clone(),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            response_format,
            tools,
            stream: None,
            reasoning,
        };

        debug!(
            model = %self.config.model,
            url = %url,
            message_count = api_request.messages.len(),
            "Sending chat completion request"
        );

        let mut req_builder = self.client.post(&url).json(&api_request);

        if let Some(ref api_key) = self.config.api_key {
            req_builder = req_builder.bearer_auth(api_key);
        }

        for (name, value) in &self.config.extra_headers {
            req_builder = req_builder.header(name.as_str(), value.as_str());
        }

        let response = req_builder.send().await.map_err(|e| {
            error!(error = %e, "Failed to send request to OpenAI API");
            LlmError::NetworkError(e.to_string())
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(status = %status, error = %error_text, "OpenAI API returned error");
            return Err(LlmError::ApiError(format!(
                "OpenAI API error ({}): {}",
                status, error_text
            )));
        }

        let completion: OpenAiChatResponse = response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse OpenAI API response");
            LlmError::SerializationError(e.to_string())
        })?;

        let choice = completion.choices.into_iter().next().ok_or_else(|| {
            warn!("No choices in OpenAI API response");
            LlmError::ProviderError("No response from AI".to_string())
        })?;

        let tool_calls = choice.message.tool_calls.map(|calls| {
            calls
                .into_iter()
                .map(|c| super::ToolCall {
                    id: c.id,
                    name: c.function.name,
                    arguments: c.function.arguments,
                })
                .collect()
        });

        let message_content = choice.message.content;
        let reasoning = choice
            .message
            .reasoning
            .or(choice.message.reasoning_content);

        info!(
            has_content = message_content.is_some(),
            has_tools = tool_calls.is_some(),
            has_reasoning = reasoning.is_some(),
            "Received chat completion response"
        );

        Ok(LlmResponse {
            content: message_content,
            tool_calls,
            reasoning,
        })
    }

    async fn chat_completion_stream(
        &self,
        request: LlmRequest,
    ) -> Result<BoxStream<'static, Result<super::LlmStreamEvent, LlmError>>, LlmError> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let reasoning = self.reasoning_for(&request);

        let response_format = if request.force_json {
            Some(ResponseFormat {
                format_type: "json_object".to_string(),
            })
        } else {
            None
        };

        let messages: Vec<OpenAiChatMessage> = request
            .messages
            .into_iter()
            .map(|msg| OpenAiChatMessage {
                role: match msg.role {
                    MessageRole::System => "system".to_string(),
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                    MessageRole::Tool => "tool".to_string(),
                },
                content: msg.content,
                tool_calls: msg.tool_calls.map(|calls| {
                    calls
                        .into_iter()
                        .map(|c| OpenAiToolCall {
                            id: c.id,
                            tool_type: "function".to_string(),
                            function: OpenAiFunctionCall {
                                name: c.name,
                                arguments: c.arguments,
                            },
                        })
                        .collect()
                }),
                tool_call_id: msg.tool_call_id,
                name: msg.name,
                reasoning: None,
                reasoning_content: None,
            })
            .collect();

        let tools = request.tools.map(|tools| {
            tools
                .into_iter()
                .map(|t| OpenAiTool {
                    tool_type: "function".to_string(),
                    function: OpenAiFunction {
                        name: t.name,
                        description: t.description,
                        parameters: t.parameters,
                    },
                })
                .collect()
        });

        let api_request = OpenAiChatRequest {
            model: self.config.model.clone(),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            response_format,
            tools,
            stream: Some(true),
            reasoning,
        };

        debug!(
            model = %self.config.model,
            url = %url,
            "Sending streaming chat completion request"
        );

        let mut req_builder = self.client.post(&url).json(&api_request);

        if let Some(ref api_key) = self.config.api_key {
            req_builder = req_builder.bearer_auth(api_key);
        }

        for (name, value) in &self.config.extra_headers {
            req_builder = req_builder.header(name.as_str(), value.as_str());
        }

        let response = req_builder.send().await.map_err(|e| {
            error!(error = %e, "Failed to send streaming request to OpenAI API");
            LlmError::NetworkError(e.to_string())
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(status = %status, error = %error_text, "OpenAI API returned error on stream");
            return Err(LlmError::ApiError(format!(
                "OpenAI stream error ({}): {}",
                status, error_text
            )));
        }

        let mut event_stream = response.bytes_stream().eventsource();

        let stream = async_stream::try_stream! {
            // Track partial tool calls by index
            let mut pending_tools: std::collections::HashMap<u32, super::ToolCall> = std::collections::HashMap::new();

            while let Some(event_res) = event_stream.next().await {
                let event = match event_res {
                    Ok(e) => e,
                    Err(e) => {
                        yield Err(LlmError::NetworkError(format!("SSE error: {}", e)))?;
                        continue;
                    }
                };

                let data = event.data;
                if data == "[DONE]" {
                    // Flush any pending tool calls before exiting
                    let mut indices: Vec<u32> = pending_tools.keys().copied().collect();
                    indices.sort_unstable();
                    for idx in indices {
                        if let Some(tool_call) = pending_tools.remove(&idx) {
                            yield super::LlmStreamEvent::ToolCall(tool_call);
                        }
                    }
                    break;
                }

                let chunk: OpenAiStreamChunk = match serde_json::from_str(&data) {
                    Ok(c) => c,
                    Err(_e) => {
                        // Sometimes providers send non-JSON ping events, just ignore if parsing fails
                        debug!("Skipping unparseable SSE data chunk: {}", data);
                        continue;
                    }
                };

                for choice in chunk.choices {
                    if let Some(reasoning) = choice.delta.reasoning.or(choice.delta.reasoning_content)
                        && !reasoning.is_empty()
                    {
                        yield super::LlmStreamEvent::Reasoning(reasoning);
                    }

                    if let Some(content) = choice.delta.content
                        && !content.is_empty()
                    {
                        yield super::LlmStreamEvent::ContentChunk(content);
                    }

                    if let Some(tool_calls) = choice.delta.tool_calls {
                        for call in tool_calls {
                            let idx = call.index;

                            let mut new_args = String::new();
                            let mut tool_name = None;

                            // If we see a new tool call but haven't flushed a previous one, it's possible
                            // the previous one is done. Let's not flush immediately unless we are sure it's done.
                            // OpenAI gives us tool calls grouped by index over time.
                            let entry = pending_tools.entry(idx).or_insert_with(|| {
                                let name = call.function.as_ref().and_then(|f| f.name.clone()).unwrap_or_default();
                                tool_name = Some(name.clone());
                                super::ToolCall {
                                    id: call.id.clone().unwrap_or_default(),
                                    name,
                                    arguments: String::new(),
                                }
                            });

                            if let Some(f) = call.function
                                && let Some(args) = f.arguments
                            {
                                new_args = args.clone();
                                entry.arguments.push_str(&args);
                            }

                            yield super::LlmStreamEvent::ToolCallChunk {
                                id: entry.id.clone(),
                                name: tool_name,
                                arguments: new_args,
                            };
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatMessage, Reasoning, ReasoningEffort};

    fn provider(supports_reasoning: bool, reasoning: Option<Reasoning>) -> OpenAiProvider {
        OpenAiProvider::new(OpenAiConfig {
            base_url: "http://localhost/v1".to_string(),
            model: "test-model".to_string(),
            api_key: None,
            extra_headers: Vec::new(),
            supports_reasoning,
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

    fn wire(reasoning: Option<OpenRouterReasoning>) -> serde_json::Value {
        serde_json::to_value(reasoning).expect("serializes")
    }

    /// The configured model's setting applies to every request that doesn't
    /// speak for itself — that is what makes `[llm] reasoning` a property of the
    /// model rather than something each handler has to remember to pass.
    #[test]
    fn the_configured_reasoning_applies_when_a_request_asks_for_nothing() {
        let sent = provider(true, Some(Reasoning::Off)).reasoning_for(&request(None));
        assert_eq!(wire(sent), serde_json::json!({ "enabled": false }));
    }

    /// …and a request that does ask overrides it, so a caller with a reason
    /// (`complex_agent` streaming its thinking) is not overruled by config.
    #[test]
    fn a_request_overrides_the_configured_reasoning() {
        let sent = provider(true, Some(Reasoning::Off))
            .reasoning_for(&request(Some(Reasoning::Effort(ReasoningEffort::High))));
        assert_eq!(
            wire(sent),
            serde_json::json!({ "effort": "high", "enabled": true })
        );
    }

    #[test]
    fn a_budget_is_sent_as_a_reasoning_token_cap() {
        let sent = provider(true, None).reasoning_for(&request(Some(Reasoning::Budget(2000))));
        assert_eq!(
            wire(sent),
            serde_json::json!({ "max_tokens": 2000, "enabled": true })
        );
    }

    /// Plain OpenAI and local servers reject unknown parameters, so a request
    /// carrying reasoning must still be *answerable* there: the adapter drops
    /// the parameter rather than failing the call or making every caller check
    /// first.
    #[test]
    fn an_endpoint_without_the_parameter_sends_the_request_without_it() {
        let sent = provider(false, Some(Reasoning::Effort(ReasoningEffort::High)))
            .reasoning_for(&request(Some(Reasoning::Budget(2000))));
        assert!(
            sent.is_none(),
            "nothing may be sent to an endpoint that has no such field"
        );
    }

    #[test]
    fn nothing_is_sent_when_nobody_asked() {
        assert!(provider(true, None).reasoning_for(&request(None)).is_none());
    }
}
