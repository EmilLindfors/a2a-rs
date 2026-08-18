//! The small set of facts an agent keeps about a context, apart from the
//! transcript.

use async_trait::async_trait;

use crate::domain::{A2AError, ContextId, ContextState, StateKey, StateScope};

/// Reads and writes the state bag visible from one context.
///
/// Separate from [`AsyncConversationStore`] because the two are independently
/// useful: an agent that carries no transcript (`mode = "none"`) can still be
/// told the user's name, and one that carries the whole conversation need not
/// keep anything apart from it. Wiring them as one port would make each of
/// those configurations pull in the other.
///
/// ## The caller argument
///
/// As on [`AsyncConversationStore`], every method takes the authenticated
/// principal's id. It does two jobs here. Context-scoped keys are protected the
/// same way the transcript is — whoever holds a `context_id` would otherwise
/// read what was remembered in it — and [`StateScope::User`] keys are filed
/// under the principal itself, so it is the storage key rather than a check.
///
/// A `user:` key with no principal is [`A2AError::InvalidParams`]: there is
/// nothing to file it under, and storing it against the context instead would
/// promise a lifetime it does not have.
///
/// [`AsyncConversationStore`]: crate::port::AsyncConversationStore
#[async_trait]
pub trait AsyncContextStateStore: Send + Sync {
    /// Everything visible from this context: its own keys, plus the caller's
    /// `user:` keys.
    ///
    /// An unknown context is an empty bag, not an error.
    async fn load_state(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
    ) -> Result<ContextState, A2AError>;

    /// Record `value` under `key`, replacing whatever it held.
    ///
    /// [`StateScope::Temp`] keys never reach a store — [`is_stored`] is the
    /// check, and an implementation that is handed one anyway should treat it
    /// as a no-op rather than persisting a key whose name says it is not
    /// persisted.
    ///
    /// [`is_stored`]: StateScope::is_stored
    async fn remember(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        key: &StateKey,
        value: &str,
    ) -> Result<(), A2AError>;

    /// Drop `key`, reporting whether it held anything.
    ///
    /// The bool is what lets a caller tell "removed" from "there was nothing
    /// there", which are different answers to give a model that just asked for
    /// one of them.
    async fn forget(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        key: &StateKey,
    ) -> Result<bool, A2AError>;
}

/// The error for a `user:` key written by a caller the agent cannot name.
///
/// Shared by every implementation so the wording is one thing, and so the
/// condition is decided the same way in all of them.
pub fn user_scope_needs_a_principal(key: &StateKey) -> A2AError {
    A2AError::InvalidParams(format!(
        "'{key}' is scoped to the user, and this agent authenticates nobody — configure \
         `[server.auth]`, or drop the `user:` prefix to keep it against this conversation"
    ))
}

/// Which storage key a scope files a name under: the context, or the principal.
///
/// Returns `None` for [`StateScope::Temp`], which is never stored. Kept next to
/// the port so both adapters partition the bag the same way.
pub fn scope_key<'a>(
    scope: StateScope,
    context_id: &'a str,
    caller: Option<&'a str>,
    key: &StateKey,
) -> Result<Option<&'a str>, A2AError> {
    match scope {
        StateScope::Context => Ok(Some(context_id)),
        StateScope::User => match caller {
            Some(caller) => Ok(Some(caller)),
            None => Err(user_scope_needs_a_principal(key)),
        },
        StateScope::Temp => Ok(None),
    }
}

/// A store that remembers nothing.
///
/// The adapter for an agent with the state bag switched off: loads are empty
/// and writes are dropped. It exists for the same reason
/// [`NoConversationMemory`] does — so "this agent keeps no state" is a wired-up
/// choice and the handler takes one code path either way.
///
/// [`NoConversationMemory`]: crate::port::NoConversationMemory
#[derive(Debug, Clone, Copy, Default)]
pub struct NoContextState;

#[async_trait]
impl AsyncContextStateStore for NoContextState {
    async fn load_state(
        &self,
        _context_id: &ContextId,
        _caller: Option<&str>,
    ) -> Result<ContextState, A2AError> {
        Ok(ContextState::new())
    }

    async fn remember(
        &self,
        _context_id: &ContextId,
        _caller: Option<&str>,
        _key: &StateKey,
        _value: &str,
    ) -> Result<(), A2AError> {
        Ok(())
    }

    async fn forget(
        &self,
        _context_id: &ContextId,
        _caller: Option<&str>,
        _key: &StateKey,
    ) -> Result<bool, A2AError> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn key(raw: &str) -> StateKey {
        raw.parse().unwrap()
    }

    #[tokio::test]
    async fn the_no_state_store_remembers_nothing() {
        let store = NoContextState;
        let context = ContextId::from_str("ctx-1").unwrap();

        store
            .remember(&context, None, &key("project"), "a2a-rs")
            .await
            .unwrap();
        assert!(store.load_state(&context, None).await.unwrap().is_empty());
        assert!(!store.forget(&context, None, &key("project")).await.unwrap());
    }

    #[test]
    fn a_context_key_is_filed_under_the_context_and_a_user_key_under_the_caller() {
        assert_eq!(
            scope_key(StateScope::Context, "ctx-1", Some("alice"), &key("project")).unwrap(),
            Some("ctx-1")
        );
        assert_eq!(
            scope_key(StateScope::User, "ctx-1", Some("alice"), &key("user:tone")).unwrap(),
            Some("alice")
        );
    }

    #[test]
    fn a_temp_key_is_filed_nowhere() {
        assert_eq!(
            scope_key(StateScope::Temp, "ctx-1", Some("alice"), &key("temp:draft")).unwrap(),
            None
        );
    }

    /// Storing it against the context would be the wrong answer rather than a
    /// lenient one: the key says it outlives the conversation, and filed there
    /// it would not.
    #[test]
    fn a_user_key_with_no_principal_is_refused() {
        let err = scope_key(StateScope::User, "ctx-1", None, &key("user:tone")).unwrap_err();
        assert!(matches!(err, A2AError::InvalidParams(_)));
        assert!(err.to_string().contains("server.auth"));
    }
}
