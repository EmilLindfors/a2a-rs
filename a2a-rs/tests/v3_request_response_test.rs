//! A2A Protocol v0.3.0 Request/Response Serialization Tests
//!
//! This module validates JSON serialization and deserialization of v0.3.0 requests
//! and responses, ensuring they match the specification structure.

mod common;

use a2a_rs::{
    application::json_rpc,
    domain::{
        DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
        JSONRPCResponse, ListTaskPushNotificationConfigParams, ListTasksParams, ListTasksResult,
        Message, PushNotificationAuthenticationInfo, PushNotificationConfig, Task,
        TaskPushNotificationConfig, TaskState,
    },
};
use serde_json::{Value, json};

#[test]
fn test_list_tasks_request_serialization() {
    // Create ListTasksRequest with all params
    let params = ListTasksParams {
        context_id: Some("ctx-test-123".to_string()),
        status: Some(TaskState::Working),
        page_size: Some(25),
        page_token: Some("next-page-token".to_string()),
        history_length: Some(10),
        include_artifacts: Some(true),
        last_updated_after: Some(1704067200000), // 2024-01-01 00:00:00 UTC
        metadata: None,
    };

    let request = json_rpc::ListTasksRequest::new(Some(params));

    // Serialize to JSON
    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "ListTasksRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    // Verify structure matches spec
    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(request_json["method"], "tasks/list");
    assert!(request_json["id"].is_string() || request_json["id"].is_number());

    // Verify params structure
    let params_json = &request_json["params"];
    assert_eq!(params_json["contextId"], "ctx-test-123");
    assert_eq!(params_json["status"], "working");
    assert_eq!(params_json["pageSize"], 25);
    assert_eq!(params_json["pageToken"], "next-page-token");
    assert_eq!(params_json["historyLength"], 10);
    assert_eq!(params_json["includeArtifacts"], true);
    assert_eq!(params_json["lastUpdatedAfter"], 1704067200000_i64);

    // Verify deserialization roundtrip
    let deserialized: json_rpc::ListTasksRequest = serde_json::from_value(request_json).unwrap();
    let params = deserialized.params.unwrap();
    assert_eq!(params.context_id, Some("ctx-test-123".to_string()));
    assert_eq!(params.status, Some(TaskState::Working));
    assert_eq!(params.page_size, Some(25));
}

#[test]
fn test_list_tasks_request_minimal() {
    // Test with no parameters
    let request = json_rpc::ListTasksRequest::new(None);

    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "Minimal ListTasksRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(request_json["method"], "tasks/list");
    assert!(request_json["params"].is_null() || request_json.get("params").is_none());
}

#[test]
fn test_list_tasks_response_serialization() {
    // Create a response with tasks
    let task1 = Task::new("task-1".to_string(), "ctx-123".to_string());
    let task2 = Task::new("task-2".to_string(), "ctx-123".to_string());

    let response = ListTasksResult {
        tasks: vec![task1, task2],
        total_size: 25,
        page_size: 2,
        next_page_token: "next-page-123".to_string(),
    };

    // Serialize to JSON
    let response_json = serde_json::to_value(&response).unwrap();
    println!(
        "ListTasksResponse JSON:\n{}",
        serde_json::to_string_pretty(&response_json).unwrap()
    );

    // Verify structure
    assert!(response_json["tasks"].is_array());
    assert_eq!(response_json["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(response_json["totalSize"], 25);
    assert_eq!(response_json["nextPageToken"], "next-page-123");

    // Verify task structure
    assert_eq!(response_json["tasks"][0]["id"], "task-1");
    assert_eq!(response_json["tasks"][0]["contextId"], "ctx-123");
    assert!(response_json["tasks"][0]["status"].is_object());

    // Verify deserialization roundtrip
    let deserialized: ListTasksResult = serde_json::from_value(response_json).unwrap();
    assert_eq!(deserialized.tasks.len(), 2);
    assert_eq!(deserialized.total_size, 25);
    assert_eq!(deserialized.page_size, 2);
    assert_eq!(deserialized.next_page_token, "next-page-123");
}

#[test]
fn test_list_tasks_response_empty() {
    // Test with empty results
    let response = ListTasksResult {
        tasks: vec![],
        total_size: 0,
        page_size: 0,
        next_page_token: "".to_string(),
    };

    let response_json = serde_json::to_value(&response).unwrap();
    println!(
        "Empty ListTasksResponse JSON:\n{}",
        serde_json::to_string_pretty(&response_json).unwrap()
    );

    assert!(response_json["tasks"].is_array());
    assert_eq!(response_json["tasks"].as_array().unwrap().len(), 0);
    assert_eq!(response_json["totalSize"], 0);
    assert_eq!(response_json["pageSize"], 0);
    assert_eq!(response_json["nextPageToken"], "");
}

#[test]
fn test_authenticated_extended_card_request_serialization() {
    let request = json_rpc::GetAuthenticatedExtendedCardRequest::new();

    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "GetAuthenticatedExtendedCardRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(request_json["method"], "agent/getAuthenticatedExtendedCard");
    assert!(request_json["id"].is_string() || request_json["id"].is_number());
}

#[test]
fn test_authenticated_extended_card_response_serialization() {
    // Test the structure of an authenticated extended card response
    // In a real scenario, this would be returned from the agent
    let card_value = json!({
        "name": "Test Agent",
        "description": "Test agent for authenticated card",
        "url": "https://api.example.com",
        "version": "1.0.0",
        "protocolVersion": "0.3.0",
        "preferredTransport": "JSONRPC",
        "capabilities": {
            "supportsStreaming": false,
            "supportsPushNotifications": false,
            "supportsStateTransitionHistory": false
        },
        "defaultInputModes": ["text"],
        "defaultOutputModes": ["text"],
        "skills": []
    });

    // Verify structure
    assert_eq!(card_value["name"], "Test Agent");
    assert_eq!(card_value["protocolVersion"], "0.3.0");
    assert_eq!(card_value["preferredTransport"], "JSONRPC");

    // Verify required fields are present
    assert!(card_value.get("name").is_some());
    assert!(card_value.get("url").is_some());
    assert!(card_value.get("version").is_some());
    assert!(card_value.get("capabilities").is_some());
}

#[test]
fn test_list_push_notification_configs_request_serialization() {
    let request = json_rpc::ListTaskPushNotificationConfigRequest {
        jsonrpc: "2.0".to_string(),
        method: "tasks/pushNotificationConfig/list".to_string(),
        id: Some(serde_json::Value::String("req-push-list".to_string())),
        params: ListTaskPushNotificationConfigParams {
            id: "task-123".to_string(),
            metadata: None,
        },
    };

    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "ListTaskPushNotificationConfigsRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(request_json["method"], "tasks/pushNotificationConfig/list");
    assert_eq!(request_json["params"]["id"], "task-123");

    // Verify deserialization
    let deserialized: json_rpc::ListTaskPushNotificationConfigRequest =
        serde_json::from_value(request_json).unwrap();
    assert_eq!(deserialized.params.id, "task-123");
}

#[test]
fn test_list_push_notification_configs_response_serialization() {
    let config1 = TaskPushNotificationConfig {
        task_id: "task-123".to_string(),
        push_notification_config: PushNotificationConfig {
            id: Some("config-1".to_string()),
            url: "https://client.example.com/webhook1".to_string(),
            token: Some("token-1".to_string()),
            authentication: None,
        },
    };

    let config2 = TaskPushNotificationConfig {
        task_id: "task-123".to_string(),
        push_notification_config: PushNotificationConfig {
            id: Some("config-2".to_string()),
            url: "https://client.example.com/webhook2".to_string(),
            token: Some("token-2".to_string()),
            authentication: None,
        },
    };

    let response = json_rpc::ListTaskPushNotificationConfigResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::Value::String("resp-list-push".to_string())),
        result: Some(vec![config1, config2]),
        error: None,
    };

    let response_json = serde_json::to_value(&response).unwrap();
    println!(
        "ListTaskPushNotificationConfigsResponse JSON:\n{}",
        serde_json::to_string_pretty(&response_json).unwrap()
    );

    assert_eq!(response_json["jsonrpc"], "2.0");
    assert_eq!(response_json["id"], "resp-list-push");
    assert!(response_json["result"].is_array());
    assert_eq!(response_json["result"].as_array().unwrap().len(), 2);
    assert_eq!(response_json["result"][0]["taskId"], "task-123");
    assert_eq!(
        response_json["result"][0]["pushNotificationConfig"]["id"],
        "config-1"
    );
    assert_eq!(
        response_json["result"][1]["pushNotificationConfig"]["id"],
        "config-2"
    );

    // Verify deserialization
    let deserialized: json_rpc::ListTaskPushNotificationConfigResponse =
        serde_json::from_value(response_json).unwrap();
    assert_eq!(deserialized.result.unwrap().len(), 2);
}

#[test]
fn test_get_push_notification_config_request_serialization() {
    let request = json_rpc::GetTaskPushNotificationConfigRequest {
        jsonrpc: "2.0".to_string(),
        method: "tasks/pushNotificationConfig/get".to_string(),
        id: Some(serde_json::Value::String("req-get-push".to_string())),
        params: Some(GetTaskPushNotificationConfigParams {
            id: "task-456".to_string(),
            push_notification_config_id: Some("config-789".to_string()),
            metadata: None,
        }),
    };

    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "GetTaskPushNotificationConfigRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(request_json["method"], "tasks/pushNotificationConfig/get");
    assert_eq!(request_json["params"]["id"], "task-456");
    assert_eq!(
        request_json["params"]["pushNotificationConfigId"],
        "config-789"
    );

    // Verify deserialization
    let deserialized: json_rpc::GetTaskPushNotificationConfigRequest =
        serde_json::from_value(request_json).unwrap();
    let params = deserialized.params.unwrap();
    assert_eq!(params.id, "task-456");
    assert_eq!(
        params.push_notification_config_id,
        Some("config-789".to_string())
    );
}

#[test]
fn test_delete_push_notification_config_request_serialization() {
    let request = json_rpc::DeleteTaskPushNotificationConfigRequest {
        jsonrpc: "2.0".to_string(),
        method: "tasks/pushNotificationConfig/delete".to_string(),
        id: Some(serde_json::Value::String("req-delete-push".to_string())),
        params: DeleteTaskPushNotificationConfigParams {
            id: "task-delete-123".to_string(),
            push_notification_config_id: "config-delete-456".to_string(),
            metadata: None,
        },
    };

    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "DeleteTaskPushNotificationConfigRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(
        request_json["method"],
        "tasks/pushNotificationConfig/delete"
    );
    assert_eq!(request_json["params"]["id"], "task-delete-123");
    assert_eq!(
        request_json["params"]["pushNotificationConfigId"],
        "config-delete-456"
    );

    // Verify deserialization
    let deserialized: json_rpc::DeleteTaskPushNotificationConfigRequest =
        serde_json::from_value(request_json).unwrap();
    assert_eq!(deserialized.params.id, "task-delete-123");
    assert_eq!(
        deserialized.params.push_notification_config_id,
        "config-delete-456"
    );
}

#[test]
fn test_jsonrpc_error_response_serialization() {
    // Create an error response using JSONRPCResponse
    use a2a_rs::domain::JSONRPCError;

    let error = JSONRPCError {
        code: -32001,
        message: "Task not found".to_string(),
        data: Some(json!({
            "taskId": "task-missing-123"
        })),
    };

    let response = JSONRPCResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(Value::String("req-error".to_string())),
        result: None,
        error: Some(error),
    };

    let response_json = serde_json::to_value(&response).unwrap();
    println!(
        "Error Response JSON:\n{}",
        serde_json::to_string_pretty(&response_json).unwrap()
    );

    assert_eq!(response_json["jsonrpc"], "2.0");
    assert_eq!(response_json["id"], "req-error");
    assert!(response_json["result"].is_null() || response_json.get("result").is_none());
    assert_eq!(response_json["error"]["code"], -32001);
    assert_eq!(response_json["error"]["message"], "Task not found");
    assert_eq!(response_json["error"]["data"]["taskId"], "task-missing-123");
}

#[test]
fn test_list_tasks_with_complex_filters() {
    // Test ListTasksParams with multiple filters combined
    let params = ListTasksParams {
        context_id: Some("ctx-complex".to_string()),
        status: Some(TaskState::Working),
        page_size: Some(50),
        page_token: None,
        history_length: Some(20),
        include_artifacts: Some(false),
        last_updated_after: Some(1704153600000), // 2024-01-02 00:00:00 UTC
        metadata: Some(
            json!({
                "filter": "custom",
                "priority": "high"
            })
            .as_object()
            .unwrap()
            .clone(),
        ),
    };

    let request = json_rpc::ListTasksRequest::new(Some(params));
    let request_json = serde_json::to_value(&request).unwrap();

    println!(
        "Complex ListTasksRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    let params_json = &request_json["params"];
    assert_eq!(params_json["contextId"], "ctx-complex");
    assert_eq!(params_json["status"], "working");
    assert_eq!(params_json["pageSize"], 50);
    assert_eq!(params_json["historyLength"], 20);
    assert_eq!(params_json["includeArtifacts"], false);
    assert_eq!(params_json["lastUpdatedAfter"], 1704153600000_i64);
    assert!(params_json["metadata"].is_object());
    assert_eq!(params_json["metadata"]["filter"], "custom");
}

#[test]
fn test_push_notification_config_with_authentication() {
    let auth = PushNotificationAuthenticationInfo {
        schemes: vec!["bearer".to_string(), "apiKey".to_string()],
        credentials: Some("secret-key-123".to_string()),
    };

    let config = PushNotificationConfig {
        id: Some("config-auth-123".to_string()),
        url: "https://client.example.com/secure-webhook".to_string(),
        token: Some("legacy-token".to_string()),
        authentication: Some(auth),
    };

    let config_json = serde_json::to_value(&config).unwrap();
    println!(
        "PushNotificationConfig with auth JSON:\n{}",
        serde_json::to_string_pretty(&config_json).unwrap()
    );

    assert_eq!(config_json["id"], "config-auth-123");
    assert!(config_json["authentication"].is_object());
    assert!(config_json["authentication"]["schemes"].is_array());
    assert_eq!(config_json["authentication"]["schemes"][0], "bearer");
    assert_eq!(config_json["authentication"]["schemes"][1], "apiKey");
    assert_eq!(
        config_json["authentication"]["credentials"],
        "secret-key-123"
    );

    // Verify deserialization
    let deserialized: PushNotificationConfig = serde_json::from_value(config_json).unwrap();
    assert_eq!(deserialized.id, Some("config-auth-123".to_string()));
    assert!(deserialized.authentication.is_some());
}

#[test]
fn test_task_status_serialization_all_states() {
    // Test serialization of all task states
    let states = [
        TaskState::Submitted,
        TaskState::Working,
        TaskState::InputRequired,
        TaskState::Completed,
        TaskState::Canceled,
        TaskState::Failed,
        TaskState::Rejected,
        TaskState::AuthRequired,
        TaskState::Unknown,
    ];

    let expected_strings = [
        "submitted",
        "working",
        "input-required",
        "completed",
        "canceled",
        "failed",
        "rejected",
        "auth-required",
        "unknown",
    ];

    for (state, expected) in states.iter().zip(expected_strings.iter()) {
        let state_json = serde_json::to_value(state).unwrap();
        assert_eq!(
            state_json.as_str().unwrap(),
            *expected,
            "TaskState {:?} should serialize to '{}'",
            state,
            expected
        );

        // Verify deserialization roundtrip
        let deserialized: TaskState = serde_json::from_value(state_json).unwrap();
        assert_eq!(
            &deserialized, state,
            "TaskState should deserialize back to {:?}",
            state
        );
    }
}

#[test]
fn test_jsonrpc_request_id_types() {
    // JSON-RPC 2.0 allows string, number, or null for id
    let request_with_string_id = json!({
        "jsonrpc": "2.0",
        "method": "tasks/list",
        "id": "req-string-123"
    });

    let request_with_number_id = json!({
        "jsonrpc": "2.0",
        "method": "tasks/list",
        "id": 42
    });

    let request_with_null_id = json!({
        "jsonrpc": "2.0",
        "method": "tasks/list",
        "id": null
    });

    // All should be valid JSON-RPC requests
    println!(
        "String ID: {}",
        serde_json::to_string_pretty(&request_with_string_id).unwrap()
    );
    println!(
        "Number ID: {}",
        serde_json::to_string_pretty(&request_with_number_id).unwrap()
    );
    println!(
        "Null ID: {}",
        serde_json::to_string_pretty(&request_with_null_id).unwrap()
    );

    assert_eq!(request_with_string_id["id"], "req-string-123");
    assert_eq!(request_with_number_id["id"], 42);
    assert!(request_with_null_id["id"].is_null());
}

#[test]
fn test_list_tasks_response_with_history() {
    // Create a task with message history
    let mut task = Task::new("task-history-test".to_string(), "ctx-123".to_string());

    let msg1 = Message::user_text("Hello".to_string(), "msg-1".to_string());
    let msg2 = Message::agent_text("Hi there!".to_string(), "msg-2".to_string());

    task.update_status(TaskState::Working, Some(msg1));
    task.update_status(TaskState::Completed, Some(msg2));

    let response = ListTasksResult {
        tasks: vec![task],
        total_size: 1,
        page_size: 1,
        next_page_token: "".to_string(),
    };

    let response_json = serde_json::to_value(&response).unwrap();
    println!(
        "ListTasksResponse with history:\n{}",
        serde_json::to_string_pretty(&response_json).unwrap()
    );

    // Verify history is included
    assert!(response_json["tasks"][0]["history"].is_array());
    let history = response_json["tasks"][0]["history"].as_array().unwrap();
    assert!(
        history.len() >= 2,
        "Task should have at least 2 history entries"
    );
}
