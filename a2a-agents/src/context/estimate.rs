//! Estimating how many tokens a request will cost, before sending it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use a2a_llm::{ChatMessage, ToolDefinition};

/// Counts tokens well enough to decide what to send.
///
/// An estimate, not a count. An exact tokenizer is per model family, and an
/// agent pointed at OpenRouter runs whichever model its config names, so an
/// "exact" count would be exact for one family and confidently wrong for the
/// rest. What a request actually cost comes back from the provider as
/// [`TokenUsage`](a2a_llm::TokenUsage); this decides, that reconciles.
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
    /// its own. [`DriftWatch`] measures it from
    /// [`TokenUsage::prompt_tokens`](a2a_llm::TokenUsage).
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

/// Requests to accumulate before judging the ratio. One says nothing: a provider
/// counts tool schemas and its own message wrapping in ways this cannot see, and
/// a short first turn is where that fixed overhead dominates.
const MIN_SAMPLES: u64 = 8;

/// How far the accumulated ratio has to be off before it is worth saying.
/// Tokenizers disagree by 10–20% against any fixed characters-per-token ratio,
/// so a third is the point where the difference is the model rather than noise.
const TOLERANCE: f64 = 1.35;

/// What the provider charged, against what was estimated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drift {
    /// Charged prompt tokens per estimated token, accumulated over `samples`.
    /// Above one the estimate runs low, below one it runs high.
    pub ratio: f32,
    /// The `chars_per_token` that would have matched what was charged.
    pub chars_per_token: f32,
    /// Requests this is measured over.
    pub samples: u64,
}

/// Reconciles the token estimate against what the provider charged.
///
/// [`CharEstimate`] divides characters by a fixed ratio, and the right ratio is
/// a property of the model's tokenizer and the language it is reading. A few
/// percent off changes nothing. Off by half means an agent compacts long before
/// it needs to, or sends requests the model refuses despite a budget that says
/// they fit — and nothing else in the loop can tell you which.
///
/// [`record`](Self::record) accumulates and returns a [`Drift`] once, the first
/// time the gap is large enough to act on. Once, because this is a signal to
/// change a config value, not a metric to emit per request.
#[derive(Debug)]
pub struct DriftWatch {
    chars_per_token: f32,
    estimated: AtomicU64,
    charged: AtomicU64,
    samples: AtomicU64,
    reported: AtomicBool,
}

impl DriftWatch {
    /// Watch an estimator configured at `chars_per_token`.
    pub fn new(chars_per_token: f32) -> Self {
        Self {
            chars_per_token,
            estimated: AtomicU64::new(0),
            charged: AtomicU64::new(0),
            samples: AtomicU64::new(0),
            reported: AtomicBool::new(false),
        }
    }

    /// Record one request's estimated prompt tokens against the `prompt_tokens`
    /// the provider reported.
    ///
    /// Returns `Some` at most once, when enough requests have accumulated and
    /// the ratio between them is outside the tolerance. A provider that reports
    /// no prompt tokens contributes nothing rather than a zero, which would drag
    /// the ratio toward "the estimate is far too high".
    pub fn record(&self, estimated: usize, charged: u32) -> Option<Drift> {
        if charged == 0 || estimated == 0 || self.reported.load(Ordering::Relaxed) {
            return None;
        }

        let estimated_total = self
            .estimated
            .fetch_add(estimated as u64, Ordering::Relaxed)
            + estimated as u64;
        let charged_total = self
            .charged
            .fetch_add(u64::from(charged), Ordering::Relaxed)
            + u64::from(charged);
        let samples = self.samples.fetch_add(1, Ordering::Relaxed) + 1;
        if samples < MIN_SAMPLES {
            return None;
        }

        let ratio = charged_total as f64 / estimated_total as f64;
        if (1.0 / TOLERANCE..=TOLERANCE).contains(&ratio) {
            return None;
        }
        // Two threads crossing the threshold together both get here; the swap
        // decides which one says it.
        if self.reported.swap(true, Ordering::Relaxed) {
            return None;
        }

        Some(Drift {
            ratio: ratio as f32,
            chars_per_token: (f64::from(self.chars_per_token) / ratio) as f32,
            samples,
        })
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
        use a2a_llm::{MessageRole, ToolCall};

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

    /// An estimate within the tolerance is what the ratio is for. Reporting it
    /// would train the operator to ignore the one report that matters.
    #[test]
    fn an_estimate_close_to_the_charge_is_not_reported() {
        let watch = DriftWatch::new(3.5);
        for _ in 0..50 {
            assert_eq!(watch.record(1_000, 1_100), None);
        }
    }

    /// Under-estimating is the expensive direction: the budget says the request
    /// fits and the provider refuses it. The suggested ratio is lower, so the
    /// same text estimates higher.
    #[test]
    fn a_charge_far_above_the_estimate_suggests_a_smaller_ratio() {
        let watch = DriftWatch::new(3.5);
        let drift = (0..MIN_SAMPLES)
            .find_map(|_| watch.record(1_000, 2_000))
            .expect("twice the estimate is well outside the tolerance");

        assert!((drift.ratio - 2.0).abs() < 0.01, "{drift:?}");
        assert!((drift.chars_per_token - 1.75).abs() < 0.01, "{drift:?}");
        assert_eq!(drift.samples, MIN_SAMPLES);
    }

    /// Over-estimating costs money rather than answers — the agent summarizes
    /// long before it has to — and the fix is the opposite ratio.
    #[test]
    fn a_charge_far_below_the_estimate_suggests_a_larger_ratio() {
        let watch = DriftWatch::new(3.5);
        let drift = (0..MIN_SAMPLES)
            .find_map(|_| watch.record(2_000, 1_000))
            .expect("half the estimate is well outside the tolerance");

        assert!(drift.chars_per_token > 6.9, "{drift:?}");
    }

    /// One request is not evidence: a short first turn is mostly the provider's
    /// own wrapping, which no character ratio predicts.
    #[test]
    fn a_single_request_is_not_enough_to_judge() {
        let watch = DriftWatch::new(3.5);
        assert_eq!(watch.record(1_000, 5_000), None);
    }

    /// A signal to change a config value, not a metric. Saying it every request
    /// afterwards is how it gets filtered out.
    #[test]
    fn drift_is_reported_once() {
        let watch = DriftWatch::new(3.5);
        let mut reports = 0;
        for _ in 0..100 {
            if watch.record(1_000, 3_000).is_some() {
                reports += 1;
            }
        }
        assert_eq!(reports, 1);
    }

    /// Plenty of providers report no prompt tokens at all. Counting that as zero
    /// charged would accumulate toward "the estimate is far too high" and
    /// eventually suggest a ratio measured against nothing.
    #[test]
    fn a_provider_reporting_nothing_does_not_move_the_ratio() {
        let watch = DriftWatch::new(3.5);
        for _ in 0..50 {
            assert_eq!(watch.record(1_000, 0), None);
        }
        // The samples above contributed nothing, so this one starts the count.
        assert_eq!(watch.record(1_000, 3_000), None);
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
