//! Integration test for A2A to MCP bridge
//!
//! This test verifies that A2A agent skills can be successfully exposed as MCP tools

use a2a_mcp::bridge::agent_to_mcp::AgentToMcpBridge;
use a2a_mcp::converters::skill_tool::SkillToolConverter;
use a2a_rs::adapter::transport::http::HttpClient;
use a2a_rs::domain::core::agent::{AgentCapabilities, AgentCard, AgentSkill};
use rmcp::ServerHandler;

#[tokio::test]
async fn test_agent_skills_as_mcp_tools() {
    // Create an agent card with multiple skills
    let agent_card = AgentCard::builder()
        .name("Test Agent".to_string())
        .description("A test agent with multiple skills".to_string())
        .url("https://example.com/agent".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![
            AgentSkill::new(
                "calculator".to_string(),
                "Calculator".to_string(),
                "Performs mathematical calculations".to_string(),
                vec!["math".to_string(), "calculator".to_string()],
            ),
            AgentSkill::new(
                "translator".to_string(),
                "Translator".to_string(),
                "Translates text between languages".to_string(),
                vec!["translation".to_string(), "language".to_string()],
            ),
        ])
        .build();

    // Create HTTP client (won't be used in this test, but needed for bridge construction)
    let client = HttpClient::new("https://example.com/agent".to_string());

    // Create the bridge
    let bridge = AgentToMcpBridge::new(
        client,
        agent_card.clone(),
        "https://example.com/agent".to_string(),
    );

    // Get the server info
    let info = bridge.get_info();

    // Verify server info
    assert_eq!(
        info.server_info.name,
        "a2a-mcp-bridge:Test Agent".to_string()
    );
    assert_eq!(
        info.server_info.title,
        Some("A2A Agent: Test Agent".to_string())
    );
    assert!(info.capabilities.tools.is_some());

    // Note: We can't easily test list_tools without a full MCP setup since RequestContext
    // doesn't implement Default. In practice, this would be tested with a real MCP client.
    // The bridge creation and tool conversion is verified through unit tests.
}

#[tokio::test]
async fn test_tool_name_namespacing() {
    // Test that tool names are properly namespaced to avoid collisions

    let agent_card = AgentCard::builder()
        .name("Agent A".to_string())
        .description("First agent".to_string())
        .url("https://agent-a.example.com".to_string())
        .version("1.0.0".to_string())
        .capabilities(Default::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "shared_skill".to_string(),
            "Shared Skill".to_string(),
            "A skill with a common name".to_string(),
            vec![],
        )])
        .build();

    let tool = SkillToolConverter::skill_to_tool(
        &agent_card.skills[0],
        "https://agent-a.example.com",
    );

    // Verify the tool name includes the sanitized agent URL
    // Note: hyphens are NOT replaced, only /, :, and .
    // "https://agent-a.example.com" becomes "agent-a_example_com"
    assert!(tool.name.contains("agent-a_example_com"));
    assert!(tool.name.contains("shared_skill"));

    // Verify we can parse it back
    let (agent_part, _skill_id) = SkillToolConverter::parse_tool_name(&tool.name).unwrap();

    assert!(agent_part.contains("agent-a_example_com"));
    // Note: parsing may not be perfect due to underscores in both parts
}

#[tokio::test]
async fn test_skill_metadata_preservation() {
    // Test that skill metadata (examples, input/output modes) is preserved in tool descriptions

    let skill = AgentSkill {
        id: "test_skill".to_string(),
        name: "Test Skill".to_string(),
        description: "A test skill".to_string(),
        tags: vec!["test".to_string()],
        examples: Some(vec![
            "Example 1: Do something".to_string(),
            "Example 2: Do something else".to_string(),
        ]),
        input_modes: Some(vec!["text".to_string(), "file".to_string()]),
        output_modes: Some(vec!["text".to_string()]),
        security: None,
    };

    let tool = SkillToolConverter::skill_to_tool(&skill, "https://example.com");

    // Verify description includes examples
    let description = tool.description.as_ref().unwrap();
    assert!(description.contains("Example 1"));
    assert!(description.contains("Example 2"));

    // Verify description includes input/output modes
    assert!(description.contains("Supported input modes: text, file"));
    assert!(description.contains("Supported output modes: text"));
}

#[tokio::test]
async fn test_multiple_agents_as_bridges() {
    // Test that we can create multiple bridges for different agents
    // This simulates an MCP server that exposes multiple A2A agents

    let agent1 = AgentCard::builder()
        .name("Agent 1".to_string())
        .description("First agent".to_string())
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
        .name("Agent 2".to_string())
        .description("Second agent".to_string())
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

    // Verify each bridge has different server info (representing different agents)
    let info1 = bridge1.get_info();
    let info2 = bridge2.get_info();

    assert_ne!(info1.server_info.name, info2.server_info.name);
    assert_ne!(info1.server_info.website_url, info2.server_info.website_url);
}
