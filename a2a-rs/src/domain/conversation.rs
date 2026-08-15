//! A conversation: the durable, ordered record of what was said in one context.
//!
//! A `context_id` groups the tasks of one conversation, and each task records a
//! message per status transition. Read back in order, that is the conversation —
//! the same thing an agent framework calls a session or a thread. These types
//! name the pieces an agent needs to rebuild it: a sequence number, an optional
//! summary of the part already compacted, and the messages after that summary.
//!
//! Pure data. Loading and appending are the [`AsyncConversationStore`] port's
//! job.
//!
//! [`AsyncConversationStore`]: crate::port::AsyncConversationStore

use serde::{Deserialize, Serialize};

use crate::domain::core::Message;

/// Position of a message within a conversation.
///
/// Monotonic and dense enough to compare: `a < b` means `a` was recorded first.
/// Values are opaque — nothing outside a store should construct one except from
/// a [`Digest`] watermark or [`Seq::START`].
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Seq(u64);

impl Seq {
    /// Before every recorded message. Loading from here loads everything.
    pub const START: Seq = Seq(0);

    /// Build a sequence number from a store's own ordering key.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// The raw ordering key, for a store that has to persist it.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for Seq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One recorded message, with its position.
#[derive(Debug, Clone)]
pub struct SequencedMessage {
    /// Where this sits in the conversation.
    pub seq: Seq,
    /// What was said.
    pub message: Message,
}

/// A summary standing in for everything up to and including a watermark.
///
/// Digests are appended, never updated. Two turns of the same conversation can
/// decide to compact at once; both rows land, the one with the highest
/// [`covers_through`](Self::covers_through) wins on load, and the other is
/// wasted work rather than a corrupted transcript.
#[derive(Debug, Clone)]
pub struct Digest {
    /// Everything at or below this sequence number is represented by
    /// [`summary`](Self::summary) and need not be loaded.
    pub covers_through: Seq,
    /// The summary text, as written by a model.
    pub summary: String,
    /// How many messages it replaced. Reporting only.
    pub replaced_messages: u32,
    /// Which model wrote it, so a summary produced by a weak model is
    /// identifiable after the fact.
    pub model: String,
}

/// A conversation as loaded for a prompt: the summary of what came before, and
/// everything recorded since.
///
/// Both halves come from one read. Fetching them separately would let a digest
/// written in between leave either a gap (messages the summary does not cover
/// and the tail no longer includes) or duplicates (messages present in both).
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    /// The newest digest, when the conversation has been compacted.
    pub digest: Option<Digest>,
    /// Messages after the digest's watermark, oldest first.
    pub tail: Vec<SequencedMessage>,
}

impl Conversation {
    /// Whether anything was said in this context at all.
    pub fn is_empty(&self) -> bool {
        self.digest.is_none() && self.tail.is_empty()
    }

    /// The summary text, if this conversation has been compacted.
    pub fn summary(&self) -> Option<&str> {
        self.digest.as_ref().map(|digest| digest.summary.as_str())
    }

    /// The highest sequence number loaded, or the digest watermark when the
    /// tail is empty. This is what a new digest should cover through.
    pub fn watermark(&self) -> Seq {
        self.tail
            .last()
            .map(|message| message.seq)
            .or_else(|| self.digest.as_ref().map(|digest| digest.covers_through))
            .unwrap_or(Seq::START)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequenced(seq: u64) -> SequencedMessage {
        SequencedMessage {
            seq: Seq::new(seq),
            message: Message::default(),
        }
    }

    #[test]
    fn an_empty_conversation_watermarks_at_the_start() {
        assert_eq!(Conversation::default().watermark(), Seq::START);
        assert!(Conversation::default().is_empty());
    }

    #[test]
    fn the_watermark_follows_the_newest_message() {
        let conversation = Conversation {
            digest: None,
            tail: vec![sequenced(4), sequenced(9)],
        };
        assert_eq!(conversation.watermark(), Seq::new(9));
    }

    /// A conversation compacted to its very end has no tail, and its watermark
    /// is the digest's. Reporting `START` here would have the next compaction
    /// re-summarize from the beginning.
    #[test]
    fn a_fully_compacted_conversation_keeps_the_digest_watermark() {
        let conversation = Conversation {
            digest: Some(Digest {
                covers_through: Seq::new(12),
                summary: "they talked".to_string(),
                replaced_messages: 6,
                model: "test".to_string(),
            }),
            tail: Vec::new(),
        };
        assert_eq!(conversation.watermark(), Seq::new(12));
        assert!(!conversation.is_empty());
    }
}
