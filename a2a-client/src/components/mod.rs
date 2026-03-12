//! Reusable web components for A2A interfaces.
//!
//! This module provides ready-to-use components for building web applications
//! that interact with A2A agents. Components include:
//!
//! - **View Models** - [`TaskView`] and [`MessageView`] for rendering UI
//! - **Streaming** - [`create_sse_stream`] for Server-Sent Events with automatic fallback
//!
//! # Feature Requirements
//!
//! The SSE streaming functionality requires the `axum-components` feature flag (enabled by default).
//!
//! # Examples
//!
//! ## Using View Models
//!
//! ```rust
//! use a2a_client::components::{TaskView, MessageView};
//! use a2a_rs::domain::{Task, Message};
//!
//! // Convert an A2A Task to a view model
//! # let task = Task::builder().id("test-123").build();
//! let view = TaskView::from_task(task);
//! println!("Task {} has {} messages", view.task_id, view.message_count);
//!
//! // Convert a Message to a view model
//! # let message = Message::builder().text("Hello").build();
//! let msg_view = MessageView::from_message(message);
//! println!("Message content: {}", msg_view.content);
//! ```
//!
//! ## SSE Streaming (requires `axum-components` feature)
//!
//! ```rust,no_run
//! # #[cfg(feature = "axum-components")]
//! # {
//! use a2a_client::{WebA2AClient, components::create_sse_stream};
//! use axum::{Router, routing::get, extract::{State, Path}};
//! use std::sync::Arc;
//!
//! # async fn example() {
//! let client = Arc::new(WebA2AClient::new_http("http://localhost:8080".to_string()));
//!
//! let app = Router::new()
//!     .route("/stream/:task_id", get(|
//!         State(client): State<Arc<WebA2AClient>>,
//!         Path(task_id): Path<String>
//!     | async move {
//!         create_sse_stream(client, task_id)
//!     }))
//!     .with_state(client);
//! # }
//! # }
//! ```

pub mod streaming;
pub mod task_viewer;

pub use streaming::create_sse_stream;
pub use task_viewer::{MessageView, TaskView};
