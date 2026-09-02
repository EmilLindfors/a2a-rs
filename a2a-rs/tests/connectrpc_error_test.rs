//! A refusal over ConnectRPC arrives as the refusal the server made.
//!
//! Two silences this pins, both found from korps (issues #71 and #72). A
//! server that refused a subscription outright — no streaming backend, or a
//! task already settled — reached the client as an empty stream, because the
//! refusal travels in the END_STREAM envelope behind an ordinary end-of-stream.
//! And a spec error code did not survive the transport at all: Connect has no
//! code for "this agent does not do push", so the refusal was sent as
//! `Internal` and decoded as a server fault. Both are observed here over a real
//! socket, since the client library between the two ends is where the second
//! one hid.

#![cfg(all(feature = "http-client", feature = "http-server"))]

mod common;

use std::time::Duration;

use async_trait::async_trait;
use common::TestBusinessHandler;
use futures::StreamExt;

use a2a_rs::Transport;
use a2a_rs::adapter::{ConnectRpcAdapter, HttpClient, HttpServer, SimpleAgentInfo};
use a2a_rs::domain::{
    A2AError, DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigsParams, Message, SendCompletion, TaskPushNotificationConfig,
};
use a2a_rs::port::AsyncNotificationManager;

/// An agent that does not do push, the way korps' `[features]
/// push_notifications = false` does not.
#[derive(Clone)]
struct RefusesPush;

#[async_trait]
impl AsyncNotificationManager for RefusesPush {
    async fn set_config(
        &self,
        _config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        Err(A2AError::PushNotificationNotSupported)
    }

    async fn get_config(
        &self,
        _params: &GetTaskPushNotificationConfigParams,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        Err(A2AError::PushNotificationNotSupported)
    }

    async fn list_configs(
        &self,
        _params: &ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        Err(A2AError::PushNotificationNotSupported)
    }

    async fn delete_config(
        &self,
        _params: &DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        Err(A2AError::PushNotificationNotSupported)
    }
}

/// Serve `adapter` on a free port and return a client pointed at it.
async fn serve(adapter: ConnectRpcAdapter) -> HttpClient {
    let agent_info = SimpleAgentInfo::new("refusing".to_string(), "http://localhost".to_string());
    let server = HttpServer::new(adapter, agent_info, String::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { server.serve_on(listener).await.unwrap() });
    HttpClient::new(base)
}

/// The first thing a subscription says, or what it refuses to say.
async fn first(
    client: &HttpClient,
    task_id: &str,
) -> Result<Option<Result<a2a_rs::port::StreamEvent, A2AError>>, A2AError> {
    let mut stream = client.subscribe_to_task(task_id, None, None).await?;
    Ok(tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("the stream should answer within 5s"))
}

/// The adapter's default streaming port refuses every subscription. That
/// refusal must be visible: an empty stream says "the peer said nothing", and
/// the truth is "the peer said no".
#[tokio::test]
async fn a_server_with_no_streaming_backend_refuses_visibly() {
    let handler = TestBusinessHandler::new();
    let agent_info = SimpleAgentInfo::new("noop".to_string(), "http://localhost".to_string());
    let client = serve(ConnectRpcAdapter::with_handler(handler, agent_info)).await;

    let refusal = match first(&client, "task-1").await {
        Err(e) => e,
        Ok(Some(Err(e))) => e,
        Ok(Some(Ok(event))) => panic!("expected a refusal, got an event: {event:?}"),
        Ok(None) => panic!("the refusal was swallowed into an empty stream"),
    };
    assert!(
        matches!(refusal, A2AError::UnsupportedOperation(_)),
        "the refusal should arrive as the variant the server made, got {refusal:?}"
    );
}

/// A subscribe on a task that has already settled is the other pre-stream
/// refusal (`a2a.proto` specifies `UnsupportedOperationError`). It reached the
/// JSON-RPC client and vanished on this one.
#[tokio::test]
async fn a_subscribe_on_a_settled_task_refuses_visibly() {
    let handler = TestBusinessHandler::new();
    let agent_info = SimpleAgentInfo::new("settled".to_string(), "http://localhost".to_string());
    let adapter = ConnectRpcAdapter::with_handler(handler.clone(), agent_info)
        .with_streaming_handler(handler.clone());
    let client = serve(adapter).await;

    let message = Message::user_text("hello".to_string(), "m1".to_string());
    client
        .send_task_message(
            Some("task-settled"),
            &message,
            None,
            None,
            SendCompletion::WhenSettled,
        )
        .await
        .unwrap();

    let refusal = match first(&client, "task-settled").await {
        Err(e) => e,
        Ok(Some(Err(e))) => e,
        Ok(Some(Ok(event))) => panic!("expected a refusal, got an event: {event:?}"),
        Ok(None) => panic!("the refusal was swallowed into an empty stream"),
    };
    assert!(
        matches!(refusal, A2AError::UnsupportedOperation(_)),
        "got {refusal:?}"
    );
}

/// The stream ends after the refusal — a refused subscription is one error,
/// not an error per poll.
#[tokio::test]
async fn a_refused_subscription_ends_after_saying_so() {
    let handler = TestBusinessHandler::new();
    let agent_info = SimpleAgentInfo::new("noop".to_string(), "http://localhost".to_string());
    let client = serve(ConnectRpcAdapter::with_handler(handler, agent_info)).await;

    let Ok(mut stream) = client.subscribe_to_task("task-1", None, None).await else {
        // Refused at the call: nothing further to check.
        return;
    };
    let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap();
    assert!(matches!(first, Some(Err(_))), "got {first:?}");
    let second = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap();
    assert!(second.is_none(), "the refusal repeated: {second:?}");
}

/// A push refusal is a spec-defined answer (`-32003`), not a server fault
/// (`-32603`). It has no Connect code of its own, which is why it used to be
/// sent as `Internal`; the A2A code now travels in the error detail.
#[tokio::test]
async fn a_push_refusal_is_the_refusal_the_server_made() {
    let handler = TestBusinessHandler::new();
    let agent_info = SimpleAgentInfo::new("no-push".to_string(), "http://localhost".to_string());
    let adapter = ConnectRpcAdapter::new(handler.clone(), handler, RefusesPush, agent_info);
    let client = serve(adapter).await;

    let config = TaskPushNotificationConfig {
        task_id: "task-1".to_string(),
        id: "cfg-1".to_string(),
        url: "https://example.invalid/hook".to_string(),
        ..Default::default()
    };
    let refusal = client
        .set_task_push_notification(&config)
        .await
        .expect_err("the agent does not do push");

    assert!(
        matches!(refusal, A2AError::PushNotificationNotSupported),
        "got {refusal:?}"
    );
}
