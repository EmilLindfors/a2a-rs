//! The record of what a task's stream already said, so a client can be told it
//! again.

use async_trait::async_trait;

use crate::domain::A2AError;
use crate::port::streaming_handler::{SeqEvent, UpdateEvent};

/// Assigns each of a task's updates a monotonic id and keeps it long enough to
/// replay.
///
/// Split from [`AsyncStreamingHandler`] because the two answer different
/// questions. The streaming handler is about *who is listening right now* — an
/// in-process fan-out that a restart is entitled to forget. This is about *what
/// was said*, which a client resuming after a disconnect still needs, and which
/// a restart is not entitled to forget if the store is durable.
///
/// Ids are per task and start at 1. `0` is reserved for the initial task
/// snapshot, which is not a logged event and carries no id.
///
/// [`AsyncStreamingHandler`]: crate::port::AsyncStreamingHandler
#[async_trait]
pub trait AsyncEventLog: Send + Sync {
    /// Record `event` as the next event on `task_id`'s stream and return it
    /// with the id it was given.
    ///
    /// The id comes from the log rather than from a counter the caller keeps,
    /// which is what makes ids survive a restart: an in-memory counter starts
    /// again at 1 and hands out ids a resuming client has already seen.
    async fn append(&self, task_id: &str, event: UpdateEvent) -> Result<SeqEvent, A2AError>;

    /// What this log can still replay to a client whose last id was `from`.
    async fn replay(&self, task_id: &str, from: u64) -> Result<Replay, A2AError>;

    /// Forget a task's events.
    async fn discard(&self, task_id: &str) -> Result<(), A2AError>;
}

/// The answer to [`AsyncEventLog::replay`]: the events after the requested id,
/// and whether they are all of them.
#[derive(Debug, Clone, Default)]
pub struct Replay {
    /// Whether the log still holds every event after the requested id.
    ///
    /// A bounded log discards its oldest events, and a client that was
    /// disconnected long enough falls off the back of it. When this is false
    /// the tail in [`events`](Self::events) starts partway through the gap, so
    /// it is a fragment of what the client missed rather than the remainder of
    /// it.
    pub complete: bool,
    /// The retained events with an id greater than the requested one, in id
    /// order.
    pub events: Vec<SeqEvent>,
}

impl Replay {
    /// The answer for a log whose retained ids run from `oldest` (`None` when it
    /// holds nothing) and that has `events` to hand back.
    ///
    /// Completeness is `oldest <= from + 1`: the next event the client is owed
    /// is `from + 1`, and the log covers the gap when it still holds it. An
    /// empty log covers `from = 0` — a client that has seen nothing has missed
    /// nothing — and covers no other `from`, which is the answer an in-memory
    /// log gives after a restart.
    pub fn bounded_by(oldest: Option<u64>, from: u64, events: Vec<SeqEvent>) -> Self {
        Self {
            complete: oldest.map_or(from == 0, |oldest| oldest <= from + 1),
            events,
        }
    }
}
