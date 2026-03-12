//! Natural Language Processing utilities for agents.
//!
//! This module provides simple NLP tools for common agent tasks like
//! intent classification and entity extraction.

mod intent;
mod entity;

pub use intent::IntentClassifier;
pub use entity::EntityExtractor;
