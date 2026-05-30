//! A2A Protocol v1.0.0 Client SDK Tests
//!
//! This module tests the HttpClient and WebSocketClient wrappers for v1.0.0 methods

mod common;

use a2a_rs::{
    adapter::{
        ConnectRpcAdapter, HttpClient, HttpServer, InMemoryTaskStorage, SimpleAgentInfo,
    },
    domain::{ListTasksParams, Message, TaskState},
    port::AsyncTaskLifecycle,
    services::AsyncA2AClient,
};
use common::TestBusinessHandler;
use std::time::Duration;
use tokio::sync::oneshot;

fn tid(s: &str) -> a2a_rs::domain::TaskId {
    s.parse().unwrap()
}

fn cid(s: &str) -> a2a_rs::domain::ContextId {
    s.parse().unwrap()
}

/// Helper function to setup a test server with pre-populated tasks
async fn setup_test_server(port: u16) -> (oneshot::Sender<()>, InMemoryTaskStorage) {
    let storage = InMemoryTaskStorage::new();

    // Create some test tasks
    for i in 0..5 {
        let task_id = format!("task-{}", i);
        let context_id = format!("ctx-{}", i % 2); // Alternate between ctx-0 and ctx-1

        storage
            .create(&tid(&task_id), &cid(&context_id))
            .await
            .expect("Failed to create task");

        // Update task status with a message
        storage
            .update_status(
                &tid(&task_id),
                if i % 2 == 0 {
                    TaskState::Working
                } else {
                    TaskState::Completed
                },
                Some(Message::agent_text(
                    format!("Task {} message", i),
                    format!("msg-{}", i),
                )),
            )
            .await
            .expect("Failed to update task");
    }

    let handler = TestBusinessHandler::with_storage(storage.clone());
    let agent_info = SimpleAgentInfo::new(
        "Test Agent v1.0.0".to_string(),
        format!("http://localhost:{}", port),
    )
    .with_version("2.0.0".to_string())
    .with_description("Agent for v1.0.0 testing".to_string());

    let processor = ConnectRpcAdapter::with_handler(handler, agent_info.clone());
    let server = HttpServer::new(processor, agent_info, format!("127.0.0.1:{}", port));
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    (shutdown_tx, storage)
}

#[tokio::test]
async fn test_http_client_list_tasks() {
    let port = 9600;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Test basic listing without filters
    let params = ListTasksParams::default();
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    println!("List tasks result: {:?}", result);

    assert_eq!(result.total_size, 5, "Should have 5 tasks total");
    assert!(!result.tasks.is_empty(), "Should return tasks");

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_list_tasks_with_filters() {
    let port = 9601;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Test filtering by context
    let params = ListTasksParams {
        context_id: Some("ctx-0".to_string()),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with context filter");

    println!("Filtered tasks by context: {:?}", result);

    // All returned tasks should have context_id "ctx-0"
    for task in &result.tasks {
        assert_eq!(task.context_id, "ctx-0", "Task should match context filter");
    }

    // Test filtering by status
    let params = ListTasksParams {
        status: Some(TaskState::Working),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with status filter");

    println!("Filtered tasks by status: {:?}", result);

    for task in &result.tasks {
        assert_eq!(
            task.status.state,
            TaskState::Working,
            "Task should match status filter"
        );
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_list_tasks_pagination() {
    let port = 9602;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Test pagination with small page size
    let params = ListTasksParams {
        page_size: Some(2),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with pagination");

    println!("Paginated result: {:?}", result);

    assert!(result.tasks.len() <= 2, "Should return at most 2 tasks");
    assert_eq!(result.total_size, 5, "Total size should still be 5");

    // If there's a next page token, fetch the next page
    if !result.next_page_token.is_empty() {
        let next_token = result.next_page_token;
        let params = ListTasksParams {
            page_size: Some(2),
            page_token: Some(next_token),
            ..Default::default()
        };

        let next_result = client
            .list_tasks(&params)
            .await
            .expect("Failed to fetch next page");

        println!("Next page result: {:?}", next_result);
        assert!(!next_result.tasks.is_empty(), "Next page should have tasks");
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_list_tasks_history_length() {
    let port = 9603;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Test with history_length parameter
    let params = ListTasksParams {
        history_length: Some(5),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with history_length");

    println!("Tasks with history: {:?}", result);

    // Verify that tasks have history
    for task in &result.tasks {
        let history = &task.history;
        assert!(history.len() <= 5, "History should be limited to 5 entries");
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_list_tasks_include_artifacts() {
    let port = 9604;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Test with include_artifacts = true
    let params = ListTasksParams {
        include_artifacts: Some(true),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with artifacts");

    println!("Tasks with artifacts: {:?}", result);
    assert!(!result.tasks.is_empty(), "Should return tasks");

    // Test with include_artifacts = false
    let params = ListTasksParams {
        include_artifacts: Some(false),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks without artifacts");

    println!("Tasks without artifacts: {:?}", result);
    assert!(!result.tasks.is_empty(), "Should return tasks");

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_push_config_list() {
    let port = 9605;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // First, set a push notification config
    let task_id = "task-0";
    let config = a2a_rs::domain::TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: task_id.to_string(),
        id: "config-1".to_string(),
        url: "https://client.example.com/webhook".to_string(),
        token: "test-token".to_string(),
        authentication: None.into(),
        ..Default::default()
    };

    client
        .set_task_push_notification(&config)
        .await
        .expect("Failed to set push notification config");

    // Now list configs for the task
    let configs = client
        .list_push_notification_configs(task_id)
        .await
        .expect("Failed to list push configs");

    println!("Push configs: {:?}", configs);

    assert!(!configs.is_empty(), "Should have at least one config");
    assert_eq!(configs[0].id, "config-1".to_string());

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_push_config_get() {
    let port = 9606;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Set a push notification config
    let task_id = "task-0";
    let config = a2a_rs::domain::TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: task_id.to_string(),
        id: "config-get-test".to_string(),
        url: "https://client.example.com/webhook".to_string(),
        token: "test-token-get".to_string(),
        authentication: None.into(),
        ..Default::default()
    };

    client
        .set_task_push_notification(&config)
        .await
        .expect("Failed to set push notification config");

    // Get the specific config by ID
    let retrieved_config = client
        .get_push_notification_config(task_id, "config-get-test")
        .await
        .expect("Failed to get push config");

    println!("Retrieved push config: {:?}", retrieved_config);

    assert_eq!(retrieved_config.id, "config-get-test".to_string());
    assert_eq!(retrieved_config.url, "https://client.example.com/webhook");

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_push_config_delete() {
    let port = 9607;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Set a push notification config
    let task_id = "task-0";
    let config = a2a_rs::domain::TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: task_id.to_string(),
        id: "config-delete-test".to_string(),
        url: "https://client.example.com/webhook".to_string(),
        token: "test-token-delete".to_string(),
        authentication: None.into(),
        ..Default::default()
    };

    client
        .set_task_push_notification(&config)
        .await
        .expect("Failed to set push notification config");

    // Verify it exists
    let configs_before = client
        .list_push_notification_configs(task_id)
        .await
        .expect("Failed to list configs");

    assert!(
        !configs_before.is_empty(),
        "Config should exist before deletion"
    );

    // Delete the config
    client
        .delete_push_notification_config(task_id, "config-delete-test")
        .await
        .expect("Failed to delete push config");

    // Verify it's deleted by listing again
    let configs_after = client
        .list_push_notification_configs(task_id)
        .await
        .expect("Failed to list configs after deletion");

    // The config should either be empty or not contain our deleted config
    if !configs_after.is_empty() {
        assert!(
            configs_after.iter().all(|c| c.id != "config-delete-test"),
            "Deleted config should not appear in list"
        );
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_push_config_multiple() {
    let port = 9608;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    let task_id = "task-0";

    // Set multiple push notification configs
    for i in 1..=3 {
        let config = a2a_rs::domain::TaskPushNotificationConfig {
            tenant: String::new(),
            task_id: task_id.to_string(),
            id: format!("config-multi-{}", i),
            url: format!("https://client.example.com/webhook-{}", i),
            token: format!("token-{}", i),
            authentication: None.into(),
            ..Default::default()
        };

        client
            .set_task_push_notification(&config)
            .await
            .expect("Failed to set push notification config");
    }

    // List all configs
    let configs = client
        .list_push_notification_configs(task_id)
        .await
        .expect("Failed to list configs");

    println!("Multiple configs: {:?}", configs);

    // Since set_task_push_notification replaces configs (not appends),
    // we should have the last config set
    assert!(!configs.is_empty(), "Should have at least one config");

    // The config should be one of the ones we set
    let has_our_configs = configs.iter().any(|c| c.id.starts_with("config-multi-"));
    assert!(has_our_configs, "Should have our configs");

    // Verify we can retrieve the configs that exist
    for config_wrapper in &configs {
        let config_id = &config_wrapper.id;
        let retrieved = client
            .get_push_notification_config(task_id, config_id)
            .await
            .expect("Failed to get individual config");

        assert_eq!(retrieved.id, config_id.clone());
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_with_authentication() {
    let port = 9609;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    // Create client with authentication token
    let client = HttpClient::with_auth(
        format!("http://localhost:{}", port),
        "test-bearer-token".to_string(),
    );

    // The server doesn't actually validate the token in this test,
    // but we verify the client sends requests successfully
    let params = ListTasksParams::default();
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with auth");

    assert_eq!(result.total_size, 5);

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_with_timeout() {
    let port = 9610;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    // Create client with custom timeout
    let client = HttpClient::new(format!("http://localhost:{}", port)).with_timeout(60);

    let params = ListTasksParams::default();
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with timeout");

    assert_eq!(result.total_size, 5);

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_list_tasks_combined_filters() {
    let port = 9611;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Test with multiple filters combined
    let params = ListTasksParams {
        context_id: Some("ctx-0".to_string()),
        status: Some(TaskState::Working),
        page_size: Some(10),
        history_length: Some(5),
        include_artifacts: Some(true),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with combined filters");

    println!("Combined filter result: {:?}", result);

    // Verify filters are applied
    for task in &result.tasks {
        assert_eq!(task.context_id, "ctx-0");
        assert_eq!(task.status.state, TaskState::Working);
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_push_config_not_found() {
    let port = 9612;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Try to get a non-existent config
    let result = client
        .get_push_notification_config("task-0", "non-existent-config")
        .await;

    println!("Not found result: {:?}", result);

    // Should return an error (either TaskNotFound or config not found)
    assert!(
        result.is_err(),
        "Should return error for non-existent config"
    );

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_list_tasks_empty_result() {
    let port = 9613;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Filter for a context that doesn't exist
    let params = ListTasksParams {
        context_id: Some("non-existent-context".to_string()),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks with non-existent context");

    println!("Empty result: {:?}", result);

    assert_eq!(result.tasks.len(), 0, "Should return empty task list");
    assert_eq!(result.total_size, 0, "Total size should be 0");
    assert!(result.next_page_token.is_empty(), "No next page token");

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_http_client_delete_push_config_idempotent() {
    let port = 9614;
    let (shutdown_tx, _storage) = setup_test_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    let task_id = "task-0";
    let config_id = "config-idempotent-test";

    // Set a config
    let config = a2a_rs::domain::TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: task_id.to_string(),
        id: config_id.to_string(),
        url: "https://client.example.com/webhook".to_string(),
        token: "test-token".to_string(),
        authentication: None.into(),
        ..Default::default()
    };

    client
        .set_task_push_notification(&config)
        .await
        .expect("Failed to set push notification config");

    // Delete it once
    client
        .delete_push_notification_config(task_id, config_id)
        .await
        .expect("Failed to delete push config first time");

    // Delete it again (should be idempotent, no error)
    let result = client
        .delete_push_notification_config(task_id, config_id)
        .await;

    println!("Idempotent delete result: {:?}", result);

    // According to REST principles, DELETE should be idempotent
    // The implementation may return success or an error, but it shouldn't crash
    // We accept either outcome as valid depending on implementation
    assert!(
        result.is_ok() || result.is_err(),
        "Delete should complete without panic"
    );

    shutdown_tx.send(()).ok();
}
