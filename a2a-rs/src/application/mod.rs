//! Application services for the A2A protocol

#[cfg(feature = "server")]
pub mod task_service;
#[cfg(feature = "server")]
pub mod task_status_broadcast;

#[cfg(feature = "server")]
pub use task_service::{TaskService, UpdateStream};
#[cfg(feature = "server")]
pub use task_status_broadcast::{HasStreaming, HasTaskLifecycle, TaskStatusBroadcast};
