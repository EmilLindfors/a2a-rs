//! Fitting a conversation into a model's context window.
//!
//! Everything here is pure — no I/O, no provider, no storage. A caller loads a
//! conversation from wherever it lives, [`project`]s it into a message list,
//! and [`fit`]s that to a [`ContextBudget`]. What [`fit`] cannot solve by
//! trimming it reports as [`Fit::ShouldCompact`], and the caller — which has
//! the LLM and the conversation store — summarizes.
//!
//! The split matters for testing: the trimming rules are the part with edge
//! cases, and they can be exercised without a database or a model.

mod budget;
mod estimate;
mod project;

pub use budget::{ContextBudget, ContextError, Fit, cap_tool_result, fit};
pub use estimate::{CharEstimate, TokenEstimate};
pub use project::{Turn, TurnRole, project};
