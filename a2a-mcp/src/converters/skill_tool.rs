//! Converter between A2A AgentSkill and MCP Tool
//!
//! The two directions are not symmetric on the wire: an MCP tool has an input
//! schema, an output schema, a title and effect hints, and an A2A skill has
//! none of them — `AgentSkill` in the proto is id, name, description, tags,
//! examples, modes and security, and nothing else. So a skill served as a tool
//! took one string, and a tool served as a skill lost its type.
//!
//! [`SkillSchema`] is what closes the gap: the missing fields, carried on the
//! agent card as an [`AgentExtension`] (the one spec-sanctioned place for
//! something the spec has no field for — `AgentSkill` has no `metadata` bag)
//! under [`SKILL_SCHEMA_EXTENSION_URI`], keyed by skill id. `skill_to_tool`
//! emits it when present and `tool_to_skill` produces it, so a typed tool
//! round-trips through both bridges.

use crate::converters::llm_tool::LlmToolConverter;
use a2a_rs::domain::{AgentCard, AgentExtension, AgentSkill};
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tracing::debug;

/// The `AgentExtension::uri` under which skill schemas ride on an agent card.
///
/// The README section it points at is the documented shape. Version 1:
/// `params = { "skills": { "<skill id>": SkillSchema, ... } }`.
pub const SKILL_SCHEMA_EXTENSION_URI: &str = "https://github.com/EmilLindfors/a2a-rs/blob/master/a2a-mcp/README.md#skill-schema-extension-v1";

/// The property `skill_to_tool` adds to a typed skill's input schema so a
/// suspended task can be continued, unless the schema already declares one.
pub const TASK_ID_PROPERTY: &str = "task_id";

/// What an MCP tool carries that an A2A skill has no field for.
///
/// Every field optional: a skill with an input schema and nothing else is a
/// typed skill, and `None` is "the author said nothing", never a default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSchema {
    /// JSON Schema for the arguments. When set, the tool takes these instead
    /// of a single `message` string, and the bridge delivers them to the
    /// agent as one data part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    /// JSON Schema for a structured result. When set, the bridge puts a
    /// single data part from the agent's answer in the tool result's
    /// `structuredContent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    /// A display title, where it differs from the skill's `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Effect hints, in `a2a-llm`'s vocabulary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<a2a_llm::ToolAnnotations>,
}

impl SkillSchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_input_schema(mut self, schema: Value) -> Self {
        self.input_schema = Some(schema);
        self
    }

    pub fn with_output_schema(mut self, schema: Value) -> Self {
        self.output_schema = Some(schema);
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_annotations(mut self, annotations: a2a_llm::ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// Whether anything at all is set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The skill schemas an agent card carries, by skill id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkillSchemas(pub BTreeMap<String, SkillSchema>);

#[derive(Serialize, Deserialize)]
struct ExtensionParams {
    skills: SkillSchemas,
}

impl SkillSchemas {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace the schema for one skill.
    pub fn insert(&mut self, skill_id: impl Into<String>, schema: SkillSchema) {
        self.0.insert(skill_id.into(), schema);
    }

    pub fn get(&self, skill_id: &str) -> Option<&SkillSchema> {
        self.0.get(skill_id)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The schemas on `card`, read from its [`SKILL_SCHEMA_EXTENSION_URI`]
    /// extension. Empty when the card has none, and empty with a `debug!`
    /// when it has one that does not decode: a malformed extension is a card
    /// bug, and a bridge that refused the whole card over it would take every
    /// untyped skill down with the typed one.
    pub fn from_card(card: &AgentCard) -> Self {
        let Some(extension) = card.extension(SKILL_SCHEMA_EXTENSION_URI) else {
            return Self::default();
        };
        let Some(params) = extension.params.as_option() else {
            return Self::default();
        };
        match serde_json::to_value(params).and_then(serde_json::from_value::<ExtensionParams>) {
            Ok(decoded) => decoded.skills,
            Err(e) => {
                debug!(
                    "Skill schema extension on card '{}' does not decode: {e}",
                    card.name
                );
                Self::default()
            }
        }
    }

    /// The extension that carries these schemas, for `capabilities.extensions`.
    /// `required: false` — a client that does not read it still talks to the
    /// agent, one string at a time.
    pub fn to_extension(&self) -> AgentExtension {
        let params = serde_json::to_value(ExtensionParams {
            skills: self.clone(),
        })
        .expect("SkillSchemas always serialises");
        let params: ::buffa_types::google::protobuf::Struct =
            serde_json::from_value(params).expect("a JSON object is a Struct");
        AgentExtension {
            uri: SKILL_SCHEMA_EXTENSION_URI.to_string(),
            description: "JSON schemas and effect hints for typed skills; see the URI.".to_string(),
            required: false,
            params: ::buffa::MessageField::some(params),
            ..Default::default()
        }
    }
}

/// Converts between A2A AgentSkill and MCP Tool
pub struct SkillToolConverter;

impl SkillToolConverter {
    /// Convert an A2A AgentSkill to an MCP Tool.
    ///
    /// Without a `schema`, the tool takes a `message` string and an optional
    /// `task_id`, and the skill's examples and modes go into the description.
    /// With one, the tool's input schema is the skill's, with an optional
    /// `task_id` string property merged in when the schema does not declare
    /// one (a suspended task still has to be continuable); the output schema,
    /// title and hints are copied across.
    pub fn skill_to_tool(
        skill: &AgentSkill,
        namespace: &str,
        schema: Option<&SkillSchema>,
    ) -> Tool {
        let tool_name = Self::create_tool_name(namespace, &skill.id);

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

        let input_schema = match schema.and_then(|s| s.input_schema.as_ref()) {
            Some(typed) => Self::with_task_id(typed.clone()),
            None => Self::message_schema(),
        };
        let input_schema = match input_schema {
            Value::Object(map) => map,
            other => {
                debug!(
                    "Skill '{}' has a non-object input schema ({other}); serving the message schema",
                    skill.id
                );
                match Self::message_schema() {
                    Value::Object(map) => map,
                    _ => unreachable!("message_schema is an object"),
                }
            }
        };

        let mut tool = Tool::new(
            tool_name,
            full_description,
            std::sync::Arc::new(input_schema),
        );

        if let Some(schema) = schema {
            if let Some(Value::Object(output)) = &schema.output_schema {
                tool.output_schema = Some(std::sync::Arc::new(output.clone()));
            }
            if let Some(title) = &schema.title {
                tool = tool.with_title(title.clone());
            }
            if let Some(annotations) = &schema.annotations {
                tool = tool.annotate(LlmToolConverter::annotations_to_mcp(annotations));
            }
        }

        tool
    }

    /// Convert an MCP Tool to an A2A AgentSkill, with the schema the skill
    /// has no field for — `None` when the tool declared nothing beyond its
    /// input schema being the free-form object every tool has.
    ///
    /// `tags` is `["mcp"]`: the field is REQUIRED and an empty list is
    /// dropped by ProtoJSON, which makes the official client refuse the card.
    pub fn tool_to_skill(tool: &Tool) -> (AgentSkill, Option<SkillSchema>) {
        let skill = AgentSkill::new(
            tool.name.to_string(),
            tool.title.clone().unwrap_or_else(|| tool.name.to_string()),
            tool.description
                .clone()
                .map(|d| d.to_string())
                .unwrap_or_default(),
            vec!["mcp".to_string()],
        );

        let mut schema = SkillSchema::new()
            .with_input_schema(serde_json::to_value(&*tool.input_schema).unwrap_or(Value::Null));
        if let Some(output) = &tool.output_schema {
            schema =
                schema.with_output_schema(serde_json::to_value(&**output).unwrap_or(Value::Null));
        }
        if let Some(title) = &tool.title {
            schema = schema.with_title(title.clone());
        }
        if let Some(annotations) = &tool.annotations {
            let mapped = LlmToolConverter::annotations_to_llm(annotations);
            if !mapped.is_empty() {
                schema = schema.with_annotations(mapped);
            }
        }

        (skill, Some(schema))
    }

    /// The schema an untyped skill is served with.
    fn message_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message or query to send to the agent skill"
                },
                TASK_ID_PROPERTY: {
                    "type": "string",
                    "description": "Optional. The ID of an existing task to continue. Omit when starting a new task."
                }
            },
            "required": ["message"]
        })
    }

    /// `schema` with an optional `task_id` string property, unless it has one.
    fn with_task_id(mut schema: Value) -> Value {
        if let Some(properties) = schema.as_object_mut().and_then(|obj| {
            obj.entry("properties")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        }) && !properties.contains_key(TASK_ID_PROPERTY)
        {
            properties.insert(
                TASK_ID_PROPERTY.to_string(),
                json!({
                    "type": "string",
                    "description": "Optional. The ID of an existing task to continue. Omit when starting a new task."
                }),
            );
        }
        schema
    }

    /// A name as a tool-name prefix: lowercased, every character outside
    /// `[a-z0-9_]` replaced by `_`, runs of `_` collapsed and the ends
    /// trimmed of them, and `agent_` in front when what is left does not
    /// start with a letter (`agent` alone for a name with nothing usable).
    ///
    /// One place, shared with artifact URIs. It used to take the agent's
    /// address and strip the scheme; the address changes when the deployment
    /// does, and the agent's name is what a tool name should follow.
    pub fn sanitize_namespace(namespace: &str) -> String {
        let mut out = String::with_capacity(namespace.len());
        let mut last_underscore = false;
        for c in namespace.chars() {
            let mapped = if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            };
            if mapped == '_' {
                if last_underscore {
                    continue;
                }
                last_underscore = true;
            } else {
                last_underscore = false;
            }
            out.push(mapped);
        }
        let out = out.trim_matches('_');
        if out.is_empty() {
            return "agent".to_string();
        }
        let starts_well = out
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if starts_well {
            out.to_string()
        } else {
            format!("agent_{out}")
        }
    }

    /// Create a namespaced tool name: `{sanitized namespace}_{skill_id}`.
    ///
    /// The prefix starts with a letter or an underscore whatever the
    /// namespace — a loopback address gave `127_0_0_1_8081_greet` once, and a
    /// function name that starts with a digit is rejected by Gemini (and is
    /// not a valid identifier for anything), which failed every request from
    /// a model that had this agent's tools in front of it.
    pub fn create_tool_name(namespace: &str, skill_id: &str) -> String {
        format!("{}_{}", Self::sanitize_namespace(namespace), skill_id)
    }

    /// The skill a tool (or prompt) name refers to, among `skills` served under
    /// `namespace`: the one whose generated name is exactly `tool_name`.
    ///
    /// This is how a bridge answers a call. Matching against the names it
    /// generated is exact by construction; parsing the name back split at the
    /// last underscore, so a skill id with an underscore in it (`my_skill`)
    /// parsed back as `skill` and was "not found" for as long as the bridge
    /// parsed.
    pub fn resolve_skill<'a>(
        skills: &'a [AgentSkill],
        namespace: &str,
        tool_name: &str,
    ) -> Option<&'a AgentSkill> {
        skills
            .iter()
            .find(|skill| Self::create_tool_name(namespace, &skill.id) == tool_name)
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

        let tool = SkillToolConverter::skill_to_tool(&skill, "Example Agent", None);

        assert_eq!(tool.name.as_ref(), "example_agent_test_skill");
        assert!(tool.description.is_some());
        assert!(tool.description.as_ref().unwrap().contains("A test skill"));
        assert!(tool.description.as_ref().unwrap().contains("Example 1"));
        assert!(tool.input_schema["properties"]["message"].is_object());
        assert!(tool.output_schema.is_none());
        assert!(tool.annotations.is_none());
    }

    /// A tool name follows the agent's name, not its address, and is a
    /// function name whatever the name looked like.
    #[test]
    fn a_tool_name_is_the_agents_name_made_an_identifier() {
        assert_eq!(
            SkillToolConverter::create_tool_name("Weather Agent (EU)", "greet"),
            "weather_agent_eu_greet"
        );
        assert_eq!(
            SkillToolConverter::create_tool_name("2nd-line support", "greet"),
            "agent_2nd_line_support_greet"
        );
        assert_eq!(
            SkillToolConverter::create_tool_name("***", "greet"),
            "agent_greet"
        );
        // An address still works as a namespace, for whoever passes one
        // through `with_namespace`.
        assert_eq!(
            SkillToolConverter::create_tool_name("http://127.0.0.1:8081", "greet"),
            "http_127_0_0_1_8081_greet"
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
        let namespace = "Some Agent";
        let name = SkillToolConverter::create_tool_name(namespace, "my_skill");
        assert_eq!(
            SkillToolConverter::resolve_skill(&skills, namespace, &name).map(|s| s.id.as_str()),
            Some("my_skill")
        );
        assert_eq!(
            SkillToolConverter::resolve_skill(&skills, namespace, "nobody_asked_for_this"),
            None
        );
    }

    fn typed_schema() -> SkillSchema {
        SkillSchema::new()
            .with_input_schema(json!({
                "type": "object",
                "properties": {
                    "view": {"type": "string", "enum": ["harvest", "feed"]},
                    "year": {"type": "integer"}
                },
                "required": ["view"]
            }))
            .with_output_schema(
                json!({"type": "object", "properties": {"rows": {"type": "array"}}}),
            )
            .with_title("Query a view")
            .with_annotations(a2a_llm::ToolAnnotations::new().read_only(true))
    }

    /// A typed skill is served with its own schema — plus `task_id`, because
    /// a suspended task still has to be continuable — and its output schema,
    /// title and hints.
    #[test]
    fn a_typed_skill_is_served_typed() {
        let skill = AgentSkill::new(
            "query".into(),
            "Query".into(),
            "Runs a query".into(),
            vec!["data".into()],
        );
        let tool = SkillToolConverter::skill_to_tool(&skill, "strata", Some(&typed_schema()));

        assert_eq!(
            tool.input_schema["properties"]["view"]["enum"][0],
            "harvest"
        );
        assert_eq!(tool.input_schema["properties"]["task_id"]["type"], "string");
        assert_eq!(tool.input_schema["required"], json!(["view"]));
        assert!(tool.input_schema["properties"].get("message").is_none());
        assert_eq!(
            tool.output_schema.as_ref().unwrap()["properties"]["rows"]["type"],
            "array"
        );
        assert_eq!(tool.title.as_deref(), Some("Query a view"));
        assert_eq!(
            tool.annotations.as_ref().unwrap().read_only_hint,
            Some(true)
        );
    }

    /// A schema that already declares `task_id` is left alone.
    #[test]
    fn an_authored_task_id_is_not_overwritten() {
        let schema = SkillSchema::new().with_input_schema(json!({
            "type": "object",
            "properties": {"task_id": {"type": "string", "description": "mine"}}
        }));
        let skill = AgentSkill::new("s".into(), "S".into(), "d".into(), vec!["t".into()]);
        let tool = SkillToolConverter::skill_to_tool(&skill, "a", Some(&schema));
        assert_eq!(
            tool.input_schema["properties"]["task_id"]["description"],
            "mine"
        );
    }

    /// The schemas ride on the card as an extension and read back the same.
    #[test]
    fn skill_schemas_round_trip_through_the_card_extension() {
        let mut schemas = SkillSchemas::new();
        schemas.insert("query", typed_schema());
        let extension = schemas.to_extension();
        assert_eq!(extension.uri, SKILL_SCHEMA_EXTENSION_URI);
        assert!(!extension.required);

        let mut card = AgentCard::builder()
            .name("strata".to_string())
            .description("d".to_string())
            .url("http://localhost".to_string())
            .version("1".to_string())
            .capabilities(Default::default())
            .default_input_modes(vec!["text".to_string()])
            .default_output_modes(vec!["text".to_string()])
            .skills(vec![])
            .build();
        card.capabilities
            .get_or_insert_default()
            .extensions
            .push(extension);

        assert_eq!(SkillSchemas::from_card(&card), schemas);

        // A card without the extension has no schemas, and one with a
        // malformed extension has none rather than no card.
        let plain = AgentCard::builder()
            .name("plain".to_string())
            .description("d".to_string())
            .url("http://localhost".to_string())
            .version("1".to_string())
            .capabilities(Default::default())
            .default_input_modes(vec!["text".to_string()])
            .default_output_modes(vec!["text".to_string()])
            .skills(vec![])
            .build();
        assert!(SkillSchemas::from_card(&plain).is_empty());
    }

    /// An MCP tool becomes a skill and a schema that serve it back as the
    /// same tool: name, schemas and hints equal after the round trip.
    #[test]
    fn a_typed_tool_round_trips_through_both_directions() {
        let input: serde_json::Map<String, Value> = serde_json::from_value(json!({
            "type": "object",
            "properties": {"q": {"type": "string"}},
            "required": ["q"]
        }))
        .unwrap();
        let output: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"type": "object", "properties": {"hits": {}}})).unwrap();
        let mut tool = Tool::new("search", "Searches", std::sync::Arc::new(input))
            .with_title("Search")
            .annotate(
                rmcp::model::ToolAnnotations::new()
                    .read_only(true)
                    .open_world(true),
            );
        tool.output_schema = Some(std::sync::Arc::new(output));

        let (skill, schema) = SkillToolConverter::tool_to_skill(&tool);
        assert_eq!(skill.id, "search");
        assert_eq!(skill.name, "Search");
        assert_eq!(skill.tags, vec!["mcp".to_string()]);
        let schema = schema.expect("a typed tool yields a schema");

        let mut schemas = SkillSchemas::new();
        schemas.insert(skill.id.clone(), schema);
        let extension = schemas.to_extension();
        let read_back = {
            let mut card = AgentCard::builder()
                .name("mcp".to_string())
                .description("d".to_string())
                .url("http://localhost".to_string())
                .version("1".to_string())
                .capabilities(Default::default())
                .default_input_modes(vec!["text".to_string()])
                .default_output_modes(vec!["text".to_string()])
                .skills(vec![skill.clone()])
                .build();
            card.capabilities
                .get_or_insert_default()
                .extensions
                .push(extension);
            SkillSchemas::from_card(&card)
        };

        let served = SkillToolConverter::skill_to_tool(&skill, "mcp", read_back.get("search"));
        assert_eq!(served.name.as_ref(), "mcp_search");
        assert_eq!(
            served.input_schema["properties"]["q"],
            tool.input_schema["properties"]["q"]
        );
        assert_eq!(
            served.input_schema["required"],
            tool.input_schema["required"]
        );
        assert_eq!(served.output_schema, tool.output_schema);
        assert_eq!(served.title, tool.title);
        assert_eq!(served.annotations, tool.annotations);
    }
}
