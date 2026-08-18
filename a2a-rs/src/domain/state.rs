//! What an agent remembers about a context, apart from what was said in it.
//!
//! A conversation is the record of the turns; this is the small set of facts an
//! agent was asked to keep — the user's name, a unit preference, a project it is
//! working on. Two things make it worth having next to the transcript:
//! compaction rewrites the transcript and a stored value survives it, and a
//! `user:` value is readable from a context the caller has not opened yet.
//!
//! Pure data. Reading and writing are the [`AsyncContextStateStore`] port's job,
//! and how it is worded to a model belongs to whoever builds the prompt.
//!
//! [`AsyncContextStateStore`]: crate::port::AsyncContextStateStore

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

/// Marks a key kept against the principal rather than the context.
const USER_PREFIX: &str = "user:";
/// Marks a key that is deliberately not stored.
const TEMP_PREFIX: &str = "temp:";

/// How long a remembered value lives, taken from the key's prefix.
///
/// The prefix is part of the key the model writes and reads back, so the scope
/// is visible wherever the key is — in the prompt, in a `forget` call, and in
/// the store. Taken from Google ADK, which spells the same three this way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateScope {
    /// `user:` — filed under the authenticated principal, so every context that
    /// principal opens reads it back.
    ///
    /// Requires an authenticator. With no principal there is nothing to file it
    /// under, and storing it against the context instead would promise a
    /// lifetime it does not have.
    User,
    /// No prefix — filed under this context, and read back only here.
    Context,
    /// `temp:` — not stored anywhere.
    ///
    /// It exists so the prefix means something: without it, `temp:draft` would
    /// be an ordinary key that outlives the turn under a name saying it does
    /// not.
    Temp,
}

impl StateScope {
    /// The prefix a key in this scope carries.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::User => USER_PREFIX,
            Self::Context => "",
            Self::Temp => TEMP_PREFIX,
        }
    }

    /// Whether a store ever sees a key in this scope.
    pub fn is_stored(self) -> bool {
        !matches!(self, Self::Temp)
    }
}

/// Why a string is not usable as a state key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateKeyError {
    #[error("a state key cannot be empty")]
    Empty,

    /// A prefix that looks like a scope and is not one.
    ///
    /// Refused rather than read as part of the name: `app:tone` treated as an
    /// ordinary key is stored under a name that says it is scoped to the
    /// application and is not, which is the mistake the prefixes exist to make
    /// visible.
    #[error(
        "'{prefix}:' is not a memory scope — write `user:{name}` for something that outlives \
         this conversation, `temp:{name}` for something that is not stored, or `{name}` on its own"
    )]
    UnknownScope { prefix: String, name: String },

    #[error("a state key cannot contain control characters or newlines")]
    ControlCharacter,

    #[error("a state key is at most {max} characters, got {len}")]
    TooLong { len: usize, max: usize },
}

/// A key into the state bag, with the scope its prefix names.
///
/// Parsed once, at the edge, so everything downstream holds a key whose scope is
/// already decided.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateKey {
    scope: StateScope,
    name: String,
}

impl StateKey {
    /// Longest a key may be, prefix included. Keys are rendered into every
    /// request, and a key long enough to matter is a value wearing the wrong
    /// shape.
    pub const MAX_LEN: usize = 128;

    /// Build a key from a scope and the name a store filed it under.
    ///
    /// The other way round from [`FromStr`]: a store keeps the scope in its own
    /// column and the name without its prefix, so reading one back has both
    /// halves already and nothing to parse.
    pub fn scoped(scope: StateScope, name: &str) -> Result<Self, StateKeyError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StateKeyError::Empty);
        }
        let len = scope.prefix().chars().count() + name.chars().count();
        if len > Self::MAX_LEN {
            return Err(StateKeyError::TooLong {
                len,
                max: Self::MAX_LEN,
            });
        }
        if name.chars().any(char::is_control) {
            return Err(StateKeyError::ControlCharacter);
        }
        Ok(Self {
            scope,
            name: name.to_string(),
        })
    }

    /// The scope this key is stored in.
    pub fn scope(&self) -> StateScope {
        self.scope
    }

    /// The key without its scope prefix, which is what a store files it under.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl FromStr for StateKey {
    type Err = StateKeyError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(StateKeyError::Empty);
        }
        let len = raw.chars().count();
        if len > Self::MAX_LEN {
            return Err(StateKeyError::TooLong {
                len,
                max: Self::MAX_LEN,
            });
        }
        if raw.chars().any(char::is_control) {
            return Err(StateKeyError::ControlCharacter);
        }

        let (scope, name) = if let Some(name) = raw.strip_prefix(USER_PREFIX) {
            (StateScope::User, name)
        } else if let Some(name) = raw.strip_prefix(TEMP_PREFIX) {
            (StateScope::Temp, name)
        } else if let Some((prefix, name)) = raw.split_once(':') {
            return Err(StateKeyError::UnknownScope {
                prefix: prefix.to_string(),
                name: name.to_string(),
            });
        } else {
            (StateScope::Context, raw)
        };

        Self::scoped(scope, name)
    }
}

impl fmt::Display for StateKey {
    /// The key as written and as read back, prefix included.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.scope.prefix(), self.name)
    }
}

/// Everything an agent remembers that is visible from one context: the
/// context's own keys and the caller's.
///
/// Ordered by scope and then by name, so the same set of facts renders the same
/// way on every turn. A prompt prefix that reorders between requests is one a
/// provider cannot serve from cache.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextState {
    entries: BTreeMap<StateKey, String>,
}

impl ContextState {
    /// Nothing remembered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `value` under `key`, replacing whatever it held.
    pub fn insert(&mut self, key: StateKey, value: impl Into<String>) {
        self.entries.insert(key, value.into());
    }

    /// What `key` holds, if anything.
    pub fn get(&self, key: &StateKey) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Drop `key`, reporting whether it held anything.
    pub fn remove(&mut self, key: &StateKey) -> bool {
        self.entries.remove(key).is_some()
    }

    /// Every key and value, ordered by scope and then name.
    pub fn iter(&self) -> impl Iterator<Item = (&StateKey, &str)> {
        self.entries
            .iter()
            .map(|(key, value)| (key, value.as_str()))
    }

    /// How many keys are held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether anything is remembered at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl FromIterator<(StateKey, String)> for ContextState {
    fn from_iter<I: IntoIterator<Item = (StateKey, String)>>(iter: I) -> Self {
        Self {
            entries: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(raw: &str) -> StateKey {
        raw.parse().unwrap()
    }

    #[test]
    fn a_bare_key_is_scoped_to_the_context() {
        let parsed = key("project");
        assert_eq!(parsed.scope(), StateScope::Context);
        assert_eq!(parsed.name(), "project");
        assert_eq!(parsed.to_string(), "project");
    }

    #[test]
    fn the_prefixes_name_the_other_two_scopes() {
        assert_eq!(key("user:tone").scope(), StateScope::User);
        assert_eq!(key("user:tone").name(), "tone");
        assert_eq!(key("temp:draft").scope(), StateScope::Temp);
        // And a key round-trips through its rendering, which is what a `forget`
        // call is given back.
        assert_eq!(key("user:tone").to_string(), "user:tone");
    }

    /// The scope has to survive the round trip through a store, which files a
    /// key under its name and reads it back with the prefix put on again.
    #[test]
    fn only_temp_is_kept_out_of_storage() {
        assert!(!StateScope::Temp.is_stored());
        assert!(StateScope::User.is_stored());
        assert!(StateScope::Context.is_stored());
    }

    /// The case the prefixes exist for. ADK also has `app:`, which this does
    /// not implement — read as an ordinary key it would be stored per context
    /// under a name promising the opposite.
    #[test]
    fn an_unknown_prefix_is_refused_rather_than_read_as_a_name() {
        let err = "app:tone".parse::<StateKey>().unwrap_err();
        assert_eq!(
            err,
            StateKeyError::UnknownScope {
                prefix: "app".to_string(),
                name: "tone".to_string(),
            }
        );
        // And the message says what to write instead, because the reader is a
        // model choosing a key, not someone reading these docs.
        assert!(err.to_string().contains("user:tone"));
    }

    #[test]
    fn an_empty_key_is_refused_with_or_without_a_prefix() {
        assert_eq!("".parse::<StateKey>(), Err(StateKeyError::Empty));
        assert_eq!("   ".parse::<StateKey>(), Err(StateKeyError::Empty));
        assert_eq!("user:".parse::<StateKey>(), Err(StateKeyError::Empty));
    }

    /// Keys are rendered into every request, and a newline in one would break
    /// the block it is rendered into.
    #[test]
    fn control_characters_are_refused() {
        assert_eq!(
            "to\nne".parse::<StateKey>(),
            Err(StateKeyError::ControlCharacter)
        );
    }

    #[test]
    fn an_over_long_key_is_refused() {
        let long = "k".repeat(StateKey::MAX_LEN + 1);
        assert!(matches!(
            long.parse::<StateKey>(),
            Err(StateKeyError::TooLong { .. })
        ));
    }

    /// Rendering order is fixed so the same facts produce the same prompt
    /// prefix on every turn. `user:` keys sort ahead of bare ones because
    /// `StateScope::User` is declared first.
    #[test]
    fn entries_iterate_in_a_stable_order() {
        let mut state = ContextState::new();
        state.insert(key("project"), "a2a-rs");
        state.insert(key("user:tone"), "brief");
        state.insert(key("area"), "storage");

        let rendered: Vec<String> = state.iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert_eq!(
            rendered,
            ["user:tone=brief", "area=storage", "project=a2a-rs"]
        );
    }

    #[test]
    fn writing_the_same_key_twice_replaces_it() {
        let mut state = ContextState::new();
        state.insert(key("project"), "old");
        state.insert(key("project"), "new");
        assert_eq!(state.len(), 1);
        assert_eq!(state.get(&key("project")), Some("new"));
    }

    #[test]
    fn removing_reports_whether_anything_was_there() {
        let mut state = ContextState::new();
        state.insert(key("project"), "a2a-rs");
        assert!(state.remove(&key("project")));
        assert!(!state.remove(&key("project")));
        assert!(state.is_empty());
    }
}
