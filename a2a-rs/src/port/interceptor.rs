//! The `CallInterceptor` port: before/after middleware around A2A calls.
//!
//! An interceptor is a cross-cutting hook that runs *around* every A2A call —
//! the chain-of-responsibility analogue of the official SDK's `CallInterceptor`.
//! It is a **port** (a capability the application needs from the edge), so the
//! trait lives here and concrete interceptors (logging, metrics, auth-token
//! injection) are adapters. The same trait is wired into both the client
//! transport ([`JsonRpcClient`](crate::adapter::JsonRpcClient)) and the server
//! transport ([`JsonRpcAdapter`](crate::adapter::JsonRpcAdapter)); the
//! [`CallContext::side`] tells an interceptor which direction it is observing.
//!
//! Chains run `before` hooks in registration order, dispatch the call, then run
//! `after` hooks in reverse order — the conventional onion ordering, so an
//! interceptor's `after` wraps everything its `before` set up. A `before` that
//! returns `Err` short-circuits the call (the dispatch never happens) but its
//! `after` still runs, observing the error.
//!
//! The hooks see call *metadata* (method name, side), not the typed
//! request/response — those differ per method and would force the trait generic.
//! Metadata is enough for the canonical uses (logging, metrics, tracing spans,
//! header/auth propagation handled by the adapter around the chain).

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::A2AError;

/// Which side of the wire an interceptor chain is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSide {
    /// The outbound client transport is making the call.
    Client,
    /// The inbound server transport is handling the call.
    Server,
}

/// Metadata about an in-flight A2A call, passed to each interceptor hook.
#[derive(Debug, Clone)]
pub struct CallContext {
    /// The A2A method name (PascalCase wire name, e.g. `"SendMessage"`).
    pub method: String,
    /// Whether this chain runs on the client or server side.
    pub side: CallSide,
}

impl CallContext {
    /// Construct a context for `method` on the given `side`.
    pub fn new(method: impl Into<String>, side: CallSide) -> Self {
        Self {
            method: method.into(),
            side,
        }
    }
}

/// A before/after hook around an A2A call (auth, logging, metrics, tracing).
///
/// Both hooks have default no-op bodies, so an interceptor overrides only the
/// side it cares about.
#[async_trait]
pub trait CallInterceptor: Send + Sync {
    /// Run before the call is dispatched. Returning `Err` short-circuits the
    /// call: the dispatch is skipped and the error is returned to the caller
    /// (after `after` hooks still run, observing it).
    async fn before(&self, _ctx: &CallContext) -> Result<(), A2AError> {
        Ok(())
    }

    /// Run after the call completes, observing its outcome (`Ok` on success,
    /// `Err` with a borrow of the error otherwise).
    async fn after(&self, _ctx: &CallContext, _outcome: Result<(), &A2AError>) {}
}

/// Run a chain's `before` hooks in registration order; the first `Err`
/// short-circuits and is returned without invoking the remaining hooks.
pub async fn run_before(
    interceptors: &[Arc<dyn CallInterceptor>],
    ctx: &CallContext,
) -> Result<(), A2AError> {
    for interceptor in interceptors {
        interceptor.before(ctx).await?;
    }
    Ok(())
}

/// Run a chain's `after` hooks in reverse registration order (onion unwinding).
pub async fn run_after(
    interceptors: &[Arc<dyn CallInterceptor>],
    ctx: &CallContext,
    outcome: Result<(), &A2AError>,
) {
    for interceptor in interceptors.iter().rev() {
        interceptor.after(ctx, outcome).await;
    }
}
