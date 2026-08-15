//! Estimating how many tokens a request will cost, before sending it.

use crate::llm::{ChatMessage, ToolDefinition};

/// Counts tokens well enough to decide what to send.
///
/// An estimate, not a count. An exact tokenizer is per model family, and an
/// agent pointed at OpenRouter runs whichever model its config names, so an
/// "exact" count would be exact for one family and confidently wrong for the
/// rest. What a request actually cost comes back from the provider as
/// [`TokenUsage`](crate::llm::TokenUsage); this decides, that reconciles.
pub trait TokenEstimate: Send + Sync {
    /// Estimate the tokens in a piece of text.
    fn estimate_text(&self, text: &str) -> usize;

    /// Estimate a whole request, including per-message overhead.
    fn estimate_messages(&self, messages: &[&ChatMessage]) -> usize {
        messages
            .iter()
            .map(|message| self.estimate_message(message))
            .sum()
    }

    /// Estimate one message: its content, its tool calls, and the wrapping the
    /// provider adds around every message.
    fn estimate_message(&self, message: &ChatMessage) -> usize {
        let mut total = PER_MESSAGE_OVERHEAD;
        if let Some(content) = &message.content {
            total += self.estimate_text(content);
        }
        if let Some(calls) = &message.tool_calls {
            for call in calls {
                total += self.estimate_text(&call.name) + self.estimate_text(&call.arguments);
            }
        }
        if let Some(name) = &message.name {
            total += self.estimate_text(name);
        }
        total
    }

    /// Estimate the tool definitions, which are sent with every request and are
    /// easy to forget — a dozen MCP tools with JSON Schema parameters is not a
    /// rounding error.
    fn estimate_tools(&self, tools: &[ToolDefinition]) -> usize {
        tools
            .iter()
            .map(|tool| {
                self.estimate_text(&tool.name)
                    + self.estimate_text(&tool.description)
                    + self.estimate_text(&tool.parameters.to_string())
                    + PER_MESSAGE_OVERHEAD
            })
            .sum()
    }
}

/// Tokens every message costs beyond its text, for role and delimiters. Four is
/// what OpenAI's own counting guidance uses, and the providers here are close
/// enough that a more precise number would be false precision.
const PER_MESSAGE_OVERHEAD: usize = 4;

/// Characters per token. English prose against a byte-pair vocabulary runs
/// closer to 4; code and JSON run denser, and tool arguments are mostly JSON.
/// Rounding down means over-estimating the cost, which errs toward compacting
/// early rather than toward a request the model rejects.
const DEFAULT_CHARS_PER_TOKEN: f32 = 3.5;

/// The default estimator: characters divided by a fixed ratio.
///
/// Crude and predictable. It runs in no time on any input, needs no model
/// vocabulary, and cannot be wrong in a way that varies by provider — which
/// makes the one thing it is used for (deciding whether to trim) stable.
#[derive(Debug, Clone, Copy)]
pub struct CharEstimate {
    chars_per_token: f32,
}

impl Default for CharEstimate {
    fn default() -> Self {
        Self {
            chars_per_token: DEFAULT_CHARS_PER_TOKEN,
        }
    }
}

impl CharEstimate {
    /// An estimator with a different ratio, for a deployment that has measured
    /// its own. Compare [`TokenUsage::prompt_tokens`](crate::llm::TokenUsage)
    /// against what was estimated to find it.
    ///
    /// A ratio at or below zero would divide by zero or negate the estimate, so
    /// it falls back to the default.
    pub fn with_chars_per_token(chars_per_token: f32) -> Self {
        if chars_per_token <= 0.0 {
            return Self::default();
        }
        Self { chars_per_token }
    }
}

impl TokenEstimate for CharEstimate {
    fn estimate_text(&self, text: &str) -> usize {
        (text.chars().count() as f32 / self.chars_per_token).ceil() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_estimates_scale_with_length() {
        let estimator = CharEstimate::default();
        let short = estimator.estimate_text("hei");
        let long = estimator.estimate_text(&"hei ".repeat(1_000));
        assert!(short < long);
        assert!(long > 900, "4000 chars should be about 1100 tokens: {long}");
    }

    /// Every message costs more than its text. A transcript of many short turns
    /// would otherwise estimate as near-free.
    #[test]
    fn every_message_costs_its_wrapping() {
        let estimator = CharEstimate::default();
        let empty = ChatMessage::user("");
        assert!(estimator.estimate_message(&empty) >= PER_MESSAGE_OVERHEAD);
    }

    /// Tool arguments are part of the request and are frequently the largest
    /// part of an assistant turn.
    #[test]
    fn tool_calls_count_toward_the_estimate() {
        use crate::llm::{MessageRole, ToolCall};

        let estimator = CharEstimate::default();
        let plain = ChatMessage::assistant("ok");
        let calling = ChatMessage {
            role: MessageRole::Assistant,
            content: Some("ok".to_string()),
            tool_calls: Some(vec![ToolCall {
                id: "1".to_string(),
                name: "search".to_string(),
                arguments: "{\"q\":\"".to_string() + &"x".repeat(400) + "\"}",
            }]),
            tool_call_id: None,
            name: None,
        };
        assert!(estimator.estimate_message(&calling) > estimator.estimate_message(&plain) + 100);
    }

    /// Tool schemas ride along on every request and are easy to leave out of a
    /// budget.
    #[test]
    fn tool_definitions_are_counted() {
        let estimator = CharEstimate::default();
        let tools = vec![ToolDefinition {
            name: "search".to_string(),
            description: "searches".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "q": { "type": "string", "description": "the query" } }
            }),
        }];
        assert!(estimator.estimate_tools(&tools) > 10);
    }

    #[test]
    fn a_nonsense_ratio_falls_back_to_the_default() {
        assert_eq!(
            CharEstimate::with_chars_per_token(0.0).chars_per_token,
            DEFAULT_CHARS_PER_TOKEN
        );
        assert_eq!(
            CharEstimate::with_chars_per_token(-3.0).chars_per_token,
            DEFAULT_CHARS_PER_TOKEN
        );
    }
}
