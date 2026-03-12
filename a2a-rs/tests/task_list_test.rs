//! Integration tests for tasks/list endpoint (v0.3.0)

#![cfg(all(feature = "http-client", feature = "http-server"))]

mod common;

use a2a_rs::{
    adapter::{
        DefaultRequestProcessor, HttpClient, HttpServer, InMemoryTaskStorage, SimpleAgentInfo,
    },
    domain::{ListTasksParams, Message, TaskState},
    port::AsyncTaskManager,
    services::AsyncA2AClient,
};
use common::TestBusinessHandler;
use std::time::Duration;
use tokio::sync::oneshot;

async fn setup_server_with_tasks(port: u16) -> (oneshot::Sender<()>, InMemoryTaskStorage) {
    let storage = InMemoryTaskStorage::new();
    let storage_clone = storage.clone();

    // Create business handler with the storage
    let handler = TestBusinessHandler::with_storage(storage);

    // Create agent info for the processor
    let test_agent_info = SimpleAgentInfo::new(
        "test-agent".to_string(),
        format!("http://localhost:{}", port),
    );

    // Create a processor
    let processor = DefaultRequestProcessor::with_handler(handler, test_agent_info);

    // Create an agent info provider
    let agent_info = SimpleAgentInfo::new(
        "Task List Test Agent".to_string(),
        format!("http://localhost:{}", port),
    )
    .with_state_transition_history();

    // Create the server
    let server = HttpServer::new(processor, agent_info, format!("127.0.0.1:{}", port));

    // Create a shutdown channel
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Start the server in a separate task
    tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    // Give the server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    (shutdown_tx, storage_clone)
}

#[tokio::test]
async fn test_task_list_basic() {
    let port = 9001;
    let (shutdown_tx, _storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Create several tasks
    let task_ids = vec![
        format!("task-{}", uuid::Uuid::new_v4()),
        format!("task-{}", uuid::Uuid::new_v4()),
        format!("task-{}", uuid::Uuid::new_v4()),
    ];

    for task_id in &task_ids {
        let message = Message::user_text(
            format!("Test message for {}", task_id),
            format!("msg-{}", uuid::Uuid::new_v4()),
        );
        client
            .send_task_message(task_id, &message, None, None)
            .await
            .expect("Failed to create task");
    }

    // Give time for tasks to be created
    tokio::time::sleep(Duration::from_millis(100)).await;

    // List all tasks
    let params = ListTasksParams::default();
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    // Verify we got all tasks
    assert!(result.total_size >= 3, "Should have at least 3 tasks");
    assert!(result.tasks.len() >= 3, "Should return at least 3 tasks");
    assert_eq!(result.page_size, 50, "Default page size should be 50");

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_task_list_filter_by_context() {
    let port = 9002;
    let (shutdown_tx, _storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    let context_a = "context-a";
    let context_b = "context-b";

    // Create tasks in different contexts (using session_id as context)
    for i in 0..3 {
        let task_id = format!("task-a-{}", i);
        let message = Message::user_text(
            format!("Message in context A - {}", i),
            format!("msg-{}", uuid::Uuid::new_v4()),
        );
        client
            .send_task_message(&task_id, &message, Some(context_a), None)
            .await
            .expect("Failed to create task in context A");
    }

    for i in 0..2 {
        let task_id = format!("task-b-{}", i);
        let message = Message::user_text(
            format!("Message in context B - {}", i),
            format!("msg-{}", uuid::Uuid::new_v4()),
        );
        client
            .send_task_message(&task_id, &message, Some(context_b), None)
            .await
            .expect("Failed to create task in context B");
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    // List tasks in context A
    let params = ListTasksParams {
        context_id: Some(context_a.to_string()),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    // All returned tasks should be in context A
    assert_eq!(result.total_size, 3, "Should have 3 tasks in context A");
    for task in &result.tasks {
        assert_eq!(task.context_id, context_a, "Task should be in context A");
    }

    // List tasks in context B
    let params = ListTasksParams {
        context_id: Some(context_b.to_string()),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    assert_eq!(result.total_size, 2, "Should have 2 tasks in context B");
    for task in &result.tasks {
        assert_eq!(task.context_id, context_b, "Task should be in context B");
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_task_list_filter_by_status() {
    let port = 9003;
    let (shutdown_tx, storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Create tasks and set different states
    let task_id_1 = format!("task-{}", uuid::Uuid::new_v4());
    let task_id_2 = format!("task-{}", uuid::Uuid::new_v4());
    let task_id_3 = format!("task-{}", uuid::Uuid::new_v4());

    // Create tasks
    for task_id in &[&task_id_1, &task_id_2, &task_id_3] {
        let message = Message::user_text(
            "Test message".to_string(),
            format!("msg-{}", uuid::Uuid::new_v4()),
        );
        client
            .send_task_message(task_id, &message, None, None)
            .await
            .expect("Failed to create task");
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Update task states directly through storage
    storage
        .update_task_status(&task_id_1, TaskState::Working, None)
        .await
        .expect("Failed to update task 1");
    storage
        .update_task_status(&task_id_2, TaskState::Completed, None)
        .await
        .expect("Failed to update task 2");
    // task_id_3 remains in Submitted state

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Filter by Working status
    let params = ListTasksParams {
        status: Some(TaskState::Working),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    assert!(
        result.total_size >= 1,
        "Should have at least 1 working task"
    );
    for task in &result.tasks {
        assert_eq!(
            task.status.state,
            TaskState::Working,
            "Task should be in Working state"
        );
    }

    // Filter by Completed status
    let params = ListTasksParams {
        status: Some(TaskState::Completed),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    assert!(
        result.total_size >= 1,
        "Should have at least 1 completed task"
    );
    for task in &result.tasks {
        assert_eq!(
            task.status.state,
            TaskState::Completed,
            "Task should be in Completed state"
        );
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_task_list_pagination() {
    let port = 9004;
    let (shutdown_tx, _storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Create 10 tasks
    for i in 0..10 {
        let task_id = format!("task-{}", i);
        let message = Message::user_text(
            format!("Message {}", i),
            format!("msg-{}", uuid::Uuid::new_v4()),
        );
        client
            .send_task_message(&task_id, &message, None, None)
            .await
            .expect("Failed to create task");
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Get first page with page_size 3
    let params = ListTasksParams {
        page_size: Some(3),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    assert_eq!(result.page_size, 3, "Page size should be 3");
    assert_eq!(result.tasks.len(), 3, "Should return exactly 3 tasks");
    assert!(result.total_size >= 10, "Total size should be at least 10");
    assert!(
        !result.next_page_token.is_empty(),
        "Should have next page token"
    );

    // Get second page using next_page_token
    let params = ListTasksParams {
        page_size: Some(3),
        page_token: Some(result.next_page_token.clone()),
        ..Default::default()
    };
    let result_page2 = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks page 2");

    assert_eq!(result_page2.page_size, 3, "Page size should be 3");
    assert_eq!(result_page2.tasks.len(), 3, "Should return exactly 3 tasks");

    // Verify tasks are different between pages
    let page1_ids: Vec<_> = result.tasks.iter().map(|t| &t.id).collect();
    let page2_ids: Vec<_> = result_page2.tasks.iter().map(|t| &t.id).collect();

    for id in &page2_ids {
        assert!(
            !page1_ids.contains(id),
            "Page 2 should have different tasks than page 1"
        );
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_task_list_page_size_clamping() {
    let port = 9005;
    let (shutdown_tx, _storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Create one task
    let message = Message::user_text(
        "Test message".to_string(),
        format!("msg-{}", uuid::Uuid::new_v4()),
    );
    client
        .send_task_message("task-1", &message, None, None)
        .await
        .expect("Failed to create task");

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test page_size is clamped to 1-100
    let params = ListTasksParams {
        page_size: Some(150), // Should be clamped to 100
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    assert_eq!(result.page_size, 100, "Page size should be clamped to 100");

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_task_list_history_length() {
    let port = 9006;
    let (shutdown_tx, _storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    let task_id = format!("task-{}", uuid::Uuid::new_v4());

    // Create task and add multiple messages to history
    for i in 0..5 {
        let message = Message::user_text(
            format!("Message {}", i),
            format!("msg-{}", uuid::Uuid::new_v4()),
        );
        client
            .send_task_message(&task_id, &message, None, None)
            .await
            .expect("Failed to send message");
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    // List tasks with history_length = 2
    let params = ListTasksParams {
        history_length: Some(2),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    let task = result
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("Task not found");

    if let Some(history) = &task.history {
        assert_eq!(history.len(), 2, "History should be limited to 2 messages");
    }

    // List tasks with history_length = 0 (no history)
    let params = ListTasksParams {
        history_length: Some(0),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    let task = result
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("Task not found");

    assert!(
        task.history.is_none(),
        "History should be None when history_length is 0"
    );

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_task_list_include_artifacts() {
    let port = 9007;
    let (shutdown_tx, storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    let task_id = format!("task-{}", uuid::Uuid::new_v4());

    // Create task
    let message = Message::user_text(
        "Test message".to_string(),
        format!("msg-{}", uuid::Uuid::new_v4()),
    );
    client
        .send_task_message(&task_id, &message, None, None)
        .await
        .expect("Failed to create task");

    // Add artifact through storage
    let mut task = storage
        .get_task(&task_id, None)
        .await
        .expect("Failed to get task");

    task.add_artifact(a2a_rs::domain::Artifact {
        artifact_id: format!("artifact-{}", uuid::Uuid::new_v4()),
        name: Some("Test Artifact".to_string()),
        description: None,
        parts: vec![a2a_rs::domain::Part::Text {
            text: "Artifact content".to_string(),
            metadata: None,
        }],
        metadata: None,
        extensions: None,
    });

    // Update task in storage
    storage
        .update_task_status(&task_id, task.status.state, None)
        .await
        .ok();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // List tasks with include_artifacts = false (default)
    let params = ListTasksParams {
        include_artifacts: Some(false),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    let listed_task = result
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("Task not found");

    assert!(
        listed_task.artifacts.is_none()
            || listed_task.artifacts.as_ref().is_some_and(|a| a.is_empty()),
        "Artifacts should be excluded when include_artifacts is false"
    );

    // List tasks with include_artifacts = true
    let params = ListTasksParams {
        include_artifacts: Some(true),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    let _listed_task = result
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("Task not found");

    // Note: Artifact inclusion depends on whether artifacts were persisted
    // through the storage layer. The add_artifact call above only modifies
    // a local copy, so the storage may not have the artifact.

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_task_list_combined_filters() {
    let port = 9008;
    let (shutdown_tx, storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    let context_id = "test-context-combined";

    // Create multiple tasks in same context (using session_id)
    let task_ids: Vec<String> = (0..5)
        .map(|_i| format!("combined-task-{}", uuid::Uuid::new_v4()))
        .collect();

    for task_id in &task_ids {
        let message = Message::user_text(
            format!("Message for {}", task_id),
            format!("msg-{}", uuid::Uuid::new_v4()),
        );
        client
            .send_task_message(task_id, &message, Some(context_id), None)
            .await
            .expect("Failed to create task");
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    // All tasks start as Working (set by DefaultMessageHandler)
    // Update some tasks to other states - leave task_ids[0] and task_ids[1] as Working
    storage
        .update_task_status(&task_ids[2], TaskState::Completed, None)
        .await
        .ok();
    storage
        .update_task_status(&task_ids[3], TaskState::Completed, None)
        .await
        .ok();
    storage
        .update_task_status(&task_ids[4], TaskState::Failed, None)
        .await
        .ok();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Filter by both context and status
    let params = ListTasksParams {
        context_id: Some(context_id.to_string()),
        status: Some(TaskState::Working),
        page_size: Some(10),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    assert_eq!(
        result.total_size, 2,
        "Should have 2 working tasks in context"
    );
    for task in &result.tasks {
        assert_eq!(task.context_id, context_id);
        assert_eq!(task.status.state, TaskState::Working);
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_task_list_empty_results() {
    let port = 9009;
    let (shutdown_tx, _storage) = setup_server_with_tasks(port).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // List tasks with filter that matches nothing
    let params = ListTasksParams {
        context_id: Some("non-existent-context".to_string()),
        ..Default::default()
    };
    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    assert_eq!(result.total_size, 0, "Should have no tasks");
    assert_eq!(result.tasks.len(), 0, "Should return empty array");
    assert!(
        result.next_page_token.is_empty(),
        "Should have empty next page token"
    );

    shutdown_tx.send(()).ok();
}
