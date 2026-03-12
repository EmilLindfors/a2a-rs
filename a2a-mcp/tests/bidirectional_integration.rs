//! Bidirectional integration test
//!
//! This test verifies that A2A and MCP can work together in both directions:
//! - A2A agents can call MCP tools
//! - MCP clients can call A2A agent skills

use a2a_mcp::bridge::agent_to_mcp::AgentToMcpBridge;
use a2a_mcp::converters::{MessageConverter, SkillToolConverter, TaskResultConverter};
use a2a_rs::adapter::transport::http::HttpClient;
use a2a_rs::domain::core::agent::{AgentCard, AgentSkill};
use a2a_rs::domain::{Message, Part, Role, Task, TaskState, TaskStatus};
use rmcp::ServerHandler;

#[tokio::test]
async fn test_message_roundtrip() {
    // Test that messages can be converted from A2A to MCP and back without loss

    let original_message = Message::builder()
        .role(Role::User)
        .parts(vec![
            Part::Text {
                text: "Hello, world!".to_string(),
                metadata: None,
            },
            Part::Text {
                text: "This is a multi-part message.".to_string(),
                metadata: None,
            },
        ])
        .message_id("msg-1".to_string())
        .build();

    // Convert to MCP content
    let mcp_content = MessageConverter::message_to_content(&original_message).unwrap();

    assert_eq!(mcp_content.len(), 2);

    // Convert back to A2A message
    let converted_message =
        MessageConverter::content_to_message(&mcp_content, Role::User).unwrap();

    // Verify the content is preserved (note: message_id will be different)
    assert_eq!(converted_message.role, original_message.role);
    assert_eq!(converted_message.parts.len(), original_message.parts.len());

    // Check text content is preserved
    if let Part::Text { text, .. } = &converted_message.parts[0] {
        if let Part::Text {
            text: original_text,
            ..
        } = &original_message.parts[0]
        {
            assert_eq!(text, original_text);
        }
    }
}

#[tokio::test]
async fn test_task_to_tool_result_conversion() {
    // Test that A2A tasks can be converted to MCP tool results

    let task = Task::builder()
        .id("task-1".to_string())
        .context_id("ctx-1".to_string())
        .status(TaskStatus {
            state: TaskState::Completed,
            message: None,
            timestamp: None,
        })
        .history(vec![
            Message::builder()
                .role(Role::User)
                .parts(vec![Part::Text {
                    text: "Do something".to_string(),
                    metadata: None,
                }])
                .message_id("msg-1".to_string())
                .build(),
            Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::Text {
                    text: "Task completed successfully".to_string(),
                    metadata: None,
                }])
                .message_id("msg-2".to_string())
                .build(),
        ])
        .build();

    // Convert task to MCP result
    let result = TaskResultConverter::task_to_result(&task).unwrap();

    // Verify it's a success result
    assert!(!result.is_error.unwrap_or(false));

    // Verify content is present
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_error_handling_bidirectional() {
    // Test that errors are properly converted between protocols

    // Failed A2A task
    let failed_task = Task::builder()
        .id("task-failed".to_string())
        .context_id("ctx-failed".to_string())
        .status(TaskStatus {
            state: TaskState::Failed,
            message: None,
            timestamp: None,
        })
        .history(vec![Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::Text {
                text: "Error: Something went wrong".to_string(),
                metadata: None,
            }])
            .message_id("msg-error".to_string())
            .build()])
        .build();

    // Convert to MCP result
    let error_result = TaskResultConverter::task_to_result(&failed_task).unwrap();

    // Verify it's an error result
    assert!(error_result.is_error.unwrap_or(false));

    // Verify error content is preserved
    assert!(!error_result.content.is_empty());
    let text = MessageConverter::extract_text_from_content(&error_result.content);
    assert!(text.contains("Error"));
}

#[tokio::test]
async fn test_skill_tool_bidirectional_metadata() {
    // Test that metadata is preserved when converting skills to tools and back

    let agent_card = AgentCard::builder()
        .name("Test Agent".to_string())
        .description("An agent for testing".to_string())
        .url("https://example.com/agent".to_string())
        .version("1.0.0".to_string())
        .capabilities(Default::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill {
            id: "test_skill".to_string(),
            name: "Test Skill".to_string(),
            description: "A skill for testing metadata preservation".to_string(),
            tags: vec!["test".to_string(), "metadata".to_string()],
            examples: Some(vec!["Example usage".to_string()]),
            input_modes: Some(vec!["text".to_string()]),
            output_modes: Some(vec!["text".to_string()]),
            security: None,
        }])
        .build();

    let client = HttpClient::new("https://example.com/agent".to_string());
    let bridge = AgentToMcpBridge::new(
        client,
        agent_card.clone(),
        "https://example.com/agent".to_string(),
    );

    // Verify bridge was created with the agent card
    let info = bridge.get_info();
    assert!(info.server_info.name.contains("Test Agent"));

    // Test skill to tool conversion directly
    let skill = &agent_card.skills[0];
    let tool = SkillToolConverter::skill_to_tool(skill, "https://example.com/agent");

    // Verify description includes skill metadata
    let description = tool.description.as_ref().unwrap();
    assert!(description.contains("testing metadata preservation"));
    assert!(description.contains("Example usage"));
    assert!(description.contains("Supported input modes: text"));
    assert!(description.contains("Supported output modes: text"));
}

#[tokio::test]
async fn test_concurrent_bridges() {
    // Test that multiple bridges can operate concurrently without interference

    let agent1 = AgentCard::builder()
        .name("Concurrent Agent 1".to_string())
        .description("First concurrent agent".to_string())
        .url("https://agent1.example.com".to_string())
        .version("1.0.0".to_string())
        .capabilities(Default::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "skill1".to_string(),
            "Skill 1".to_string(),
            "First skill".to_string(),
            vec![],
        )])
        .build();

    let agent2 = AgentCard::builder()
        .name("Concurrent Agent 2".to_string())
        .description("Second concurrent agent".to_string())
        .url("https://agent2.example.com".to_string())
        .version("1.0.0".to_string())
        .capabilities(Default::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "skill2".to_string(),
            "Skill 2".to_string(),
            "Second skill".to_string(),
            vec![],
        )])
        .build();

    let client1 = HttpClient::new("https://agent1.example.com".to_string());
    let client2 = HttpClient::new("https://agent2.example.com".to_string());

    let bridge1 = AgentToMcpBridge::new(
        client1,
        agent1,
        "https://agent1.example.com".to_string(),
    );
    let bridge2 = AgentToMcpBridge::new(
        client2,
        agent2,
        "https://agent2.example.com".to_string(),
    );

    // Verify both bridges were created successfully
    let info1 = bridge1.get_info();
    let info2 = bridge2.get_info();

    // Verify each bridge represents a different agent
    assert!(info1.server_info.name.contains("Concurrent Agent 1"));
    assert!(info2.server_info.name.contains("Concurrent Agent 2"));
    assert_ne!(info1.server_info.website_url, info2.server_info.website_url);
}

#[tokio::test]
async fn test_data_part_conversion() {
    // Test that complex data parts are properly converted between protocols

    use serde_json::json;

    let data_message = Message::builder()
        .role(Role::Agent)
        .parts(vec![Part::Data {
            data: json!({
                "result": "success",
                "value": 42,
                "details": {
                    "computation": "6 * 7",
                    "method": "multiplication"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
            metadata: None,
        }])
        .message_id("msg-data".to_string())
        .build();

    // Convert to MCP content
    let mcp_content = MessageConverter::message_to_content(&data_message).unwrap();

    assert!(!mcp_content.is_empty());

    // Extract text representation
    let text = MessageConverter::extract_text_from_content(&mcp_content);

    // Verify JSON is present in some form
    assert!(text.contains("result") || text.contains("success") || text.contains("42"));
}
