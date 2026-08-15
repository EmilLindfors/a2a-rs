//! Wiring the pure context policy to the protocol's conversation store.
//!
//! `a2a-agents-common` owns the rules (what to trim, in what order, when to
//! summarize) and knows nothing about A2A. `a2a-rs` owns the durable
//! conversation and knows nothing about models. This module is the seam: it maps
//! stored [`Message`]s to [`Turn`]s and a [`ContextConfig`] to a
//! [`ContextBudget`].

use a2a_agents_common::context::{ContextBudget, Turn};
use a2a_rs::domain::{Conversation, Message, Role, part};

use crate::core::config::ContextConfig;

/// The budget these settings describe.
///
/// `max_input_tokens = 0` means "no ceiling", which comes out as a budget large
/// enough that nothing trips it. Expressing it as a huge number rather than an
/// `Option` keeps one code path in the handler.
pub fn budget_from(config: &ContextConfig) -> ContextBudget {
    ContextBudget {
        max_input_tokens: if config.max_input_tokens == 0 {
            usize::MAX
        } else {
            config.max_input_tokens
        },
        reserve_for_output: config.reserve_for_output,
        compact_at: f32::from(config.compact_at_percent.min(100)) / 100.0,
        keep_recent_turns: config.keep_recent_turns,
        max_tool_result_chars: config.max_tool_result_chars,
    }
}

/// Map a loaded conversation to the turns a prompt is built from.
///
/// Two things are filtered out here, and both would otherwise reach the model:
///
/// - Messages with no text. A task records one per status transition, and
///   several carry none.
/// - Anything that is not a user or agent message. `Role` has no other variants
///   today, but an unrecognized one arriving off the wire must not be guessed at.
pub fn turns_from(conversation: &Conversation) -> Vec<Turn> {
    conversation
        .tail
        .iter()
        .filter_map(|entry| {
            let text = text_of(&entry.message);
            if text.trim().is_empty() {
                return None;
            }
            match entry.message.role.as_known()? {
                Role::User => Some(Turn::user(text)),
                Role::Agent => Some(Turn::agent(text)),
                _ => None,
            }
        })
        .collect()
}

/// Every text part of a message, joined.
///
/// File and data parts are dropped, which is the same limitation
/// `extract_text` has on the incoming message — see the open item about feeding
/// non-text parts to multimodal models.
pub fn text_of(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|p| match &p.content {
            Some(part::Content::Text(text)) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The prompt asking a model to summarize the part of a conversation being
/// compacted.
///
/// Written to preserve what a later turn needs and nothing else: decisions,
/// facts established, and open threads. A summary that reads as prose about the
/// conversation is useless to the model that has to continue it.
pub const SUMMARY_INSTRUCTION: &str = "\
Summarize the conversation above for your own future reference. You are writing \
notes to yourself, not a report for a reader.

Keep: decisions made, facts established, values and identifiers mentioned, what \
the user is trying to achieve, and anything still unresolved. Drop: pleasantries, \
restatements, and anything already superseded.

Write it as compact plain text. Do not add commentary about the summary itself.";

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_rs::domain::{Digest, Part, Seq, SequencedMessage};

    fn message(role: Role, text: &str) -> Message {
        Message::builder()
            .role(role)
            .parts(vec![Part::text(text.to_string())])
            .message_id("m".to_string())
            .build()
    }

    fn conversation(messages: Vec<Message>) -> Conversation {
        Conversation {
            digest: None,
            tail: messages
                .into_iter()
                .enumerate()
                .map(|(index, message)| SequencedMessage {
                    seq: Seq::new(index as u64 + 1),
                    message,
                })
                .collect(),
        }
    }

    #[test]
    fn roles_carry_across_to_turns() {
        let turns = turns_from(&conversation(vec![
            message(Role::User, "what is it"),
            message(Role::Agent, "Oslo"),
        ]));
        assert_eq!(turns, vec![Turn::user("what is it"), Turn::agent("Oslo")]);
    }

    /// A task records a message per status transition, and several carry no
    /// text. Those would be empty turns in the prompt.
    #[test]
    fn messages_with_no_text_are_dropped() {
        let empty = Message::builder()
            .role(Role::Agent)
            .parts(vec![])
            .message_id("m".to_string())
            .build();
        assert!(turns_from(&conversation(vec![empty])).is_empty());
    }

    #[test]
    fn a_zero_ceiling_means_no_ceiling() {
        let config = ContextConfig {
            max_input_tokens: 0,
            ..ContextConfig::default()
        };
        assert_eq!(budget_from(&config).max_input_tokens, usize::MAX);
    }

    #[test]
    fn the_percent_threshold_becomes_a_fraction() {
        let config = ContextConfig {
            compact_at_percent: 75,
            ..ContextConfig::default()
        };
        assert!((budget_from(&config).compact_at - 0.75).abs() < f32::EPSILON);
    }

    /// A percentage above 100 would put the compaction threshold past the
    /// ceiling, so compaction would never fire and trimming would do all the
    /// work.
    #[test]
    fn a_percentage_over_a_hundred_is_clamped() {
        let config = ContextConfig {
            compact_at_percent: 250,
            ..ContextConfig::default()
        };
        assert!((budget_from(&config).compact_at - 1.0).abs() < f32::EPSILON);
    }

    /// The digest is carried separately from the turns, so a compacted
    /// conversation projects its summary rather than the messages it replaced.
    #[test]
    fn a_digest_is_not_a_turn() {
        let conversation = Conversation {
            digest: Some(Digest {
                covers_through: Seq::new(3),
                summary: "they talked".to_string(),
                replaced_messages: 3,
                model: "test".to_string(),
            }),
            tail: vec![SequencedMessage {
                seq: Seq::new(4),
                message: message(Role::User, "and now"),
            }],
        };
        assert_eq!(turns_from(&conversation), vec![Turn::user("and now")]);
        assert_eq!(conversation.summary(), Some("they talked"));
    }
}
