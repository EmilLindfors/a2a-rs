//! Integration tests for the A2A protocol

use a2a_rs::{
    adapter::client::HttpClient,
    adapter::server::{DefaultRequestProcessor, HttpServer, InMemoryTaskStorage, SimpleAgentInfo},
    domain::{Message, Part, TaskState},
    port::client::AsyncA2AClient,
};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::oneshot;

/// Test a complete HTTP client-server interaction
#[tokio::test]
async fn test_http_client_server_interaction() {
    // Create a storage
    let storage = InMemoryTaskStorage::new();

    // Create a processor
    let processor = DefaultRequestProcessor::new(storage);

    // Create an agent info provider
    let agent_info = SimpleAgentInfo::new(
        "Test Agent".to_string(),
        "http://localhost:8182".to_string(),
    )
    .with_description("Test A2A agent for integration tests".to_string())
    .with_provider(
        "Test Organization".to_string(),
        "https://example.org".to_string(),
    )
    .with_documentation_url("https://example.org/docs".to_string())
    .with_state_transition_history()
    .add_skill(
        "test".to_string(),
        "Test Skill".to_string(),
        Some("A test skill".to_string()),
    );

    // Create the server
    let server = HttpServer::new(processor, agent_info, "127.0.0.1:8182".to_string());

    // Create a shutdown channel
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Start the server in a separate task
    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {
                // Server will be dropped and shut down
            }
        }
    });

    // Give the server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create the client
    let client = HttpClient::new("http://localhost:8182".to_string());

    // Test 1: Get agent card using direct HTTP request
    let http_client = Client::new();
    let response = http_client
        .get("http://localhost:8182/agent-card")
        .send()
        .await
        .expect("Failed to fetch agent card");

    let agent_card: Value = response.json().await.expect("Failed to parse agent card");
    assert_eq!(agent_card["name"].as_str().unwrap(), "Test Agent");
    assert!(
        agent_card["capabilities"]["stateTransitionHistory"]
            .as_bool()
            .unwrap()
    );
    assert!(!agent_card["capabilities"]["streaming"].as_bool().unwrap());

    // Test 2: Get skills using direct HTTP request
    let response = http_client
        .get("http://localhost:8182/skills")
        .send()
        .await
        .expect("Failed to fetch skills");

    let skills: Vec<Value> = response.json().await.expect("Failed to parse skills");
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["id"].as_str().unwrap(), "test");

    // Test 3: Get skill by ID using direct HTTP request
    let response = http_client
        .get("http://localhost:8182/skills/test")
        .send()
        .await
        .expect("Failed to fetch skill");

    let skill: Value = response.json().await.expect("Failed to parse skill");
    assert_eq!(skill["name"].as_str().unwrap(), "Test Skill");

    // Test 4: Send task message
    let task_id = format!("task-{}", uuid::Uuid::new_v4());
    let message_id = format!("msg-{}", uuid::Uuid::new_v4());
    let message = Message::user_text("Hello, A2A agent!".to_string(), message_id);
    let task = client
        .send_task_message(&task_id, &message, None, None)
        .await
        .expect("Failed to send task message");

    assert_eq!(task.id, task_id);
    assert_eq!(task.status.state, TaskState::Working);

    // Test 5: Get task
    let task = client
        .get_task(&task_id, None)
        .await
        .expect("Failed to get task");
    assert_eq!(task.id, task_id);
    assert!(task.history.is_some());

    // Test 6: Get task with limited history
    let task_limited = client
        .get_task(&task_id, Some(0))
        .await
        .expect("Failed to get task with limited history");
    assert_eq!(task_limited.id, task_id);
    assert!(task_limited.history.is_none());

    // Test 7: Cancel task
    println!("About to cancel task with ID: {}", task_id);
    let canceled_task = client
        .cancel_task(&task_id)
        .await
        .expect("Failed to cancel task");
    println!("Received task after cancellation: {:?}", canceled_task);
    println!("Task state: {:?}", canceled_task.status.state);
    assert_eq!(canceled_task.status.state, TaskState::Canceled);

    // Shut down the server
    shutdown_tx
        .send(())
        .expect("Failed to send shutdown signal");

    // Wait for the server to shut down
    server_handle.await.expect("Server task failed");
}

/// Test handling different message types
#[tokio::test]
async fn test_message_types() {
    // Create a message with text part
    let message_id = format!("msg-{}", uuid::Uuid::new_v4());
    let mut message = Message::user_text("Hello, A2A agent!".to_string(), message_id);

    // Add a data part
    let mut data = serde_json::Map::new();
    data.insert(
        "key".to_string(),
        serde_json::Value::String("value".to_string()),
    );
    let data_part = Part::Data {
        data,
        metadata: None,
    };
    message.add_part(data_part);

    // Add a file part
    let file_part = Part::file_from_bytes(
        "SGVsbG8sIHdvcmxkIQ==".to_string(), // Base64 encoded "Hello, world!"
        Some("greeting.txt".to_string()),
        Some("text/plain".to_string()),
    );
    message
        .add_part_validated(file_part)
        .expect("Failed to add file part");

    // Verify message parts
    assert_eq!(message.parts.len(), 3);

    // Verify part types
    match &message.parts[0] {
        Part::Text { text, .. } => assert_eq!(text, "Hello, A2A agent!"),
        _ => panic!("Expected Text part"),
    }

    match &message.parts[1] {
        Part::Data { data, .. } => assert_eq!(data.get("key").unwrap().as_str().unwrap(), "value"),
        _ => panic!("Expected Data part"),
    }

    match &message.parts[2] {
        Part::File { file, .. } => {
            assert_eq!(file.name.as_ref().unwrap(), "greeting.txt");
            assert_eq!(file.mime_type.as_ref().unwrap(), "text/plain");
            assert!(file.bytes.is_some());
            assert!(file.uri.is_none());
        }
        _ => panic!("Expected File part"),
    }
}

/// Test task history functionality
#[tokio::test]
async fn test_task_history() {
    // Create a new task
    let context_id = format!("ctx-{}", uuid::Uuid::new_v4());
    let mut task = a2a_rs::domain::Task::new("test-task-1".to_string(), context_id);

    // Create messages
    let msg_id1 = format!("msg-{}", uuid::Uuid::new_v4());
    let msg_id2 = format!("msg-{}", uuid::Uuid::new_v4());
    let msg_id3 = format!("msg-{}", uuid::Uuid::new_v4());
    let message1 = Message::user_text("Message 1".to_string(), msg_id1);
    let message2 = Message::agent_text("Message 2".to_string(), msg_id2);
    let message3 = Message::user_text("Message 3".to_string(), msg_id3);

    // Update the task with messages
    task.update_status(TaskState::Working, Some(message1));
    task.update_status(TaskState::Working, Some(message2));
    task.update_status(TaskState::Working, Some(message3));

    // Verify history has all messages
    assert!(task.history.is_some());
    let history = task.history.as_ref().unwrap();
    assert_eq!(history.len(), 3);

    // Test history truncation
    let task_limited = task.with_limited_history(Some(2));
    assert!(task_limited.history.is_some());
    let history_limited = task_limited.history.unwrap();
    assert_eq!(history_limited.len(), 2);

    // Test removing history entirely
    let task_no_history = task.with_limited_history(Some(0));
    assert!(task_no_history.history.is_none());
}
