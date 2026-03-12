//! Text formatting utilities for agent responses.
//!
//! Provides helpers for creating well-formatted output including markdown, tables, and lists.

mod markdown;
mod table;

pub use markdown::MarkdownFormatter;
pub use table::TableFormatter;
