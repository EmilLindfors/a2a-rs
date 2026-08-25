//! In-process [`AsyncEventLog`]: a bounded ring buffer per task.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::domain::A2AError;
use crate::port::event_log::{AsyncEventLog, Replay};
use crate::port::streaming_handler::{SeqEvent, UpdateEvent};

/// How many events per task an [`InMemoryEventLog`] keeps by default.
pub const DEFAULT_CAPACITY: usize = 256;

/// One task's retained tail.
struct TaskLog {
    next_id: u64,
    events: VecDeque<SeqEvent>,
}

impl TaskLog {
    fn new(capacity: usize) -> Self {
        Self {
            next_id: 0,
            events: VecDeque::with_capacity(capacity),
        }
    }

    fn append(&mut self, event: UpdateEvent, capacity: usize) -> SeqEvent {
        self.next_id += 1;
        let seq = SeqEvent::new(self.next_id, event);
        if self.events.len() == capacity {
            self.events.pop_front();
        }
        self.events.push_back(seq.clone());
        seq
    }
}

/// A task's update log held in this process's memory.
///
/// Cloning shares the log, so a clone sees the same events. This is the default
/// under [`InMemoryStreamingHandler`], and it is what makes resumption work
/// within one run of a server: a client that reconnects gets the tail it missed,
/// up to [`capacity`](Self::with_capacity) events per task.
///
/// What it cannot do is survive the process. A restart starts every task's ids
/// again at 1, and a client resuming with an id from before the restart is told
/// the log cannot cover it ([`Replay::complete`] is false) rather than handed a
/// tail that means something else. Use a durable log — `SqlxTaskStorage`
/// implements this port — where resumption has to outlive a restart.
///
/// Retained tasks are never evicted: a task that has finished still holds its
/// ring until [`discard`](AsyncEventLog::discard) is called for it. Bounded per
/// task, unbounded in task count, which is the other reason a long-running
/// server wants the durable log.
///
/// [`InMemoryStreamingHandler`]: crate::adapter::InMemoryStreamingHandler
#[derive(Clone)]
pub struct InMemoryEventLog {
    tasks: Arc<Mutex<HashMap<String, TaskLog>>>,
    capacity: usize,
}

impl InMemoryEventLog {
    /// A log keeping [`DEFAULT_CAPACITY`] events per task.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// A log keeping `capacity` events per task.
    ///
    /// A capacity of zero would retain nothing and make every resume
    /// incomplete, so it is raised to one.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            capacity: capacity.max(1),
        }
    }
}

impl Default for InMemoryEventLog {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncEventLog for InMemoryEventLog {
    async fn append(&self, task_id: &str, event: UpdateEvent) -> Result<SeqEvent, A2AError> {
        let mut guard = self.tasks.lock().await;
        Ok(guard
            .entry(task_id.to_string())
            .or_insert_with(|| TaskLog::new(self.capacity))
            .append(event, self.capacity))
    }

    async fn replay(&self, task_id: &str, from: u64) -> Result<Replay, A2AError> {
        let guard = self.tasks.lock().await;
        let Some(log) = guard.get(task_id) else {
            return Ok(Replay::bounded_by(None, from, Vec::new()));
        };
        let oldest = log.events.front().map(|event| event.id);
        let events = log
            .events
            .iter()
            .filter(|event| event.id > from)
            .cloned()
            .collect();
        Ok(Replay::bounded_by(oldest, from, events))
    }

    async fn discard(&self, task_id: &str) -> Result<(), A2AError> {
        self.tasks.lock().await.remove(task_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{TaskState, TaskStatus, TaskStatusUpdateEvent};

    fn event(state: TaskState) -> UpdateEvent {
        UpdateEvent::StatusUpdate(TaskStatusUpdateEvent {
            task_id: "t".to_string(),
            context_id: "ctx".to_string(),
            kind: "status-update".to_string(),
            status: TaskStatus::new(state, None),
            metadata: None,
        })
    }

    #[tokio::test]
    async fn ids_start_at_one_and_run_per_task() {
        let log = InMemoryEventLog::new();
        assert_eq!(
            log.append("a", event(TaskState::Working)).await.unwrap().id,
            1
        );
        assert_eq!(
            log.append("a", event(TaskState::Working)).await.unwrap().id,
            2
        );
        assert_eq!(
            log.append("b", event(TaskState::Working)).await.unwrap().id,
            1,
            "a second task counts from its own start"
        );
    }

    #[tokio::test]
    async fn a_covered_gap_replays_only_its_tail() {
        let log = InMemoryEventLog::new();
        for _ in 0..5 {
            log.append("t", event(TaskState::Working)).await.unwrap();
        }

        let replay = log.replay("t", 3).await.unwrap();
        assert!(replay.complete);
        assert_eq!(
            replay.events.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![4, 5]
        );
    }

    /// Caught up is not the same as fell behind: a client holding the newest id
    /// gets nothing back, and that is a complete answer.
    #[tokio::test]
    async fn a_caught_up_client_gets_an_empty_complete_replay() {
        let log = InMemoryEventLog::new();
        for _ in 0..3 {
            log.append("t", event(TaskState::Working)).await.unwrap();
        }

        let replay = log.replay("t", 3).await.unwrap();
        assert!(replay.complete);
        assert!(replay.events.is_empty());
    }

    /// Past the ring's capacity the log holds a fragment of the gap, not its
    /// remainder, and says so.
    #[tokio::test]
    async fn falling_off_the_ring_is_an_incomplete_replay() {
        let log = InMemoryEventLog::with_capacity(4);
        for _ in 0..10 {
            log.append("t", event(TaskState::Working)).await.unwrap();
        }

        let replay = log.replay("t", 1).await.unwrap();
        assert!(
            !replay.complete,
            "events 2..6 are gone, so this is not the tail the client asked for"
        );
        assert_eq!(
            replay.events.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![7, 8, 9, 10]
        );

        let covered = log.replay("t", 6).await.unwrap();
        assert!(
            covered.complete,
            "id 7 is still held, so the gap is covered"
        );
    }

    /// What a restart looks like from the client's side: the ids it holds are
    /// from a log that no longer exists.
    #[tokio::test]
    async fn an_empty_log_covers_only_a_client_that_has_seen_nothing() {
        let log = InMemoryEventLog::new();
        assert!(log.replay("t", 0).await.unwrap().complete);
        assert!(!log.replay("t", 7).await.unwrap().complete);
    }

    #[tokio::test]
    async fn discarding_forgets_the_task() {
        let log = InMemoryEventLog::new();
        log.append("t", event(TaskState::Working)).await.unwrap();
        log.discard("t").await.unwrap();

        let replay = log.replay("t", 0).await.unwrap();
        assert!(replay.events.is_empty());
        assert_eq!(
            log.append("t", event(TaskState::Working)).await.unwrap().id,
            1,
            "a discarded task counts from the start again"
        );
    }
}
