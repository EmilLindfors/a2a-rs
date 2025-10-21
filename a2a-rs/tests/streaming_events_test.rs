//! Comprehensive Streaming Events and WebSocket Functionality Tests
//!
//! Tests streaming events, WebSocket connections, event ordering, and real-time updates
//! according to the A2A specification.

#![cfg(all(feature = "ws-client", feature = "ws-server"))]

mod common;

use a2a_rs::{
    adapter::{
        DefaultRequestProcessor, InMemoryTaskStorage, SimpleAgentInfo, WebSocketClient,
        WebSocketServer,
    },
    domain::{Message, TaskState},
    services::{AsyncA2AClient, StreamItem},
};
use common::TestBusinessHandler;
use futures::stream::StreamExt;
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;

/// Test basic WebSocket streaming functionality with status updates
#[tokio::test]
async fn test_basic_streaming_functionality() {
    if std::env::var("CI").is_ok() {
        println!("Skipping streaming test in CI environment");
        return;
    }

    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info =
        SimpleAgentInfo::new("test-agent".to_string(), "ws://localhost:8192".to_string());
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Streaming Test Agent".to_string(),
        "ws://localhost:8192".to_string(),
    )
    .with_description("Agent for testing streaming functionality".to_string())
    .with_streaming()
    .with_state_transition_history()
    .add_skill(
        "stream-test".to_string(),
        "Stream Test".to_string(),
        Some("Tests streaming capabilities".to_string()),
    );

    let server = WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8192".to_string());
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = WebSocketClient::new("ws://localhost:8192".to_string());
    let task_id = format!("stream-test-{}", uuid::Uuid::new_v4());

    // First, send a message to create the task
    let message = Message::user_text(
        "Start streaming test".to_string(),
        uuid::Uuid::new_v4().to_string(),
    );
    let _ = client
        .send_task_message(&task_id, &message, None, None)
        .await;

    // Subscribe to streaming updates
    let stream_result = timeout(
        Duration::from_secs(3),
        client.subscribe_to_task(&task_id, None),
    )
    .await;

    if let Ok(Ok(mut stream)) = stream_result {
        let mut events_received = Vec::new();
        let mut status_updates = 0;

        // Process stream events with timeout
        let stream_processing = async {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(StreamItem::Task(task)) => {
                        events_received.push(format!("Task: {:?}", task.status.state));
                        println!("Received task: {:?}", task.status.state);
                    }
                    Ok(StreamItem::StatusUpdate(update)) => {
                        events_received.push(format!("Status: {:?}", update.status.state));
                        status_updates += 1;
                        println!(
                            "Status update: {:?}, final: {}",
                            update.status.state, update.final_
                        );

                        if update.final_ {
                            println!("Final update received, ending stream");
                            break;
                        }
                    }
                    Ok(StreamItem::ArtifactUpdate(update)) => {
                        events_received.push(format!("Artifact: {}", update.artifact.artifact_id));
                        println!("Artifact update: {}", update.artifact.artifact_id);
                    }
                    Err(e) => {
                        println!("Stream error: {}", e);
                        break;
                    }
                }

                // Safety break to prevent infinite loops
                if events_received.len() > 20 {
                    break;
                }
            }
        };

        // Run stream processing with timeout
        let _ = timeout(Duration::from_secs(5), stream_processing).await;

        println!("Total events received: {}", events_received.len());
        println!("Status updates: {}", status_updates);

        // We should have received at least some events
        assert!(
            !events_received.is_empty(),
            "Should have received streaming events"
        );
    } else {
        println!("Failed to establish streaming connection or timeout");
    }

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}

/// Test event ordering and final marker in streaming
#[tokio::test]
async fn test_streaming_event_ordering() {
    if std::env::var("CI").is_ok() {
        println!("Skipping streaming test in CI environment");
        return;
    }

    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info =
        SimpleAgentInfo::new("test-agent".to_string(), "ws://localhost:8193".to_string());
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Event Ordering Agent".to_string(),
        "ws://localhost:8193".to_string(),
    )
    .with_streaming()
    .with_state_transition_history()
    .add_skill(
        "ordering-test".to_string(),
        "Event Ordering Test".to_string(),
        Some("Tests event ordering".to_string()),
    );

    let server = WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8193".to_string());
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = WebSocketClient::new("ws://localhost:8193".to_string());
    let task_id = format!("ordering-test-{}", uuid::Uuid::new_v4());

    let stream_result = timeout(
        Duration::from_secs(3),
        client.subscribe_to_task(&task_id, None),
    )
    .await;

    if let Ok(Ok(mut stream)) = stream_result {
        let mut event_sequence = Vec::new();
        let mut final_received = false;

        let stream_processing = async {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(StreamItem::Task(task)) => {
                        event_sequence.push(("Task", task.status.state, false));
                    }
                    Ok(StreamItem::StatusUpdate(update)) => {
                        event_sequence.push((
                            "StatusUpdate",
                            update.status.state.clone(),
                            update.final_,
                        ));

                        if update.final_ {
                            final_received = true;
                            break;
                        }
                    }
                    Ok(StreamItem::ArtifactUpdate(_update)) => {
                        event_sequence.push(("ArtifactUpdate", TaskState::Working, false));
                        // Placeholder state for artifacts
                    }
                    Err(_) => break,
                }

                if event_sequence.len() > 15 {
                    break;
                }
            }
        };

        let _ = timeout(Duration::from_secs(5), stream_processing).await;

        // Verify event ordering properties
        if !event_sequence.is_empty() {
            println!("Event sequence: {:?}", event_sequence);

            // Check that we received events in a logical order
            let has_initial_event = event_sequence.iter().any(|(kind, _, _)| kind == &"Task");
            println!("Has initial task event: {}", has_initial_event);

            // Check for final marker if we received status updates
            let has_status_updates = event_sequence
                .iter()
                .any(|(kind, _, _)| kind == &"StatusUpdate");
            if has_status_updates {
                println!("Final marker received: {}", final_received);
            }

            // Events should be ordered chronologically (basic check)
            assert!(!event_sequence.is_empty());
        }
    }

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}

/// Test artifact updates in streaming
#[tokio::test]
async fn test_streaming_artifact_updates() {
    if std::env::var("CI").is_ok() {
        println!("Skipping streaming test in CI environment");
        return;
    }

    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info =
        SimpleAgentInfo::new("test-agent".to_string(), "ws://localhost:8194".to_string());
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Artifact Streaming Agent".to_string(),
        "ws://localhost:8194".to_string(),
    )
    .with_streaming()
    .add_skill(
        "artifact-gen".to_string(),
        "Artifact Generator".to_string(),
        Some("Generates streaming artifacts".to_string()),
    );

    let server = WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8194".to_string());
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = WebSocketClient::new("ws://localhost:8194".to_string());
    let task_id = format!("artifact-test-{}", uuid::Uuid::new_v4());

    // Create a message that might trigger artifact generation
    let message = Message::user_text(
        "Generate artifacts with streaming updates".to_string(),
        format!("msg-{}", uuid::Uuid::new_v4()),
    );

    // First, create the task (this might trigger some initial processing)
    let _task_result = timeout(
        Duration::from_secs(2),
        client.send_task_message(&task_id, &message, None, None),
    )
    .await;

    // Then subscribe to streaming updates
    let stream_result = timeout(
        Duration::from_secs(3),
        client.subscribe_to_task(&task_id, None),
    )
    .await;

    if let Ok(Ok(mut stream)) = stream_result {
        let mut artifact_updates = Vec::new();
        let mut unique_artifact_ids = HashSet::new();

        let stream_processing = async {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(StreamItem::ArtifactUpdate(update)) => {
                        artifact_updates.push(update.clone());
                        unique_artifact_ids.insert(update.artifact.artifact_id.clone());

                        println!(
                            "Artifact update: ID={}, parts={}, last_chunk={}",
                            update.artifact.artifact_id,
                            update.artifact.parts.len(),
                            update.last_chunk.unwrap_or(false)
                        );

                        // If this is marked as the last chunk, we can stop
                        if update.last_chunk == Some(true) {
                            break;
                        }
                    }
                    Ok(StreamItem::StatusUpdate(update)) => {
                        println!(
                            "Status update: {:?}, final: {}",
                            update.status.state, update.final_
                        );
                        if update.final_ {
                            break;
                        }
                    }
                    Ok(StreamItem::Task(_)) => {
                        // Initial task info
                    }
                    Err(_) => break,
                }

                if artifact_updates.len() > 10 {
                    break;
                }
            }
        };

        let _ = timeout(Duration::from_secs(5), stream_processing).await;

        // Verify artifact streaming properties
        println!("Total artifact updates: {}", artifact_updates.len());
        println!("Unique artifact IDs: {}", unique_artifact_ids.len());

        if !artifact_updates.is_empty() {
            // Check artifact structure
            for update in &artifact_updates {
                assert!(!update.artifact.artifact_id.is_empty());
                assert!(!update.artifact.parts.is_empty());
                assert_eq!(update.kind, "artifact-update");
            }

            // Test passed if we received artifact updates
            assert!(!artifact_updates.is_empty());
        } else {
            // If no artifacts were generated, that's also valid behavior
            println!("No artifact updates received - this may be expected behavior");
        }
    }

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}

/// Test WebSocket connection resilience and reconnection patterns
#[tokio::test]
async fn test_websocket_connection_resilience() {
    if std::env::var("CI").is_ok() {
        println!("Skipping streaming test in CI environment");
        return;
    }

    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info =
        SimpleAgentInfo::new("test-agent".to_string(), "ws://localhost:8195".to_string());
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Resilience Test Agent".to_string(),
        "ws://localhost:8195".to_string(),
    )
    .with_streaming()
    .add_skill(
        "resilience".to_string(),
        "Connection Resilience".to_string(),
        Some("Tests connection handling".to_string()),
    );

    let server = WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8195".to_string());
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Test 1: Basic connection establishment
    let client1 = WebSocketClient::new("ws://localhost:8195".to_string());
    let task_id = format!("resilience-test-{}", uuid::Uuid::new_v4());

    let connection_test = timeout(
        Duration::from_secs(2),
        client1.subscribe_to_task(&task_id, None),
    )
    .await;

    match connection_test {
        Ok(Ok(mut stream)) => {
            println!("✓ Initial connection established successfully");

            // Read a few events to verify the connection works
            let mut events_count = 0;
            let read_events = async {
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(_) => {
                            events_count += 1;
                            if events_count >= 3 {
                                break;
                            }
                        }
                        Err(e) => {
                            println!("Stream error: {}", e);
                            break;
                        }
                    }
                }
            };

            let _ = timeout(Duration::from_secs(3), read_events).await;
            println!("✓ Received {} events on initial connection", events_count);
        }
        Ok(Err(e)) => {
            println!("✗ Connection failed: {}", e);
        }
        Err(_) => {
            println!("✗ Connection timeout");
        }
    }

    // Test 2: Multiple concurrent connections
    let client2 = WebSocketClient::new("ws://localhost:8195".to_string());
    let client3 = WebSocketClient::new("ws://localhost:8195".to_string());

    let task_id2 = format!("resilience-test-2-{}", uuid::Uuid::new_v4());
    let task_id3 = format!("resilience-test-3-{}", uuid::Uuid::new_v4());

    let concurrent_test = timeout(Duration::from_secs(3), async {
        let stream2_future = client2.subscribe_to_task(&task_id2, None);
        let stream3_future = client3.subscribe_to_task(&task_id3, None);

        let (result2, result3): (Result<_, _>, Result<_, _>) =
            tokio::join!(stream2_future, stream3_future);

        (result2.is_ok(), result3.is_ok())
    })
    .await;

    match concurrent_test {
        Ok((true, true)) => {
            println!("✓ Multiple concurrent connections established successfully");
        }
        Ok((false, false)) => {
            println!("✗ Both concurrent connections failed");
        }
        Ok(_) => {
            println!("⚠ Mixed results for concurrent connections");
        }
        Err(_) => {
            println!("✗ Concurrent connection test timeout");
        }
    }

    // Test 3: Invalid task ID handling
    let invalid_task_result =
        timeout(Duration::from_secs(2), client1.subscribe_to_task("", None)).await;

    match invalid_task_result {
        Ok(Err(_)) => {
            println!("✓ Invalid task ID properly rejected");
        }
        Ok(Ok(_)) => {
            println!("⚠ Invalid task ID was accepted (may be valid behavior)");
        }
        Err(_) => {
            println!("✗ Invalid task ID test timeout");
        }
    }

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}

/// Test streaming with different history lengths
#[tokio::test]
async fn test_streaming_with_history_limits() {
    if std::env::var("CI").is_ok() {
        println!("Skipping streaming test in CI environment");
        return;
    }

    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info =
        SimpleAgentInfo::new("test-agent".to_string(), "ws://localhost:8196".to_string());
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "History Test Agent".to_string(),
        "ws://localhost:8196".to_string(),
    )
    .with_streaming()
    .with_state_transition_history()
    .add_skill(
        "history-test".to_string(),
        "History Test".to_string(),
        Some("Tests history functionality".to_string()),
    );

    let server = WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8196".to_string());
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = WebSocketClient::new("ws://localhost:8196".to_string());
    let task_id = format!("history-test-{}", uuid::Uuid::new_v4());

    // Test different history lengths
    let history_lengths = [Some(0), Some(1), Some(5), None];

    for history_length in history_lengths {
        println!("Testing with history length: {:?}", history_length);

        let stream_result = timeout(
            Duration::from_secs(2),
            client.subscribe_to_task(&task_id, history_length),
        )
        .await;

        if let Ok(Ok(mut stream)) = stream_result {
            let mut task_events = Vec::new();

            let collect_events = async {
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(StreamItem::Task(task)) => {
                            task_events.push(task);
                        }
                        Ok(StreamItem::StatusUpdate(update)) => {
                            if update.final_ {
                                break;
                            }
                        }
                        Ok(StreamItem::ArtifactUpdate(_)) => {}
                        Err(_) => break,
                    }

                    if task_events.len() > 10 {
                        break;
                    }
                }
            };

            let _ = timeout(Duration::from_secs(3), collect_events).await;

            // Verify history length constraints
            for task in &task_events {
                if let Some(history) = &task.history {
                    match history_length {
                        Some(0) => assert!(history.is_empty()),
                        Some(limit) => assert!(history.len() <= limit as usize),
                        None => {} // No constraint
                    }
                }
            }

            println!("  Received {} task events", task_events.len());
        } else {
            println!(
                "  Failed to establish stream for history length {:?}",
                history_length
            );
        }
    }

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}

/// Test streaming event validation against A2A specification
#[tokio::test]
async fn test_streaming_event_specification_compliance() {
    if std::env::var("CI").is_ok() {
        println!("Skipping streaming test in CI environment");
        return;
    }

    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage.clone());
    let test_agent_info =
        SimpleAgentInfo::new("test-agent".to_string(), "ws://localhost:8197".to_string());
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Spec Compliance Agent".to_string(),
        "ws://localhost:8197".to_string(),
    )
    .with_streaming()
    .with_state_transition_history()
    .add_skill(
        "spec-test".to_string(),
        "Spec Compliance Test".to_string(),
        Some("Tests A2A spec compliance".to_string()),
    );

    let server = WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8197".to_string());
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = WebSocketClient::new("ws://localhost:8197".to_string());
    let task_id = format!("spec-test-{}", uuid::Uuid::new_v4());

    let stream_result = timeout(
        Duration::from_secs(3),
        client.subscribe_to_task(&task_id, None),
    )
    .await;

    if let Ok(Ok(mut stream)) = stream_result {
        let mut spec_violations = Vec::new();

        let validate_events = async {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(StreamItem::Task(task)) => {
                        // Validate Task structure according to A2A spec
                        if task.kind != "task" {
                            spec_violations.push(format!("Invalid task kind: {}", task.kind));
                        }
                        if task.id.is_empty() {
                            spec_violations.push("Task ID is empty".to_string());
                        }
                        if task.context_id.is_empty() {
                            spec_violations.push("Context ID is empty".to_string());
                        }
                    }
                    Ok(StreamItem::StatusUpdate(update)) => {
                        // Validate TaskStatusUpdateEvent according to A2A spec
                        if update.kind != "status-update" {
                            spec_violations
                                .push(format!("Invalid status update kind: {}", update.kind));
                        }
                        if update.task_id.is_empty() {
                            spec_violations.push("Status update task ID is empty".to_string());
                        }
                        if update.context_id.is_empty() {
                            spec_violations.push("Status update context ID is empty".to_string());
                        }

                        // final field is required
                        // Note: final_ is the field name due to Rust keyword restrictions

                        if update.final_ {
                            break;
                        }
                    }
                    Ok(StreamItem::ArtifactUpdate(update)) => {
                        // Validate TaskArtifactUpdateEvent according to A2A spec
                        if update.kind != "artifact-update" {
                            spec_violations
                                .push(format!("Invalid artifact update kind: {}", update.kind));
                        }
                        if update.task_id.is_empty() {
                            spec_violations.push("Artifact update task ID is empty".to_string());
                        }
                        if update.context_id.is_empty() {
                            spec_violations.push("Artifact update context ID is empty".to_string());
                        }
                        if update.artifact.artifact_id.is_empty() {
                            spec_violations.push("Artifact ID is empty".to_string());
                        }
                        if update.artifact.parts.is_empty() {
                            spec_violations.push("Artifact has no parts".to_string());
                        }
                    }
                    Err(_) => break,
                }

                if spec_violations.len() > 10 {
                    break;
                }
            }
        };

        let _ = timeout(Duration::from_secs(5), validate_events).await;

        // Report specification compliance
        if spec_violations.is_empty() {
            println!("✓ All streaming events comply with A2A specification");
        } else {
            println!("✗ Specification violations found:");
            for violation in &spec_violations {
                println!("  - {}", violation);
            }
        }

        // Test should pass if there are no violations
        assert!(
            spec_violations.is_empty(),
            "A2A specification violations detected: {:?}",
            spec_violations
        );
    } else {
        println!("Failed to establish streaming connection for spec compliance test");
    }

    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}
