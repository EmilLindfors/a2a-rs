use serde::{Deserialize, Serialize};
use std::env;
use tracing::{debug, error, info, warn};

/// Configuration for the AI client
#[derive(Debug, Clone)]
pub struct AiConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl AiConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, String> {
        let base_url =
            env::var("AI_API_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".to_string());

        let model = env::var("AI_MODEL").unwrap_or_else(|_| "ministral".to_string());

        let api_key = env::var("AI_API_KEY").ok().and_then(|key| {
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

/// OpenAI-compatible chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// Response format for chat completion
#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

/// Request to the chat completion endpoint
#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

/// Response from the chat completion endpoint
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

/// Client for interacting with OpenAI-compatible APIs
#[derive(Clone)]
pub struct AiClient {
    config: AiConfig,
    client: reqwest::Client,
}

impl AiClient {
    /// Create a new AI client with the given configuration
    pub fn new(config: AiConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Create a new AI client from environment variables
    pub fn from_env() -> Result<Self, String> {
        let config = AiConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// Send a chat completion request
    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
        force_json: bool,
    ) -> Result<String, String> {
        let url = format!("{}/chat/completions", self.config.base_url);

        let response_format = if force_json {
            Some(ResponseFormat {
                format_type: "json_object".to_string(),
            })
        } else {
            None
        };

        let request = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages,
            temperature,
            max_tokens,
            response_format,
        };

        debug!(
            model = %self.config.model,
            url = %url,
            message_count = request.messages.len(),
            "Sending chat completion request"
        );

        let mut req_builder = self.client.post(&url).json(&request);

        // Add API key if configured
        if let Some(ref api_key) = self.config.api_key {
            req_builder = req_builder.bearer_auth(api_key);
        }

        let response = req_builder.send().await.map_err(|e| {
            error!(error = %e, "Failed to send request to AI API");
            format!("Failed to send request: {}", e)
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(status = %status, error = %error_text, "AI API returned error");
            return Err(format!("AI API error ({}): {}", status, error_text));
        }

        let completion: ChatCompletionResponse = response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse AI API response");
            format!("Failed to parse response: {}", e)
        })?;

        let message_content = completion
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .ok_or_else(|| {
                warn!("No choices in AI API response");
                "No response from AI".to_string()
            })?;

        info!(
            response_length = message_content.len(),
            "Received chat completion response"
        );

        Ok(message_content)
    }

    /// Simple convenience method to ask a question
    pub async fn ask_question(
        &self,
        system_prompt: &str,
        user_question: &str,
    ) -> Result<String, String> {
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_question),
        ];

        self.chat_completion(messages, Some(0.7), Some(500), false)
            .await
    }

    /// Ask a question with conversation history
    pub async fn ask_with_history(
        &self,
        system_prompt: &str,
        history: Vec<ChatMessage>,
    ) -> Result<String, String> {
        let mut messages = vec![ChatMessage::system(system_prompt)];
        messages.extend(history);

        self.chat_completion(messages, Some(0.7), Some(500), false)
            .await
    }

    /// Ask a question with conversation history and force JSON response
    pub async fn ask_with_history_json(
        &self,
        system_prompt: &str,
        history: Vec<ChatMessage>,
    ) -> Result<String, String> {
        let mut messages = vec![ChatMessage::system(system_prompt)];
        messages.extend(history);

        self.chat_completion(messages, Some(0.7), Some(500), true)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_creation() {
        let msg = ChatMessage::system("You are a helpful assistant");
        assert_eq!(msg.role, "system");
        assert_eq!(msg.content, "You are a helpful assistant");

        let msg = ChatMessage::user("Hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");

        let msg = ChatMessage::assistant("Hi there!");
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "Hi there!");
    }

    #[test]
    fn test_config_from_env() {
        // This test will use default values if env vars are not set
        let config = AiConfig::from_env().unwrap();
        assert!(!config.base_url.is_empty());
        assert!(!config.model.is_empty());
    }
}
