use super::{
    Env, LlmError, LlmProvider, LlmRequest, LlmResponse, MessageRole, Reasoning, ReasoningSupport,
    TokenUsage, classify_api_error, describe_transport_error, refuses_reasoning,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use serde::{Deserialize, Serialize};
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
    /// How this endpoint spells the reasoning parameter. The wire dialect is the
    /// adapter's business, not something every caller should have to ask about
    /// first.
    pub reasoning_dialect: ReasoningDialect,
    /// Reasoning applied to requests that don't ask for their own — the model's
    /// setting, configured where the model is. `None` sends nothing.
    pub reasoning: Option<Reasoning>,
    /// Whether this endpoint accepts `stream_options.include_usage`, which is
    /// what makes a streaming response report what it cost.
    ///
    /// Opt-in rather than always-on: OpenAI and OpenRouter take it, but local
    /// OpenAI-compatible servers vary, and one that rejects unknown parameters
    /// fails the whole call. A non-streaming response reports usage either way,
    /// so this only gates the streaming path.
    pub stream_usage: bool,
}

/// Which reasoning parameter an OpenAI-compatible endpoint takes.
///
/// The two dialects are not interchangeable: OpenRouter's is a `reasoning`
/// object that also carries a token budget, OpenAI's is a bare
/// `reasoning_effort` string with no budget at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReasoningDialect {
    /// OpenRouter's unified `reasoning` object, which it normalizes per model
    /// and ignores where a model cannot reason. See
    /// <https://openrouter.ai/docs/use-cases/reasoning-tokens>.
    OpenRouter,
    /// OpenAI's `reasoning_effort` string, which is what the compatible servers
    /// copied. Whether a *model* accepts it is only known from the 400 it
    /// answers with, which is what [`ReasoningSupport`] is for.
    #[default]
    OpenAi,
}

/// OpenAI's `reasoning_effort` for a requested [`Reasoning`], or `None` when the
/// setting has no spelling on that API.
///
/// [`Reasoning::Budget`] is the gap: Chat Completions has no reasoning-token cap
/// field, so a budget cannot be asked for there at all. Public so provider
/// selection can report that drop before any request is made, rather than
/// discovering it per call.
///
/// `Reasoning::Off` maps to `none`, which only models from gpt-5.1 on accept —
/// an older model refuses it, the request is retried without it, and a model
/// that does not reason satisfies "off" anyway.
pub fn reasoning_effort(reasoning: Reasoning) -> Option<&'static str> {
    match reasoning {
        Reasoning::Off => Some("none"),
        Reasoning::Effort(effort) => Some(effort.as_str()),
        Reasoning::Budget(_) => None,
    }
}

/// Default base URL for the OpenRouter API (OpenAI-compatible surface).
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// OpenAI's own endpoint. Named because it is the one OpenAI-compatible URL
/// whose parameter support is known rather than guessed — see
/// [`OpenAiConfig::stream_usage`].
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// Model used when neither a config nor `OPENROUTER_MODEL` names one. Shared by
/// both paths so they cannot default differently.
pub const OPENROUTER_DEFAULT_MODEL: &str = "z-ai/glm-4.6";

impl OpenAiConfig {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self::from_lookup(Env::os()))
    }

    /// Read an OpenAI-compatible config from `env`.
    ///
    /// Infallible: an OpenAI-compatible endpoint may legitimately want no key at
    /// all (a local Ollama), so there is nothing here that can be missing.
    pub(crate) fn from_lookup(env: Env<'_>) -> Self {
        Self {
            base_url: env
                .get("OPENAI_API_BASE_URL")
                .or_else(|| env.get("AI_API_BASE_URL"))
                .unwrap_or_else(|| "http://localhost:11434/v1".to_string()),
            model: env
                .get("OPENAI_MODEL")
                .or_else(|| env.get("AI_MODEL"))
                .unwrap_or_else(|| "ministral".to_string()),
            api_key: env.get("OPENAI_API_KEY").or_else(|| env.get("AI_API_KEY")),
            extra_headers: Vec::new(),
            reasoning_dialect: ReasoningDialect::OpenAi,
            reasoning: None,
            // This path defaults to a local server (Ollama), which is exactly
            // the population that varies on `stream_options`.
            stream_usage: false,
        }
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
            reasoning_dialect: ReasoningDialect::OpenRouter,
            reasoning: None,
            stream_usage: true,
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
        Self::openrouter_from_lookup(Env::os())
    }

    /// Read an OpenRouter config from `env`. See [`Self::openrouter_from_env`].
    pub(crate) fn openrouter_from_lookup(env: Env<'_>) -> Result<Self, String> {
        let api_key = env
            .get("OPENROUTER_API_KEY")
            .ok_or_else(|| "OPENROUTER_API_KEY environment variable is required".to_string())?;

        let model = env
            .get("OPENROUTER_MODEL")
            .unwrap_or_else(|| OPENROUTER_DEFAULT_MODEL.to_string());
        let reasoning = env
            .get("OPENROUTER_REASONING")
            .map(|value| {
                value
                    .parse::<Reasoning>()
                    .map_err(|e| format!("OPENROUTER_REASONING: {e}"))
            })
            .transpose()?;

        Ok(Self {
            reasoning,
            ..Self::openrouter(
                api_key,
                model,
                env.get("OPENROUTER_API_BASE_URL"),
                env.get("OPENROUTER_HTTP_REFERER"),
                env.get("OPENROUTER_X_TITLE"),
            )
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
    /// Asks a streaming response to report usage in its final chunk. Only sent
    /// when `OpenAiConfig::stream_usage` says the endpoint accepts it.
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
    /// OpenRouter's unified reasoning control. At most one of this and
    /// `reasoning_effort` is ever set — see [`ReasoningParam`].
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<OpenRouterReasoning>,
    /// OpenAI's reasoning control.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'static str>,
}

impl OpenAiChatRequest {
    /// Put `param` on the request in whichever field its dialect uses.
    fn with_reasoning(mut self, param: Option<ReasoningParam>) -> Self {
        match param {
            Some(ReasoningParam::Unified(reasoning)) => self.reasoning = Some(reasoning),
            Some(ReasoningParam::Effort(effort)) => self.reasoning_effort = Some(effort),
            None => {}
        }
        self
    }

    /// Whether this request asks for reasoning at all, in either dialect.
    fn asks_for_reasoning(&self) -> bool {
        self.reasoning.is_some() || self.reasoning_effort.is_some()
    }

    /// Drop the reasoning parameter, for the retry after an endpoint refuses it.
    fn clear_reasoning(&mut self) {
        self.reasoning = None;
        self.reasoning_effort = None;
    }
}

/// The reasoning parameter as one endpoint's dialect spells it.
enum ReasoningParam {
    /// OpenRouter's `reasoning` object.
    Unified(OpenRouterReasoning),
    /// OpenAI's `reasoning_effort` string.
    Effort(&'static str),
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// OpenAI's `usage` block. Present on every non-streaming response, and on the
/// final streaming chunk when `stream_options.include_usage` was sent.
#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
    /// OpenAI/OpenRouter break reasoning out here; absent on most others.
    completion_tokens_details: Option<OpenAiCompletionDetails>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompletionDetails {
    reasoning_tokens: Option<u32>,
}

impl From<OpenAiUsage> for TokenUsage {
    fn from(usage: OpenAiUsage) -> Self {
        Self {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            reasoning_tokens: usage
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens),
            total_tokens: usage.total_tokens,
        }
    }
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
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: OpenAiChatMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChunk {
    /// Empty on the final usage-only chunk, which is why this defaults rather
    /// than failing the parse.
    #[serde(default)]
    choices: Vec<StreamChoice>,
    usage: Option<OpenAiUsage>,
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
    /// Whether this endpoint's model has already refused the reasoning
    /// parameter. Shared across clones.
    reasoning_support: ReasoningSupport,
}

/// The one word every message about a refused `reasoning` / `reasoning_effort`
/// contains, in either dialect.
const REASONING_FIELD: &str = "reasoning";

impl OpenAiProvider {
    pub fn new(config: OpenAiConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            reasoning_support: ReasoningSupport::default(),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        let config = OpenAiConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// What reasoning parameter this request carries, if any.
    ///
    /// The request's own setting wins over the model's configured default. A
    /// budget is dropped on the OpenAI dialect, which has no field for one, and
    /// so is everything once the model has refused the parameter — the point of
    /// remembering that is not to keep paying a round trip to learn it again.
    fn reasoning_for(&self, request: &LlmRequest) -> Option<ReasoningParam> {
        let reasoning = request.reasoning.or(self.config.reasoning)?;
        if self.reasoning_support.refused() {
            return None;
        }
        match self.config.reasoning_dialect {
            ReasoningDialect::OpenRouter => Some(ReasoningParam::Unified(reasoning.into())),
            ReasoningDialect::OpenAi => match reasoning_effort(reasoning) {
                Some(effort) => Some(ReasoningParam::Effort(effort)),
                None => {
                    debug!(
                        base_url = %self.config.base_url,
                        %reasoning,
                        "`reasoning_effort` has no field for a token budget; sending the request without it"
                    );
                    None
                }
            },
        }
    }

    /// POST `body`, with the configured key and attribution headers.
    async fn post(
        &self,
        url: &str,
        body: &OpenAiChatRequest,
    ) -> Result<reqwest::Response, LlmError> {
        let mut req_builder = self.client.post(url).json(body);

        if let Some(ref api_key) = self.config.api_key {
            req_builder = req_builder.bearer_auth(api_key);
        }

        for (name, value) in &self.config.extra_headers {
            req_builder = req_builder.header(name.as_str(), value.as_str());
        }

        req_builder.send().await.map_err(|e| {
            error!(url = %url, error = %e, "Failed to send request to OpenAI API");
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
        error!(status = %status, error = %error_text, "OpenAI API returned error");
        classify_api_error(format!("{label} ({status}): {error_text}"))
    }

    /// POST `body`, and if the model refuses the reasoning parameter it carries,
    /// send it once more without one.
    ///
    /// Which models take `reasoning_effort` is not knowable from the endpoint,
    /// so the alternative to this is a model-name table that is wrong about
    /// every model released after it was written. Nothing was generated on a
    /// 400, so the retry costs a round trip and no tokens, and only the first
    /// refused call pays it.
    async fn send_chat_request(
        &self,
        url: &str,
        mut body: OpenAiChatRequest,
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

        if !(body.asks_for_reasoning()
            && refuses_reasoning(status.as_u16(), &error_text, REASONING_FIELD))
        {
            error!(status = %status, error = %error_text, "OpenAI API returned error");
            return Err(classify_api_error(format!(
                "{label} ({status}): {error_text}"
            )));
        }

        warn!(
            model = %self.config.model,
            %status,
            error = %error_text,
            "model refused the reasoning parameter; retrying without it"
        );
        body.clear_reasoning();

        let retried = self.post(url, &body).await?;
        if retried.status().is_success() {
            // Dropping it fixed the call, so the parameter was the problem and
            // this endpoint gets no more of them. A retry that fails the same
            // way says the 400 was about something else, and remembering it
            // would disable reasoning for the rest of the process over an
            // unrelated failure.
            self.reasoning_support.record_refusal();
            Ok(retried)
        } else {
            Err(self.api_error(label, retried).await)
        }
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
            stream_options: None,
            reasoning: None,
            reasoning_effort: None,
        }
        .with_reasoning(reasoning);

        debug!(
            model = %self.config.model,
            url = %url,
            message_count = api_request.messages.len(),
            "Sending chat completion request"
        );

        let response = self
            .send_chat_request(&url, api_request, "OpenAI API error")
            .await?;

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
            usage: completion.usage.map(TokenUsage::from),
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
            stream_options: self.config.stream_usage.then_some(StreamOptions {
                include_usage: true,
            }),
            reasoning: None,
            reasoning_effort: None,
        }
        .with_reasoning(reasoning);

        debug!(
            model = %self.config.model,
            url = %url,
            "Sending streaming chat completion request"
        );

        let response = self
            .send_chat_request(&url, api_request, "OpenAI stream error")
            .await?;

        let mut event_stream = response.bytes_stream().eventsource();

        let stream = async_stream::try_stream! {
            // Track partial tool calls by index
            let mut pending_tools: std::collections::HashMap<u32, super::ToolCall> = std::collections::HashMap::new();

            while let Some(event_res) = event_stream.next().await {
                let event = match event_res {
                    Ok(e) => e,
                    Err(e) => {
                        yield Err(LlmError::NetworkError(format!("SSE error: {}", describe_transport_error(&e))))?;
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

                // Arrives in the final chunk, which carries no choices. Emitted
                // before the loop below so a chunk that somehow carries both
                // still reports content first.
                if let Some(usage) = chunk.usage {
                    let usage = TokenUsage::from(usage);
                    if !usage.is_empty() {
                        yield super::LlmStreamEvent::Usage(usage);
                    }
                }

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
    use crate::{ChatMessage, Reasoning, ReasoningEffort};

    fn provider(dialect: ReasoningDialect, reasoning: Option<Reasoning>) -> OpenAiProvider {
        OpenAiProvider::new(OpenAiConfig {
            base_url: "http://localhost/v1".to_string(),
            model: "test-model".to_string(),
            api_key: None,
            extra_headers: Vec::new(),
            reasoning_dialect: dialect,
            reasoning,
            stream_usage: false,
        })
    }

    fn request(reasoning: Option<Reasoning>) -> LlmRequest {
        let request = LlmRequest::new(vec![ChatMessage::user("hi")]);
        match reasoning {
            Some(reasoning) => request.reasoning(reasoning),
            None => request,
        }
    }

    /// Whatever the request body carries about reasoning, in whichever dialect —
    /// serialized, so the field names and the "send nothing" case are the wire's
    /// and not a restatement of the mapping.
    fn wire(provider: &OpenAiProvider, request: &LlmRequest) -> serde_json::Value {
        let body = OpenAiChatRequest {
            model: provider.config.model.clone(),
            messages: Vec::new(),
            temperature: None,
            max_tokens: None,
            response_format: None,
            tools: None,
            stream: None,
            stream_options: None,
            reasoning: None,
            reasoning_effort: None,
        }
        .with_reasoning(provider.reasoning_for(request));

        let mut value = serde_json::to_value(&body).expect("serializes");
        value
            .as_object_mut()
            .expect("an object")
            .retain(|key, _| key.starts_with("reasoning"));
        value
    }

    /// The configured model's setting applies to every request that doesn't
    /// speak for itself — that is what makes `[llm] reasoning` a property of the
    /// model rather than something each handler has to remember to pass.
    #[test]
    fn the_configured_reasoning_applies_when_a_request_asks_for_nothing() {
        let provider = provider(ReasoningDialect::OpenRouter, Some(Reasoning::Off));
        assert_eq!(
            wire(&provider, &request(None)),
            serde_json::json!({ "reasoning": { "enabled": false } })
        );
    }

    /// …and a request that does ask overrides it, so a caller with a reason
    /// (`complex_agent` streaming its thinking) is not overruled by config.
    #[test]
    fn a_request_overrides_the_configured_reasoning() {
        let provider = provider(ReasoningDialect::OpenRouter, Some(Reasoning::Off));
        assert_eq!(
            wire(
                &provider,
                &request(Some(Reasoning::Effort(ReasoningEffort::High)))
            ),
            serde_json::json!({ "reasoning": { "effort": "high", "enabled": true } })
        );
    }

    #[test]
    fn a_budget_is_sent_as_a_reasoning_token_cap() {
        let provider = provider(ReasoningDialect::OpenRouter, None);
        assert_eq!(
            wire(&provider, &request(Some(Reasoning::Budget(2000)))),
            serde_json::json!({ "reasoning": { "max_tokens": 2000, "enabled": true } })
        );
    }

    /// OpenAI's own parameter is a bare string beside the messages, not an
    /// object — sending OpenRouter's shape there is a 400.
    #[test]
    fn the_openai_dialect_sends_a_bare_effort_string() {
        let provider = provider(ReasoningDialect::OpenAi, None);
        assert_eq!(
            wire(
                &provider,
                &request(Some(Reasoning::Effort(ReasoningEffort::Medium)))
            ),
            serde_json::json!({ "reasoning_effort": "medium" })
        );
    }

    /// `off` has a spelling on this API too, and it is the one an operator with
    /// a small fast model wants. Only gpt-5.1 and later accept it; an older
    /// model refuses it and the request is retried without it, which leaves a
    /// non-reasoning model doing what was asked anyway.
    #[test]
    fn turning_thinking_off_is_asked_for_as_none() {
        let provider = provider(ReasoningDialect::OpenAi, Some(Reasoning::Off));
        assert_eq!(
            wire(&provider, &request(None)),
            serde_json::json!({ "reasoning_effort": "none" })
        );
    }

    /// Chat Completions has no reasoning-token cap at all, so a budget cannot be
    /// asked for — and a request carrying one must still be answerable.
    #[test]
    fn a_token_budget_has_no_openai_spelling_and_is_not_sent() {
        let provider = provider(ReasoningDialect::OpenAi, None);
        assert_eq!(
            wire(&provider, &request(Some(Reasoning::Budget(2000)))),
            serde_json::json!({}),
            "nothing may be sent for a setting this API cannot express"
        );
    }

    /// The two dialects are mutually exclusive: a body carrying both is a 400 on
    /// either endpoint.
    #[test]
    fn only_one_dialect_reaches_the_body() {
        for dialect in [ReasoningDialect::OpenRouter, ReasoningDialect::OpenAi] {
            let provider = provider(dialect, None);
            let sent = wire(
                &provider,
                &request(Some(Reasoning::Effort(ReasoningEffort::Low))),
            );
            assert_eq!(
                sent.as_object().expect("an object").len(),
                1,
                "exactly one reasoning field for {dialect:?}, got {sent}"
            );
        }
    }

    /// Whether a model takes the parameter is only knowable from the 400 it
    /// answers with. Once one has come back, sending it again would buy a wasted
    /// round trip on every call for the life of the process.
    #[test]
    fn a_model_that_refused_the_parameter_is_not_asked_again() {
        let provider = provider(
            ReasoningDialect::OpenAi,
            Some(Reasoning::Effort(ReasoningEffort::High)),
        );
        assert_eq!(
            wire(&provider, &request(None)),
            serde_json::json!({ "reasoning_effort": "high" })
        );

        provider.reasoning_support.record_refusal();
        assert_eq!(
            wire(&provider, &request(None)),
            serde_json::json!({}),
            "a refusal is remembered, not rediscovered per call"
        );
    }

    /// The memory is shared by clones: a provider is cloned per handler and they
    /// all call the same model.
    #[test]
    fn the_refusal_is_shared_across_clones() {
        let provider = provider(
            ReasoningDialect::OpenAi,
            Some(Reasoning::Effort(ReasoningEffort::High)),
        );
        let clone = provider.clone();
        provider.reasoning_support.record_refusal();
        assert_eq!(wire(&clone, &request(None)), serde_json::json!({}));
    }

    #[test]
    fn nothing_is_sent_when_nobody_asked() {
        for dialect in [ReasoningDialect::OpenRouter, ReasoningDialect::OpenAi] {
            let provider = provider(dialect, None);
            assert_eq!(
                wire(&provider, &request(None)),
                serde_json::json!({}),
                "for {dialect:?}"
            );
        }
    }
}
