//! How long a store keeps what it was given.

use std::time::Duration;

use chrono::{DateTime, Utc};

/// What a store is allowed to delete, and after how long.
///
/// Every knob is off by default. Deleting a conversation is not free housekeeping:
/// `tasks/get` with `history_length` is a protocol feature, so a swept context
/// turns a call the client is entitled to make into a `TaskNotFound`. An
/// operator turns each knob on knowing that.
///
/// Idleness is measured from the last **write**. Reading a conversation or a
/// state bag records nothing — in either storage adapter — so an agent that only
/// ever reads a context does not keep it alive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionPolicy {
    idle_contexts_after: Option<Duration>,
    idle_user_state_after: Option<Duration>,
}

impl RetentionPolicy {
    /// The default: nothing is ever deleted.
    pub const fn keep_everything() -> Self {
        Self {
            idle_contexts_after: None,
            idle_user_state_after: None,
        }
    }

    /// Delete a context, its tasks, its conversation, its digests and its
    /// `context:`-scoped state once nothing has written to it for `idle`.
    ///
    /// A context holding an unfinished task (`submitted`, `working`, or a state
    /// this build does not recognize) is never swept, however long it has been
    /// quiet — that is a task something may still be working on. `input-required`
    /// and `auth-required` do count as finished for this purpose: they are
    /// waiting on a caller who, after `idle`, is not coming back.
    #[must_use]
    pub const fn delete_contexts_idle_for(mut self, idle: Duration) -> Self {
        self.idle_contexts_after = Some(idle);
        self
    }

    /// Delete a principal's `user:`-scoped state once none of its keys has been
    /// written for `idle`.
    ///
    /// Separate from [`delete_contexts_idle_for`](Self::delete_contexts_idle_for)
    /// because a `user:` bucket belongs to a principal rather than to a context:
    /// it is readable from a context that principal has not opened yet, so no
    /// context going idle says anything about whether it is stale.
    ///
    /// The unit is the principal, not the key. Expiring keys one at a time
    /// leaves a bag that remembers somebody's city and not their name, which
    /// reads to the model as fact rather than as absence.
    #[must_use]
    pub const fn delete_user_state_idle_for(mut self, idle: Duration) -> Self {
        self.idle_user_state_after = Some(idle);
        self
    }

    /// How long a context may go unwritten before it is swept, if ever.
    pub const fn idle_contexts_after(&self) -> Option<Duration> {
        self.idle_contexts_after
    }

    /// How long a principal's `user:` state may go unwritten before it is
    /// swept, if ever.
    pub const fn idle_user_state_after(&self) -> Option<Duration> {
        self.idle_user_state_after
    }

    /// The instant a context must have been written after to survive a sweep
    /// run at `now`, or `None` when contexts are never swept.
    ///
    /// Both adapters read the cutoff from here rather than each doing the
    /// subtraction, which is also where the one edge case lives: a window so
    /// long that `now - window` is not a representable instant expires nothing,
    /// so it reads as "keep".
    pub fn context_cutoff(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        cutoff(self.idle_contexts_after, now)
    }

    /// The instant a principal's `user:` state must have been written after to
    /// survive a sweep run at `now`, or `None` when it is never swept.
    pub fn user_state_cutoff(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        cutoff(self.idle_user_state_after, now)
    }

    /// Whether a sweep under this policy could delete anything at all.
    ///
    /// Lets a caller skip scheduling a sweep it has not configured, rather than
    /// waking up nightly to run a query that can only return zero.
    pub const fn is_noop(&self) -> bool {
        self.idle_contexts_after.is_none() && self.idle_user_state_after.is_none()
    }
}

/// Whether reading a principal's `user:` state counts as keeping it alive.
///
/// [`RetentionPolicy`] measures idleness from writes alone, which is what keeps
/// a read from costing one. The cost of that rule is a `user:` fact the model
/// reads on every turn and rewrites never: it expires under the agent, and the
/// agent forgets something it was using.
///
/// This buys the fact back without paying a write per turn. A read refreshes
/// the bag only once it is already `after` old, so the extra writes are bounded
/// at one per principal per window however often it is read — which is the
/// whole reason there is a window here rather than a bool.
///
/// Off by default, so a store that is not told otherwise behaves exactly as it
/// did: a read records nothing. It lives here rather than in either adapter for
/// the reason the policy does — two stores deciding separately what keeps a bag
/// alive would be two retention rules wearing one name.
///
/// The unit is the principal, matching
/// [`RetentionPolicy::delete_user_state_idle_for`]. Contexts need none of this:
/// one that is in use is written to by its own task history, so its idleness
/// maintains itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadRefresh {
    after: Option<Duration>,
}

impl ReadRefresh {
    /// The default: a read records nothing.
    pub const fn never() -> Self {
        Self { after: None }
    }

    /// Refresh a principal's `user:` bag when a read finds it `after` old.
    ///
    /// Shorter than the policy's `user:` window, or the bag expires between the
    /// refresh and the sweep and the knob buys nothing.
    /// [`halfway_through`](Self::halfway_through) picks such a value from the
    /// policy rather than leaving the two numbers to be kept in step by hand.
    #[must_use]
    pub const fn after(after: Duration) -> Self {
        Self { after: Some(after) }
    }

    /// Refresh once a bag is halfway to the cutoff `policy` would sweep it at.
    ///
    /// The wiring worth having: an operator names one window — the one they
    /// were going to name anyway — and a read-kept bag is refreshed with half
    /// the window still to spare. A policy that never sweeps `user:` state
    /// gives [`never`](Self::never), since there is nothing to keep it ahead of.
    pub fn halfway_through(policy: &RetentionPolicy) -> Self {
        Self {
            after: policy.idle_user_state_after().map(|window| window / 2),
        }
    }

    /// How old a bag must be before a read refreshes it, if ever.
    pub const fn after_window(&self) -> Option<Duration> {
        self.after
    }

    /// The instant a bag must have been written after to need no refresh at
    /// `now`, or `None` when reads refresh nothing.
    ///
    /// What a store binds into a conditional update. Shares the policy's
    /// arithmetic, including that a window too long to subtract reads as
    /// "nothing to do" rather than refreshing everything.
    pub fn cutoff(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        cutoff(self.after, now)
    }

    /// Whether a bag last written at `last_write` is due a refresh at `now`.
    ///
    /// The same question [`cutoff`](Self::cutoff) asks, for a store that
    /// already holds the timestamp and has nothing to bind it into.
    ///
    /// Inclusive at the boundary where a sweep's is exclusive, and the
    /// asymmetry is deliberate: a sweep deletes, so a bag exactly at its cutoff
    /// is kept; a refresh writes, so one exactly at this cutoff is refreshed.
    /// Erring early costs a write nobody notices, erring late costs the bag.
    pub fn due(&self, last_write: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        self.cutoff(now).is_some_and(|cutoff| last_write <= cutoff)
    }
}

/// `now - window`, or `None` if there is no window or the result is not a
/// representable instant.
fn cutoff(window: Option<Duration>, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let window = chrono::TimeDelta::from_std(window?).ok()?;
    now.checked_sub_signed(window)
}

/// What one sweep reclaimed.
///
/// Counted per table rather than as one number, because the tables answer
/// different questions: `messages` is how much transcript is gone, `state_keys`
/// how many remembered facts, and an operator sizing a retention window wants
/// them apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    /// Contexts swept whole, counting one per context id — including a context
    /// that only ever held tasks and so had no row of its own.
    pub contexts: u64,
    /// Tasks deleted with their contexts. Their push-notification configs and
    /// stream events go with them and are not counted separately.
    pub tasks: u64,
    /// Conversation messages deleted with their contexts.
    pub messages: u64,
    /// Compaction digests deleted with their contexts.
    pub digests: u64,
    /// State keys deleted: `context:`-scoped ones swept with their context,
    /// plus every `user:`-scoped key of an expired principal.
    pub state_keys: u64,
}

impl Swept {
    /// Whether the sweep deleted nothing.
    pub const fn is_empty(&self) -> bool {
        self.contexts == 0
            && self.tasks == 0
            && self.messages == 0
            && self.digests == 0
            && self.state_keys == 0
    }
}

/// Accumulate one context's sweep into a running total. A sweep visits each
/// context on its own, so the totals are built this way.
impl std::ops::AddAssign for Swept {
    fn add_assign(&mut self, other: Self) {
        self.contexts += other.contexts;
        self.tasks += other.tasks;
        self.messages += other.messages;
        self.digests += other.digests;
        self.state_keys += other.state_keys;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    #[test]
    fn the_default_policy_deletes_nothing() {
        let policy = RetentionPolicy::default();
        assert_eq!(policy, RetentionPolicy::keep_everything());
        assert!(policy.is_noop());
        assert_eq!(policy.idle_contexts_after(), None);
        assert_eq!(policy.idle_user_state_after(), None);
    }

    #[test]
    fn each_knob_is_set_independently() {
        let week = Duration::from_secs(7 * 24 * 60 * 60);
        let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(week);

        assert!(!policy.is_noop());
        assert_eq!(policy.idle_contexts_after(), Some(week));
        // Sweeping contexts says nothing about `user:` state, which belongs to a
        // principal rather than to any context.
        assert_eq!(policy.idle_user_state_after(), None);
    }

    #[test]
    fn a_sweep_that_deleted_one_row_is_not_empty() {
        assert!(Swept::default().is_empty());
        assert!(
            !Swept {
                state_keys: 1,
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn the_cutoff_is_the_window_before_now() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let day = Duration::from_secs(24 * 60 * 60);
        let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(day);

        assert_eq!(
            policy.context_cutoff(now),
            DateTime::<Utc>::from_timestamp(1_700_000_000 - 86_400, 0)
        );
        assert_eq!(policy.user_state_cutoff(now), None);
    }

    #[test]
    fn a_read_refreshes_nothing_by_default() {
        let refresh = ReadRefresh::default();
        assert_eq!(refresh, ReadRefresh::never());
        assert_eq!(refresh.after_window(), None);

        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let ancient = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        assert_eq!(refresh.cutoff(now), None);
        assert!(
            !refresh.due(ancient, now),
            "nothing is old enough when nothing is refreshed"
        );
    }

    #[test]
    fn a_bag_is_due_only_once_it_is_older_than_the_window() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let day = Duration::from_secs(24 * 60 * 60);
        let refresh = ReadRefresh::after(day);

        let cutoff = refresh.cutoff(now).unwrap();
        assert_eq!(
            cutoff,
            DateTime::<Utc>::from_timestamp(1_700_000_000 - 86_400, 0).unwrap()
        );

        assert!(refresh.due(cutoff - TimeDelta::seconds(1), now));
        assert!(refresh.due(cutoff, now), "the boundary refreshes");
        assert!(
            !refresh.due(cutoff + TimeDelta::seconds(1), now),
            "a bag written since the cutoff is still fresh"
        );
    }

    /// One number, not two that have to be kept in a fixed relation.
    #[test]
    fn the_refresh_window_is_half_the_policys() {
        let week = Duration::from_secs(7 * 24 * 60 * 60);
        let policy = RetentionPolicy::keep_everything().delete_user_state_idle_for(week);

        assert_eq!(
            ReadRefresh::halfway_through(&policy).after_window(),
            Some(week / 2)
        );
        // Nothing to stay ahead of, so nothing to refresh.
        assert_eq!(
            ReadRefresh::halfway_through(&RetentionPolicy::keep_everything()),
            ReadRefresh::never()
        );
        // A policy that only sweeps contexts says nothing about a `user:` bag.
        let contexts_only = RetentionPolicy::keep_everything().delete_contexts_idle_for(week);
        assert_eq!(
            ReadRefresh::halfway_through(&contexts_only),
            ReadRefresh::never()
        );
    }

    /// The same edge the policy has, answered the same way: a window that
    /// cannot be subtracted refreshes nothing rather than everything.
    #[test]
    fn an_unrepresentable_refresh_window_refreshes_nothing() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        assert_eq!(ReadRefresh::after(Duration::MAX).cutoff(now), None);
    }

    /// A window longer than the calendar expires nothing rather than panicking
    /// or wrapping into the future and deleting everything.
    #[test]
    fn an_unrepresentable_window_keeps_everything() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(Duration::MAX);

        assert_eq!(policy.context_cutoff(now), None);
    }
}
