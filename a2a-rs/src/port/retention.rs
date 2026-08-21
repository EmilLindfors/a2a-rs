//! Reclaiming what a retention policy marks stale.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::{A2AError, RetentionPolicy, Swept};

/// Deletes the contexts and remembered facts a [`RetentionPolicy`] has expired.
///
/// A store implements this; nothing calls it on its own. There is no timer in
/// here and no background task: a sweep deletes data an operator can never get
/// back, so *when* it runs belongs to whoever assembled the agent, next to the
/// policy they chose. A supervisor calls [`sweep`](Self::sweep) nightly; a test
/// calls it with a `now` it picked.
///
/// Kept off [`AsyncTaskLifecycle`](crate::port::AsyncTaskLifecycle) and the
/// conversation and state ports deliberately. Those are what a *handler* needs
/// to serve a turn, and a handler has no business deleting a conversation. This
/// is an operator capability, so an assembly can hold the stores without
/// exposing it, and a store that cannot delete (a read-only replica, an audited
/// log) can decline to implement it rather than stubbing a method.
#[async_trait]
pub trait AsyncRetention: Send + Sync {
    /// Delete everything `policy` marks stale as of `now`.
    ///
    /// `now` is a parameter rather than read from the clock inside: the cutoff
    /// is the whole decision, and a caller that cannot name it cannot test a
    /// retention window without waiting one out.
    ///
    /// Each context is swept in one transaction, so a context is either gone or
    /// intact — never a task list pointing at a conversation that was deleted
    /// underneath it. Contexts are independent of each other, so a sweep that
    /// fails partway has still finished the ones before it; the error names the
    /// context it stopped on and the next sweep picks up the rest.
    ///
    /// Sweeping under [`RetentionPolicy::keep_everything`] is not an error. It
    /// deletes nothing and returns an empty [`Swept`].
    async fn sweep(&self, policy: &RetentionPolicy, now: DateTime<Utc>) -> Result<Swept, A2AError>;
}
