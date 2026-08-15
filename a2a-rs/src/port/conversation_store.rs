//! Durable conversation memory for a context.

use async_trait::async_trait;

use crate::domain::{A2AError, ContextId, Conversation, Digest};

/// Reads and compacts the conversation recorded against one context.
///
/// Two methods, one capability. Splitting the tail and the digest into separate
/// ports reads tidier and is wrong: a digest written between two reads leaves
/// either a gap (messages the summary does not cover and the tail no longer
/// includes) or duplicates, and nothing in two separate signatures says the
/// reads have to agree. One method makes that boundary the implementation's
/// problem, which is where it can actually be solved.
///
/// ## The caller argument
///
/// Every method takes the authenticated principal's id, or `None` where the
/// agent runs without an authenticator. This is not ceremony: a `context_id`
/// only groups tasks today, but a store that hands back a conversation turns it
/// into a capability — whoever holds one can read what was said in it.
/// Ownership is claimed on first write and enforced on every read, and a
/// mismatch is [`A2AError::ContextAccessDenied`].
#[async_trait]
pub trait AsyncConversationStore: Send + Sync {
    /// Load the newest digest and the messages recorded after its watermark.
    ///
    /// `limit` caps the tail, keeping the **newest** messages — a conversation
    /// too long to load whole is more usefully truncated at its start, and the
    /// part before it is what compaction summarizes. `None` loads everything
    /// after the watermark.
    ///
    /// An unknown context is an empty [`Conversation`], not an error: the first
    /// turn of a conversation asks for history that does not exist yet.
    async fn load(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Conversation, A2AError>;

    /// Append a digest covering everything through
    /// [`Digest::covers_through`], claiming the context for `caller` if it is
    /// new.
    ///
    /// Appends. Nothing is deleted and no earlier digest is replaced, so a
    /// concurrent compaction of the same conversation costs duplicated work
    /// rather than a lost or doubled transcript.
    async fn compact(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        digest: Digest,
    ) -> Result<(), A2AError>;
}

/// Conveniences over [`AsyncConversationStore`].
///
/// Blanket-implemented, so they ride along on `Arc<dyn AsyncConversationStore>`
/// too. `?Sized` is what makes that work.
#[async_trait]
pub trait AsyncConversationStoreExt: AsyncConversationStore {
    /// Load a conversation, keeping at most `keep` of the most recent messages.
    async fn load_recent(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        keep: u32,
    ) -> Result<Conversation, A2AError> {
        self.load(context_id, caller, Some(keep)).await
    }

    /// Summarize everything loaded so far under one digest.
    ///
    /// Takes the watermark from the conversation itself, which is the common
    /// case and the easy one to get wrong: a watermark computed from anything
    /// other than what was actually summarized either re-summarizes messages or
    /// drops them.
    async fn compact_through(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        conversation: &Conversation,
        summary: String,
        model: String,
    ) -> Result<(), A2AError> {
        let digest = Digest {
            covers_through: conversation.watermark(),
            summary,
            replaced_messages: conversation.tail.len() as u32,
            model,
        };
        self.compact(context_id, caller, digest).await
    }
}

impl<T: AsyncConversationStore + ?Sized> AsyncConversationStoreExt for T {}

/// A store that remembers nothing.
///
/// The adapter for `mode = "none"`: every load is an empty conversation and
/// every compaction is a no-op. It exists so "this agent does not carry history"
/// is a wired-up choice rather than an absent collaborator, which is what lets
/// the handler take one code path either way.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoConversationMemory;

#[async_trait]
impl AsyncConversationStore for NoConversationMemory {
    async fn load(
        &self,
        _context_id: &ContextId,
        _caller: Option<&str>,
        _limit: Option<u32>,
    ) -> Result<Conversation, A2AError> {
        Ok(Conversation::default())
    }

    async fn compact(
        &self,
        _context_id: &ContextId,
        _caller: Option<&str>,
        _digest: Digest,
    ) -> Result<(), A2AError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn the_no_memory_store_reports_an_empty_conversation() {
        let store = NoConversationMemory;
        let context = ContextId::from_str("ctx-1").unwrap();

        let conversation = store.load(&context, None, None).await.unwrap();
        assert!(conversation.is_empty());

        // And compacting it is not an error, so a handler needs no branch.
        store
            .compact_through(
                &context,
                None,
                &conversation,
                "nothing happened".to_string(),
                "test".to_string(),
            )
            .await
            .unwrap();
    }
}
