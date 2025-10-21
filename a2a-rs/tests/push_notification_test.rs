//! Push notification tests

#![cfg(all(feature = "http-client", feature = "http-server"))]

mod common;

use a2a_rs::{
    adapter::{
        DefaultRequestProcessor, HttpClient, HttpServer, InMemoryTaskStorage,
        PushNotificationSender, SimpleAgentInfo,
    },
    domain::{
        A2AError, Message, Part, PushNotificationConfig, TaskArtifactUpdateEvent,
        TaskStatusUpdateEvent,
    },
    services::AsyncA2AClient,
};
use async_trait::async_trait;
use common::TestBusinessHandler;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// Mock push notification sender for testing
#[derive(Clone)]
struct MockPushNotificationSender {
    status_updates: Arc<Mutex<Vec<String>>>,
    artifact_updates: Arc<Mutex<Vec<String>>>,
}

impl MockPushNotificationSender {
    fn new() -> Self {
        Self {
            status_updates: Arc::new(Mutex::new(Vec::new())),
            artifact_updates: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_status_updates(&self) -> Vec<String> {
        self.status_updates.lock().unwrap().clone()
    }

    #[allow(dead_code)]
    fn get_artifact_updates(&self) -> Vec<String> {
        self.artifact_updates.lock().unwrap().clone()
    }
}

#[async_trait]
impl PushNotificationSender for MockPushNotificationSender {
    async fn send_status_update(
        &self,
        config: &PushNotificationConfig,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        // Record the update
        let update = format!(
            "Status update for task {} to URL {}",
            event.task_id, config.url
        );
        self.status_updates.lock().unwrap().push(update);
        Ok(())
    }

    async fn send_artifact_update(
        &self,
        config: &PushNotificationConfig,
        event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        // Record the update
        let update = format!(
            "Artifact update for task {} to URL {}",
            event.task_id, config.url
        );
        self.artifact_updates.lock().unwrap().push(update);
        Ok(())
    }
}

/// Test push notification functionality
#[tokio::test]
async fn test_push_notifications() {
    // Create a mock push notification sender
    let push_sender = MockPushNotificationSender::new();
    let push_sender_clone = push_sender.clone();

    // Create a storage with the push sender
    let storage = InMemoryTaskStorage::with_push_sender(push_sender_clone);

    // Create business handler with the storage
    let handler = TestBusinessHandler::with_storage(storage);

    // Create agent info for the processor
    let test_agent_info = SimpleAgentInfo::new(
        "test-agent".to_string(),
        "http://localhost:8184".to_string(),
    );

    // Create a processor
    let processor = DefaultRequestProcessor::with_handler(handler, test_agent_info);

    // Create an agent info provider
    let agent_info = SimpleAgentInfo::new(
        "Push Test Agent".to_string(),
        "http://localhost:8184".to_string(),
    )
    .with_push_notifications()
    .with_state_transition_history();

    // Create the server
    let server = HttpServer::new(processor, agent_info, "127.0.0.1:8184".to_string());

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
    let client = HttpClient::new("http://localhost:8184".to_string());

    // Test 1: Set push notification
    let task_id = format!("push-task-{}", uuid::Uuid::new_v4());
    let push_config = a2a_rs::domain::TaskPushNotificationConfig {
        task_id: task_id.clone(),
        push_notification_config: PushNotificationConfig {
            url: "https://example.com/webhook".to_string(),
            token: Some("test-token".to_string()),
            authentication: None,
        },
    };

    let result = client.set_task_push_notification(&push_config).await;
    assert!(result.is_ok());

    // Test 2: Send a task message
    let message_id = format!("msg-{}", uuid::Uuid::new_v4());
    let message = Message::user_text("Hello, Push Notification Agent!".to_string(), message_id);
    let _task = client
        .send_task_message(&task_id, &message, None, None)
        .await
        .expect("Failed to send task message");

    // Give time for push notifications to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Test 3: Add an artifact
    let artifact_part = Part::Text {
        text: "Artifact content".to_string(),
        metadata: None,
    };

    let _artifact = a2a_rs::domain::Artifact {
        artifact_id: format!("artifact-{}", uuid::Uuid::new_v4()),
        name: Some("test-artifact".to_string()),
        description: Some("A test artifact".to_string()),
        parts: vec![artifact_part],
        metadata: None,
    };

    let artifact_message_id = format!("msg-{}", uuid::Uuid::new_v4());
    let artifact_message = Message {
        message_id: artifact_message_id,
        context_id: Some("default".to_string()),
        role: a2a_rs::domain::Role::Agent,
        kind: "message".to_string(),
        parts: vec![],
        metadata: None,
        reference_task_ids: None,
        task_id: None,
    };

    // Send the artifact message
    let _task = client
        .send_task_message(&task_id, &artifact_message, None, None)
        .await
        .expect("Failed to send artifact message");

    // Give time for push notifications to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Test 4: Cancel the task
    let _canceled_task = client
        .cancel_task(&task_id)
        .await
        .expect("Failed to cancel task");

    // Give time for push notifications to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify that push notifications were sent
    let status_updates = push_sender.get_status_updates();
    println!("Status updates: {:?}", status_updates);
    assert!(
        !status_updates.is_empty(),
        "Should have sent at least one status update"
    );

    // Shut down the server
    shutdown_tx
        .send(())
        .expect("Failed to send shutdown signal");

    // Wait for the server to shut down
    server_handle.await.expect("Server task failed");
}
