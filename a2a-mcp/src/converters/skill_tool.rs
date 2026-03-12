//! Converter between A2A AgentSkill and MCP Tool

use crate::error::{A2aMcpError, Result};
use a2a_rs::domain::AgentSkill;
use rmcp::model::Tool;
use serde_json::json;

/// Converts between A2A AgentSkill and MCP Tool
pub struct SkillToolConverter;

impl SkillToolConverter {
    /// Convert an A2A AgentSkill to an MCP Tool
    ///
    /// The skill's name becomes the tool name, and the description is preserved.
    /// Input/output modes and examples are embedded in the description.
    pub fn skill_to_tool(skill: &AgentSkill, agent_url: &str) -> Tool {
        // Create a unique tool name by combining agent URL and skill ID
        let tool_name = Self::create_tool_name(agent_url, &skill.id);

        // Build enhanced description
        let mut description_parts = vec![skill.description.clone()];

        if let Some(examples) = &skill.examples {
            if !examples.is_empty() {
                description_parts.push(format!("\n\nExamples:\n{}", examples.join("\n- ")));
            }
        }

        if let Some(input_modes) = &skill.input_modes {
            description_parts.push(format!("\nSupported input modes: {}", input_modes.join(", ")));
        }

        if let Some(output_modes) = &skill.output_modes {
            description_parts.push(format!("\nSupported output modes: {}", output_modes.join(", ")));
        }

        let full_description = description_parts.join("");

        // Create a simple input schema for the message parameter
        // Using serde_json to create the schema JSON directly
        let input_schema = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message or query to send to the agent skill"
                }
            },
            "required": ["message"]
        }))
        .expect("Failed to parse schema JSON");

        Tool {
            name: tool_name.into(),
            title: None,
            description: Some(full_description.into()),
            input_schema: std::sync::Arc::new(input_schema),
            output_schema: None,
            annotations: None,
            icons: None,
            meta: None,
        }
    }

    /// Create a namespaced tool name
    ///
    /// Format: `{sanitized_agent_url}_{skill_id}`
    pub fn create_tool_name(agent_url: &str, skill_id: &str) -> String {
        // Sanitize the agent URL to create a valid tool name prefix
        let sanitized_url = agent_url
            .replace("https://", "")
            .replace("http://", "")
            .replace('/', "_")
            .replace(':', "_")
            .replace('.', "_");

        format!("{}_{}", sanitized_url, skill_id)
    }

    /// Parse a tool name back into agent URL and skill ID
    ///
    /// This is a best-effort operation and may not perfectly reverse the sanitization
    pub fn parse_tool_name(tool_name: &str) -> Result<(String, String)> {
        // Find the last underscore to split agent identifier from skill ID
        let parts: Vec<&str> = tool_name.rsplitn(2, '_').collect();

        if parts.len() != 2 {
            return Err(A2aMcpError::InvalidToolCall(format!(
                "Invalid tool name format: {}",
                tool_name
            )));
        }

        // parts[0] is skill_id, parts[1] is agent identifier (reversed due to rsplitn)
        Ok((parts[1].to_string(), parts[0].to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_to_tool_conversion() {
        let skill = AgentSkill {
            id: "test_skill".to_string(),
            name: "Test Skill".to_string(),
            description: "A test skill for demonstration".to_string(),
            tags: vec!["test".to_string()],
            examples: Some(vec!["Example 1".to_string(), "Example 2".to_string()]),
            input_modes: Some(vec!["text".to_string()]),
            output_modes: Some(vec!["text".to_string()]),
            security: None,
        };

        let tool = SkillToolConverter::skill_to_tool(&skill, "https://example.com/agent");

        assert!(tool.name.contains("test_skill"));
        assert!(tool.description.is_some());
        assert!(tool.description.as_ref().unwrap().contains("A test skill"));
        assert!(tool.description.as_ref().unwrap().contains("Example 1"));
    }

    #[test]
    fn test_tool_name_creation() {
        let name = SkillToolConverter::create_tool_name("https://example.com:8080/api/agent", "my_skill");
        assert_eq!(name, "example_com_8080_api_agent_my_skill");
    }

    #[test]
    fn test_parse_tool_name() {
        let (agent_id, skill_id) =
            SkillToolConverter::parse_tool_name("example_com_8080_api_agent_my_skill").unwrap();
        assert_eq!(agent_id, "example_com_8080_api_agent_my");
        assert_eq!(skill_id, "skill");
    }
}
