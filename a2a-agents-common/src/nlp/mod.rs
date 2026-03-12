//! Natural Language Processing utilities for agents.
//!
//! This module provides simple NLP tools for common agent tasks like
//! intent classification and entity extraction.

mod entity;
mod intent;

pub use entity::EntityExtractor;
pub use intent::IntentClassifier;
