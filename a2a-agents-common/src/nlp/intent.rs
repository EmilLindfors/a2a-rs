//! Intent classification for understanding user queries.
//!
//! Provides a simple keyword-based intent classifier.

use std::collections::HashMap;

/// Simple keyword-based intent classifier.
///
/// # Example
///
/// ```
/// use a2a_agents_common::nlp::IntentClassifier;
///
/// let classifier = IntentClassifier::new()
///     .add_intent("get_quote", &["price", "quote", "what's", "show me"])
///     .add_intent("analyze", &["analyze", "analysis", "should i buy"]);
///
/// assert_eq!(classifier.classify("What's the price of AAPL?"), Some("get_quote"));
/// assert_eq!(classifier.classify("Should I buy Tesla?"), Some("analyze"));
/// assert_eq!(classifier.classify("Random text"), None);
/// ```
#[derive(Debug, Clone)]
pub struct IntentClassifier {
    intents: HashMap<String, Vec<String>>,
}

impl IntentClassifier {
    /// Create a new intent classifier.
    pub fn new() -> Self {
        Self {
            intents: HashMap::new(),
        }
    }

    /// Add an intent with associated keywords.
    pub fn add_intent(mut self, intent: &str, keywords: &[&str]) -> Self {
        self.intents.insert(
            intent.to_string(),
            keywords.iter().map(|s| s.to_lowercase()).collect(),
        );
        self
    }

    /// Add keywords to an existing intent or create it if it doesn't exist.
    pub fn add_keywords(&mut self, intent: &str, keywords: &[&str]) {
        self.intents
            .entry(intent.to_string())
            .or_insert_with(Vec::new)
            .extend(keywords.iter().map(|s| s.to_lowercase()));
    }

    /// Classify a text by finding the intent with the most matching keywords.
    ///
    /// Returns the intent name if a match is found, or None if no keywords match.
    pub fn classify(&self, text: &str) -> Option<&str> {
        let text_lower = text.to_lowercase();

        let mut best_intent: Option<&str> = None;
        let mut best_score = 0;

        for (intent, keywords) in &self.intents {
            let score = keywords
                .iter()
                .filter(|kw| text_lower.contains(kw.as_str()))
                .count();

            if score > best_score {
                best_score = score;
                best_intent = Some(intent.as_str());
            }
        }

        best_intent
    }

    /// Get all matching intents with their scores.
    ///
    /// Returns a vector of (intent, score) tuples sorted by score descending.
    pub fn classify_all(&self, text: &str) -> Vec<(&str, usize)> {
        let text_lower = text.to_lowercase();

        let mut results: Vec<(&str, usize)> = self
            .intents
            .iter()
            .map(|(intent, keywords)| {
                let score = keywords
                    .iter()
                    .filter(|kw| text_lower.contains(kw.as_str()))
                    .count();
                (intent.as_str(), score)
            })
            .filter(|(_, score)| *score > 0)
            .collect();

        results.sort_by(|a, b| b.1.cmp(&a.1));
        results
    }
}

impl Default for IntentClassifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_classification() {
        let classifier = IntentClassifier::new()
            .add_intent("greeting", &["hello", "hi", "hey"])
            .add_intent("farewell", &["bye", "goodbye", "see you"]);

        assert_eq!(classifier.classify("Hello there!"), Some("greeting"));
        assert_eq!(classifier.classify("Goodbye friend"), Some("farewell"));
        assert_eq!(classifier.classify("Random text"), None);
    }

    #[test]
    fn test_best_match() {
        let classifier = IntentClassifier::new()
            .add_intent("quote", &["price", "quote"])
            .add_intent("analyze", &["analyze", "analysis", "price"]);

        // "price" matches both, but only one keyword
        // Should pick the first match
        let result = classifier.classify("What's the price?");
        assert!(result == Some("quote") || result == Some("analyze"));

        // "analyze price" should match "analyze" better (2 keywords)
        assert_eq!(
            classifier.classify("I want to analyze the price"),
            Some("analyze")
        );
    }

    #[test]
    fn test_classify_all() {
        let classifier = IntentClassifier::new()
            .add_intent("quote", &["price", "quote"])
            .add_intent("analyze", &["analyze", "price"]);

        let results = classifier.classify_all("What's the price and can you analyze it?");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "analyze"); // 2 matches
        assert_eq!(results[1].0, "quote"); // 1 match
    }

    #[test]
    fn test_case_insensitive() {
        let classifier = IntentClassifier::new().add_intent("greeting", &["HELLO", "Hi"]);

        assert_eq!(classifier.classify("hello there"), Some("greeting"));
        assert_eq!(classifier.classify("HI FRIEND"), Some("greeting"));
    }
}
