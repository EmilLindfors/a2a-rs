//! Test the new v0.3.0 message/send and message/stream API methods
//!
//! This test verifies that the message/send and message/stream methods
//! work correctly with the new MessageSendParams structure.

mod common;

use a2a_rs::{
    application::json_rpc::{SendMessageRequest, SendMessageStreamingRequest},
    domain::{Message, MessageSendConfiguration, MessageSendParams, Part, Role},
};

#[test]
fn test_message_send_request_serialization() {
    // Create a message
    let message = Message::builder()
        .role(Role::User)
        .message_id("msg-123".to_string())
        .task_id("task-456".to_string())
        .context_id("ctx-789".to_string())
        .parts(vec![Part::Text {
            text: "Hello, agent!".to_string(),
            metadata: None,
        }])
        .build();

    // Create configuration
    let configuration = MessageSendConfiguration {
        accepted_output_modes: Some(vec!["text/plain".to_string()]),
        history_length: Some(10),
        push_notification_config: None,
        blocking: Some(false),
    };

    // Create params
    let params = MessageSendParams {
        message,
        configuration: Some(configuration),
        metadata: None,
    };

    // Create request
    let request = SendMessageRequest::new(params);

    // Serialize to JSON
    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "SendMessageRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    // Verify structure matches spec
    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(request_json["method"], "message/send");
    assert!(request_json["id"].is_string() || request_json["id"].is_number());

    // Verify params structure
    let params_json = &request_json["params"];
    assert!(params_json["message"].is_object());
    assert_eq!(params_json["message"]["messageId"], "msg-123");
    assert_eq!(params_json["message"]["taskId"], "task-456");
    assert_eq!(params_json["message"]["contextId"], "ctx-789");
    assert_eq!(params_json["message"]["role"], "user");

    // Verify configuration
    assert!(params_json["configuration"].is_object());
    assert_eq!(params_json["configuration"]["historyLength"], 10);
    assert_eq!(params_json["configuration"]["blocking"], false);

    // Verify deserialization roundtrip
    let deserialized: SendMessageRequest = serde_json::from_value(request_json).unwrap();
    assert_eq!(deserialized.params.message.message_id, "msg-123");
    assert_eq!(
        deserialized.params.message.task_id,
        Some("task-456".to_string())
    );
    assert_eq!(
        deserialized.params.message.context_id,
        Some("ctx-789".to_string())
    );
}

#[test]
fn test_message_send_minimal() {
    // Create minimal message (only required fields)
    let message = Message::builder()
        .role(Role::User)
        .message_id("msg-minimal".to_string())
        .parts(vec![Part::Text {
            text: "Minimal message".to_string(),
            metadata: None,
        }])
        .build();

    // Create minimal params (no configuration)
    let params = MessageSendParams {
        message,
        configuration: None,
        metadata: None,
    };

    let request = SendMessageRequest::new(params);

    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "Minimal SendMessageRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(request_json["method"], "message/send");
    assert_eq!(
        request_json["params"]["message"]["messageId"],
        "msg-minimal"
    );

    // Configuration should not be present or be null
    assert!(
        request_json["params"]["configuration"].is_null()
            || request_json["params"].get("configuration").is_none()
    );
}

#[test]
fn test_message_stream_request_serialization() {
    // Create a message for streaming
    let message = Message::builder()
        .role(Role::User)
        .message_id("msg-stream-123".to_string())
        .task_id("task-stream-456".to_string())
        .parts(vec![Part::Text {
            text: "Stream this message".to_string(),
            metadata: None,
        }])
        .build();

    let params = MessageSendParams {
        message,
        configuration: None,
        metadata: None,
    };

    // Create streaming request
    let request = SendMessageStreamingRequest::new(params);

    let request_json = serde_json::to_value(&request).unwrap();
    println!(
        "SendMessageStreamingRequest JSON:\n{}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    // Verify structure
    assert_eq!(request_json["jsonrpc"], "2.0");
    assert_eq!(request_json["method"], "message/stream");
    assert_eq!(
        request_json["params"]["message"]["messageId"],
        "msg-stream-123"
    );
    assert_eq!(
        request_json["params"]["message"]["taskId"],
        "task-stream-456"
    );

    // Verify deserialization
    let deserialized: SendMessageStreamingRequest = serde_json::from_value(request_json).unwrap();
    assert_eq!(deserialized.params.message.message_id, "msg-stream-123");
    assert_eq!(
        deserialized.params.message.task_id,
        Some("task-stream-456".to_string())
    );
}
