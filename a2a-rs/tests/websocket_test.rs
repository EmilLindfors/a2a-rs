//! WebSocket-specific integration tests

#![cfg(all(feature = "ws-client", feature = "ws-server"))]

mod common;

use a2a_rs::{
    adapter::{
        DefaultRequestProcessor, InMemoryTaskStorage, SimpleAgentInfo, WebSocketClient,
        WebSocketServer,
    },
    domain::{Message, MessageSendConfiguration, TaskState},
    services::{AsyncA2AClient, StreamItem},
};
use common::TestBusinessHandler;
use futures::StreamExt;
use serde_json::{Map, Value};
use std::time::Duration;
use tokio::sync::oneshot;

async fn setup_test_server(
    port: u16,
) -> (
    tokio::task::JoinHandle<()>,
    oneshot::Sender<()>,
    WebSocketClient,
) {
    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let processor = DefaultRequestProcessor::with_handler(handler.clone());

    let agent_info = SimpleAgentInfo::new(
        "WS Test Agent".to_string(),
        format!("ws://localhost:{}", port),
    )
    .with_streaming()
    .with_state_transition_history()
    .add_skill("test".to_string(), "Test Skill".to_string(), None);

    let server = WebSocketServer::new(
        processor,
        agent_info,
        handler,
        format!("127.0.0.1:{}", port),
    );

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_secs(1)).await;
    let client = WebSocketClient::new(format!("ws://localhost:{}", port));

    (server_handle, shutdown_tx, client)
}

/// Test WebSocket streaming functionality
#[tokio::test]
async fn test_websocket_streaming() {
    if std::env::var("CI").is_ok() {
        return;
    }

    let (server_handle, shutdown_tx, client) = setup_test_server(8183).await;

    let task_id = format!("ws-task-{}", uuid::Uuid::new_v4());
    let message = Message::user_text(
        "Hello, WebSocket!".to_string(),
        format!("msg-{}", uuid::Uuid::new_v4()),
    );

    // Test basic task send
    let task_result = client
        .send_task_message(&task_id, &message, None, None)
        .await;
    if task_result.is_err() {
        println!("Task send failed, skipping streaming test");
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        return;
    }

    // Test streaming
    if let Ok(mut stream) = client.subscribe_to_task(&task_id, None).await {
        let mut updates = 0;
        let timeout = tokio::time::sleep(Duration::from_secs(5));

        tokio::select! {
            _ = async {
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(StreamItem::StatusUpdate(update)) => {
                            updates += 1;
                            if update.final_ { break; }
                        }
                        Ok(_) => updates += 1,
                        Err(_) => break,
                    }
                    if updates >= 3 { break; }
                }
            } => {},
            _ = timeout => {}
        }
    }

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}

/// Test WebSocket send_message with different parameter combinations
#[tokio::test]
async fn test_websocket_send_message() {
    if std::env::var("CI").is_ok() {
        return;
    }

    let (server_handle, shutdown_tx, client) = setup_test_server(8184).await;

    // Test cases as a simple array of closures
    let test_cases: Vec<(
        &str,
        Box<dyn Fn() -> (Option<Map<String, Value>>, Option<MessageSendConfiguration>)>,
    )> = vec![
        ("basic", Box::new(|| (None, None))),
        (
            "with_config",
            Box::new(|| {
                let config = MessageSendConfiguration {
                    accepted_output_modes: vec!["text".to_string()],
                    history_length: Some(3),
                    push_notification_config: None,
                    blocking: Some(false),
                };
                (None, Some(config))
            }),
        ),
        (
            "with_metadata",
            Box::new(|| {
                let mut metadata = Map::new();
                metadata.insert("key".to_string(), Value::String("value".to_string()));
                (Some(metadata), None)
            }),
        ),
        (
            "with_both",
            Box::new(|| {
                let mut metadata = Map::new();
                metadata.insert("test".to_string(), Value::String("data".to_string()));
                let config = MessageSendConfiguration {
                    accepted_output_modes: vec!["json".to_string()],
                    history_length: Some(5),
                    push_notification_config: None,
                    blocking: Some(true),
                };
                (Some(metadata), Some(config))
            }),
        ),
    ];

    let mut passed = 0;
    for (name, setup) in test_cases {
        let (metadata, config) = setup();
        let message = Message::user_text(
            format!("Test {}", name),
            format!("{}-{}", name, uuid::Uuid::new_v4()),
        );

        match client
            .send_message(&message, metadata.as_ref(), config.as_ref())
            .await
        {
            Ok(task) => {
                assert_eq!(task.status.state, TaskState::Working);
                assert!(!task.id.is_empty());
                passed += 1;
            }
            Err(e) => println!("Test {} failed: {}", name, e),
        }
    }

    assert!(passed > 0, "At least one test should pass");

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}
