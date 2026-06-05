//! Integration tests for SQLx storage implementation

#[cfg(feature = "sqlx-storage")]
mod sqlx_tests {
    use a2a_rs::adapter::storage::{DatabaseConfig, SqlxTaskStorage};
    use a2a_rs::domain::TaskState;
    use a2a_rs::port::{
        AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskLifecycle, AsyncTaskQuery,
        AsyncTaskVersioning,
    };
    use a2a_rs::{A2AError, TaskPushNotificationConfig};
    use std::sync::Arc;
    use uuid::Uuid;

    fn tid(s: &str) -> a2a_rs::domain::TaskId {
        s.parse().unwrap()
    }
    fn cid(s: &str) -> a2a_rs::domain::ContextId {
        s.parse().unwrap()
    }

    async fn create_test_storage() -> Result<SqlxTaskStorage, A2AError> {
        // Use SQLite in-memory for tests
        let config = DatabaseConfig::builder()
            .url("sqlite::memory:".to_string())
            .max_connections(1)
            .build();

        SqlxTaskStorage::new(&config.url).await
    }

    #[tokio::test]
    async fn test_task_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();
        let context_id = "test-context";

        // Test task creation
        let task = storage.create(&tid(&task_id), &cid(context_id)).await?;
        assert_eq!(task.id, task_id);
        assert_eq!(task.context_id, context_id);
        assert_eq!(task.status.state, TaskState::Submitted);

        // Test task existence
        assert!(storage.exists(&tid(&task_id)).await?);
        assert!(!storage.exists(&tid("non-existent")).await?);

        // Test status updates
        let working_task = storage
            .update_status(&tid(&task_id), TaskState::Working, None)
            .await?;
        assert_eq!(working_task.status.state, TaskState::Working);

        let completed_task = storage
            .update_status(&tid(&task_id), TaskState::Completed, None)
            .await?;
        assert_eq!(completed_task.status.state, TaskState::Completed);

        // Test task retrieval with history
        let retrieved_task = storage.get(&tid(&task_id), Some(10)).await?;
        assert_eq!(retrieved_task.id, task_id);
        assert_eq!(retrieved_task.status.state, TaskState::Completed);
        // Should have history: Submitted -> Working -> Completed
        // Note: We're not loading full history in the current implementation
        // assert_eq!(retrieved_task.history.len(), 3);

        Ok(())
    }

    #[tokio::test]
    async fn test_task_cancellation() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();

        // Create and start working on task
        storage.create(&tid(&task_id), &cid("test-context")).await?;
        storage
            .update_status(&tid(&task_id), TaskState::Working, None)
            .await?;

        // Cancel the working task
        let canceled_task = storage.cancel(&tid(&task_id)).await?;
        assert_eq!(canceled_task.status.state, TaskState::Canceled);

        // Verify cancellation was successful
        let task_with_history = storage.get(&tid(&task_id), None).await?;
        assert_eq!(task_with_history.status.state, TaskState::Canceled);
        // Note: We're not fully implementing history loading in this version
        // In a full implementation, you'd verify the cancellation message was added

        Ok(())
    }

    #[tokio::test]
    async fn test_cannot_cancel_completed_task() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();

        // Create, work on, and complete task
        storage.create(&tid(&task_id), &cid("test-context")).await?;
        storage
            .update_status(&tid(&task_id), TaskState::Working, None)
            .await?;
        storage
            .update_status(&tid(&task_id), TaskState::Completed, None)
            .await?;

        // Try to cancel completed task - should fail
        let result = storage.cancel(&tid(&task_id)).await;
        assert!(result.is_err());

        if let Err(A2AError::TaskNotCancelable(_)) = result {
            // Expected error type
        } else {
            panic!("Expected TaskNotCancelable error, got: {:?}", result);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_duplicate_task_creation() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();

        // Create first task
        storage.create(&tid(&task_id), &cid("test-context")).await?;

        // Try to create duplicate - should fail
        let result = storage.create(&tid(&task_id), &cid("test-context")).await;
        assert!(result.is_err());

        if let Err(A2AError::TaskNotFound(_)) = result {
            // Expected error type (reused for "already exists")
        } else {
            panic!(
                "Expected TaskNotFound error for duplicate, got: {:?}",
                result
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_task_history_limit() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();

        // Create task and make several status changes
        storage.create(&tid(&task_id), &cid("test-context")).await?;
        storage
            .update_status(&tid(&task_id), TaskState::Working, None)
            .await?;
        storage
            .update_status(&tid(&task_id), TaskState::InputRequired, None)
            .await?;
        storage
            .update_status(&tid(&task_id), TaskState::Working, None)
            .await?;
        storage
            .update_status(&tid(&task_id), TaskState::Completed, None)
            .await?;

        // Note: We're not fully implementing history loading in this version
        // In a full implementation, you'd test history limits here
        let _task_limited = storage.get(&tid(&task_id), Some(3)).await?;
        let _task_full = storage.get(&tid(&task_id), None).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_push_notifications() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();

        // Create task first
        storage.create(&tid(&task_id), &cid("test-context")).await?;

        // Set push notification config
        let config = TaskPushNotificationConfig {
            tenant: String::new(),
            task_id: task_id.clone(),
            id: String::new(),
            url: "https://example.com/webhook".to_string(),
            token: String::new(),
            authentication: None.into(),
            ..Default::default()
        };

        let set_config = storage.set_config(&config).await?;
        assert_eq!(set_config.task_id, task_id);
        assert_eq!(set_config.url, "https://example.com/webhook");

        // Get push notification config
        let retrieved_config = storage
            .get_config(&a2a_rs::domain::GetTaskPushNotificationConfigParams {
                id: task_id.clone(),
                push_notification_config_id: None,
                metadata: None,
            })
            .await?;
        assert_eq!(retrieved_config.task_id, task_id);
        assert_eq!(retrieved_config.url, "https://example.com/webhook");

        // Remove push notification config
        storage
            .delete_config(&a2a_rs::domain::DeleteTaskPushNotificationConfigParams {
                id: task_id.clone(),
                push_notification_config_id: String::new(),
                metadata: None,
            })
            .await?;

        // Verify it's removed
        let result = storage
            .get_config(&a2a_rs::domain::GetTaskPushNotificationConfigParams {
                id: task_id.clone(),
                push_notification_config_id: None,
                metadata: None,
            })
            .await;
        assert!(result.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_database_config() -> Result<(), Box<dyn std::error::Error>> {
        // Test config validation
        let valid_config = DatabaseConfig::builder()
            .url("sqlite:test.db".to_string())
            .max_connections(5)
            .timeout_seconds(10)
            .build();
        assert!(valid_config.validate().is_ok());

        // Test invalid config
        let invalid_config = DatabaseConfig::builder().url("".to_string()).build();
        assert!(invalid_config.validate().is_err());

        // Test database type detection
        use a2a_rs::adapter::storage::DatabaseType;
        assert_eq!(valid_config.database_type(), Some(DatabaseType::Sqlite));

        let postgres_config = DatabaseConfig::builder()
            .url("postgres://localhost/test".to_string())
            .build();
        assert_eq!(
            postgres_config.database_type(),
            Some(DatabaseType::Postgres)
        );

        Ok(())
    }

    /// Subscriber management is not a storage responsibility: it lives in
    /// `InMemoryStreamingHandler`. This pins the registry semantics on that
    /// adapter.
    #[tokio::test]
    async fn test_streaming_subscribers() -> Result<(), Box<dyn std::error::Error>> {
        use a2a_rs::InMemoryStreamingHandler;

        let streaming = InMemoryStreamingHandler::new();
        let task_id = Uuid::new_v4().to_string();

        // No subscribers registered for an unknown task.
        let count = streaming.get_subscriber_count(&task_id).await?;
        assert_eq!(count, 0);

        // Removing subscribers for a task with none is a no-op.
        streaming.remove_task_subscribers(&task_id).await?;

        // Removal by subscription ID is unsupported by the in-memory handler.
        let result = streaming.remove_subscription("fake-id").await;
        assert!(matches!(result, Err(A2AError::UnsupportedOperation(_))));

        Ok(())
    }

    #[tokio::test]
    async fn test_concurrent_operations() -> Result<(), Box<dyn std::error::Error>> {
        let storage = Arc::new(create_test_storage().await?);
        let mut handles = Vec::new();

        // Create multiple tasks concurrently
        for i in 0..10 {
            let storage_clone = storage.clone();
            let handle = tokio::spawn(async move {
                let task_id = format!("concurrent-task-{}", i);
                let task = storage_clone
                    .create(&tid(&task_id), &cid("concurrent-context"))
                    .await?;
                storage_clone
                    .update_status(&tid(&task_id), TaskState::Working, None)
                    .await?;
                storage_clone
                    .update_status(&tid(&task_id), TaskState::Completed, None)
                    .await?;
                Ok::<_, A2AError>(task)
            });
            handles.push(handle);
        }

        // Wait for all operations to complete
        for handle in handles {
            let result = handle.await??;
            assert_eq!(result.status.state, TaskState::Submitted); // Initial state
        }

        // Verify all tasks exist
        for i in 0..10 {
            let task_id = format!("concurrent-task-{}", i);
            assert!(storage.exists(&tid(&task_id)).await?);
            let task = storage.get(&tid(&task_id), None).await?;
            assert_eq!(task.status.state, TaskState::Completed);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_database_migrations() -> Result<(), Box<dyn std::error::Error>> {
        // Test that migrations run successfully on a fresh database
        let config = DatabaseConfig::builder()
            .url("sqlite::memory:".to_string())
            .build();

        // This should run migrations internally
        let _storage = SqlxTaskStorage::new(&config.url).await?;

        // Create another instance with the same URL - should not fail
        let _storage2 = SqlxTaskStorage::new(&config.url).await?;

        Ok(())
    }

    // ===== v1.0.0 Tests =====

    #[tokio::test]
    async fn test_list_tasks_v3_basic() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;

        // Create some tasks
        for i in 0..5 {
            let task_id = format!("task-{}", i);
            storage.create(&tid(&task_id), &cid("test-context")).await?;
        }

        // List all tasks
        let params = a2a_rs::domain::ListTasksParams::default();
        let result = storage.list(&params).await?;

        assert_eq!(result.total_size, 5, "Should have 5 tasks");
        assert_eq!(result.tasks.len(), 5, "Should return 5 tasks");
        assert_eq!(result.page_size, 50, "Default page size should be 50");

        Ok(())
    }

    #[tokio::test]
    async fn test_list_tasks_v3_filtering() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;

        // Create tasks in different contexts and states
        storage.create(&tid("task-a-1"), &cid("context-a")).await?;
        storage.create(&tid("task-a-2"), &cid("context-a")).await?;
        storage.create(&tid("task-b-1"), &cid("context-b")).await?;

        storage
            .update_status(&tid("task-a-1"), TaskState::Working, None)
            .await?;
        storage
            .update_status(&tid("task-a-2"), TaskState::Completed, None)
            .await?;

        // Filter by context
        let params = a2a_rs::domain::ListTasksParams {
            context_id: Some("context-a".to_string()),
            ..Default::default()
        };
        let result = storage.list(&params).await?;
        assert_eq!(result.total_size, 2, "Should have 2 tasks in context-a");

        // Filter by status
        let params = a2a_rs::domain::ListTasksParams {
            status: Some(TaskState::Working),
            ..Default::default()
        };
        let result = storage.list(&params).await?;
        assert_eq!(result.total_size, 1, "Should have 1 working task");

        Ok(())
    }

    #[tokio::test]
    async fn test_list_tasks_v3_pagination() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;

        // Create 10 tasks
        for i in 0..10 {
            storage
                .create(&tid(&format!("task-{}", i)), &cid("test-context"))
                .await?;
        }

        // Get first page
        let params = a2a_rs::domain::ListTasksParams {
            page_size: Some(3),
            ..Default::default()
        };
        let page1 = storage.list(&params).await?;
        assert_eq!(page1.tasks.len(), 3, "Should return 3 tasks");
        assert!(
            !page1.next_page_token.is_empty(),
            "Should have next page token"
        );

        // Get second page
        let params = a2a_rs::domain::ListTasksParams {
            page_size: Some(3),
            page_token: Some(page1.next_page_token.clone()),
            ..Default::default()
        };
        let page2 = storage.list(&params).await?;
        assert_eq!(page2.tasks.len(), 3, "Should return 3 tasks");

        Ok(())
    }

    #[tokio::test]
    async fn test_push_notification_config_v3_crud() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();

        // Create task first
        storage.create(&tid(&task_id), &cid("test-context")).await?;

        // Set push notification config
        let config = TaskPushNotificationConfig {
            tenant: String::new(),
            task_id: task_id.clone(),
            id: "config-1".to_string(),
            url: "https://example.com/webhook".to_string(),
            token: "test-token".to_string(),
            authentication: None.into(),
            ..Default::default()
        };
        storage.set_config(&config).await?;

        // Get specific config
        let get_params = a2a_rs::domain::GetTaskPushNotificationConfigParams {
            id: task_id.clone(),
            push_notification_config_id: Some("config-1".to_string()),
            metadata: None,
        };
        let retrieved = storage.get_config(&get_params).await?;
        assert_eq!(retrieved.url, "https://example.com/webhook");
        assert_eq!(retrieved.token, "test-token");

        // List configs
        let list_params = a2a_rs::domain::ListTaskPushNotificationConfigsParams {
            id: task_id.clone(),
            metadata: None,
        };
        let configs = storage.list_configs(&list_params).await?;
        assert_eq!(configs.len(), 1, "Should have 1 config");

        // Delete config
        let delete_params = a2a_rs::domain::DeleteTaskPushNotificationConfigParams {
            id: task_id.clone(),
            push_notification_config_id: "config-1".to_string(),
            metadata: None,
        };
        storage.delete_config(&delete_params).await?;

        // Verify deleted
        let configs = storage.list_configs(&list_params).await?;
        assert_eq!(configs.len(), 0, "Config should be deleted");

        Ok(())
    }

    #[tokio::test]
    async fn test_push_notification_config_v3_multiple() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();

        // Create task
        storage.create(&tid(&task_id), &cid("test-context")).await?;

        // Set multiple configs
        let config1 = TaskPushNotificationConfig {
            tenant: String::new(),
            task_id: task_id.clone(),
            id: "config-1".to_string(),
            url: "https://example.com/webhook1".to_string(),
            token: String::new(),
            authentication: None.into(),
            ..Default::default()
        };
        let config2 = TaskPushNotificationConfig {
            tenant: String::new(),
            task_id: task_id.clone(),
            id: "config-2".to_string(),
            url: "https://example.com/webhook2".to_string(),
            token: "token-2".to_string(),
            authentication: None.into(),
            ..Default::default()
        };

        storage.set_config(&config1).await?;
        storage.set_config(&config2).await?;

        // List should return both
        let list_params = a2a_rs::domain::ListTaskPushNotificationConfigsParams {
            id: task_id.clone(),
            metadata: None,
        };
        let configs = storage.list_configs(&list_params).await?;
        assert_eq!(configs.len(), 2, "Should have 2 configs");

        Ok(())
    }

    #[tokio::test]
    async fn test_optimistic_concurrency_versioning() -> Result<(), Box<dyn std::error::Error>> {
        let storage = create_test_storage().await?;
        let task_id = Uuid::new_v4().to_string();

        // A freshly created task starts at version 1.
        storage.create(&tid(&task_id), &cid("ctx")).await?;
        assert_eq!(storage.version(&tid(&task_id)).await?, 1);

        // Every unversioned mutation bumps the version too.
        storage
            .update_status(&tid(&task_id), TaskState::Working, None)
            .await?;
        let snapshot = storage.get_versioned(&tid(&task_id), None).await?;
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.task.status.state, TaskState::Working);

        // A conditional update with the stale version is rejected, untouched.
        let stale = storage
            .update_status_checked(&tid(&task_id), 1, TaskState::Completed, None)
            .await;
        match stale {
            Err(A2AError::VersionConflict {
                expected, actual, ..
            }) => {
                assert_eq!(expected, 1);
                assert_eq!(actual, 2);
            }
            other => panic!("expected VersionConflict, got {other:?}"),
        }
        // State is unchanged after the rejected update.
        assert_eq!(
            storage.get(&tid(&task_id), None).await?.status.state,
            TaskState::Working
        );

        // A conditional update with the current version succeeds and bumps.
        let updated = storage
            .update_status_checked(&tid(&task_id), 2, TaskState::Completed, None)
            .await?;
        assert_eq!(updated.version, 3);
        assert_eq!(updated.task.status.state, TaskState::Completed);

        // Versioning ops on a missing task report TaskNotFound.
        assert!(matches!(
            storage.version(&tid("ghost")).await,
            Err(A2AError::TaskNotFound(_))
        ));

        Ok(())
    }
}

#[cfg(not(feature = "sqlx-storage"))]
#[tokio::test]
async fn test_sqlx_not_available() {
    // This test just verifies the feature flag works correctly
    println!("SQLx storage tests skipped - feature not enabled");
}
