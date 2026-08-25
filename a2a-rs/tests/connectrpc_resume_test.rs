//! ConnectRPC streaming resumption: `Last-Event-ID` in, per-event ids out.
//!
//! ConnectRPC has no SSE `id:` field, so the a2a-rs resumption enhancement
//! carries the id in the update event's `metadata` — and only for a client that
//! asked for it with the `a2a-rs-event-ids` header, since that is a change to
//! the payload rather than an inert protocol field. Two things need proving:
//! the wire is untouched for a client that did not ask, and our own client and
//! server agree over a real socket.

#![cfg(all(feature = "http-client", feature = "http-server"))]

mod common;

use std::time::Duration;

use common::TestBusinessHandler;
use futures::StreamExt;

use a2a_rs::adapter::{
    ConnectRpcAdapter, HttpClient, HttpServer, InMemoryTaskStorage, SimpleAgentInfo,
};
use a2a_rs::domain::generated::{A2aService, StreamResponse, SubscribeToTaskRequest};
use a2a_rs::domain::{Message, SendCompletion, TaskState, TaskStatus, TaskStatusUpdateEvent};
use a2a_rs::port::AsyncStreamingHandler;
use a2a_rs::{StreamItem, Transport};

const TASK: &str = "task-resume";

fn message() -> Message {
    Message::user_text("hello".to_string(), "m1".to_string())
}

fn status_update(state: TaskState) -> TaskStatusUpdateEvent {
    TaskStatusUpdateEvent {
        task_id: TASK.to_string(),
        context_id: "ctx".to_string(),
        kind: "status-update".to_string(),
        status: TaskStatus::new(state, None),
        metadata: None,
    }
}

fn adapter(handler: &TestBusinessHandler) -> ConnectRpcAdapter {
    let agent_info = SimpleAgentInfo::new("resume".to_string(), "http://localhost".to_string());
    ConnectRpcAdapter::with_handler(handler.clone(), agent_info)
        .with_streaming_handler(handler.clone())
}

/// Put a task on the wire and broadcast `Working` then `Completed` into its
/// stream, so a later subscribe has two buffered events to replay.
async fn task_with_two_buffered_events(client: &HttpClient, handler: &TestBusinessHandler) {
    client
        .send_task_message(
            Some(TASK),
            &message(),
            None,
            None,
            SendCompletion::WhenSettled,
        )
        .await
        .unwrap();
    handler
        .broadcast_status_update(TASK, status_update(TaskState::Working))
        .await
        .unwrap();
    handler
        .broadcast_status_update(TASK, status_update(TaskState::Completed))
        .await
        .unwrap();
}

/// The event id a stamped `StreamResponse` carries, if any.
fn stamped_id(response: &StreamResponse) -> Option<String> {
    let json = serde_json::to_value(response).ok()?;
    let update = json
        .get("statusUpdate")
        .or_else(|| json.get("status_update"))?;
    Some(
        update
            .get("metadata")?
            .get("a2a_rs_event_id")?
            .as_str()?
            .to_string(),
    )
}

/// A client that does not ask for event ids gets the bytes the spec describes.
/// The key is namespaced, but it still lands in the agent's own metadata, so it
/// must not appear uninvited.
#[tokio::test]
async fn a_client_that_does_not_ask_gets_unstamped_events() {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let adapter = adapter(&handler);

    handler
        .broadcast_status_update(TASK, status_update(TaskState::Working))
        .await
        .unwrap();

    let request = SubscribeToTaskRequest {
        id: TASK.to_string(),
        ..Default::default()
    };
    let mut headers = http::HeaderMap::new();
    headers.insert("last-event-id", http::HeaderValue::from_static("0"));

    let (mut stream, _) = adapter
        .subscribe_to_task(
            connectrpc::Context::new(headers),
            buffa::view::OwnedView::from_owned(&request).unwrap(),
        )
        .await
        .unwrap();

    let response = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("an event within 5s")
        .expect("stream not empty")
        .expect("ok event");
    assert_eq!(
        stamped_id(&response),
        None,
        "no `a2a-rs-event-ids` header was sent, so nothing may be stamped"
    );
}

/// With the header, every update carries its id — which is what makes resuming
/// possible at all, since the id has to be known *before* the disconnect.
#[tokio::test]
async fn a_client_that_asks_gets_an_id_on_every_update() {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let adapter = adapter(&handler);

    handler
        .broadcast_status_update(TASK, status_update(TaskState::Working))
        .await
        .unwrap();

    let request = SubscribeToTaskRequest {
        id: TASK.to_string(),
        ..Default::default()
    };
    let mut headers = http::HeaderMap::new();
    headers.insert("last-event-id", http::HeaderValue::from_static("0"));
    headers.insert("a2a-rs-event-ids", http::HeaderValue::from_static("1"));

    let (mut stream, _) = adapter
        .subscribe_to_task(
            connectrpc::Context::new(headers),
            buffa::view::OwnedView::from_owned(&request).unwrap(),
        )
        .await
        .unwrap();

    let response = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("an event within 5s")
        .expect("stream not empty")
        .expect("ok event");
    assert_eq!(
        stamped_id(&response).as_deref(),
        Some("1"),
        "the first buffered update is event 1"
    );
}

/// End-to-end over a real socket: the client records the ids the server stamps,
/// reconnects with `Last-Event-ID`, and gets only the tail it missed. This is
/// what `RetryingTransport` does for a caller after a dropped connection.
#[tokio::test]
async fn subscribe_resumes_from_last_event_id() {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let agent_info = SimpleAgentInfo::new("resume".to_string(), "http://localhost".to_string());
    let server = HttpServer::new(adapter(&handler), agent_info, String::new());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { server.serve_on(listener).await.unwrap() });

    let client = HttpClient::new(base);
    task_with_two_buffered_events(&client, &handler).await;

    // A first subscription replays everything (`id > 0`). The message handler
    // may emit events of its own, so the id of `Completed` is discovered rather
    // than assumed.
    let mut all = client
        .subscribe_to_task(TASK, None, Some("0"))
        .await
        .unwrap();
    let mut completed_id = None;
    for _ in 0..16 {
        match tokio::time::timeout(Duration::from_secs(2), all.next()).await {
            Ok(Some(Ok(event))) => {
                if let StreamItem::StatusUpdate(update) = &event.item
                    && update.status.state == ::buffa::EnumValue::from(TaskState::Completed)
                {
                    completed_id = event.event_id;
                    break;
                }
            }
            _ => break,
        }
    }
    let completed_id = completed_id.expect("the Completed event should arrive with an id");
    drop(all);

    let mut stream = client
        .subscribe_to_task(TASK, None, Some(&(completed_id - 1).to_string()))
        .await
        .unwrap();

    let mut got = Vec::new();
    while got.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("an event within 5s")
            .expect("stream not empty")
            .expect("ok event");
        got.push(event);
    }

    assert!(
        matches!(got[0].item, StreamItem::Task(_)),
        "the snapshot comes first"
    );
    assert_eq!(
        got[0].event_id, None,
        "the snapshot has no id to resume from"
    );
    assert_eq!(
        got[1].event_id,
        Some(completed_id),
        "only the events after Last-Event-ID replay"
    );
    match &got[1].item {
        StreamItem::StatusUpdate(update) => {
            assert_eq!(
                update.status.state,
                ::buffa::EnumValue::from(TaskState::Completed)
            );
            assert_eq!(
                update.metadata, None,
                "the stamped id is stripped, leaving the metadata the agent sent"
            );
        }
        other => panic!("expected a StatusUpdate, got {other:?}"),
    }
}
