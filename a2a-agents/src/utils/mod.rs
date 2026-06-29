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
/// other character becomes `separator`, and leading/trailing separators are
/// trimmed. Used to derive stable, URL/identifier-safe slugs from agent names
/// (e.g. an `ask_<slug>` tool name or an [`AgentId`](crate::registry::AgentId)).
pub fn slugify(input: &str, separator: char) -> String {
    let mapped: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                separator
            }
        })
        .collect();
    mapped.trim_matches(separator).to_string()
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
}
