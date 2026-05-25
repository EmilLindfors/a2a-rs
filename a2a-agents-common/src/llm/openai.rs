use super::{LlmError, LlmProvider, LlmRequest, LlmResponse, MessageRole};
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
}

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
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat_completion(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/chat/completions", self.config.base_url);

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

        info!(
            has_content = message_content.is_some(),
            has_tools = tool_calls.is_some(),
            "Received chat completion response"
        );

        Ok(LlmResponse {
            content: message_content,
            tool_calls,
        })
    }

    async fn chat_completion_stream(
        &self,
        request: LlmRequest,
    ) -> Result<BoxStream<'static, Result<super::LlmStreamEvent, LlmError>>, LlmError> {
        let url = format!("{}/chat/completions", self.config.base_url);

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
                    if let Some(content) = choice.delta.content {
                        if !content.is_empty() {
                            yield super::LlmStreamEvent::ContentChunk(content);
                        }
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

                            if let Some(f) = call.function {
                                if let Some(args) = f.arguments {
                                    new_args = args.clone();
                                    entry.arguments.push_str(&args);
                                }
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
