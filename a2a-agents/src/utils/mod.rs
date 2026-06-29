//! Utility modules for agent development.
//!
//! This module provides common utilities that agent developers can use:
//! - Text parsing and NLP helpers
//! - Caching utilities
//! - Data formatting helpers

pub mod parsing;

// Re-export commonly used items
pub use parsing::*;

/// Slugify a free-form string: lowercase ASCII alphanumerics are kept, every
/// other character becomes `separator`, runs of separators collapse to one, and
/// leading/trailing separators are trimmed. Used to derive stable,
/// URL/identifier-safe slugs from agent names (e.g. an `ask_<slug>` tool name or
/// an [`AgentId`](crate::registry::AgentId)).
///
/// Idempotent on an already-canonical slug, so feeding a slug back through it is
/// a no-op — which is what lets `AgentId` canonicalize raw lookup keys to the
/// same value [`from_name`](crate::registry::AgentId::from_name) produced.
///
/// Returns an empty string when `input` has no ASCII alphanumerics; callers that
/// need a non-empty identifier must reject that at their boundary.
pub fn slugify(input: &str, separator: char) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_sep = false;
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep {
            out.push(separator);
            prev_sep = true;
        }
    }
    let trimmed = out.trim_matches(separator);
    // `trim_matches` borrows from `out`; only allocate again if it actually trimmed.
    if trimmed.len() == out.len() {
        out
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod slug_tests {
    use super::slugify;

    #[test]
    fn slugify_lowercases_and_replaces() {
        assert_eq!(slugify("Weather Agent", '_'), "weather_agent");
        assert_eq!(slugify("billing-v2", '-'), "billing-v2");
        assert_eq!(slugify("  Spaces  ", '-'), "spaces");
    }

    #[test]
    fn slugify_collapses_separator_runs() {
        assert_eq!(slugify("Weather  Agent", '-'), "weather-agent");
        assert_eq!(slugify("a // b __ c", '-'), "a-b-c");
    }

    #[test]
    fn slugify_is_idempotent_on_canonical_slug() {
        let once = slugify("Weather Agent", '-');
        assert_eq!(slugify(&once, '-'), once);
    }

    #[test]
    fn slugify_empty_when_no_alphanumerics() {
        assert_eq!(slugify("!!!", '-'), "");
        assert_eq!(slugify("---", '-'), "");
    }
}
