//! WebSocket-specific integration tests

use a2a_rs::{
    adapter::client::WebSocketClient,
    adapter::server::{
        DefaultRequestProcessor, InMemoryTaskStorage, SimpleAgentInfo, WebSocketServer,
    },
    domain::Message,
    port::client::{AsyncA2AClient, StreamItem},
};
use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::oneshot;

/// Test WebSocket streaming functionality
#[tokio::test]
async fn test_websocket_streaming() {
    // Create a storage for server
    let storage = InMemoryTaskStorage::new();

    // Create a processor
    let processor = DefaultRequestProcessor::new(storage.clone());

    // Create an agent info provider
    let agent_info = SimpleAgentInfo::new(
        "WS Test Agent".to_string(),
        "ws://localhost:8183".to_string(),
    )
    .with_description("WebSocket Test A2A agent".to_string())
    .with_provider(
        "Test Organization".to_string(),
        Some("https://example.org".to_string()),
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
    let server = WebSocketServer::new(processor, agent_info, storage, "127.0.0.1:8183".to_string());

    // Create a shutdown channel
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let (ready_tx, ready_rx) = oneshot::channel::<()>();

    // Start the server in a separate task
    let server_handle = tokio::spawn(async move {
        println!("Starting WebSocket server...");
        let server_result = tokio::select! {
            result = server.start() => {
                if let Err(e) = &result {
                    eprintln!("WebSocket server error: {}", e);
                }
                result
            },
            _ = shutdown_rx => {
                println!("Server shutdown requested");
                Ok(())
            }
        };
        
        if let Err(e) = server_result {
            eprintln!("Server exited with error: {}", e);
        }
    });
    
    // Let the server know we're ready to start
    let _ = ready_tx.send(());

    // Give the server time to start
    println!("Waiting for server to start...");
    tokio::time::sleep(Duration::from_secs(1)).await;
    println!("Proceeding with test...");

    // Create the client
    let client = WebSocketClient::new("ws://localhost:8183".to_string());

    // Test 1: Get agent card using HTTP client
    let http_client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build HTTP client");
    
    let response = http_client
        .get("http://localhost:8183/agent-card")
        .header("Connection", "keep-alive")
        .send()
        .await
        .expect("Failed to fetch agent card");

    let agent_card: Value = response.json().await.expect("Failed to parse agent card");
    assert_eq!(agent_card["name"].as_str().unwrap(), "WS Test Agent");
    assert!(agent_card["capabilities"]["streaming"].as_bool().unwrap());

    // Test 2: Subscribe to task updates
    let task_id = format!("ws-task-{}", uuid::Uuid::new_v4());
    let message = Message::user_text("Hello, WebSocket A2A agent!".to_string());

    println!("Attempting to subscribe to task: {}", task_id);
    
    // Give additional time for server to be ready for websocket connections
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Create a task subscription
    let subscribe_result = client
        .subscribe_to_task(&task_id, &message, Some("test-session"), None)
        .await;
        
    if let Err(ref e) = subscribe_result {
        println!("Failed to subscribe to task: {}", e);
    }
    
    let mut stream = subscribe_result.expect("Failed to subscribe to task");

    // Process streaming updates
    let mut status_updates = 0;
    let mut artifact_updates = 0;
    let mut final_received = false;

    // Create a timeout future
    let timeout_future = tokio::time::sleep(Duration::from_secs(5));

    // Wait for streaming updates
    tokio::select! {
        _ = async {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(StreamItem::StatusUpdate(update)) => {
                        status_updates += 1;
                        println!("Status update: {:?}", update.status.state);

                        if update.final_ {
                            final_received = true;
                            break;
                        }
                    }
                    Ok(StreamItem::ArtifactUpdate(_)) => {
                        artifact_updates += 1;
                    }
                    Err(e) => {
                        panic!("Stream error: {}", e);
                    }
                }
            }
        } => {},
        _ = timeout_future => {
            // Cancel the task after timeout
            client.cancel_task(&task_id).await.expect("Failed to cancel task");
        }
    }

    // Verify that we received at least one status update
    assert!(
        status_updates > 0,
        "Should have received at least one status update"
    );

    // Get the task to verify history
    let task = client
        .get_task(&task_id, None)
        .await
        .expect("Failed to get task");
    assert_eq!(task.id, task_id);
    assert!(task.history.is_some());

    // Shut down the server
    shutdown_tx
        .send(())
        .expect("Failed to send shutdown signal");

    // Wait for the server to shut down
    server_handle.await.expect("Server task failed");
}
