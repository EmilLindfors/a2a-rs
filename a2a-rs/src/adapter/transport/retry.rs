//! Resilient streaming over the [`Transport`] port: reconnect with exponential
//! backoff and resume via `Last-Event-ID`.
//!
//! This is the one place backoff lives. [`subscribe_resilient`] is the reusable
//! core — a free function that owns an `Arc<dyn Transport>` so the stream it
//! returns is `'static` and can re-subscribe after a disconnect, threading the
//! last observed event id back as `Last-Event-ID` so the server replays the gap.
//! [`RetryingTransport`] is a thin decorator that *is* a [`Transport`]: it passes
//! unary calls straight through and only wraps `subscribe_to_task`, so wrapping a
//! negotiated transport at the composition edge makes every existing call site
//! resilient with no signature change.
//!
//! # Spec note (A2A v1.0): this is an opt-in enhancement, not a spec feature
//!
//! The A2A protocol defines reconnection by re-issuing the subscribe call
//! (`SubscribeToTask`), which re-attaches from the task's *current* state; it
//! does **not** define `Last-Event-ID` gap-replay, and `SubscribeToTaskRequest`
//! has no resume field. The gap-free resumption here is an a2a-rs enhancement
//! built on the **W3C SSE-standard** `id:` field and `Last-Event-ID` header:
//!
//! - **Interop is preserved.** Against a spec-compliant server that ignores
//!   `Last-Event-ID`, [`subscribe_resilient`] still reconnects via the spec's
//!   subscribe call and resumes from current state — it simply can't replay the
//!   gap. Against our own server it replays the missed tail.
//! - **It is not cross-SDK guaranteed.** Only an a2a-rs server honors our
//!   `Last-Event-ID`; do not assume gap-free resume against third-party agents.
//!
//! For a strictly spec-shaped single subscribe (no reconnection, no
//! `Last-Event-ID`), call [`Transport::subscribe_to_task`] directly with
//! `last_event_id = None` instead of using this module.

use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::{Stream, StreamExt};

use crate::domain::core::task::TaskStateExt;
use crate::domain::{
    A2AError, ListTasksParams, ListTasksResult, Message, RetryPolicy, Task,
    TaskPushNotificationConfig,
};
use crate::port::{StreamEvent, StreamItem, Transport};

type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>;

/// Subscribe to a task's updates with automatic reconnect + backoff.
///
/// The returned stream forwards [`StreamEvent`]s until the task reaches a
/// terminal state, recording each event id along the way. On a disconnect (the
/// inner stream errors or ends without a terminal status) it sleeps per `policy`
/// and re-subscribes, passing the last seen id as `Last-Event-ID` so a resumable
/// server replays the missed tail. After `policy.max_retries` consecutive failed
/// reconnects it yields a final error and ends.
pub fn subscribe_resilient(
    transport: Arc<dyn Transport>,
    task_id: impl Into<String>,
    history_length: Option<u32>,
    last_event_id: Option<u64>,
    policy: RetryPolicy,
) -> EventStream {
    let task_id = task_id.into();
    let seed = seed_for(&task_id);

    struct State {
        transport: Arc<dyn Transport>,
        task_id: String,
        history_length: Option<u32>,
        policy: RetryPolicy,
        seed: u64,
        last_event_id: Option<u64>,
        attempt: u32,
        inner: Option<EventStream>,
        done: bool,
    }

    let state = State {
        transport,
        task_id,
        history_length,
        policy,
        seed,
        last_event_id,
        attempt: 0,
        inner: None,
        done: false,
    };

    Box::pin(futures::stream::unfold(state, |mut st| async move {
        loop {
            if st.done {
                return None;
            }

            // (Re)connect when we have no live inner stream.
            if st.inner.is_none() {
                if st.attempt > st.policy.max_retries {
                    st.done = true;
                    return Some((
                        Err(A2AError::Internal(format!(
                            "subscription to '{}' failed after {} retries",
                            st.task_id, st.policy.max_retries
                        ))),
                        st,
                    ));
                }
                if st.attempt > 0 {
                    let delay = st.policy.backoff(st.attempt, st.seed);
                    tokio::time::sleep(delay).await;
                }
                let resume = st.last_event_id.map(|n| n.to_string());
                match st
                    .transport
                    .subscribe_to_task(&st.task_id, st.history_length, resume.as_deref())
                    .await
                {
                    Ok(stream) => st.inner = Some(stream),
                    Err(_) => {
                        st.attempt += 1;
                        continue;
                    }
                }
            }

            // Pull the next event from the live inner stream.
            match st.inner.as_mut().unwrap().next().await {
                Some(Ok(event)) => {
                    // Any progress resets the backoff counter.
                    st.attempt = 0;
                    if let Some(id) = event.event_id {
                        st.last_event_id = Some(id);
                    }
                    if is_terminal(&event.item) {
                        st.done = true;
                    }
                    return Some((Ok(event), st));
                }
                // Stream errored or ended without a terminal status: reconnect.
                Some(Err(_)) | None => {
                    st.inner = None;
                    st.attempt += 1;
                    continue;
                }
            }
        }
    }))
}

/// Whether a stream item represents a terminal task state (ends the stream).
fn is_terminal(item: &StreamItem) -> bool {
    match item {
        StreamItem::Task(task) => task
            .status
            .as_option()
            .map(|s| s.state.is_terminal())
            .unwrap_or(false),
        StreamItem::StatusUpdate(event) => event.status.state.is_terminal(),
        StreamItem::ArtifactUpdate(_) => false,
    }
}

/// Derive a per-task jitter seed from the task id and a coarse time sample. The
/// time sample (the only impure input) lives here in the adapter, keeping
/// [`RetryPolicy::backoff`] pure.
fn seed_for(task_id: &str) -> u64 {
    let mut state = 0u64;
    for &b in task_id.as_bytes() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(b as u64);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    state.wrapping_mul(6364136223846793005).wrapping_add(now)
}

/// A [`Transport`] decorator that adds reconnect + backoff to `subscribe_to_task`
/// and passes every unary method straight through to the inner transport.
///
/// Wrap a negotiated transport once at the composition edge —
/// `RetryingTransport::wrap(connect(...).await?, policy)` — and all callers gain
/// resilient streaming transparently.
pub struct RetryingTransport {
    inner: Arc<dyn Transport>,
    policy: RetryPolicy,
}

impl RetryingTransport {
    /// Decorate a shared transport with a retry policy.
    pub fn new(inner: Arc<dyn Transport>, policy: RetryPolicy) -> Self {
        Self { inner, policy }
    }

    /// Decorate an owned (e.g. negotiated `Box<dyn Transport>`) transport.
    pub fn wrap(inner: Box<dyn Transport>, policy: RetryPolicy) -> Self {
        Self {
            inner: Arc::from(inner),
            policy,
        }
    }
}

#[async_trait]
impl Transport for RetryingTransport {
    fn protocol(&self) -> &str {
        self.inner.protocol()
    }

    async fn send_task_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        self.inner
            .send_task_message(task_id, message, session_id, history_length)
            .await
    }

    async fn get_task(&self, task_id: &str, history_length: Option<u32>) -> Result<Task, A2AError> {
        self.inner.get_task(task_id, history_length).await
    }

    async fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError> {
        self.inner.cancel_task(task_id).await
    }

    async fn set_task_push_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.inner.set_task_push_notification(config).await
    }

    async fn get_task_push_notification(
        &self,
        task_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.inner.get_task_push_notification(task_id).await
    }

    async fn list_tasks(&self, params: &ListTasksParams) -> Result<ListTasksResult, A2AError> {
        self.inner.list_tasks(params).await
    }

    async fn list_push_notification_configs(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        self.inner.list_push_notification_configs(task_id).await
    }

    async fn get_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        self.inner
            .get_push_notification_config(task_id, config_id)
            .await
    }

    async fn delete_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<(), A2AError> {
        self.inner
            .delete_push_notification_config(task_id, config_id)
            .await
    }

    async fn subscribe_to_task(
        &self,
        task_id: &str,
        history_length: Option<u32>,
        last_event_id: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>, A2AError> {
        let resume = last_event_id.and_then(|s| s.trim().parse::<u64>().ok());
        Ok(subscribe_resilient(
            self.inner.clone(),
            task_id.to_string(),
            history_length,
            resume,
            self.policy,
        ))
    }
}
