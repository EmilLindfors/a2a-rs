//! Utility functions for A2A web clients.
//!
//! This module provides helper functions for working with A2A protocol types
//! in web applications. It includes formatting utilities for displaying tasks
//! and messages in user interfaces.
//!
//! # Examples
//!
//! ```rust
//! use a2a_client::utils::{format_task_state, format_message_content, truncate_preview};
//! use a2a_rs::domain::{TaskState, Part};
//!
//! // Format a task state for display
//! let state = TaskState::Working;
//! assert_eq!(format_task_state(&state), "Working");
//!
//! // Format message parts as text
//! let parts = vec![Part::text("Hello, world!".to_string())];
//! let content = format_message_content(&parts);
//! assert_eq!(content, "Hello, world!");
//!
//! // Truncate text for previews
//! let preview = truncate_preview("This is a very long message", 10);
//! assert_eq!(preview, "This is a ...");
//! ```

pub mod formatters;

pub use formatters::{format_message_content, format_task_state, truncate_preview};
