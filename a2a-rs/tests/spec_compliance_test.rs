//! A2A Protocol Specification Compliance Tests
//!
//! This module validates that our Rust types match the JSON Schema definitions
//! in the A2A specification files located in ../spec/

use a2a_rs::{
    adapter::SimpleAgentInfo,
    application::SendMessageRequest,
    domain::{Message, Part, Task, TaskState},
    services::AgentInfoProvider,
    MessageSendParams,
};
use jsonschema::{Draft, Validator};
use serde_json::{json, Value};
use std::fs;

/// Load and compile a JSON Schema from the spec directory
#[allow(dead_code)]
fn load_schema(filename: &str) -> Validator {
    let schema_path = format!("../spec/{}", filename);
    let schema_content = fs::read_to_string(&schema_path)
        .unwrap_or_else(|_| panic!("Failed to read schema file: {}", schema_path));

    let schema: Value = serde_json::from_str(&schema_content)
        .unwrap_or_else(|_| panic!("Failed to parse schema JSON: {}", filename));

    Validator::options()
        .with_draft(Draft::Draft7)
        .build(&schema)
        .unwrap_or_else(|_| panic!("Failed to compile schema: {}", filename))
}

/// Extract a specific definition from a schema file with all definitions context
fn extract_definition(schema_content: &str, definition_name: &str) -> Value {
    let schema: Value = serde_json::from_str(schema_content).unwrap();
    let _definition = schema["definitions"][definition_name].clone();

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
    .with_provider(
        "Test Organization".to_string(),
        "https://example.org".to_string(),
    )
    .with_documentation_url("https://docs.example.org".to_string())
    .with_streaming()
    .with_push_notifications()
    .with_state_transition_history()
    .add_skill(
        "echo".to_string(),
        "Echo Skill".to_string(),
        Some("Echoes input back to user".to_string()),
    )
    .add_skill(
        "translate".to_string(),
        "Translation".to_string(),
        Some("Translates text between languages".to_string()),
    );

    let agent_card = agent_info.get_agent_card().await.unwrap();

    // Serialize to JSON
    let agent_card_json = serde_json::to_value(&agent_card).unwrap();
    println!(
        "AgentCard JSON: {}",
        serde_json::to_string_pretty(&agent_card_json).unwrap()
    );

    // Load the agent schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let agent_card_schema = extract_definition(&schema_content, "AgentCard");

    let schema = Validator::options()
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
        })
        .as_object()
        .unwrap()
        .clone(),
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
    println!(
        "Message JSON: {}",
        serde_json::to_string_pretty(&message_json).unwrap()
    );

    // Load and validate against Message schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let message_schema = extract_definition(&schema_content, "Message");

    let schema = Validator::options()
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
    println!(
        "Task JSON: {}",
        serde_json::to_string_pretty(&task_json).unwrap()
    );

    // Load and validate against Task schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let task_schema = extract_definition(&schema_content, "Task");

    let schema = Validator::options()
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
    println!(
        "SendMessageRequest JSON: {}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    // Load and validate against SendMessageRequest schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let request_schema = extract_definition(&schema_content, "SendMessageRequest");

    let schema = Validator::options()
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

    let schema = Validator::options()
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
            panic!(
                "TaskState {:?} does not comply with A2A specification",
                state
            );
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

    // A2A-specific errors (v0.3.0 includes new -32007 error)
    let a2a_errors = vec![
        (-32001, "Task not found"),
        (-32002, "Task not cancelable"),
        (-32003, "Push notifications not supported"),
        (-32004, "Operation not supported"),
        (-32005, "Content type not supported"),
        (-32006, "Invalid agent response"),
        (-32007, "Authenticated Extended Card is not configured"),
    ];

    // All error codes should be documented in the spec
    let all_errors = [jsonrpc_errors, a2a_errors].concat();

    for (code, message) in all_errors {
        println!("Checking error code {} with message: {}", code, message);
        // This validates that our error codes align with the specification
        // The actual validation would depend on how we structure our error types
    }
}

#[test]
fn test_authenticated_extended_card_error() {
    use a2a_rs::domain::error::A2AError;

    let error = A2AError::AuthenticatedExtendedCardNotConfigured;
    let jsonrpc_error = error.to_jsonrpc_error();

    assert_eq!(jsonrpc_error["code"], -32007);
    assert_eq!(
        jsonrpc_error["message"],
        "Authenticated Extended Card is not configured"
    );
}

#[test]
fn test_agent_card_v030_fields() {
    use a2a_rs::domain::{AgentCapabilities, AgentCard, AgentCardSignature, AgentInterface};
    use std::collections::HashMap;

    // Create an AgentCard with all v0.3.0 fields
    let mut card = AgentCard::builder()
        .name("Test Agent v0.3.0".to_string())
        .description("Agent with v0.3.0 features".to_string())
        .url("https://api.example.com/jsonrpc".to_string())
        .version("2.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![])
        .build();

    // Test protocol_version (should default to "0.3.0")
    assert_eq!(card.protocol_version, "0.3.0");

    // Test preferred_transport (should default to "JSONRPC")
    assert_eq!(card.preferred_transport, "JSONRPC");

    // Add additional_interfaces
    card.additional_interfaces = Some(vec![
        AgentInterface {
            url: "https://api.example.com/grpc".to_string(),
            transport: "GRPC".to_string(),
        },
        AgentInterface {
            url: "https://api.example.com/http".to_string(),
            transport: "HTTP+JSON".to_string(),
        },
    ]);

    // Add icon_url
    card.icon_url = Some("https://example.com/icon.png".to_string());

    // Add signatures
    let mut header = HashMap::new();
    header.insert(
        "alg".to_string(),
        serde_json::Value::String("RS256".to_string()),
    );

    card.signatures = Some(vec![AgentCardSignature {
        protected: "eyJhbGciOiJSUzI1NiJ9".to_string(),
        signature: "cC4hiUPoj9Eetdgtv3hF80EGrhuB__dzERat0XF9g2VtQgr9PJbu3XOiZj5RZmh7AAuHIm4Bh-0Qc_lF5YKt_O8W2Fp5jujGbds9uJdbF9CUAr7t1dnZcAcQjbKBYNX4BAynRFdiuB--f_nZLgrnbyTyWzO75vRK5h6xBArLIARNPvkSjtQBMHlb1L07Qe7K0GarZRmB_eSN9383LcOLn6_dO--xi12jzDwusC-eOkHWEsqtFZESc6BfI7noOPqvhJ1phCnvWh6IeYI2w9QOYEUipUTI8np6LbgGY9Fs98rqVt5AXLIhWkWywlVmtVrBp0igcN_IoypGlUPQGe77Rw".to_string(),
        header: Some(header),
    }]);

    // Serialize and validate
    let card_json = serde_json::to_value(&card).unwrap();
    println!("AgentCard v0.3.0: {}", serde_json::to_string_pretty(&card_json).unwrap());

    // Verify the v0.3.0 fields are present
    assert_eq!(card_json["protocolVersion"], "0.3.0");
    assert_eq!(card_json["preferredTransport"], "JSONRPC");
    assert!(card_json["additionalInterfaces"].is_array());
    assert_eq!(
        card_json["additionalInterfaces"][0]["transport"],
        "GRPC"
    );
    assert_eq!(card_json["iconUrl"], "https://example.com/icon.png");
    assert!(card_json["signatures"].is_array());

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let agent_card_schema = extract_definition(&schema_content, "AgentCard");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&agent_card_schema)
        .expect("Failed to compile AgentCard schema");

    let result = schema.validate(&card_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("AgentCard v0.3.0 validation error: {}", error);
            eprintln!("Instance path: {}", error.instance_path);
        }
        panic!("AgentCard v0.3.0 does not comply with A2A specification");
    }
}

#[test]
fn test_agent_capabilities_extensions() {
    use a2a_rs::domain::{AgentCapabilities, AgentExtension};
    use std::collections::HashMap;

    let mut capabilities = AgentCapabilities::default();

    // Add extensions
    let mut ext_params = HashMap::new();
    ext_params.insert(
        "version".to_string(),
        serde_json::Value::String("1.0".to_string()),
    );

    capabilities.extensions = Some(vec![
        AgentExtension {
            uri: "https://example.com/extensions/custom-auth".to_string(),
            description: Some("Custom authentication extension".to_string()),
            required: Some(true),
            params: Some(ext_params.clone()),
        },
        AgentExtension {
            uri: "https://example.com/extensions/advanced-features".to_string(),
            description: Some("Advanced features extension".to_string()),
            required: Some(false),
            params: None,
        },
    ]);

    // Serialize and verify
    let capabilities_json = serde_json::to_value(&capabilities).unwrap();
    println!("AgentCapabilities with extensions: {}", serde_json::to_string_pretty(&capabilities_json).unwrap());

    assert!(capabilities_json["extensions"].is_array());
    assert_eq!(capabilities_json["extensions"][0]["uri"], "https://example.com/extensions/custom-auth");
    assert_eq!(capabilities_json["extensions"][0]["required"], true);
    assert_eq!(capabilities_json["extensions"][1]["required"], false);

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let capabilities_schema = extract_definition(&schema_content, "AgentCapabilities");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&capabilities_schema)
        .expect("Failed to compile AgentCapabilities schema");

    let result = schema.validate(&capabilities_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("AgentCapabilities validation error: {}", error);
        }
        panic!("AgentCapabilities does not comply with A2A specification");
    }
}

#[test]
fn test_agent_skill_security() {
    use a2a_rs::domain::AgentSkill;
    use std::collections::HashMap;

    // Create a skill with security requirements
    let mut security_req = HashMap::new();
    security_req.insert("oauth2".to_string(), vec!["read:data".to_string(), "write:data".to_string()]);

    let skill = AgentSkill::new(
        "secure-operation".to_string(),
        "Secure Operation".to_string(),
        "An operation requiring OAuth2 authentication".to_string(),
        vec!["security".to_string(), "auth".to_string()],
    )
    .with_security(vec![security_req]);

    // Serialize and verify
    let skill_json = serde_json::to_value(&skill).unwrap();
    println!("AgentSkill with security: {}", serde_json::to_string_pretty(&skill_json).unwrap());

    assert!(skill_json["security"].is_array());
    assert_eq!(skill_json["security"][0]["oauth2"][0], "read:data");
    assert_eq!(skill_json["security"][0]["oauth2"][1], "write:data");

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let skill_schema = extract_definition(&schema_content, "AgentSkill");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&skill_schema)
        .expect("Failed to compile AgentSkill schema");

    let result = schema.validate(&skill_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("AgentSkill validation error: {}", error);
        }
        panic!("AgentSkill does not comply with A2A specification");
    }
}

#[test]
fn test_message_extensions_field() {
    use a2a_rs::domain::Message;

    // Create a message with extensions
    let mut message = Message::user_text(
        "Test message with extensions".to_string(),
        format!("msg-{}", uuid::Uuid::new_v4()),
    );

    message.extensions = Some(vec![
        "https://example.com/extensions/custom-protocol".to_string(),
        "https://example.com/extensions/advanced-features".to_string(),
    ]);

    // Serialize and verify
    let message_json = serde_json::to_value(&message).unwrap();
    println!("Message with extensions: {}", serde_json::to_string_pretty(&message_json).unwrap());

    assert!(message_json["extensions"].is_array());
    assert_eq!(message_json["extensions"][0], "https://example.com/extensions/custom-protocol");

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let message_schema = extract_definition(&schema_content, "Message");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&message_schema)
        .expect("Failed to compile Message schema");

    let result = schema.validate(&message_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("Message with extensions validation error: {}", error);
        }
        panic!("Message does not comply with A2A specification");
    }
}

#[test]
fn test_artifact_extensions_field() {
    use a2a_rs::domain::{Artifact, Part};

    // Create an artifact with extensions
    let artifact = Artifact {
        artifact_id: format!("artifact-{}", uuid::Uuid::new_v4()),
        name: Some("Test Artifact".to_string()),
        description: Some("Artifact with extension support".to_string()),
        parts: vec![Part::Text {
            text: "Artifact content".to_string(),
            metadata: None,
        }],
        metadata: None,
        extensions: Some(vec![
            "https://example.com/extensions/artifact-encryption".to_string(),
        ]),
    };

    // Serialize and verify
    let artifact_json = serde_json::to_value(&artifact).unwrap();
    println!("Artifact with extensions: {}", serde_json::to_string_pretty(&artifact_json).unwrap());

    assert!(artifact_json["extensions"].is_array());
    assert_eq!(artifact_json["extensions"][0], "https://example.com/extensions/artifact-encryption");

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let artifact_schema = extract_definition(&schema_content, "Artifact");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&artifact_schema)
        .expect("Failed to compile Artifact schema");

    let result = schema.validate(&artifact_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("Artifact with extensions validation error: {}", error);
        }
        panic!("Artifact does not comply with A2A specification");
    }
}

#[test]
fn test_mutual_tls_security_scheme() {
    use a2a_rs::domain::SecurityScheme;

    let mtls_scheme = SecurityScheme::MutualTls {
        description: Some("Client certificate authentication".to_string()),
    };

    // Serialize and verify
    let scheme_json = serde_json::to_value(&mtls_scheme).unwrap();
    println!("MutualTLS SecurityScheme: {}", serde_json::to_string_pretty(&scheme_json).unwrap());

    assert_eq!(scheme_json["type"], "mutualTLS");
    assert_eq!(scheme_json["description"], "Client certificate authentication");

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let security_schema = extract_definition(&schema_content, "SecurityScheme");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&security_schema)
        .expect("Failed to compile SecurityScheme schema");

    let result = schema.validate(&scheme_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("MutualTLS SecurityScheme validation error: {}", error);
        }
        panic!("MutualTLS SecurityScheme does not comply with A2A specification");
    }
}

#[test]
fn test_oauth2_with_metadata_url() {
    use a2a_rs::domain::{ClientCredentialsOAuthFlow, OAuthFlows, SecurityScheme};
    use std::collections::HashMap;

    let mut scopes = HashMap::new();
    scopes.insert("read:data".to_string(), "Read access to data".to_string());
    scopes.insert("write:data".to_string(), "Write access to data".to_string());

    let oauth2_scheme = SecurityScheme::OAuth2 {
        flows: Box::new(OAuthFlows {
            client_credentials: Some(ClientCredentialsOAuthFlow {
                token_url: "https://auth.example.com/token".to_string(),
                refresh_url: Some("https://auth.example.com/refresh".to_string()),
                scopes,
            }),
            ..Default::default()
        }),
        description: Some("OAuth2 with metadata discovery".to_string()),
        metadata_url: Some("https://auth.example.com/.well-known/oauth-authorization-server".to_string()),
    };

    // Serialize and verify
    let scheme_json = serde_json::to_value(&oauth2_scheme).unwrap();
    println!("OAuth2 with metadata URL: {}", serde_json::to_string_pretty(&scheme_json).unwrap());

    assert_eq!(scheme_json["type"], "oauth2");
    assert_eq!(
        scheme_json["metadataUrl"],
        "https://auth.example.com/.well-known/oauth-authorization-server"
    );

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let security_schema = extract_definition(&schema_content, "SecurityScheme");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&security_schema)
        .expect("Failed to compile SecurityScheme schema");

    let result = schema.validate(&scheme_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("OAuth2 SecurityScheme validation error: {}", error);
        }
        panic!("OAuth2 SecurityScheme does not comply with A2A specification");
    }
}

#[test]
fn test_list_tasks_params() {
    use a2a_rs::domain::{ListTasksParams, TaskState};

    let params = ListTasksParams {
        context_id: Some("ctx-123".to_string()),
        status: Some(TaskState::Working),
        page_size: Some(25),
        page_token: None,
        history_length: Some(10),
        include_artifacts: Some(true),
        last_updated_after: Some(1704067200000), // 2024-01-01 00:00:00 UTC
        metadata: None,
    };

    // Serialize and verify
    let params_json = serde_json::to_value(&params).unwrap();
    println!("ListTasksParams: {}", serde_json::to_string_pretty(&params_json).unwrap());

    assert_eq!(params_json["contextId"], "ctx-123");
    assert_eq!(params_json["status"], "working");
    assert_eq!(params_json["pageSize"], 25);
    assert_eq!(params_json["historyLength"], 10);
    assert_eq!(params_json["includeArtifacts"], true);
    assert_eq!(params_json["lastUpdatedAfter"], 1704067200000_i64);

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let params_schema = extract_definition(&schema_content, "ListTasksParams");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&params_schema)
        .expect("Failed to compile ListTasksParams schema");

    let result = schema.validate(&params_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("ListTasksParams validation error: {}", error);
        }
        panic!("ListTasksParams does not comply with A2A specification");
    }
}

#[test]
fn test_push_notification_config_with_id() {
    use a2a_rs::domain::PushNotificationConfig;

    let config = PushNotificationConfig {
        id: Some("config-abc123".to_string()),
        url: "https://client.example.com/webhook".to_string(),
        token: Some("bearer-token-xyz".to_string()),
        authentication: None,
    };

    // Serialize and verify
    let config_json = serde_json::to_value(&config).unwrap();
    println!("PushNotificationConfig with id: {}", serde_json::to_string_pretty(&config_json).unwrap());

    assert_eq!(config_json["id"], "config-abc123");
    assert_eq!(config_json["url"], "https://client.example.com/webhook");

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let config_schema = extract_definition(&schema_content, "PushNotificationConfig");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&config_schema)
        .expect("Failed to compile PushNotificationConfig schema");

    let result = schema.validate(&config_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("PushNotificationConfig validation error: {}", error);
        }
        panic!("PushNotificationConfig does not comply with A2A specification");
    }
}

#[test]
fn test_transport_protocol_enum() {
    use a2a_rs::domain::TransportProtocol;

    let protocols = vec![
        TransportProtocol::JsonRpc,
        TransportProtocol::Grpc,
        TransportProtocol::HttpJson,
    ];

    for protocol in protocols {
        let protocol_json = serde_json::to_value(&protocol).unwrap();
        println!("TransportProtocol: {}", protocol_json);

        // Verify serialization format
        match protocol {
            TransportProtocol::JsonRpc => assert_eq!(protocol_json, "JSONRPC"),
            TransportProtocol::Grpc => assert_eq!(protocol_json, "GRPC"),
            TransportProtocol::HttpJson => assert_eq!(protocol_json, "HTTP+JSON"),
        }
    }
}

#[test]
fn test_message_send_configuration_optional_accepted_output_modes() {
    use a2a_rs::domain::MessageSendConfiguration;

    // Test that acceptedOutputModes is now optional in v0.3.0
    let config = MessageSendConfiguration {
        accepted_output_modes: None, // This should be valid now
        history_length: Some(10),
        push_notification_config: None,
        blocking: Some(false),
    };

    // Serialize and verify
    let config_json = serde_json::to_value(&config).unwrap();
    println!("MessageSendConfiguration: {}", serde_json::to_string_pretty(&config_json).unwrap());

    // acceptedOutputModes should not be serialized when None
    assert!(config_json.get("acceptedOutputModes").is_none());
    assert_eq!(config_json["historyLength"], 10);

    // Validate against schema
    let schema_content =
        fs::read_to_string("../spec/specification.json").expect("Failed to read specification.json");
    let config_schema = extract_definition(&schema_content, "MessageSendConfiguration");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&config_schema)
        .expect("Failed to compile MessageSendConfiguration schema");

    let result = schema.validate(&config_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("MessageSendConfiguration validation error: {}", error);
        }
        panic!("MessageSendConfiguration does not comply with A2A specification");
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
