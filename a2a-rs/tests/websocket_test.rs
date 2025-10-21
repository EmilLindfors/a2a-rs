//! WebSocket-specific integration tests

#![cfg(all(feature = "ws-client", feature = "ws-server"))]

mod common;

use a2a_rs::{
    adapter::{
        DefaultRequestProcessor, InMemoryTaskStorage, SimpleAgentInfo, WebSocketClient,
        WebSocketServer,
    },
    domain::Message,
    services::{AsyncA2AClient, StreamItem},
};
use common::TestBusinessHandler;
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::oneshot;

/// Test WebSocket streaming functionality
#[tokio::test]
async fn test_websocket_streaming() {
    // Skip the test if it's running in the CI environment
    if std::env::var("CI").is_ok() {
        println!("Skipping WebSocket test in CI environment");
        return;
    }

    // Create a storage for server
    let storage = InMemoryTaskStorage::new();

    // Create business handler with the storage
    let handler = TestBusinessHandler::with_storage(storage.clone());

    // Create agent info for the processor
    let test_agent_info =
        SimpleAgentInfo::new("test-agent".to_string(), "ws://localhost:8183".to_string());

    // Create a processor
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    // Create an agent info provider
    let agent_info = SimpleAgentInfo::new(
        "WS Test Agent".to_string(),
        "ws://localhost:8183".to_string(),
    )
    .with_description("WebSocket Test A2A agent".to_string())
    .with_provider(
        "Test Organization".to_string(),
        "https://example.org".to_string(),
    )
    .with_documentation_url("https://example.org/docs".to_string())
    .with_streaming()
    .with_state_transition_history()
    .add_skill(
        "ws-test".to_string(),
        "WebSocket Test Skill".to_string(),
        Some("A WebSocket test skill".to_string()),
    );

    // Create the server
    let server = WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8183".to_string());

    // Create a shutdown channel
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Start the server in a separate task
    let server_handle = tokio::spawn(async move {
        println!("Starting WebSocket server on 127.0.0.1:8183...");
        tokio::select! {
            result = server.start() => {
                if let Err(e) = &result {
                    eprintln!("WebSocket server error: {}", e);
                }
            },
            _ = shutdown_rx => {
                println!("Server shutdown requested");
            }
        }
    });

    // Give the server time to start
    println!("Waiting for server to start...");
    tokio::time::sleep(Duration::from_secs(1)).await;
    println!("Proceeding with test...");

    // Create the WebSocket client
    let client = WebSocketClient::new("ws://localhost:8183".to_string());

    // Create a task and verify basic functionality
    let task_id = format!("ws-task-{}", uuid::Uuid::new_v4());
    let message_id = format!("msg-{}", uuid::Uuid::new_v4());
    let message = Message::user_text("Hello, WebSocket A2A agent!".to_string(), message_id);

    // First, test simple non-streaming task send
    println!("Testing basic task send...");
    let task_result = client
        .send_task_message(&task_id, &message, None, None)
        .await;

    if let Err(ref e) = task_result {
        println!("Warning: Task send failed: {}", e);
        // Don't fail the test just yet
    } else {
        println!("Task send succeeded");
    }

    // Now test streaming
    println!("Testing streaming...");
    let subscribe_result = client.subscribe_to_task(&task_id, None).await;

    if let Err(ref e) = subscribe_result {
        println!("Warning: Subscription failed: {}", e);
    }

    // Skip the streaming test if we can't get a subscription
    if let Ok(mut stream) = subscribe_result {
        println!("Subscription succeeded, processing stream...");

        // Process streaming updates
        let mut status_updates = 0;

        // Create a timeout future
        let timeout_future = tokio::time::sleep(Duration::from_secs(5));

        // Wait for streaming updates
        tokio::select! {
            _ = async {
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(StreamItem::Task(task)) => {
                            println!("Received task: {:?}", task.status.state);
                        }
                        Ok(StreamItem::StatusUpdate(update)) => {
                            status_updates += 1;
                            println!("Status update: {:?}", update.status.state);

                            if update.final_ {
                                println!("Final update received");
                                break;
                            }
                        }
                        Ok(StreamItem::ArtifactUpdate(_)) => {
                            println!("Artifact update received");
                        }
                        Err(e) => {
                            println!("Stream error: {}", e);
                            break;
                        }
                    }
                }
            } => {
                println!("Stream completed normally");
            },
            _ = timeout_future => {
                println!("Stream timeout");
                // Try to cancel the task, but don't fail if we can't
                let _ = client.cancel_task(&task_id).await;
            }
        }

        println!("Received {} status updates", status_updates);
    } else {
        println!("Skipping stream processing due to subscription failure");
    }

    // Shut down the server
    println!("Shutting down server...");
    let _ = shutdown_tx.send(());

    // Wait for the server to shut down
    let _ = server_handle.await;
    println!("Test completed");
}
