//! Turning a stored conversation into the message list sent to a model.
//!
//! The input is deliberately not an A2A `Message`: this crate does not depend
//! on `a2a-rs`, and keeping the projection over a neutral [`Turn`] means the
//! rules here can be tested without standing up a task store. Whoever owns the
//! protocol types maps them to [`Turn`] on the way in.

use crate::llm::ChatMessage;

/// Who produced a turn. Only the two roles a conversation alternates between —
/// tool traffic is not stored, and the system prompt comes from the agent's
/// config rather than from history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnRole {
    User,
    Agent,
}

/// One stored turn of a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: TurnRole,
    pub text: String,
}

impl Turn {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: TurnRole::User,
            text: text.into(),
        }
    }

    pub fn agent(text: impl Into<String>) -> Self {
        Self {
            role: TurnRole::Agent,
            text: text.into(),
        }
    }
}

/// How a summary of earlier turns is introduced to the model. Phrased as a fact
/// about the conversation so a model does not answer it as if it were a turn.
const SUMMARY_PREFIX: &str = "Summary of the earlier part of this conversation:\n\n";

/// Build the message list for a request.
///
/// Order is system prompt, the summary of everything already compacted, the
/// turns after it, then the message being answered. Empty turns are dropped:
/// a task settles with a message on every status transition and some of them
/// carry no text, and an empty content field serializes as an empty string that
/// costs tokens and says nothing.
pub fn project(
    system_prompt: &str,
    summary: Option<&str>,
    turns: &[Turn],
    current: &str,
) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(turns.len() + 3);
    messages.push(ChatMessage::system(system_prompt));

    if let Some(summary) = summary.map(str::trim).filter(|s| !s.is_empty()) {
        messages.push(ChatMessage::system(format!("{SUMMARY_PREFIX}{summary}")));
    }

    for turn in turns {
        let text = turn.text.trim();
        if text.is_empty() {
            continue;
        }
        messages.push(match turn.role {
            TurnRole::User => ChatMessage::user(text),
            TurnRole::Agent => ChatMessage::assistant(text),
        });
    }

    messages.push(ChatMessage::user(current));
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MessageRole;

    fn roles(messages: &[ChatMessage]) -> Vec<MessageRole> {
        messages.iter().map(|m| m.role.clone()).collect()
    }

    #[test]
    fn a_fresh_conversation_is_the_system_prompt_and_the_question() {
        let messages = project("be helpful", None, &[], "hei");
        assert_eq!(
            roles(&messages),
            vec![MessageRole::System, MessageRole::User]
        );
        assert_eq!(messages[1].content.as_deref(), Some("hei"));
    }

    #[test]
    fn prior_turns_alternate_between_user_and_assistant() {
        let turns = vec![Turn::user("what is the capital"), Turn::agent("Oslo")];
        let messages = project("be helpful", None, &turns, "and the population");

        assert_eq!(
            roles(&messages),
            vec![
                MessageRole::System,
                MessageRole::User,
                MessageRole::Assistant,
                MessageRole::User
            ]
        );
        assert_eq!(messages[3].content.as_deref(), Some("and the population"));
    }

    /// A summary stands ahead of the turns it replaced, and is labelled so the
    /// model reads it as history rather than as something to answer.
    #[test]
    fn a_summary_precedes_the_turns_that_survived_it() {
        let turns = vec![Turn::user("and then")];
        let messages = project(
            "be helpful",
            Some("They discussed Norway."),
            &turns,
            "go on",
        );

        assert_eq!(
            roles(&messages),
            vec![
                MessageRole::System,
                MessageRole::System,
                MessageRole::User,
                MessageRole::User
            ]
        );
        let summary = messages[1].content.as_deref().unwrap();
        assert!(summary.contains("They discussed Norway."), "{summary}");
        assert!(summary.starts_with(SUMMARY_PREFIX), "{summary}");
    }

    /// A task records a message on every status transition and some carry no
    /// text. Those cost tokens and say nothing.
    #[test]
    fn empty_turns_are_dropped() {
        let turns = vec![
            Turn::user("hei"),
            Turn::agent("   "),
            Turn::agent(""),
            Turn::agent("hallo"),
        ];
        let messages = project("be helpful", None, &turns, "og nå");

        assert_eq!(messages.len(), 4, "{messages:?}");
        assert!(
            messages
                .iter()
                .all(|m| !m.content.as_deref().unwrap_or_default().trim().is_empty())
        );
    }

    /// A blank summary must not produce a labelled block with nothing under it.
    #[test]
    fn a_blank_summary_adds_no_message() {
        let messages = project("be helpful", Some("  "), &[], "hei");
        assert_eq!(
            roles(&messages),
            vec![MessageRole::System, MessageRole::User]
        );
    }
}
