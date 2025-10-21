//! Multi-Transport Integration Tests
//!
//! Tests end-to-end workflows across different transport protocols (HTTP and WebSocket)
//! to ensure compatibility and consistent behavior.

#![cfg(all(
    feature = "http-client",
    feature = "http-server",
    feature = "ws-client",
    feature = "ws-server"
))]

mod common;

use a2a_rs::{
    adapter::{
        DefaultRequestProcessor, HttpClient, HttpServer, InMemoryTaskStorage, SimpleAgentInfo,
        WebSocketClient, WebSocketServer,
    },
    domain::{Message, Part, TaskState},
    services::{AsyncA2AClient, StreamItem},
};
use base64::Engine;
use common::TestBusinessHandler;
use futures::StreamExt;
use reqwest::Client as ReqwestClient;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;

/// Test that the same agent can serve both HTTP and WebSocket protocols
#[tokio::test]
async fn test_dual_protocol_agent() {
    let storage = InMemoryTaskStorage::new();

    // Create business handler that will be shared between protocols
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info = SimpleAgentInfo::new(
        "test-agent".to_string(),
        "http://localhost:8184".to_string(),
    );
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    // Create agent info with capabilities for both protocols
    let agent_info = SimpleAgentInfo::new(
        "Dual Protocol Agent".to_string(),
        "http://localhost:8184".to_string(),
    )
    .with_description("Agent supporting both HTTP and WebSocket".to_string())
    .with_version("1.0.0".to_string())
    .with_provider(
        "Test Organization".to_string(),
        "https://example.org".to_string(),
    )
    .with_streaming() // WebSocket capability
    .with_push_notifications() // HTTP capability
    .with_state_transition_history()
    .add_skill(
        "echo".to_string(),
        "Echo Service".to_string(),
        Some("Returns input as output".to_string()),
    )
    .add_skill(
        "transform".to_string(),
        "Data Transform".to_string(),
        Some("Transforms data between formats".to_string()),
    );

    // Start HTTP server
    let http_server = HttpServer::new(
        processor.clone(),
        agent_info.clone(),
        "127.0.0.1:8184".to_string(),
    );
    let (http_shutdown_tx, http_shutdown_rx) = oneshot::channel::<()>();
    let http_handle = tokio::spawn(async move {
        tokio::select! {
            _ = http_server.start() => {},
            _ = http_shutdown_rx => {}
        }
    });

    // Start WebSocket server on different port
    let ws_server =
        WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8185".to_string());
    let (ws_shutdown_tx, ws_shutdown_rx) = oneshot::channel::<()>();
    let ws_handle = tokio::spawn(async move {
        tokio::select! {
            _ = ws_server.start() => {},
            _ = ws_shutdown_rx => {}
        }
    });

    // Give servers time to start
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test 1: Verify both protocols expose the same agent card
    let http_client = ReqwestClient::new();
    let http_agent_card_response = http_client
        .get("http://localhost:8184/agent-card")
        .send()
        .await
        .expect("Failed to fetch HTTP agent card");
    let http_agent_card: Value = http_agent_card_response
        .json()
        .await
        .expect("Failed to parse HTTP agent card");

    // Verify agent card has dual capabilities
    assert_eq!(
        http_agent_card["name"].as_str().unwrap(),
        "Dual Protocol Agent"
    );
    assert!(http_agent_card["capabilities"]["streaming"]
        .as_bool()
        .unwrap());
    assert!(http_agent_card["capabilities"]["pushNotifications"]
        .as_bool()
        .unwrap());
    assert!(http_agent_card["capabilities"]["stateTransitionHistory"]
        .as_bool()
        .unwrap());

    // Test 2: Create clients for both protocols
    let http_a2a_client = HttpClient::new("http://localhost:8184".to_string());
    let ws_a2a_client = WebSocketClient::new("ws://localhost:8185".to_string());

    // Test 3: Send the same message via both protocols
    let task_id_http = format!("http-task-{}", uuid::Uuid::new_v4());
    let task_id_ws = format!("ws-task-{}", uuid::Uuid::new_v4());
    let message_content = "Test message for dual protocol verification";

    let http_message = Message::user_text(
        message_content.to_string(),
        format!("http-msg-{}", uuid::Uuid::new_v4()),
    );
    let ws_message = Message::user_text(
        message_content.to_string(),
        format!("ws-msg-{}", uuid::Uuid::new_v4()),
    );

    // Send via HTTP
    let http_task = http_a2a_client
        .send_task_message(&task_id_http, &http_message, None, None)
        .await
        .expect("Failed to send HTTP task message");

    // Send via WebSocket (basic message, not streaming)
    let ws_task = ws_a2a_client
        .send_task_message(&task_id_ws, &ws_message, None, None)
        .await
        .expect("Failed to send WebSocket task message");

    // Test 4: Verify both tasks were created with consistent behavior
    assert_eq!(http_task.id, task_id_http);
    assert_eq!(ws_task.id, task_id_ws);
    assert_eq!(http_task.status.state, TaskState::Working);
    assert_eq!(ws_task.status.state, TaskState::Working);

    // Test 5: Retrieve tasks via both protocols
    let http_retrieved_task = http_a2a_client
        .get_task(&task_id_http, None)
        .await
        .expect("Failed to retrieve HTTP task");
    let ws_retrieved_task = ws_a2a_client
        .get_task(&task_id_ws, None)
        .await
        .expect("Failed to retrieve WebSocket task");

    assert!(http_retrieved_task.history.is_some());
    assert!(ws_retrieved_task.history.is_some());

    // Test 6: Cancel tasks via both protocols
    let http_canceled_task = http_a2a_client
        .cancel_task(&task_id_http)
        .await
        .expect("Failed to cancel HTTP task");
    let ws_canceled_task = ws_a2a_client
        .cancel_task(&task_id_ws)
        .await
        .expect("Failed to cancel WebSocket task");

    assert_eq!(http_canceled_task.status.state, TaskState::Canceled);
    assert_eq!(ws_canceled_task.status.state, TaskState::Canceled);

    // Clean up
    let _ = http_shutdown_tx.send(());
    let _ = ws_shutdown_tx.send(());
    let _ = tokio::join!(http_handle, ws_handle);
}

/// Test cross-protocol task interaction (create via HTTP, stream via WebSocket)
#[tokio::test]
async fn test_cross_protocol_task_interaction() {
    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info = SimpleAgentInfo::new(
        "test-agent".to_string(),
        "http://localhost:8186".to_string(),
    );
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Cross Protocol Agent".to_string(),
        "http://localhost:8186".to_string(),
    )
    .with_description("Agent for cross-protocol testing".to_string())
    .with_streaming()
    .with_state_transition_history()
    .add_skill(
        "process".to_string(),
        "Process Data".to_string(),
        Some("Processes data with status updates".to_string()),
    );

    // Start servers
    let http_server = HttpServer::new(
        processor.clone(),
        agent_info.clone(),
        "127.0.0.1:8186".to_string(),
    );
    let ws_server =
        WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8187".to_string());

    let (http_shutdown_tx, http_shutdown_rx) = oneshot::channel::<()>();
    let (ws_shutdown_tx, ws_shutdown_rx) = oneshot::channel::<()>();

    let http_handle = tokio::spawn(async move {
        tokio::select! {
            _ = http_server.start() => {},
            _ = http_shutdown_rx => {}
        }
    });

    let ws_handle = tokio::spawn(async move {
        tokio::select! {
            _ = ws_server.start() => {},
            _ = ws_shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Create clients
    let http_client = HttpClient::new("http://localhost:8186".to_string());
    let ws_client = WebSocketClient::new("ws://localhost:8187".to_string());

    // Test scenario: Create task via HTTP, then monitor via WebSocket streaming
    let task_id = format!("cross-protocol-{}", uuid::Uuid::new_v4());
    let message = Message::user_text(
        "Process this data".to_string(),
        format!("msg-{}", uuid::Uuid::new_v4()),
    );

    // Step 1: Create task via HTTP
    let created_task = http_client
        .send_task_message(&task_id, &message, None, None)
        .await
        .expect("Failed to create task via HTTP");

    assert_eq!(created_task.id, task_id);

    // Step 2: Stream task updates via WebSocket (if supported)
    let stream_result = timeout(
        Duration::from_secs(2),
        ws_client.subscribe_to_task(&task_id, None),
    )
    .await;

    if let Ok(Ok(mut stream)) = stream_result {
        let mut received_updates = 0;

        // Process a few streaming updates
        while received_updates < 3 {
            let next_item = timeout(Duration::from_millis(500), stream.next()).await;

            match next_item {
                Ok(Some(Ok(StreamItem::Task(task)))) => {
                    println!("Received task update: {:?}", task.status.state);
                    received_updates += 1;
                }
                Ok(Some(Ok(StreamItem::StatusUpdate(update)))) => {
                    println!("Received status update: {:?}", update.status.state);
                    received_updates += 1;

                    if update.final_ {
                        break;
                    }
                }
                Ok(Some(Ok(StreamItem::ArtifactUpdate(_)))) => {
                    println!("Received artifact update");
                    received_updates += 1;
                }
                Ok(Some(Err(e))) => {
                    println!("Stream error: {}", e);
                    break;
                }
                Ok(None) => {
                    println!("Stream ended");
                    break;
                }
                Err(_) => {
                    println!("Stream timeout");
                    break;
                }
            }
        }

        println!("Received {} streaming updates", received_updates);
    } else {
        println!("WebSocket streaming not available or timed out");
    }

    // Step 3: Verify task state via HTTP after streaming
    let final_task = http_client
        .get_task(&task_id, None)
        .await
        .expect("Failed to retrieve final task state via HTTP");

    assert_eq!(final_task.id, task_id);

    // Step 4: Cancel via WebSocket, verify via HTTP
    let canceled_task = ws_client
        .cancel_task(&task_id)
        .await
        .expect("Failed to cancel task via WebSocket");
    assert_eq!(canceled_task.status.state, TaskState::Canceled);

    // Verify cancellation via HTTP
    let verified_task = http_client
        .get_task(&task_id, None)
        .await
        .expect("Failed to verify canceled task via HTTP");
    assert_eq!(verified_task.status.state, TaskState::Canceled);

    // Clean up
    let _ = http_shutdown_tx.send(());
    let _ = ws_shutdown_tx.send(());
    let _ = tokio::join!(http_handle, ws_handle);
}

/// Test complex message types across both protocols
#[tokio::test]
async fn test_complex_message_types_across_transports() {
    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info = SimpleAgentInfo::new(
        "test-agent".to_string(),
        "http://localhost:8188".to_string(),
    );
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Complex Message Agent".to_string(),
        "http://localhost:8188".to_string(),
    )
    .with_description("Agent for testing complex message types".to_string())
    .with_streaming()
    .add_skill(
        "analyze".to_string(),
        "Data Analysis".to_string(),
        Some("Analyzes various data formats".to_string()),
    );

    // Start servers
    let http_server = HttpServer::new(
        processor.clone(),
        agent_info.clone(),
        "127.0.0.1:8188".to_string(),
    );
    let ws_server =
        WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8189".to_string());

    let (http_shutdown_tx, http_shutdown_rx) = oneshot::channel::<()>();
    let (ws_shutdown_tx, ws_shutdown_rx) = oneshot::channel::<()>();

    let http_handle = tokio::spawn(async move {
        tokio::select! {
            _ = http_server.start() => {},
            _ = http_shutdown_rx => {}
        }
    });

    let ws_handle = tokio::spawn(async move {
        tokio::select! {
            _ = ws_server.start() => {},
            _ = ws_shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    let http_client = HttpClient::new("http://localhost:8188".to_string());
    let ws_client = WebSocketClient::new("ws://localhost:8189".to_string());

    // Create complex messages with multiple part types
    let mut http_message = Message::user_text(
        "Analyze this complex data set".to_string(),
        format!("complex-http-{}", uuid::Uuid::new_v4()),
    );

    let mut ws_message = Message::user_text(
        "Analyze this complex data set".to_string(),
        format!("complex-ws-{}", uuid::Uuid::new_v4()),
    );

    // Add data parts with structured data
    let data_part = Part::Data {
        data: serde_json::json!({
            "dataset": "sales_2024",
            "metrics": ["revenue", "users", "conversions"],
            "filters": {
                "date_range": "2024-01-01 to 2024-12-31",
                "regions": ["US", "EU", "APAC"]
            },
            "parameters": {
                "confidence_level": 0.95,
                "sample_size": 1000
            }
        })
        .as_object()
        .unwrap()
        .clone(),
        metadata: None,
    };

    http_message.add_part(data_part.clone());
    ws_message.add_part(data_part);

    // Add file parts with base64 encoded data
    let file_content =
        b"Sample CSV data\ndate,revenue,users\n2024-01-01,1000,50\n2024-01-02,1100,55";
    let encoded_content = base64::engine::general_purpose::STANDARD.encode(file_content);

    let file_part = Part::file_from_bytes(
        encoded_content,
        Some("sample_data.csv".to_string()),
        Some("text/csv".to_string()),
    );

    http_message
        .add_part_validated(file_part.clone())
        .expect("Failed to add file part to HTTP message");
    ws_message
        .add_part_validated(file_part)
        .expect("Failed to add file part to WebSocket message");

    // Send complex messages via both protocols
    let http_task_id = format!("complex-http-{}", uuid::Uuid::new_v4());
    let ws_task_id = format!("complex-ws-{}", uuid::Uuid::new_v4());

    let http_task = http_client
        .send_task_message(&http_task_id, &http_message, None, None)
        .await
        .expect("Failed to send complex HTTP message");

    let ws_task = ws_client
        .send_task_message(&ws_task_id, &ws_message, None, None)
        .await
        .expect("Failed to send complex WebSocket message");

    // Verify both tasks were created successfully
    assert_eq!(http_task.id, http_task_id);
    assert_eq!(ws_task.id, ws_task_id);

    // Retrieve tasks and verify message complexity was preserved
    let http_retrieved = http_client
        .get_task(&http_task_id, None)
        .await
        .expect("Failed to retrieve HTTP task");

    let ws_retrieved = ws_client
        .get_task(&ws_task_id, None)
        .await
        .expect("Failed to retrieve WebSocket task");

    // Verify history contains complex messages
    assert!(http_retrieved.history.is_some());
    assert!(ws_retrieved.history.is_some());

    let http_history = http_retrieved.history.unwrap();
    let ws_history = ws_retrieved.history.unwrap();

    assert!(!http_history.is_empty());
    assert!(!ws_history.is_empty());

    // Both should have messages with multiple parts
    assert!(http_history[0].parts.len() >= 3); // text + data + file
    assert!(ws_history[0].parts.len() >= 3);

    // Verify part types are preserved
    let http_parts = &http_history[0].parts;
    let ws_parts = &ws_history[0].parts;

    let has_text_part = |parts: &[Part]| parts.iter().any(|p| matches!(p, Part::Text { .. }));
    let has_data_part = |parts: &[Part]| parts.iter().any(|p| matches!(p, Part::Data { .. }));
    let has_file_part = |parts: &[Part]| parts.iter().any(|p| matches!(p, Part::File { .. }));

    assert!(has_text_part(http_parts));
    assert!(has_data_part(http_parts));
    assert!(has_file_part(http_parts));

    assert!(has_text_part(ws_parts));
    assert!(has_data_part(ws_parts));
    assert!(has_file_part(ws_parts));

    // Clean up
    let _ = http_client.cancel_task(&http_task_id).await;
    let _ = ws_client.cancel_task(&ws_task_id).await;
    let _ = http_shutdown_tx.send(());
    let _ = ws_shutdown_tx.send(());
    let _ = tokio::join!(http_handle, ws_handle);
}

/// Test error handling consistency across protocols
#[tokio::test]
async fn test_error_handling_across_transports() {
    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info = SimpleAgentInfo::new(
        "test-agent".to_string(),
        "http://localhost:8190".to_string(),
    );
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Error Test Agent".to_string(),
        "http://localhost:8190".to_string(),
    )
    .with_description("Agent for testing error handling".to_string())
    .with_streaming()
    .add_skill(
        "test".to_string(),
        "Test Skill".to_string(),
        Some("For testing purposes".to_string()),
    );

    // Start servers
    let http_server = HttpServer::new(
        processor.clone(),
        agent_info.clone(),
        "127.0.0.1:8190".to_string(),
    );
    let ws_server =
        WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8191".to_string());

    let (http_shutdown_tx, http_shutdown_rx) = oneshot::channel::<()>();
    let (ws_shutdown_tx, ws_shutdown_rx) = oneshot::channel::<()>();

    let http_handle = tokio::spawn(async move {
        tokio::select! {
            _ = http_server.start() => {},
            _ = http_shutdown_rx => {}
        }
    });

    let ws_handle = tokio::spawn(async move {
        tokio::select! {
            _ = ws_server.start() => {},
            _ = ws_shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    let http_client = HttpClient::new("http://localhost:8190".to_string());
    let ws_client = WebSocketClient::new("ws://localhost:8191".to_string());

    // Test 1: Task not found errors
    let nonexistent_task_id = "nonexistent-task-id";

    let http_error = http_client.get_task(nonexistent_task_id, None).await;
    let ws_error = ws_client.get_task(nonexistent_task_id, None).await;

    assert!(http_error.is_err());
    assert!(ws_error.is_err());

    // Test 2: Cancel non-existent task
    let http_cancel_error = http_client.cancel_task(nonexistent_task_id).await;
    let ws_cancel_error = ws_client.cancel_task(nonexistent_task_id).await;

    assert!(http_cancel_error.is_err());
    assert!(ws_cancel_error.is_err());

    // Test 3: Create task then try to cancel an already completed task
    // (This tests business logic consistency)
    let message = Message::user_text(
        "Test message".to_string(),
        format!("msg-{}", uuid::Uuid::new_v4()),
    );
    let task_id = format!("test-task-{}", uuid::Uuid::new_v4());

    // Create task via HTTP
    let _task = http_client
        .send_task_message(&task_id, &message, None, None)
        .await
        .expect("Failed to create task");

    // Try to get the same task via WebSocket (should work)
    let retrieved_task = ws_client
        .get_task(&task_id, None)
        .await
        .expect("Failed to retrieve task via WebSocket");

    assert_eq!(retrieved_task.id, task_id);

    // Clean up
    let _ = http_client.cancel_task(&task_id).await;
    let _ = http_shutdown_tx.send(());
    let _ = ws_shutdown_tx.send(());
    let _ = tokio::join!(http_handle, ws_handle);
}
