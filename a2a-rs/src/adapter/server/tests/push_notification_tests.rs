#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use crate::adapter::server::push_notification::{
        PushNotificationRegistry, PushNotificationSender,
    };
    use crate::domain::{
        A2AError, Message, Part, PushNotificationConfig, TaskArtifactUpdateEvent,
        TaskStatusUpdateEvent,
    };

    // Mock push notification sender for testing
    struct MockPushNotificationSender {
        status_updates: Arc<Mutex<Vec<String>>>,
        artifact_updates: Arc<Mutex<Vec<String>>>,
        should_fail: bool,
    }

    impl MockPushNotificationSender {
        fn new(should_fail: bool) -> Self {
            Self {
                status_updates: Arc::new(Mutex::new(Vec::new())),
                artifact_updates: Arc::new(Mutex::new(Vec::new())),
                should_fail,
            }
        }

        fn get_status_updates(&self) -> Vec<String> {
            self.status_updates.lock().unwrap().clone()
        }

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
            if self.should_fail {
                return Err(A2AError::Internal("Simulated failure".to_string()));
            }

            // Record the update
            let update = format!("Status update for task {} to URL {}", event.task_id, config.url);
            self.status_updates.lock().unwrap().push(update);

            Ok(())
        }

        async fn send_artifact_update(
            &self,
            config: &PushNotificationConfig,
            event: &TaskArtifactUpdateEvent,
        ) -> Result<(), A2AError> {
            if self.should_fail {
                return Err(A2AError::Internal("Simulated failure".to_string()));
            }

            // Record the update
            let update = format!(
                "Artifact update for task {} to URL {}",
                event.task_id, config.url
            );
            self.artifact_updates.lock().unwrap().push(update);

            Ok(())
        }
    }

    #[tokio::test]
    async fn test_push_notification_registry() {
        // Create a mock sender
        let mock_sender = MockPushNotificationSender::new(false);
        let status_updates = mock_sender.status_updates.clone();
        let artifact_updates = mock_sender.artifact_updates.clone();

        // Create the registry
        let registry = PushNotificationRegistry::new(mock_sender);

        // Register a push notification
        let task_id = "test-task-1";
        let config = PushNotificationConfig {
            url: "https://example.com/push".to_string(),
            token: Some("secret-token".to_string()),
            authentication: None,
        };

        registry.register(task_id, config.clone()).await.unwrap();

        // Create some events
        let status_event = TaskStatusUpdateEvent {
            task_id: task_id.to_string(),
            context_id: "test-context".to_string(),
            kind: "status-update".to_string(),
            status: crate::domain::TaskStatus {
                state: crate::domain::TaskState::Working,
                message: Some(Message::agent_text("Working on it...".to_string(), "msg1".to_string())),
                timestamp: Some(chrono::Utc::now()),
            },
            final_: false,
            metadata: None,
        };

        let artifact_event = TaskArtifactUpdateEvent {
            task_id: task_id.to_string(),
            context_id: "test-context".to_string(),
            kind: "artifact-update".to_string(),
            artifact: crate::domain::Artifact {
                artifact_id: "artifact-1".to_string(),
                name: Some("test-artifact".to_string()),
                description: Some("A test artifact".to_string()),
                parts: vec![Part::text("Artifact content".to_string())],
                metadata: None,
            },
            append: None,
            last_chunk: Some(true),
            metadata: None,
        };

        // Send the notifications
        registry
            .send_status_update(task_id, &status_event)
            .await
            .unwrap();
        registry
            .send_artifact_update(task_id, &artifact_event)
            .await
            .unwrap();

        // Check that updates were recorded
        assert_eq!(status_updates.lock().unwrap().len(), 1);
        assert_eq!(artifact_updates.lock().unwrap().len(), 1);

        // Unregister
        registry.unregister(task_id).await.unwrap();

        // Send again - should not fail but also not record another update
        registry
            .send_status_update(task_id, &status_event)
            .await
            .unwrap();
        registry
            .send_artifact_update(task_id, &artifact_event)
            .await
            .unwrap();

        // Check that no new updates were recorded
        assert_eq!(status_updates.lock().unwrap().len(), 1);
        assert_eq!(artifact_updates.lock().unwrap().len(), 1);

        // Test get_config
        let result = registry.get_config(task_id).await.unwrap();
        assert!(result.is_none());

        // Register again and test get_config
        registry.register(task_id, config.clone()).await.unwrap();
        let result = registry.get_config(task_id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().url, config.url);
    }

    #[tokio::test]
    async fn test_push_notification_failure() {
        // Create a mock sender that fails
        let mock_sender = MockPushNotificationSender::new(true);

        // Create the registry
        let registry = PushNotificationRegistry::new(mock_sender);

        // Register a push notification
        let task_id = "test-task-1";
        let config = PushNotificationConfig {
            url: "https://example.com/push".to_string(),
            token: Some("secret-token".to_string()),
            authentication: None,
        };

        registry.register(task_id, config).await.unwrap();

        // Create a status event
        let status_event = TaskStatusUpdateEvent {
            task_id: task_id.to_string(),
            context_id: "test-context".to_string(),
            kind: "status-update".to_string(),
            status: crate::domain::TaskStatus {
                state: crate::domain::TaskState::Working,
                message: Some(Message::agent_text("Working on it...".to_string(), "msg1".to_string())),
                timestamp: Some(chrono::Utc::now()),
            },
            final_: false,
            metadata: None,
        };

        // Send should return an error
        let result = registry.send_status_update(task_id, &status_event).await;
        assert!(result.is_err());
    }
}
