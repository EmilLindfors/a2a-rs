//! Entity extraction from text using regex patterns.

use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;

lazy_static! {
    /// Regex for extracting email addresses
    static ref EMAIL_REGEX: Regex = Regex::new(
        r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"
    ).unwrap();

    /// Regex for extracting URLs
    static ref URL_REGEX: Regex = Regex::new(
        r"https?://[^\s]+"
    ).unwrap();

    /// Regex for extracting numbers (including decimals and negatives)
    static ref NUMBER_REGEX: Regex = Regex::new(
        r"-?\d+\.?\d*"
    ).unwrap();

    /// Regex for extracting currency amounts (e.g., $100, €50.99)
    static ref CURRENCY_REGEX: Regex = Regex::new(
        r"[$€£¥]\s*\d+(?:\.\d{2})?"
    ).unwrap();

    /// Regex for extracting dates (simple formats: YYYY-MM-DD, MM/DD/YYYY, DD.MM.YYYY)
    static ref DATE_REGEX: Regex = Regex::new(
        r"\d{4}-\d{2}-\d{2}|\d{1,2}/\d{1,2}/\d{4}|\d{1,2}\.\d{1,2}\.\d{4}"
    ).unwrap();
}

/// Entity extractor for finding structured data in text.
///
/// # Example
///
/// ```
/// use a2a_agents_common::nlp::EntityExtractor;
///
/// let extractor = EntityExtractor::new();
///
/// let entities = extractor.extract("Contact me at user@example.com or visit https://example.com");
/// assert_eq!(entities.get("email").unwrap()[0], "user@example.com");
/// assert_eq!(entities.get("url").unwrap()[0], "https://example.com");
/// ```
#[derive(Debug, Clone)]
pub struct EntityExtractor {
    custom_patterns: HashMap<String, Regex>,
}

impl EntityExtractor {
    /// Create a new entity extractor.
    pub fn new() -> Self {
        Self {
            custom_patterns: HashMap::new(),
        }
    }

    /// Add a custom pattern for extracting entities.
    ///
    /// # Example
    ///
    /// ```
    /// use a2a_agents_common::nlp::EntityExtractor;
    ///
    /// let extractor = EntityExtractor::new()
    ///     .with_pattern("stock_symbol", r"[A-Z]{1,5}");
    ///
    /// let entities = extractor.extract("Check AAPL and MSFT");
    /// assert_eq!(entities.get("stock_symbol").unwrap().len(), 2);
    /// ```
    pub fn with_pattern(mut self, name: &str, pattern: &str) -> Self {
        if let Ok(regex) = Regex::new(pattern) {
            self.custom_patterns.insert(name.to_string(), regex);
        }
        self
    }

    /// Add a custom pattern (mutable version).
    pub fn add_pattern(&mut self, name: &str, pattern: &str) -> Result<(), regex::Error> {
        let regex = Regex::new(pattern)?;
        self.custom_patterns.insert(name.to_string(), regex);
        Ok(())
    }

    /// Extract all entities from text.
    ///
    /// Returns a HashMap where keys are entity types and values are vectors of extracted strings.
    pub fn extract(&self, text: &str) -> HashMap<String, Vec<String>> {
        let mut entities: HashMap<String, Vec<String>> = HashMap::new();

        // Extract built-in entity types
        self.extract_builtin("email", &EMAIL_REGEX, text, &mut entities);
        self.extract_builtin("url", &URL_REGEX, text, &mut entities);
        self.extract_builtin("number", &NUMBER_REGEX, text, &mut entities);
        self.extract_builtin("currency", &CURRENCY_REGEX, text, &mut entities);
        self.extract_builtin("date", &DATE_REGEX, text, &mut entities);

        // Extract custom patterns
        for (name, regex) in &self.custom_patterns {
            self.extract_builtin(name, regex, text, &mut entities);
        }

        entities
    }

    /// Extract a specific entity type.
    pub fn extract_type(&self, text: &str, entity_type: &str) -> Vec<String> {
        match entity_type {
            "email" => self.extract_with_regex(&EMAIL_REGEX, text),
            "url" => self.extract_with_regex(&URL_REGEX, text),
            "number" => self.extract_with_regex(&NUMBER_REGEX, text),
            "currency" => self.extract_with_regex(&CURRENCY_REGEX, text),
            "date" => self.extract_with_regex(&DATE_REGEX, text),
            custom => {
                if let Some(regex) = self.custom_patterns.get(custom) {
                    self.extract_with_regex(regex, text)
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn extract_builtin(
        &self,
        name: &str,
        regex: &Regex,
        text: &str,
        entities: &mut HashMap<String, Vec<String>>,
    ) {
        let matches = self.extract_with_regex(regex, text);
        if !matches.is_empty() {
            entities.insert(name.to_string(), matches);
        }
    }

    fn extract_with_regex(&self, regex: &Regex, text: &str) -> Vec<String> {
        regex
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect()
    }
}

impl Default for EntityExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_emails() {
        let extractor = EntityExtractor::new();
        let entities = extractor.extract("Contact support@example.com or sales@example.org");

        let emails = entities.get("email").unwrap();
        assert_eq!(emails.len(), 2);
        assert!(emails.contains(&"support@example.com".to_string()));
        assert!(emails.contains(&"sales@example.org".to_string()));
    }

    #[test]
    fn test_extract_urls() {
        let extractor = EntityExtractor::new();
        let entities = extractor.extract("Visit https://example.com or http://test.org");

        let urls = entities.get("url").unwrap();
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn test_extract_numbers() {
        let extractor = EntityExtractor::new();
        let entities = extractor.extract("I need 150.50 and 200 more, maybe -10");

        let numbers = entities.get("number").unwrap();
        assert!(numbers.len() >= 3);
    }

    #[test]
    fn test_extract_currency() {
        let extractor = EntityExtractor::new();
        let entities = extractor.extract("That costs $150.99 or €200");

        let currency = entities.get("currency").unwrap();
        assert_eq!(currency.len(), 2);
    }

    #[test]
    fn test_extract_dates() {
        let extractor = EntityExtractor::new();
        let entities =
            extractor.extract("Meeting on 2025-12-06 or 12/25/2025 or maybe 25.12.2025");

        let dates = entities.get("date").unwrap();
        assert_eq!(dates.len(), 3);
    }

    #[test]
    fn test_custom_pattern() {
        let extractor = EntityExtractor::new().with_pattern("stock_symbol", r"\b[A-Z]{2,5}\b");

        let entities = extractor.extract("Check AAPL and MSFT stock prices");
        let symbols = entities.get("stock_symbol").unwrap();
        assert_eq!(symbols.len(), 2);
        assert!(symbols.contains(&"AAPL".to_string()));
        assert!(symbols.contains(&"MSFT".to_string()));
    }

    #[test]
    fn test_extract_specific_type() {
        let extractor = EntityExtractor::new();
        let emails = extractor.extract_type("Email me at test@example.com", "email");

        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0], "test@example.com");
    }
}
