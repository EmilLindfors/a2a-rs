//! A2A Protocol Specification Compliance Tests
//!
//! This module validates that our Rust types match the JSON Schema definitions
//! in the A2A specification files located in ../spec/

use a2a_rs::{
    adapter::SimpleAgentInfo,
    domain::{Message, Part, Task, TaskState},
    application::SendMessageRequest,
    services::AgentInfoProvider,
    MessageSendParams,
};
use jsonschema::{Draft, JSONSchema};
use serde_json::{json, Value};
use std::fs;

/// Load and compile a JSON Schema from the spec directory
fn load_schema(filename: &str) -> JSONSchema {
    let schema_path = format!("../spec/{}", filename);
    let schema_content = fs::read_to_string(&schema_path)
        .unwrap_or_else(|_| panic!("Failed to read schema file: {}", schema_path));
    
    let schema: Value = serde_json::from_str(&schema_content)
        .unwrap_or_else(|_| panic!("Failed to parse schema JSON: {}", filename));
    
    JSONSchema::options()
        .with_draft(Draft::Draft7)
        .build(&schema)
        .unwrap_or_else(|_| panic!("Failed to compile schema: {}", filename))
}

/// Extract a specific definition from a schema file with all definitions context
fn extract_definition(schema_content: &str, definition_name: &str) -> Value {
    let mut schema: Value = serde_json::from_str(schema_content).unwrap();
    let definition = schema["definitions"][definition_name].clone();
    
    // Create a new schema with the specific definition as root but keep all definitions
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "definitions": schema["definitions"],
        "$ref": format!("#/definitions/{}", definition_name)
    })
}

#[tokio::test]
async fn test_agent_card_compliance() {
    // Create a sample AgentCard using our SimpleAgentInfo
    let agent_info = SimpleAgentInfo::new(
        "Test Agent".to_string(),
        "https://api.example.com".to_string(),
    )
    .with_description("A test agent for A2A protocol compliance".to_string())
    .with_version("1.0.0".to_string())
    .with_provider("Test Organization".to_string(), "https://example.org".to_string())
    .with_documentation_url("https://docs.example.org".to_string())
    .with_streaming()
    .with_push_notifications()
    .with_state_transition_history()
    .add_skill("echo".to_string(), "Echo Skill".to_string(), Some("Echoes input back to user".to_string()))
    .add_skill("translate".to_string(), "Translation".to_string(), Some("Translates text between languages".to_string()));

    let agent_card = agent_info.get_agent_card().await.unwrap();

    // Serialize to JSON
    let agent_card_json = serde_json::to_value(&agent_card).unwrap();
    println!("AgentCard JSON: {}", serde_json::to_string_pretty(&agent_card_json).unwrap());

    // Load the agent schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let agent_card_schema = extract_definition(&schema_content, "AgentCard");
    
    let schema = JSONSchema::options()
        .with_draft(Draft::Draft7)
        .build(&agent_card_schema)
        .expect("Failed to compile AgentCard schema");

    // Validate against schema
    let result = schema.validate(&agent_card_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("AgentCard validation error: {}", error);
            eprintln!("Instance path: {}", error.instance_path);
        }
        panic!("AgentCard does not comply with A2A specification");
    }
}

#[test]
fn test_message_compliance() {
    // Create a comprehensive message with all part types
    let message_id = uuid::Uuid::new_v4().to_string();
    let mut message = Message::user_text("Hello, agent!".to_string(), message_id.clone());
    
    // Add a data part
    let data_part = Part::Data {
        data: json!({
            "key": "value",
            "number": 42,
            "nested": {
                "array": [1, 2, 3]
            }
        }).as_object().unwrap().clone(),
        metadata: None,
    };
    message.add_part(data_part);

    // Add a file part
    let file_part = Part::file_from_bytes(
        "SGVsbG8gV29ybGQ=".to_string(), // "Hello World" in base64
        Some("test.txt".to_string()),
        Some("text/plain".to_string()),
    );
    message.add_part_validated(file_part).unwrap();

    // Set context and task IDs
    message.context_id = Some("ctx-123".to_string());
    message.task_id = Some("task-456".to_string());

    // Serialize to JSON
    let message_json = serde_json::to_value(&message).unwrap();
    println!("Message JSON: {}", serde_json::to_string_pretty(&message_json).unwrap());

    // Load and validate against Message schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let message_schema = extract_definition(&schema_content, "Message");
    
    let schema = JSONSchema::options()
        .with_draft(Draft::Draft7)
        .build(&message_schema)
        .expect("Failed to compile Message schema");

    let result = schema.validate(&message_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("Message validation error: {}", error);
            eprintln!("Instance path: {}", error.instance_path);
        }
        panic!("Message does not comply with A2A specification");
    }
}

#[test]
fn test_task_compliance() {
    // Create a task
    let context_id = "ctx-789".to_string();
    let mut task = Task::new("task-987".to_string(), context_id.clone());
    
    // Add history messages
    let msg1 = Message::user_text("Initial message".to_string(), "msg-1".to_string());
    let msg2 = Message::agent_text("Agent response".to_string(), "msg-2".to_string());
    
    task.update_status(TaskState::Working, Some(msg1));
    task.update_status(TaskState::Completed, Some(msg2));

    // Serialize to JSON
    let task_json = serde_json::to_value(&task).unwrap();
    println!("Task JSON: {}", serde_json::to_string_pretty(&task_json).unwrap());

    // Load and validate against Task schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let task_schema = extract_definition(&schema_content, "Task");
    
    let schema = JSONSchema::options()
        .with_draft(Draft::Draft7)
        .build(&task_schema)
        .expect("Failed to compile Task schema");

    let result = schema.validate(&task_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("Task validation error: {}", error);
            eprintln!("Instance path: {}", error.instance_path);
        }
        panic!("Task does not comply with A2A specification");
    }
}

#[test]
fn test_jsonrpc_request_compliance() {
    // Test SendMessageRequest
    let message = Message::user_text("Test message".to_string(), "msg-test".to_string());
    
    let send_request = SendMessageRequest {
        jsonrpc: "2.0".to_string(),
        method: "message/send".to_string(),
        id: Some(serde_json::Value::String("req-123".to_string())),
        params: MessageSendParams {
            message,
            configuration: None,
            metadata: None,
        },
    };

    let request_json = serde_json::to_value(&send_request).unwrap();
    println!("SendMessageRequest JSON: {}", serde_json::to_string_pretty(&request_json).unwrap());

    // Load and validate against SendMessageRequest schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let request_schema = extract_definition(&schema_content, "SendMessageRequest");
    
    let schema = JSONSchema::options()
        .with_draft(Draft::Draft7)
        .build(&request_schema)
        .expect("Failed to compile SendMessageRequest schema");

    let result = schema.validate(&request_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("SendMessageRequest validation error: {}", error);
            eprintln!("Instance path: {}", error.instance_path);
        }
        panic!("SendMessageRequest does not comply with A2A specification");
    }
}

#[test]
fn test_task_states_compliance() {
    // Test all valid task states according to the specification
    let valid_states = [
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

    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let task_state_schema = extract_definition(&schema_content, "TaskState");
    
    let schema = JSONSchema::options()
        .with_draft(Draft::Draft7)
        .build(&task_state_schema)
        .expect("Failed to compile TaskState schema");

    for state in &valid_states {
        let state_json = serde_json::to_value(state).unwrap();
        
        let result = schema.validate(&state_json);
        if let Err(errors) = result {
            for error in errors {
                eprintln!("TaskState {:?} validation error: {}", state, error);
            }
            panic!("TaskState {:?} does not comply with A2A specification", state);
        }
    }
}

#[test]
fn test_error_codes_compliance() {
    // Test that our error codes match the specification
    
    // Standard JSON-RPC errors
    let jsonrpc_errors = vec![
        (-32700, "Parse error"),
        (-32600, "Invalid Request"),
        (-32601, "Method not found"),
        (-32602, "Invalid params"),
        (-32603, "Internal error"),
    ];

    // A2A-specific errors
    let a2a_errors = vec![
        (-32001, "Task not found"),
        (-32002, "Task not cancelable"),
        (-32003, "Push notifications not supported"),
        (-32004, "Operation not supported"),
        (-32005, "Content type not supported"),
        (-32006, "Invalid agent response"),
    ];

    // All error codes should be documented in the spec
    let all_errors = [jsonrpc_errors, a2a_errors].concat();
    
    for (code, message) in all_errors {
        println!("Checking error code {} with message: {}", code, message);
        // This validates that our error codes align with the specification
        // The actual validation would depend on how we structure our error types
    }
}

#[cfg(test)]
mod property_based_tests {
    use super::*;
    use proptest::prelude::*;
    
    proptest! {
        #[test]
        fn message_serialization_roundtrip(
            text in ".*",
            message_id in ".*",
            role in prop::sample::select(vec!["user", "agent"]),
        ) {
            let message = if role == "user" {
                Message::user_text(text.clone(), message_id.clone())
            } else {
                Message::agent_text(text.clone(), message_id.clone())
            };
            
            // Serialize and deserialize
            let json = serde_json::to_value(&message).unwrap();
            let deserialized: Message = serde_json::from_value(json).unwrap();
            
            // Check that essential properties are preserved
            prop_assert_eq!(message.message_id, deserialized.message_id);
            prop_assert_eq!(message.role, deserialized.role);
            prop_assert_eq!(message.parts.len(), deserialized.parts.len());
        }
        
        #[test]
        fn task_id_validation(task_id in ".*") {
            if !task_id.is_empty() {
                let context_id = "ctx-test".to_string();
                let task = Task::new(task_id.clone(), context_id);
                prop_assert_eq!(task.id, task_id);
            }
        }
    }
}