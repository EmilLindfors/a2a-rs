//! Strongly-typed identifiers for the A2A protocol.
//!
//! Applies "parse, don't validate" to the codebase's own identifiers: a
//! [`TaskId`], [`ContextId`], or [`PushConfigId`] can only be constructed from a
//! non-empty string via [`FromStr`]/[`TryFrom`], so port methods that accept one
//! never have to re-check emptiness, and argument-order mix-ups
//! (`cancel(context_id, task_id)`) become compile errors.
//!
//! ## Deserialization caveat
//!
//! These newtypes derive `Deserialize` with `#[serde(transparent)]`, which means
//! a value reconstructed from the wire does **not** pass through the validating
//! [`FromStr`] path. That is intentional: deserialized identifiers are validated
//! once at the RPC boundary (the request processor converts wire strings through
//! [`FromStr`] before they reach a port). Treat [`FromStr`]/[`TryFrom`] as the
//! only validating constructors; `Deserialize` is a transport convenience.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::domain::error::A2AError;

/// Generates a validating string newtype identifier.
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, $field:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Borrow the identifier as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the identifier, returning the owned string.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl FromStr for $name {
            type Err = A2AError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.trim().is_empty() {
                    return Err(A2AError::ValidationError {
                        field: $field.to_string(),
                        message: concat!($field, " cannot be empty").to_string(),
                    });
                }
                Ok(Self(s.to_owned()))
            }
        }

        impl TryFrom<&str> for $name {
            type Error = A2AError;

            fn try_from(s: &str) -> Result<Self, Self::Error> {
                s.parse()
            }
        }

        impl TryFrom<String> for $name {
            type Error = A2AError;

            fn try_from(s: String) -> Result<Self, Self::Error> {
                s.as_str().parse()
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

define_id!(
    /// Identifies a task within an agent.
    TaskId,
    "task_id"
);

define_id!(
    /// Identifies a conversation/session context grouping related tasks.
    ContextId,
    "context_id"
);

define_id!(
    /// Identifies a single push-notification configuration for a task.
    PushConfigId,
    "push_notification_config_id"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_whitespace() {
        assert!(TaskId::from_str("").is_err());
        assert!(TaskId::from_str("   ").is_err());
        assert!(ContextId::from_str("").is_err());
    }

    #[test]
    fn accepts_non_empty() {
        let id = TaskId::from_str("task-123").unwrap();
        assert_eq!(id.as_str(), "task-123");
        assert_eq!(id.to_string(), "task-123");
    }

    #[test]
    fn try_from_owned_and_borrowed() {
        assert!(TaskId::try_from("x").is_ok());
        assert!(TaskId::try_from("x".to_string()).is_ok());
        assert!(TaskId::try_from(String::new()).is_err());
    }
}
