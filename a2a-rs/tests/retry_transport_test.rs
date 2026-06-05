//! Unit tests for the resilient streaming core (`subscribe_resilient` /
//! `RetryingTransport`) against a scripted fake [`Transport`].

#![cfg(feature = "client")]

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use futures::{Stream, StreamExt};

use a2a_rs::domain::{
    A2AError, ListTasksParams, ListTasksResult, Message, RetryPolicy, Task,
    TaskPushNotificationConfig, TaskState, TaskStatus, TaskStatusUpdateEvent,
};
use a2a_rs::port::{StreamEvent, StreamItem, Transport};
use a2a_rs::subscribe_resilient;

type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>;
/// One successful subscribe yields these events; a `None` script entry makes the
/// subscribe call itself fail (a connection error).
type Script = Vec<Result<StreamEvent, A2AError>>;

#[derive(Default)]
struct FakeInner {
    scripts: Mutex<VecDeque<Option<Script>>>,
    calls: Mutex<u32>,
    seen_resume: Mutex<Vec<Option<String>>>,
}

#[derive(Clone, Default)]
struct FakeTransport {
    inner: Arc<FakeInner>,
}

impl FakeTransport {
    fn with_scripts(scripts: Vec<Option<Script>>) -> Self {
        Self {
            inner: Arc::new(FakeInner {
                scripts: Mutex::new(scripts.into()),
                ..Default::default()
            }),
        }
    }
    fn calls(&self) -> u32 {
        *self.inner.calls.lock().unwrap()
    }
    fn seen_resume(&self) -> Vec<Option<String>> {
        self.inner.seen_resume.lock().unwrap().clone()
    }
}

fn fast_policy(max_retries: u32) -> RetryPolicy {
    RetryPolicy {
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(2),
        max_retries,
        jitter_ms: 0,
    }
}

fn working(id: u64) -> StreamEvent {
    StreamEvent::new(
        Some(id),
        StreamItem::StatusUpdate(TaskStatusUpdateEvent {
            task_id: "t".to_string(),
            context_id: "c".to_string(),
            kind: "status-update".to_string(),
            status: TaskStatus::new(TaskState::Working, None),
            metadata: None,
        }),
    )
}

fn terminal_task() -> StreamEvent {
    let task = Task::builder()
        .id("t".to_string())
        .status(TaskStatus::new(TaskState::Completed, None))
        .build();
    StreamEvent::untagged(StreamItem::Task(task))
}

#[async_trait]
impl Transport for FakeTransport {
    fn protocol(&self) -> &str {
        "FAKE"
    }
    async fn send_task_message(
        &self,
        _: &str,
        _: &Message,
        _: Option<&str>,
        _: Option<u32>,
    ) -> Result<Task, A2AError> {
        unimplemented!()
    }
    async fn get_task(&self, _: &str, _: Option<u32>) -> Result<Task, A2AError> {
        unimplemented!()
    }
    async fn cancel_task(&self, _: &str) -> Result<Task, A2AError> {
        unimplemented!()
    }
    async fn set_task_push_notification(
        &self,
        _: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        unimplemented!()
    }
    async fn get_task_push_notification(
        &self,
        _: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        unimplemented!()
    }
    async fn list_tasks(&self, _: &ListTasksParams) -> Result<ListTasksResult, A2AError> {
        unimplemented!()
    }
    async fn list_push_notification_configs(
        &self,
        _: &str,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        unimplemented!()
    }
    async fn get_push_notification_config(
        &self,
        _: &str,
        _: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        unimplemented!()
    }
    async fn delete_push_notification_config(&self, _: &str, _: &str) -> Result<(), A2AError> {
        unimplemented!()
    }
    async fn subscribe_to_task(
        &self,
        _task_id: &str,
        _history_length: Option<u32>,
        last_event_id: Option<&str>,
    ) -> Result<EventStream, A2AError> {
        *self.inner.calls.lock().unwrap() += 1;
        self.inner
            .seen_resume
            .lock()
            .unwrap()
            .push(last_event_id.map(|s| s.to_string()));
        match self.inner.scripts.lock().unwrap().pop_front() {
            Some(Some(script)) => Ok(Box::pin(futures::stream::iter(script))),
            _ => Err(A2AError::Internal("connect failed".to_string())),
        }
    }
}

/// The core retries failed connects with backoff, then forwards events from the
/// first successful connect and ends on the terminal task.
#[tokio::test]
async fn retries_then_succeeds() {
    let fake = FakeTransport::with_scripts(vec![None, None, Some(vec![Ok(terminal_task())])]);
    let mut stream = subscribe_resilient(Arc::new(fake.clone()), "t", None, None, fast_policy(5));

    let items: Vec<_> = collect(&mut stream).await;
    assert_eq!(items.len(), 1, "one terminal event");
    assert!(matches!(items[0], StreamItem::Task(_)));
    assert_eq!(fake.calls(), 3, "two failures + one success");
}

/// After `max_retries` consecutive failed connects the stream yields a final
/// error and ends.
#[tokio::test]
async fn gives_up_after_max_retries() {
    let fake = FakeTransport::with_scripts(vec![None, None, None, None, None]);
    let mut stream = subscribe_resilient(Arc::new(fake.clone()), "t", None, None, fast_policy(2));

    let mut last = None;
    while let Some(item) = stream.next().await {
        last = Some(item);
    }
    assert!(matches!(last, Some(Err(_))), "ends with an error item");
    // attempt 0 (initial) + retries 1,2 = 3 connects, then attempt 3 > 2 gives up.
    assert_eq!(fake.calls(), 3);
}

/// On reconnect the core echoes the last observed event id as `Last-Event-ID`.
#[tokio::test]
async fn reconnect_threads_last_event_id() {
    let fake = FakeTransport::with_scripts(vec![
        Some(vec![Ok(working(5))]), // first connect: one event id 5, then ends
        Some(vec![Ok(terminal_task())]), // reconnect: terminal
    ]);
    let mut stream = subscribe_resilient(Arc::new(fake.clone()), "t", None, None, fast_policy(5));

    let items = collect(&mut stream).await;
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0], StreamItem::StatusUpdate(_)));
    assert!(matches!(items[1], StreamItem::Task(_)));
    // First connect carries no resume id; the reconnect echoes id 5.
    assert_eq!(fake.seen_resume(), vec![None, Some("5".to_string())]);
}

async fn collect(stream: &mut EventStream) -> Vec<StreamItem> {
    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(ev) => out.push(ev.item),
            Err(_) => break,
        }
    }
    out
}
