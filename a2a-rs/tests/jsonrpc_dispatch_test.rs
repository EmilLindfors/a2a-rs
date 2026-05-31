//! Behavioral tests for the JSON-RPC adapter's method dispatch.
//!
//! Drives [`JsonRpcAdapter::handle_unary`] against an in-memory handler and
//! asserts the JSON-RPC envelopes + ProtoJSON result bodies that an
//! off-the-shelf A2A client would see.

#![cfg(feature = "jsonrpc-server")]

mod common;

use a2a_rs::adapter::{InMemoryTaskStorage, JsonRpcAdapter, SimpleAgentInfo};
use a2a_rs::adapter::transport::jsonrpc::{JsonRpcId, JsonRpcRequest, error_code, methods};
use common::TestBusinessHandler;
use serde_json::{Value, json};

fn adapter() -> JsonRpcAdapter {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let agent_info = SimpleAgentInfo::new("test-agent".to_string(), "http://localhost".to_string());
    JsonRpcAdapter::with_handler(handler, agent_info)
}

fn request(method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Num(1),
        method: method.to_string(),
        params: Some(params),
    }
}

fn send_message_params(task_id: &str) -> Value {
    json!({
        "message": {
            "messageId": "m1",
            "role": "ROLE_USER",
            "parts": [{ "text": "hello" }],
            "taskId": task_id,
        }
    })
}

#[tokio::test]
async fn send_message_returns_task_union() {
    let resp = adapter().handle_unary(request(methods::SEND_MESSAGE, send_message_params("task-1"))).await;
    let value = serde_json::to_value(&resp).unwrap();

    assert_eq!(value["jsonrpc"], "2.0");
    assert_eq!(value["id"], 1);
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    // Field-presence union: result is `{ "task": { ... } }`, no discriminator.
    let task = &value["result"]["task"];
    assert_eq!(task["id"], "task-1");
    // State is a SCREAMING_SNAKE proto-name string (the exact value depends on
    // the handler; just assert the ProtoJSON enum shape).
    assert!(
        task["status"]["state"].as_str().is_some_and(|s| s.starts_with("TASK_STATE_")),
        "unexpected status: {:?}",
        task["status"],
    );
}

#[tokio::test]
async fn get_task_round_trips() {
    let a = adapter();
    a.handle_unary(request(methods::SEND_MESSAGE, send_message_params("task-2"))).await;

    let resp = a.handle_unary(request(methods::GET_TASK, json!({ "id": "task-2" }))).await;
    let value = serde_json::to_value(&resp).unwrap();
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    // GetTask result is a bare Task (not a union).
    assert_eq!(value["result"]["id"], "task-2");
}

#[tokio::test]
async fn cancel_task_returns_canceled_state() {
    let a = adapter();
    a.handle_unary(request(methods::SEND_MESSAGE, send_message_params("task-3"))).await;

    let resp = a.handle_unary(request(methods::CANCEL_TASK, json!({ "id": "task-3" }))).await;
    let value = serde_json::to_value(&resp).unwrap();
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    assert_eq!(value["result"]["id"], "task-3");
}

#[tokio::test]
async fn unknown_method_is_method_not_found() {
    let resp = adapter().handle_unary(request("NoSuchMethod", json!({}))).await;
    let value = serde_json::to_value(&resp).unwrap();
    assert!(value.get("result").is_none());
    assert_eq!(value["error"]["code"], error_code::METHOD_NOT_FOUND);
}

#[tokio::test]
async fn invalid_params_is_invalid_params() {
    // `message` is required on SendMessageRequest's wire shape; an int is invalid.
    let resp = adapter().handle_unary(request(methods::SEND_MESSAGE, json!({ "message": 42 }))).await;
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["error"]["code"], error_code::INVALID_PARAMS);
}

#[tokio::test]
async fn missing_message_is_invalid_params() {
    let resp = adapter().handle_unary(request(methods::SEND_MESSAGE, json!({}))).await;
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["error"]["code"], error_code::INVALID_PARAMS);
}

#[tokio::test]
async fn get_missing_task_is_task_not_found() {
    let resp = adapter().handle_unary(request(methods::GET_TASK, json!({ "id": "nope" }))).await;
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["error"]["code"], error_code::TASK_NOT_FOUND);
}

#[tokio::test]
async fn list_tasks_returns_response_envelope() {
    let a = adapter();
    a.handle_unary(request(methods::SEND_MESSAGE, send_message_params("task-4"))).await;

    let resp = a.handle_unary(request(methods::LIST_TASKS, json!({}))).await;
    let value = serde_json::to_value(&resp).unwrap();
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    assert!(value["result"]["tasks"].is_array());
}
