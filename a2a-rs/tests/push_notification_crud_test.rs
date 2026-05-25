//! Integration tests for push notification config CRUD endpoints (v1.0.0)
//!
//! Tests the spec-compliant implementation of:
//! - tasks/pushNotificationConfig/list
//! - tasks/pushNotificationConfig/get
//! - tasks/pushNotificationConfig/delete

#![cfg(all(feature = "http-client", feature = "http-server"))]

mod common;

use a2a_rs::{
    TaskPushNotificationConfig,
    adapter::{
        DefaultRequestProcessor, HttpClient, HttpServer, InMemoryTaskStorage, SimpleAgentInfo,
    },
    port::{AsyncNotificationManager, AsyncTaskManager},
    services::AsyncA2AClient,
};
use common::TestBusinessHandler;
use std::time::Duration;
use tokio::sync::oneshot;

/// Helper to set up a server with a task
async fn setup_server(port: u16) -> (oneshot::Sender<()>, InMemoryTaskStorage) {
    let storage = InMemoryTaskStorage::new();
    let storage_clone = storage.clone();

    let handler = TestBusinessHandler::with_storage(storage);

    let test_agent_info = SimpleAgentInfo::new(
        "test-agent".to_string(),
        format!("http://localhost:{}", port),
    );

    let processor = DefaultRequestProcessor::with_handler(handler, test_agent_info);

    let agent_info = SimpleAgentInfo::new(
        "Push Config Test Agent".to_string(),
        format!("http://localhost:{}", port),
    );

    let server = HttpServer::new(processor, agent_info, format!("127.0.0.1:{}", port));

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    (shutdown_tx, storage_clone)
}

#[tokio::test]
async fn test_list_push_notification_configs_empty() {
    let port = 9070;
    let (shutdown, storage) = setup_server(port).await;

    // Create a task WITHOUT any push notification config
    storage
        .create_task("task_no_configs", "test_context")
        .await
        .unwrap();

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // List configs for task with no configs
    let result = client
        .list_push_notification_configs("task_no_configs")
        .await;

    shutdown.send(()).ok();

    // Verify successful response with empty array
    assert!(result.is_ok(), "List should succeed even with no configs");
    let configs = result.unwrap();
    assert_eq!(configs.len(), 0, "Should have 0 configs");
}

#[tokio::test]
async fn test_set_and_list_push_notification_config() {
    let port = 9071;
    let (shutdown, storage) = setup_server(port).await;

    // Create a task
    storage
        .create_task("task_with_config", "test_context")
        .await
        .unwrap();

    // Set a push notification config using the storage API
    let config = TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: "task_with_config".to_string(),
        id: "config_1".to_string(),
        url: "https://example.com/webhook1".to_string(),
        token: "secret_token_123".to_string(),
        authentication: None.into(),
        ..Default::default()
    };

    storage.set_task_notification(&config).await.unwrap();

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // List configs
    let result = client
        .list_push_notification_configs("task_with_config")
        .await;

    shutdown.send(()).ok();

    // Verify successful response
    assert!(
        result.is_ok(),
        "List configs should succeed: {:?}",
        result.err()
    );
    let configs = result.unwrap();

    // Should have 1 config (current implementation supports only 1 per task)
    assert_eq!(configs.len(), 1, "Should have 1 config");

    // Verify config details
    let returned_config = &configs[0];
    assert_eq!(returned_config.task_id, "task_with_config");
    assert_eq!(returned_config.id, "config_1".to_string());
    assert_eq!(returned_config.url, "https://example.com/webhook1");
    assert_eq!(returned_config.token, "secret_token_123".to_string());
}

#[tokio::test]
async fn test_get_push_notification_config() {
    let port = 9072;
    let (shutdown, storage) = setup_server(port).await;

    // Create a task
    storage
        .create_task("task_get_config", "test_context")
        .await
        .unwrap();

    // Set a push notification config
    let config = TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: "task_get_config".to_string(),
        id: "config_abc".to_string(),
        url: "https://example.com/notifications".to_string(),
        token: "bearer_token_xyz".to_string(),
        authentication: None.into(),
        ..Default::default()
    };

    storage.set_task_notification(&config).await.unwrap();

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Get the config (note: pushNotificationConfigId is optional, current impl ignores it)
    let result = client
        .get_push_notification_config("task_get_config", "config_abc")
        .await;

    shutdown.send(()).ok();

    // Verify successful response
    assert!(
        result.is_ok(),
        "Get config should succeed: {:?}",
        result.err()
    );
    let returned_config = result.unwrap();

    // Verify config details
    assert_eq!(returned_config.task_id, "task_get_config");
    assert_eq!(returned_config.id, "config_abc".to_string());
    assert_eq!(returned_config.url, "https://example.com/notifications");
    assert_eq!(returned_config.token, "bearer_token_xyz".to_string());
}

#[tokio::test]
async fn test_get_push_notification_config_not_found() {
    let port = 9073;
    let (shutdown, storage) = setup_server(port).await;

    // Create a task WITHOUT a config
    storage
        .create_task("task_no_config", "test_context")
        .await
        .unwrap();

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Try to get non-existent config
    let result = client
        .get_push_notification_config("task_no_config", "nonexistent")
        .await;

    shutdown.send(()).ok();

    // Verify error is returned
    assert!(result.is_err(), "Get nonexistent config should fail");
}

#[tokio::test]
async fn test_delete_push_notification_config() {
    let port = 9074;
    let (shutdown, storage) = setup_server(port).await;

    // Create a task
    storage
        .create_task("task_delete", "test_context")
        .await
        .unwrap();

    // Set a push notification config
    let config = TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: "task_delete".to_string(),
        id: "config_to_delete".to_string(),
        url: "https://example.com/webhook".to_string(),
        token: String::new(),
        authentication: None.into(),
        ..Default::default()
    };

    storage.set_task_notification(&config).await.unwrap();

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Verify config exists
    let configs_before = client
        .list_push_notification_configs("task_delete")
        .await
        .unwrap();
    assert_eq!(
        configs_before.len(),
        1,
        "Should have 1 config before deletion"
    );

    // Delete the config
    let result = client
        .delete_push_notification_config("task_delete", "config_to_delete")
        .await;

    // Verify successful deletion
    assert!(
        result.is_ok(),
        "Delete config should succeed: {:?}",
        result.err()
    );

    // Verify config is gone
    let configs_after = client
        .list_push_notification_configs("task_delete")
        .await
        .unwrap();

    shutdown.send(()).ok();

    assert_eq!(
        configs_after.len(),
        0,
        "Should have 0 configs after deletion"
    );
}

#[tokio::test]
async fn test_delete_nonexistent_push_config() {
    let port = 9075;
    let (shutdown, storage) = setup_server(port).await;

    // Create a task without config
    storage
        .create_task("task_empty", "test_context")
        .await
        .unwrap();

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Try to delete non-existent config (DELETE is idempotent, so this should succeed)
    let result = client
        .delete_push_notification_config("task_empty", "nonexistent")
        .await;

    shutdown.send(()).ok();

    // Idempotent delete: succeeds even if config doesn't exist
    assert!(
        result.is_ok(),
        "Delete is idempotent, should succeed even for nonexistent config"
    );
}

#[tokio::test]
async fn test_delete_push_config_from_nonexistent_task() {
    let port = 9076;
    let (shutdown, _storage) = setup_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Try to delete config from non-existent task (DELETE is idempotent, so this should succeed)
    let result = client
        .delete_push_notification_config("nonexistent_task", "config_1")
        .await;

    shutdown.send(()).ok();

    // Idempotent delete: succeeds even if task doesn't exist
    assert!(
        result.is_ok(),
        "Delete is idempotent, should succeed even for nonexistent task"
    );
}

#[tokio::test]
async fn test_push_notification_config_with_authentication() {
    let port = 9077;
    let (shutdown, storage) = setup_server(port).await;

    // Create a task
    storage
        .create_task("task_with_auth", "test_context")
        .await
        .unwrap();

    // Set a push notification config with authentication
    let config = TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: "task_with_auth".to_string(),
        id: "config_auth".to_string(),
        url: "https://example.com/secure-webhook".to_string(),
        token: "validation_token".to_string(),
        authentication: Some(a2a_rs::domain::PushNotificationAuthenticationInfo {
            scheme: "Bearer".to_string(),
            credentials: "secret_credentials".to_string(),
            ..Default::default()
        })
        .into(),
        ..Default::default()
    };

    storage.set_task_notification(&config).await.unwrap();

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // List and verify authentication is preserved
    let result = client
        .list_push_notification_configs("task_with_auth")
        .await;

    shutdown.send(()).ok();

    assert!(result.is_ok());
    let configs = result.unwrap();
    assert_eq!(configs.len(), 1);

    let returned_config = &configs[0];
    assert!(returned_config.authentication.as_option().is_some());

    let auth = returned_config.authentication.as_option().unwrap();
    assert_eq!(auth.scheme, "Bearer".to_string());
    assert_eq!(auth.credentials, "secret_credentials".to_string());
}

#[tokio::test]
async fn test_list_configs_for_nonexistent_task() {
    let port = 9078;
    let (shutdown, _storage) = setup_server(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Try to list configs for non-existent task
    let result = client
        .list_push_notification_configs("nonexistent_task")
        .await;

    shutdown.send(()).ok();

    // Should succeed with empty array (spec doesn't require task to exist for listing)
    assert!(
        result.is_ok(),
        "List should succeed even for nonexistent task"
    );
    let configs = result.unwrap();
    assert_eq!(configs.len(), 0, "Should return empty array");
}
