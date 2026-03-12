//! A2A Agents Common - Shared utilities for building A2A Protocol agents
//!
//! This crate provides reusable utilities and patterns that agent developers can use
//! to build their agents more quickly and easily.
//!
//! # Modules
//!
//! - [`nlp`] - Natural language processing utilities (intent classification, entity extraction)
//! - [`formatting`] - Output formatting helpers (markdown, tables, charts)
//! - [`caching`] - Caching utilities for performance optimization (requires `async` feature)
//! - [`testing`] - Testing utilities and fixtures
//!
//! # Quick Start
//!
//! ```rust
//! use a2a_agents_common::nlp::IntentClassifier;
//!
//! let classifier = IntentClassifier::new()
//!     .add_intent("greeting", &["hello", "hi", "hey"])
//!     .add_intent("farewell", &["bye", "goodbye", "see you"]);
//!
//! let intent = classifier.classify("Hello there!");
//! assert_eq!(intent, Some("greeting"));
//! ```
//!
//! # Features
//!
//! - `async` - Enable async utilities like caching
//! - `full` - Enable all features

pub mod nlp;
pub mod formatting;
#[cfg(feature = "async")]
pub mod caching;
pub mod testing;

// Re-export commonly used items
pub use nlp::{IntentClassifier, EntityExtractor};
pub use formatting::{MarkdownFormatter, TableFormatter};

/// Common error type for utilities
#[derive(Debug, thiserror::Error)]
pub enum CommonError {
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

pub type Result<T> = std::result::Result<T, CommonError>;
