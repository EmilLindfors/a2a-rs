//! CONSTRUCT Prompt Generator using ggen-inspired patterns
//!
//! Generates bounded, deterministic AI agent prompts using Tera templates

use serde::{Deserialize, Serialize};
use tera::{Context, Tera};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintSpec {
    pub sentence_count: String,
    pub max_chars: usize,
    pub behavioral_rules: Vec<String>,
    pub output_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub parameters: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPersona {
    pub role: String,
    pub description: String,
    pub constraints: ConstraintSpec,
    pub tools: Option<Vec<ToolSpec>>,
}

pub struct ConstructGenerator {
    tera: Tera,
}

impl ConstructGenerator {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut tera = Tera::default();

        // Base CONSTRUCT template
        tera.add_raw_template(
            "construct_base",
            r#"CONSTRUCT: {{ role }}.
CONSTRAINTS:
{% for rule in constraints.behavioral_rules -%}
- {{ rule }}
{% endfor -%}
- Exactly {{ constraints.sentence_count }} per response
- Maximum {{ constraints.max_chars }} characters total
{% if tools -%}
TOOLS: {% for tool in tools %}{{ tool.name }}({{ tool.parameters }}){% if not loop.last %}, {% endif %}{% endfor %}
{% endif -%}
OUTPUT: {{ constraints.output_format }}"#,
        )?;

        // Tool-enabled agent template
        tera.add_raw_template(
            "construct_tool_agent",
            r#"CONSTRUCT: {{ description }}
CONSTRAINTS:
{% for rule in constraints.behavioral_rules -%}
- {{ rule }}
{% endfor -%}
- Exactly {{ constraints.sentence_count }} sentences
- Maximum {{ constraints.max_chars }} characters total
- {{ tool_usage_requirement }}
TOOLS: {% for tool in tools %}{{ tool.name }}({{ tool.parameters }}){% if not loop.last %}, {% endif %}{% endfor %}
OUTPUT: {{ constraints.output_format }}"#,
        )?;

        // Debate agent template
        tera.add_raw_template(
            "construct_debate",
            r#"CONSTRUCT: {{ description }}
CONSTRAINTS:
- Exactly {{ constraints.sentence_count }} per argument
- Maximum {{ constraints.max_chars }} characters total
- {{ position_statement }}
- {{ evidence_requirement }}
- Remain respectful and evidence-based
- Address opponent's claims directly
OUTPUT: {{ constraints.output_format }}"#,
        )?;

        // Socratic dialogue template
        tera.add_raw_template(
            "construct_socratic",
            r#"CONSTRUCT: {{ description }}
CONSTRAINTS:
- Exactly {{ constraints.sentence_count }} per response
- Maximum {{ constraints.max_chars }} characters total
- {{ question_requirement }}
- Reference previous context
- {{ reasoning_style }}
OUTPUT: {{ constraints.output_format }}"#,
        )?;

        Ok(Self { tera })
    }

    /// Generate a CONSTRUCT prompt from a persona specification
    pub fn generate(&self, persona: &AgentPersona) -> Result<String, Box<dyn std::error::Error>> {
        let mut context = Context::new();
        context.insert("role", &persona.role);
        context.insert("description", &persona.description);
        context.insert("constraints", &persona.constraints);

        if let Some(tools) = &persona.tools {
            context.insert("tools", tools);
        }

        // Always use construct_base template for the simple generate method
        Ok(self.tera.render("construct_base", &context)?)
    }

    /// Generate a tool-enabled agent CONSTRUCT prompt
    pub fn generate_tool_agent(
        &self,
        description: &str,
        constraints: &ConstraintSpec,
        tools: &[ToolSpec],
        tool_usage_requirement: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut context = Context::new();
        context.insert("description", description);
        context.insert("constraints", constraints);
        context.insert("tools", tools);
        context.insert("tool_usage_requirement", tool_usage_requirement);

        Ok(self.tera.render("construct_tool_agent", &context)?)
    }

    /// Generate a debate agent CONSTRUCT prompt
    pub fn generate_debate_agent(
        &self,
        description: &str,
        constraints: &ConstraintSpec,
        position_statement: &str,
        evidence_requirement: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut context = Context::new();
        context.insert("description", description);
        context.insert("constraints", constraints);
        context.insert("position_statement", position_statement);
        context.insert("evidence_requirement", evidence_requirement);

        Ok(self.tera.render("construct_debate", &context)?)
    }

    /// Generate a Socratic dialogue agent CONSTRUCT prompt
    pub fn generate_socratic_agent(
        &self,
        description: &str,
        constraints: &ConstraintSpec,
        question_requirement: &str,
        reasoning_style: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut context = Context::new();
        context.insert("description", description);
        context.insert("constraints", constraints);
        context.insert("question_requirement", question_requirement);
        context.insert("reasoning_style", reasoning_style);

        Ok(self.tera.render("construct_socratic", &context)?)
    }
}

impl Default for ConstructGenerator {
    fn default() -> Self {
        Self::new().expect("Failed to create ConstructGenerator")
    }
}

/// Preset persona generators
pub mod presets {
    use super::*;

    pub fn math_agent() -> AgentPersona {
        AgentPersona {
            role: "Mathematical computation assistant".to_string(),
            description: "Mathematical computation assistant with bounded operations".to_string(),
            constraints: ConstraintSpec {
                sentence_count: "2-3 sentences".to_string(),
                max_chars: 150,
                behavioral_rules: vec![
                    "Use calculate tool for ALL numeric operations".to_string(),
                    "Show calculation then explain".to_string(),
                    "No approximations, exact values only".to_string(),
                ],
                output_format: "Tool result + brief explanation".to_string(),
            },
            tools: Some(vec![ToolSpec {
                name: "Calculate".to_string(),
                parameters: "a, b, operation".to_string(),
            }]),
        }
    }

    pub fn researcher_agent() -> AgentPersona {
        AgentPersona {
            role: "Information retrieval specialist".to_string(),
            description: "Information retrieval specialist with bounded search".to_string(),
            constraints: ConstraintSpec {
                sentence_count: "2-3 sentences".to_string(),
                max_chars: 180,
                behavioral_rules: vec![
                    "Use search tool for information gathering".to_string(),
                    "Cite tool results explicitly".to_string(),
                    "Focus on key facts only".to_string(),
                ],
                output_format: "Synthesized facts from search results".to_string(),
            },
            tools: Some(vec![ToolSpec {
                name: "Search".to_string(),
                parameters: "query".to_string(),
            }]),
        }
    }

    pub fn socrates_agent() -> AgentPersona {
        AgentPersona {
            role: "Socratic questioner".to_string(),
            description: "You are Socrates using the Socratic method".to_string(),
            constraints: ConstraintSpec {
                sentence_count: "2-3 sentences".to_string(),
                max_chars: 200,
                behavioral_rules: vec![
                    "Ask exactly 1 probing question".to_string(),
                    "Reference previous context".to_string(),
                    "No explanations, only questions".to_string(),
                ],
                output_format: "One question that challenges assumptions".to_string(),
            },
            tools: None,
        }
    }

    pub fn student_agent() -> AgentPersona {
        AgentPersona {
            role: "Thoughtful student".to_string(),
            description: "You are a thoughtful student exploring ideas".to_string(),
            constraints: ConstraintSpec {
                sentence_count: "2-3 sentences".to_string(),
                max_chars: 200,
                behavioral_rules: vec![
                    "Build on previous point".to_string(),
                    "Ask 1 follow-up question".to_string(),
                    "Show reasoning process".to_string(),
                ],
                output_format: "Insight + question that advances dialogue".to_string(),
            },
            tools: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_math_agent() {
        let gen = ConstructGenerator::new().unwrap();
        let persona = presets::math_agent();
        let prompt = gen.generate(&persona).unwrap();

        assert!(prompt.contains("CONSTRUCT:"));
        assert!(prompt.contains("CONSTRAINTS:"));
        assert!(prompt.contains("TOOLS:"));
        assert!(prompt.contains("OUTPUT:"));
    }

    #[test]
    fn test_generate_socrates() {
        let gen = ConstructGenerator::new().unwrap();
        let persona = presets::socrates_agent();
        let prompt = gen.generate(&persona).unwrap();

        assert!(prompt.contains("Socratic method"));
        assert!(prompt.contains("200 characters"));
    }
}
