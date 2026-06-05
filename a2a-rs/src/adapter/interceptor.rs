//! Built-in [`CallInterceptor`](crate::port::CallInterceptor) adapters.
//!
//! Concrete interceptors live in the adapter layer (the port is just the trait).
//! They attach to either transport via `with_interceptor`.

#[cfg(feature = "tracing")]
use async_trait::async_trait;

#[cfg(feature = "tracing")]
use crate::domain::A2AError;
#[cfg(feature = "tracing")]
use crate::port::{CallContext, CallInterceptor};

/// A [`CallInterceptor`](crate::port::CallInterceptor) that logs each call's
/// start and outcome via `tracing`.
///
/// Register it on a client or server transport to get one structured log line
/// per call boundary (method, side) plus a success/failure line with the error.
/// A drop-in for the official SDK's logging interceptor.
#[cfg(feature = "tracing")]
#[derive(Debug, Clone, Default)]
pub struct LoggingInterceptor;

#[cfg(feature = "tracing")]
#[async_trait]
impl CallInterceptor for LoggingInterceptor {
    async fn before(&self, ctx: &CallContext) -> Result<(), A2AError> {
        tracing::debug!(method = %ctx.method, side = ?ctx.side, "A2A call started");
        Ok(())
    }

    async fn after(&self, ctx: &CallContext, outcome: Result<(), &A2AError>) {
        match outcome {
            Ok(()) => {
                tracing::debug!(method = %ctx.method, side = ?ctx.side, "A2A call succeeded")
            }
            Err(e) => {
                tracing::warn!(method = %ctx.method, side = ?ctx.side, error = %e, "A2A call failed")
            }
        }
    }
}
