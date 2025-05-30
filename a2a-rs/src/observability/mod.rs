//! Observability and logging utilities for a2a-rs
//!
//! This module provides utilities for structured logging, tracing, and metrics collection
//! to help with debugging, monitoring, and understanding system behavior.

#[cfg(feature = "tracing")]
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize tracing with sensible defaults for the A2A application
///
/// This sets up:
/// - Console output with timestamps
/// - Environment-based filtering (RUST_LOG environment variable)
/// - Structured JSON output for production environments
///
/// # Examples
///
/// ```rust,no_run
/// use a2a_rs::observability;
///
/// // Initialize with default settings
/// observability::init_tracing();
///
/// // Or with custom environment filter
/// observability::init_tracing_with_filter("a2a_rs=debug,tower_http=debug");
/// ```
#[cfg(feature = "tracing")]
pub fn init_tracing() {
    init_tracing_with_filter("a2a_rs=info");
}

/// Initialize tracing with a custom filter string
///
/// # Arguments
///
/// * `filter` - Environment filter string (e.g., "a2a_rs=debug,tower_http=debug")
#[cfg(feature = "tracing")]
pub fn init_tracing_with_filter(filter: &str) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter));

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_thread_ids(true)
        .with_thread_names(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

/// Initialize tracing for production environments with JSON output
///
/// This provides structured JSON logs suitable for log aggregation systems
#[cfg(feature = "tracing")]
pub fn init_tracing_json() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("a2a_rs=info,warn"));

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_thread_ids(true)
        .with_thread_names(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

/// Utility struct for creating spans with consistent naming
pub struct SpanBuilder;

impl SpanBuilder {
    /// Create a span for message processing operations
    #[cfg(feature = "tracing")]
    pub fn message_processing(message_id: &str) -> tracing::Span {
        tracing::info_span!(
            "message_processing",
            message.id = %message_id,
            processing.duration_ms = tracing::field::Empty,
            processing.status = tracing::field::Empty,
        )
    }

    /// Create a span for task operations
    #[cfg(feature = "tracing")]
    pub fn task_operation(task_id: &str, operation: &str) -> tracing::Span {
        tracing::info_span!(
            "task_operation",
            task.id = %task_id,
            task.operation = %operation,
            operation.duration_ms = tracing::field::Empty,
            operation.status = tracing::field::Empty,
        )
    }

    /// Create a span for HTTP requests
    #[cfg(feature = "tracing")]
    pub fn http_request(method: &str, path: &str) -> tracing::Span {
        tracing::info_span!(
            "http_request",
            http.method = %method,
            http.path = %path,
            http.status_code = tracing::field::Empty,
            http.duration_ms = tracing::field::Empty,
        )
    }

    /// Create a span for WebSocket connections
    #[cfg(feature = "tracing")]
    pub fn websocket_connection(connection_id: &str) -> tracing::Span {
        tracing::info_span!(
            "websocket_connection",
            ws.connection_id = %connection_id,
            ws.messages_sent = 0u64,
            ws.messages_received = 0u64,
            ws.duration_ms = tracing::field::Empty,
        )
    }

    /// Create a span for authentication operations
    #[cfg(feature = "tracing")]
    pub fn authentication(scheme: &str) -> tracing::Span {
        tracing::info_span!(
            "authentication",
            auth.scheme = %scheme,
            auth.status = tracing::field::Empty,
            auth.duration_ms = tracing::field::Empty,
        )
    }
}

/// Helper macros for consistent error logging
#[macro_export]
macro_rules! log_error {
    ($err:expr) => {
        tracing::error!(
            error = %$err,
            error_type = std::any::type_name_of_val(&$err),
            "Operation failed"
        )
    };
    ($err:expr, $msg:expr) => {
        tracing::error!(
            error = %$err,
            error_type = std::any::type_name_of_val(&$err),
            message = $msg,
            "Operation failed"
        )
    };
}

/// Helper macro for performance tracking
#[macro_export]
macro_rules! measure_duration {
    ($span:expr, $field:expr, $block:expr) => {{
        let start = std::time::Instant::now();
        let result = $block;
        let duration = start.elapsed();
        $span.record($field, duration.as_millis() as u64);
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "tracing")]
    fn test_span_builders() {
        // Just ensure they compile and can be created
        let _ = SpanBuilder::message_processing("test-message-id");
        let _ = SpanBuilder::task_operation("test-task-id", "create");
        let _ = SpanBuilder::http_request("GET", "/api/v1/agent-card");
        let _ = SpanBuilder::websocket_connection("ws-conn-123");
        let _ = SpanBuilder::authentication("bearer");
    }
}
