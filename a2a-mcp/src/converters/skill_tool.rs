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

        if !skill.examples.is_empty() {
            description_parts.push(format!("\n\nExamples:\n- {}", skill.examples.join("\n- ")));
        }

        if !skill.input_modes.is_empty() {
            description_parts.push(format!(
                "\nSupported input modes: {}",
                skill.input_modes.join(", ")
            ));
        }

        if !skill.output_modes.is_empty() {
            description_parts.push(format!(
                "\nSupported output modes: {}",
                skill.output_modes.join(", ")
            ));
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
                },
                "task_id": {
                    "type": "string",
                    "description": "Optional. The ID of an existing task to continue. Omit when starting a new task."
                }
            },
            "required": ["message"]
        }))
        .expect("Failed to parse schema JSON");

        Tool::new(
            tool_name,
            full_description,
            std::sync::Arc::new(input_schema),
        )
    }

    /// The agent URL as a name prefix: scheme dropped, `/`, `:` and `.`
    /// replaced by `_`. One place, shared with artifact URIs.
    pub fn sanitize_namespace(agent_url: &str) -> String {
        agent_url
            .replace("https://", "")
            .replace("http://", "")
            .replace(['/', ':', '.'], "_")
    }

    /// Create a namespaced tool name
    ///
    /// Format: `{sanitized_agent_url}_{skill_id}`, with `agent_` in front when
    /// the sanitized URL does not start with a letter or an underscore — an
    /// address like `http://127.0.0.1:8081` did, and a function name that
    /// starts with a digit is rejected by Gemini (and is not a valid
    /// identifier for anything), which failed every request from a model that
    /// had this agent's tools in front of it.
    pub fn create_tool_name(agent_url: &str, skill_id: &str) -> String {
        let namespace = Self::sanitize_namespace(agent_url);
        let starts_well = namespace
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if starts_well {
            format!("{namespace}_{skill_id}")
        } else {
            format!("agent_{namespace}_{skill_id}")
        }
    }

    /// The skill a tool (or prompt) name refers to, among `skills` served under
    /// `agent_url`: the one whose generated name is exactly `tool_name`.
    ///
    /// This is how a bridge should answer a call. Matching against the names
    /// it generated is exact by construction; [`parse_tool_name`] is
    /// best-effort and splits at the last underscore, so a skill id with an
    /// underscore in it (`my_skill`) parsed back as `skill` and was "not
    /// found" for as long as the bridge parsed.
    ///
    /// [`parse_tool_name`]: Self::parse_tool_name
    pub fn resolve_skill<'a>(
        skills: &'a [AgentSkill],
        agent_url: &str,
        tool_name: &str,
    ) -> Option<&'a AgentSkill> {
        skills
            .iter()
            .find(|skill| Self::create_tool_name(agent_url, &skill.id) == tool_name)
    }

    /// Parse a tool name back into agent URL and skill ID
    ///
    /// This is a best-effort operation and may not perfectly reverse the
    /// sanitization: it splits at the *last* underscore, so a skill id that
    /// contains one comes back cut. Prefer [`resolve_skill`](Self::resolve_skill)
    /// wherever the skills are at hand.
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
        let skill = AgentSkill::new(
            "test_skill".to_string(),
            "Test Skill".to_string(),
            "A test skill for demonstration".to_string(),
            vec!["test".to_string()],
        )
        .with_examples(vec!["Example 1".to_string(), "Example 2".to_string()])
        .with_input_modes(vec!["text".to_string()])
        .with_output_modes(vec!["text".to_string()]);

        let tool = SkillToolConverter::skill_to_tool(&skill, "https://example.com/agent");

        assert!(tool.name.contains("test_skill"));
        assert!(tool.description.is_some());
        assert!(tool.description.as_ref().unwrap().contains("A test skill"));
        assert!(tool.description.as_ref().unwrap().contains("Example 1"));
    }

    #[test]
    fn test_tool_name_creation() {
        let name =
            SkillToolConverter::create_tool_name("https://example.com:8080/api/agent", "my_skill");
        assert_eq!(name, "example_com_8080_api_agent_my_skill");
    }

    /// A loopback address gave a name starting with a digit, which is not a
    /// function name anywhere and which Gemini refuses outright.
    #[test]
    fn a_tool_name_starts_with_a_letter_whatever_the_address() {
        assert_eq!(
            SkillToolConverter::create_tool_name("http://127.0.0.1:8081", "greet"),
            "agent_127_0_0_1_8081_greet"
        );
        // And a name that already does is left alone.
        assert_eq!(
            SkillToolConverter::create_tool_name("https://tools.example", "greet"),
            "tools_example_greet"
        );
    }

    /// A call is answered by the skill whose generated name it carries —
    /// including one with an underscore in its id, which parsing cut.
    #[test]
    fn a_call_resolves_to_its_skill_by_the_generated_name() {
        let skills = vec![
            AgentSkill::new("my_skill".into(), "Mine".into(), "d".into(), vec![]),
            AgentSkill::new("skill".into(), "Other".into(), "d".into(), vec![]),
        ];
        let url = "http://127.0.0.1:8081";
        let name = SkillToolConverter::create_tool_name(url, "my_skill");
        assert_eq!(
            SkillToolConverter::resolve_skill(&skills, url, &name).map(|s| s.id.as_str()),
            Some("my_skill")
        );
        assert_eq!(
            SkillToolConverter::resolve_skill(&skills, url, "nobody_asked_for_this"),
            None
        );
    }

    #[test]
    fn test_parse_tool_name() {
        let (agent_id, skill_id) =
            SkillToolConverter::parse_tool_name("example_com_8080_api_agent_my_skill").unwrap();
        assert_eq!(agent_id, "example_com_8080_api_agent_my");
        assert_eq!(skill_id, "skill");
    }
}
