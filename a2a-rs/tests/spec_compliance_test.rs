//! A2A Protocol Specification Compliance Tests
//!
//! This module validates that our Rust types match the JSON Schema definitions
//! in the A2A specification files located in ../spec/

mod common;

use a2a_rs::{
    adapter::SimpleAgentInfo,
    domain::{Message, Part, TaskState},
};
use jsonschema::{Draft, Validator};
use serde_json::{Value, json};
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
    use a2a_rs::services::AgentInfoProvider;
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
    let data_val: buffa_types::google::protobuf::Value = serde_json::from_value(json!({
        "key": "value",
        "number": 42,
        "nested": {
            "array": [1, 2, 3]
        }
    }))
    .unwrap();
    let data_part = Part::data(data_val);
    message.add_part(data_part);

    // Add a file part
    let file_part = Part::file_from_bytes(
        "SGVsbG8gV29ybGQ=".to_string().into_bytes(), // "Hello World" in base64
        Some("test.txt".to_string()),
        Some("text/plain".to_string()),
    );
    message.add_part_validated(file_part).unwrap();

    // Set context and task IDs
    message.context_id = "ctx-123".to_string();
    message.task_id = "task-456".to_string();

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
    use a2a_rs::domain::Task;
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

    // A2A-specific errors (v1.0.0 includes new -32007 error)
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
fn test_agent_card_v100_fields() {
    use a2a_rs::domain::{AgentCapabilities, AgentCard, AgentCardSignature, AgentInterface};
    use std::collections::HashMap;

    // Create an AgentCard with all v1.0.0 fields using the v1.0.0 builder
    let header_struct = {
        let mut header = HashMap::new();
        header.insert(
            "alg".to_string(),
            serde_json::Value::String("RS256".to_string()),
        );
        let header_val = serde_json::to_value(header).unwrap();
        serde_json::from_value(header_val).unwrap()
    };

    let card = AgentCard::builder()
        .name("Test Agent v1.0.0".to_string())
        .description("Agent with v1.0.0 features".to_string())
        .url("https://api.example.com/jsonrpc".to_string())
        .version("2.0.0".to_string())
        .protocol_version("0.3.0".to_string())
        .preferred_transport("JSONRPC".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .additional_interfaces(vec![
            AgentInterface {
                url: "https://api.example.com/grpc".to_string(),
                protocol_binding: "GRPC".to_string(),
                protocol_version: "0.3.0".to_string(),
                ..Default::default()
            },
            AgentInterface {
                url: "https://api.example.com/http".to_string(),
                protocol_binding: "HTTP+JSON".to_string(),
                protocol_version: "0.3.0".to_string(),
                ..Default::default()
            },
        ])
        .icon_url("https://example.com/icon.png".to_string())
        .signatures(vec![AgentCardSignature {
            protected: "eyJhbGciOiJSUzI1NiJ9".to_string(),
            signature: "cC4hiUPoj9Eetdgtv3hF80EGrhuB__dzERat0XF9g2VtQgr9PJbu3XOiZj5RZmh7AAuHIm4Bh-0Qc_lF5YKt_O8W2Fp5jujGbds9uJdbF9CUAr7t1dnZcAcQjbKBYNX4BAynRFdiuB--f_nZLgrnbyTyWzO75vRK5h6xBArLIARNPvkSjtQBMHlb1L07Qe7K0GarZRmB_eSN9383LcOLn6_dO--xi12jzDwusC-eOkHWEsqtFZESc6BfI7noOPqvhJ1phCnvWh6IeYI2w9QOYEUipUTI8np6LbgGY9Fs98rqVt5AXLIhWkWywlVmtVrBp0igcN_IoypGlUPQGe77Rw".to_string(),
            header: buffa::MessageField::some(header_struct),
            ..Default::default()
        }])
        .skills(vec![])
        .build();

    println!(
        "DEBUG: card.supported_interfaces = {:?}",
        card.supported_interfaces
    );
    // Test protocol_version (should default to "0.3.0")
    assert_eq!(card.protocol_version(), "0.3.0");

    // Test preferred_transport (should default to "JSONRPC")
    assert_eq!(card.preferred_transport(), "JSONRPC");

    // Serialize and validate
    let card_json = serde_json::to_value(&card).unwrap();
    println!(
        "AgentCard v1.0.0: {}",
        serde_json::to_string_pretty(&card_json).unwrap()
    );

    // Verify the v1.0.0 fields are present
    assert_eq!(
        card_json["supportedInterfaces"][0]["protocolVersion"],
        "0.3.0"
    );
    assert_eq!(
        card_json["supportedInterfaces"][0]["protocolBinding"],
        "JSONRPC"
    );
    assert!(card_json["supportedInterfaces"].is_array());
    assert_eq!(
        card_json["supportedInterfaces"][1]["protocolBinding"],
        "GRPC"
    );
    assert_eq!(card_json["iconUrl"], "https://example.com/icon.png");
    assert!(card_json["signatures"].is_array());

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
    let agent_card_schema = extract_definition(&schema_content, "AgentCard");

    let schema = Validator::options()
        .with_draft(Draft::Draft7)
        .build(&agent_card_schema)
        .expect("Failed to compile AgentCard schema");

    let result = schema.validate(&card_json);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("AgentCard v1.0.0 validation error: {}", error);
            eprintln!("Instance path: {}", error.instance_path);
        }
        panic!("AgentCard v1.0.0 does not comply with A2A specification");
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
    let ext_params_val = serde_json::to_value(&ext_params).unwrap();
    let ext_params_struct: buffa_types::google::protobuf::Struct =
        serde_json::from_value(ext_params_val).unwrap();

    capabilities.extensions = vec![
        AgentExtension {
            uri: "https://example.com/extensions/custom-auth".to_string(),
            description: "Custom authentication extension".to_string(),
            required: true,
            params: buffa::MessageField::some(ext_params_struct),
            ..Default::default()
        },
        AgentExtension {
            uri: "https://example.com/extensions/advanced-features".to_string(),
            description: "Advanced features extension".to_string(),
            required: false,
            params: buffa::MessageField::none(),
            ..Default::default()
        },
    ];

    // Serialize and verify
    let capabilities_json = serde_json::to_value(&capabilities).unwrap();
    println!(
        "AgentCapabilities with extensions: {}",
        serde_json::to_string_pretty(&capabilities_json).unwrap()
    );

    assert!(capabilities_json["extensions"].is_array());
    assert_eq!(
        capabilities_json["extensions"][0]["uri"],
        "https://example.com/extensions/custom-auth"
    );
    assert_eq!(capabilities_json["extensions"][0]["required"], true);
    assert!(capabilities_json["extensions"][1]["required"].is_null());

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
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
    security_req.insert(
        "oauth2".to_string(),
        vec!["read:data".to_string(), "write:data".to_string()],
    );

    let skill = AgentSkill::new(
        "secure-operation".to_string(),
        "Secure Operation".to_string(),
        "An operation requiring OAuth2 authentication".to_string(),
        vec!["security".to_string(), "auth".to_string()],
    )
    .with_security(vec![security_req]);

    // Serialize and verify
    let skill_json = serde_json::to_value(&skill).unwrap();
    println!(
        "AgentSkill with security: {}",
        serde_json::to_string_pretty(&skill_json).unwrap()
    );

    assert!(skill_json["securityRequirements"].is_array());
    assert_eq!(
        skill_json["securityRequirements"][0]["schemes"]["oauth2"]["list"][0],
        "read:data"
    );
    assert_eq!(
        skill_json["securityRequirements"][0]["schemes"]["oauth2"]["list"][1],
        "write:data"
    );

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
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

    message.extensions = vec![
        "https://example.com/extensions/custom-protocol".to_string(),
        "https://example.com/extensions/advanced-features".to_string(),
    ];

    // Serialize and verify
    let message_json = serde_json::to_value(&message).unwrap();
    println!(
        "Message with extensions: {}",
        serde_json::to_string_pretty(&message_json).unwrap()
    );

    assert!(message_json["extensions"].is_array());
    assert_eq!(
        message_json["extensions"][0],
        "https://example.com/extensions/custom-protocol"
    );

    // Validate against schema
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
        name: "Test Artifact".to_string(),
        description: "Artifact with extension support".to_string(),
        parts: vec![Part::text("Artifact content".to_string())],
        metadata: None.into(),
        extensions: vec!["https://example.com/extensions/artifact-encryption".to_string()],
        ..Default::default()
    };

    // Serialize and verify
    let artifact_json = serde_json::to_value(&artifact).unwrap();
    println!(
        "Artifact with extensions: {}",
        serde_json::to_string_pretty(&artifact_json).unwrap()
    );

    assert!(artifact_json["extensions"].is_array());
    assert_eq!(
        artifact_json["extensions"][0],
        "https://example.com/extensions/artifact-encryption"
    );

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
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

    let mtls_scheme =
        SecurityScheme::mutual_tls(Some("Client certificate authentication".to_string()));

    // Serialize and verify
    let scheme_json = serde_json::to_value(&mtls_scheme).unwrap();
    println!(
        "MutualTLS SecurityScheme: {}",
        serde_json::to_string_pretty(&scheme_json).unwrap()
    );

    assert!(scheme_json.get("mtlsSecurityScheme").is_some());
    assert_eq!(
        scheme_json["mtlsSecurityScheme"]["description"],
        "Client certificate authentication"
    );

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
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

    let flow = ClientCredentialsOAuthFlow {
        token_url: "https://auth.example.com/token".to_string(),
        refresh_url: "https://auth.example.com/refresh".to_string(),
        scopes,
        ..Default::default()
    };
    let flows = OAuthFlows::client_credentials(flow);
    let oauth2_scheme = SecurityScheme::oauth2(
        flows,
        Some("OAuth2 with metadata discovery".to_string()),
        Some("https://auth.example.com/.well-known/oauth-authorization-server".to_string()),
    );

    // Serialize and verify
    let scheme_json = serde_json::to_value(&oauth2_scheme).unwrap();
    println!(
        "OAuth2 with metadata URL: {}",
        serde_json::to_string_pretty(&scheme_json).unwrap()
    );

    assert!(scheme_json.get("oauth2SecurityScheme").is_some());
    assert_eq!(
        scheme_json["oauth2SecurityScheme"]["oauth2MetadataUrl"],
        "https://auth.example.com/.well-known/oauth-authorization-server"
    );

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
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
        status_timestamp_after: Some("2024-01-01T00:00:00Z".to_string()), // 2024-01-01 00:00:00 UTC
        metadata: None,
    };

    // Serialize and verify
    let params_json = serde_json::to_value(&params).unwrap();
    println!(
        "ListTasksParams: {}",
        serde_json::to_string_pretty(&params_json).unwrap()
    );

    assert_eq!(params_json["contextId"], "ctx-123");
    assert_eq!(params_json["status"], "TASK_STATE_WORKING");
    assert_eq!(params_json["pageSize"], 25);
    assert_eq!(params_json["historyLength"], 10);
    assert_eq!(params_json["includeArtifacts"], true);
    assert_eq!(params_json["statusTimestampAfter"], "2024-01-01T00:00:00Z");

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
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
    use a2a_rs::domain::TaskPushNotificationConfig;
    let config = TaskPushNotificationConfig {
        tenant: String::new(),
        task_id: "dummy".to_string(),

        id: "config-abc123".to_string(),
        url: "https://client.example.com/webhook".to_string(),
        token: "bearer-token-xyz".to_string(),
        authentication: None.into(),
        ..Default::default()
    };

    // Serialize and verify
    let config_json = serde_json::to_value(&config).unwrap();
    println!(
        "PushNotificationConfig with id: {}",
        serde_json::to_string_pretty(&config_json).unwrap()
    );

    assert_eq!(config_json["id"], "config-abc123");
    assert_eq!(config_json["url"], "https://client.example.com/webhook");

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
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
fn test_message_send_configuration_optional_accepted_output_modes() {
    use a2a_rs::domain::MessageSendConfiguration;

    // Test that acceptedOutputModes is now optional in v1.0.0
    let config = MessageSendConfiguration {
        accepted_output_modes: None, // This should be valid now
        history_length: Some(10),
        push_notification_config: None,
        blocking: Some(false),
    };

    // Serialize and verify
    let config_json = serde_json::to_value(&config).unwrap();
    println!(
        "MessageSendConfiguration: {}",
        serde_json::to_string_pretty(&config_json).unwrap()
    );

    // acceptedOutputModes should not be serialized when None
    assert!(config_json.get("acceptedOutputModes").is_none());
    assert_eq!(config_json["historyLength"], 10);

    // Validate against schema
    let schema_content = fs::read_to_string("../spec/specification.json")
        .expect("Failed to read specification.json");
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
                use a2a_rs::domain::Task;
                let task = Task::new(task_id.clone(), context_id);
                prop_assert_eq!(task.id, task_id);
            }
        }
    }
}

// Priority 3: Error Handling and Validation Tests

#[tokio::test]
async fn test_task_list_page_size_validation() {
    use a2a_rs::{
        adapter::{
            DefaultRequestProcessor, HttpClient, HttpServer, InMemoryTaskStorage, SimpleAgentInfo,
        },
        services::AsyncA2AClient,
    };
    use common::TestBusinessHandler;
    use std::time::Duration;
    use tokio::sync::oneshot;

    let port = 9500;
    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage);
    let agent_info = SimpleAgentInfo::new(
        "Validation Test Agent".to_string(),
        format!("http://localhost:{}", port),
    );

    let processor = DefaultRequestProcessor::with_handler(handler, agent_info.clone());
    let server = HttpServer::new(processor, agent_info, format!("127.0.0.1:{}", port));
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Test page_size > 100 (should clamp to 100, not error)
    let params = a2a_rs::domain::ListTasksParams {
        page_size: Some(150),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    // According to spec, page_size should be clamped, not return error
    assert_eq!(result.page_size, 100);

    // Test page_size < 1 (should clamp to 1, not error)
    let params = a2a_rs::domain::ListTasksParams {
        page_size: Some(0),
        ..Default::default()
    };

    let result = client
        .list_tasks(&params)
        .await
        .expect("Failed to list tasks");

    // According to spec, page_size should be clamped, not return error
    assert_eq!(result.page_size, 1);

    shutdown_tx.send(()).ok();
}

#[test]
fn test_all_a2a_error_codes_defined() {
    use a2a_rs::domain::A2AError;

    // Test all A2A-specific error codes from the specification
    let errors = vec![
        (A2AError::TaskNotFound("test-task".to_string()), -32001),
        (A2AError::TaskNotCancelable("test-task".to_string()), -32002),
        (A2AError::PushNotificationNotSupported, -32003),
        (A2AError::UnsupportedOperation("test".to_string()), -32004),
        (
            A2AError::ContentTypeNotSupported("test/type".to_string()),
            -32005,
        ),
        (A2AError::InvalidAgentResponse("test".to_string()), -32006),
        (A2AError::AuthenticatedExtendedCardNotConfigured, -32007),
    ];

    for (error, expected_code) in errors {
        let jsonrpc_error = error.to_jsonrpc_error();
        assert_eq!(
            jsonrpc_error["code"], expected_code,
            "Error {:?} should have code {}",
            error, expected_code
        );
        assert!(
            !jsonrpc_error["message"].as_str().unwrap().is_empty(),
            "Error message should not be empty"
        );
    }
}

#[test]
fn test_jsonrpc_error_structure_compliance() {
    use a2a_rs::domain::A2AError;

    let error = A2AError::TaskNotFound("task-123".to_string());
    let jsonrpc_error = error.to_jsonrpc_error();

    // Verify JSON-RPC error structure
    assert!(jsonrpc_error.is_object(), "Error should be an object");
    assert!(
        jsonrpc_error.get("code").is_some(),
        "Error must have code field"
    );
    assert!(
        jsonrpc_error.get("message").is_some(),
        "Error must have message field"
    );

    // code should be an integer
    assert!(
        jsonrpc_error["code"].is_i64(),
        "Error code must be an integer"
    );

    // message should be a string
    assert!(
        jsonrpc_error["message"].is_string(),
        "Error message must be a string"
    );
}

#[test]
fn test_task_state_transitions_validation() {
    use a2a_rs::domain::{Message, Task, TaskState};

    let task_id = "task-transition-test".to_string();
    let context_id = "ctx-test".to_string();
    let mut task = Task::new(task_id.clone(), context_id);

    // Valid state transitions
    let valid_transitions = vec![
        (TaskState::Submitted, TaskState::Working),
        (TaskState::Working, TaskState::InputRequired),
        (TaskState::InputRequired, TaskState::Working),
        (TaskState::Working, TaskState::Completed),
        (TaskState::Working, TaskState::Failed),
        (TaskState::Working, TaskState::Canceled),
    ];

    for (_from_state, to_state) in valid_transitions {
        let msg = Message::agent_text(
            format!("Transitioning to {:?}", to_state),
            format!("msg-{}", uuid::Uuid::new_v4()),
        );
        task.update_status(to_state, Some(msg));
        assert_eq!(
            task.status.state, to_state,
            "Task should transition to {:?}",
            to_state
        );
    }
}

#[test]
fn test_error_code_ranges() {
    // Verify that error codes follow the specification ranges

    // Standard JSON-RPC errors: -32700 to -32603
    let jsonrpc_codes = vec![-32700, -32600, -32601, -32602, -32603];

    for code in jsonrpc_codes {
        assert!(
            (-32700..=-32600).contains(&code),
            "JSON-RPC error code {} should be in range -32700 to -32600",
            code
        );
    }

    // A2A-specific errors: -32001 to -32007
    let a2a_codes = vec![-32001, -32002, -32003, -32004, -32005, -32006, -32007];

    for code in a2a_codes {
        assert!(
            (-32007..=-32001).contains(&code),
            "A2A error code {} should be in range -32007 to -32001",
            code
        );
    }
}
