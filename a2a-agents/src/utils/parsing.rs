//! Text parsing utilities for agent message processing.
//!
//! Common helpers for extracting information from user messages.

use regex::Regex;
use std::sync::LazyLock;

/// Regex for extracting email addresses
static EMAIL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap()
});

/// Regex for extracting URLs
static URL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s]+").unwrap());

/// Regex for extracting numbers (including decimals)
static NUMBER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"-?\d+\.?\d*").unwrap());

/// Extract email addresses from text
pub fn extract_emails(text: &str) -> Vec<String> {
    EMAIL_REGEX
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Extract URLs from text
pub fn extract_urls(text: &str) -> Vec<String> {
    URL_REGEX
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Extract numeric values from text
pub fn extract_numbers(text: &str) -> Vec<f64> {
    NUMBER_REGEX
        .find_iter(text)
        .filter_map(|m| m.as_str().parse::<f64>().ok())
        .collect()
}

/// Simple keyword matching for intent classification
pub fn contains_any_keyword(text: &str, keywords: &[&str]) -> bool {
    let text_lower = text.to_lowercase();
    keywords.iter().any(|&kw| text_lower.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_emails() {
        let text = "Contact us at support@example.com or sales@example.org";
        let emails = extract_emails(text);
        assert_eq!(emails.len(), 2);
        assert!(emails.contains(&"support@example.com".to_string()));
        assert!(emails.contains(&"sales@example.org".to_string()));
    }

    #[test]
    fn test_extract_urls() {
        let text = "Visit https://example.com or http://test.org for more info";
        let urls = extract_urls(text);
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn test_extract_numbers() {
        let text = "I need $150.50 and 200 more";
        let numbers = extract_numbers(text);
        assert_eq!(numbers.len(), 2);
        assert!(numbers.contains(&150.50));
        assert!(numbers.contains(&200.0));
    }

    #[test]
    fn test_contains_any_keyword() {
        assert!(contains_any_keyword("I need help", &["help", "assist"]));
        assert!(!contains_any_keyword("hello there", &["help", "assist"]));
    }
}
