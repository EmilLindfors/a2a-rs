use super::{LlmError, LlmProvider, LlmRequest, LlmResponse, MessageRole};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{stream::BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use std::env;
use tracing::{debug, error, info, warn};

/// Configuration for the Gemini AI client
#[derive(Debug, Clone)]
pub struct GeminiConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

impl GeminiConfig {
    pub fn from_env() -> Result<Self, String> {
        let base_url = env::var("GEMINI_API_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta/models".to_string());

        let model = env::var("GEMINI_MODEL")
            .unwrap_or_else(|_| "gemini-1.5-pro".to_string());

        let api_key = env::var("GEMINI_API_KEY").map_err(|_| {
            "GEMINI_API_KEY environment variable is required".to_string()
        })?;

        Ok(Self {
            base_url,
            model,
            api_key,
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

#[derive(Debug, Deserialize)]
struct GeminiGenerateContentResponse {
    candidates: Option<Vec<Candidate>>,
    #[serde(rename = "promptFeedback")]
    _prompt_feedback: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Option<ResponseContent>,
    #[serde(rename = "finishReason")]
    _finish_reason: Option<String>,
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
}

impl GeminiProvider {
    pub fn new(config: GeminiConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        let config = GeminiConfig::from_env()?;
        Ok(Self::new(config))
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn chat_completion(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/{}:generateContent?key={}", self.config.base_url, self.config.model, self.config.api_key);

        let mut system_instruction_parts = Vec::new();
        let mut contents = Vec::new();

        // Gemini only supports "user" and "model" roles in contents.
        // System prompt goes into `systemInstruction`.
        for msg in request.messages {
            match msg.role {
                MessageRole::System => {
                    if let Some(text) = msg.content {
                        system_instruction_parts.push(Part { text: Some(text), function_call: None, function_response: None });
                    }
                }
                MessageRole::User => {
                    if let Some(text) = msg.content {
                        contents.push(Content {
                            role: "user".to_string(),
                            parts: vec![Part { text: Some(text), function_call: None, function_response: None }],
                        });
                    }
                }
                MessageRole::Assistant => {
                    let mut parts = Vec::new();
                    if let Some(text) = msg.content {
                        parts.push(Part { text: Some(text), function_call: None, function_response: None });
                    }
                    if let Some(tool_calls) = msg.tool_calls {
                        for call in tool_calls {
                            parts.push(Part {
                                text: None,
                                function_call: Some(GeminiFunctionCall {
                                    name: call.name,
                                    args: serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null),
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
                            serde_json::from_str(&content).unwrap_or(serde_json::Value::String(content))
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
        };

        let tools = request.tools.map(|tools| {
            vec![GeminiTool {
                function_declarations: tools.into_iter().map(|t| GeminiFunctionDeclaration {
                    name: t.name,
                    description: t.description,
                    parameters: t.parameters,
                }).collect(),
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

        let response = self.client.post(&url).json(&api_request).send().await.map_err(|e| {
            error!(error = %e, "Failed to send request to Gemini API");
            LlmError::NetworkError(e.to_string())
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(status = %status, error = %error_text, "Gemini API returned error");
            return Err(LlmError::ApiError(format!(
                "Gemini API error ({}): {}",
                status, error_text
            )));
        }

        let completion: GeminiGenerateContentResponse = response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse Gemini API response");
            LlmError::SerializationError(e.to_string())
        })?;

        let candidates = completion.candidates.ok_or_else(|| {
            warn!("No candidates in Gemini API response");
            LlmError::ProviderError("No candidates in response".to_string())
        })?;

        let candidate = candidates.into_iter().next().ok_or_else(|| {
            warn!("Empty candidates array");
            LlmError::ProviderError("Empty candidates array".to_string())
        })?;

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
        })
    }

    async fn chat_completion_stream(
        &self,
        request: LlmRequest,
    ) -> Result<BoxStream<'static, Result<super::LlmStreamEvent, LlmError>>, LlmError> {
        let url = format!("{}/{}:streamGenerateContent?alt=sse&key={}", self.config.base_url, self.config.model, self.config.api_key);

        let mut system_instruction_parts = Vec::new();
        let mut contents = Vec::new();

        for msg in request.messages {
            match msg.role {
                MessageRole::System => {
                    if let Some(text) = msg.content {
                        system_instruction_parts.push(Part { text: Some(text), function_call: None, function_response: None });
                    }
                }
                MessageRole::User => {
                    if let Some(text) = msg.content {
                        contents.push(Content {
                            role: "user".to_string(),
                            parts: vec![Part { text: Some(text), function_call: None, function_response: None }],
                        });
                    }
                }
                MessageRole::Assistant => {
                    let mut parts = Vec::new();
                    if let Some(text) = msg.content {
                        parts.push(Part { text: Some(text), function_call: None, function_response: None });
                    }
                    if let Some(tool_calls) = msg.tool_calls {
                        for call in tool_calls {
                            parts.push(Part {
                                text: None,
                                function_call: Some(GeminiFunctionCall {
                                    name: call.name,
                                    args: serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null),
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
                            serde_json::from_str(&content).unwrap_or(serde_json::Value::String(content))
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
        };

        let tools = request.tools.map(|tools| {
            vec![GeminiTool {
                function_declarations: tools.into_iter().map(|t| GeminiFunctionDeclaration {
                    name: t.name,
                    description: t.description,
                    parameters: t.parameters,
                }).collect(),
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

        let response = self.client.post(&url).json(&api_request).send().await.map_err(|e| {
            error!(error = %e, "Failed to send streaming request to Gemini API");
            LlmError::NetworkError(e.to_string())
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(status = %status, error = %error_text, "Gemini API returned error on stream");
            return Err(LlmError::ApiError(format!(
                "Gemini stream error ({}): {}",
                status, error_text
            )));
        }

        let mut event_stream = response.bytes_stream().eventsource();

        let stream = async_stream::try_stream! {
            while let Some(event_res) = event_stream.next().await {
                let event = match event_res {
                    Ok(e) => e,
                    Err(e) => {
                        yield Err(LlmError::NetworkError(format!("SSE error: {}", e)))?;
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

                if let Some(candidates) = chunk.candidates {
                    for candidate in candidates {
                        if let Some(content) = candidate.content {
                            if let Some(parts) = content.parts {
                                for part in parts {
                                    if let Some(text) = part.text {
                                        if !text.is_empty() {
                                            yield super::LlmStreamEvent::ContentChunk(text);
                                        }
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
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}
