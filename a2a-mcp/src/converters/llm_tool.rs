//! Converter between MCP Tools and LLM Tool Primitives

use crate::error::{A2aMcpError, Result};
use a2a_llm::{ToolCall, ToolDefinition, ToolResult};
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use serde_json::Value;

use super::MessageConverter;

/// Converts between MCP Tools and A2A LLM Tool Primitives
pub struct LlmToolConverter;

impl LlmToolConverter {
    /// Convert an MCP `Tool` into an LLM `ToolDefinition`.
    ///
    /// Name, description and input schema go to the three fields every
    /// provider sends; the title, the annotations and the output schema ride
    /// beside them for the consumer, so a read-only query tool is tellable
    /// from one that writes after the bridge and not only before it.
    pub fn mcp_to_llm_tool(tool: &Tool) -> ToolDefinition {
        let schema_val = serde_json::to_value(&*tool.input_schema).unwrap_or(Value::Null);

        let mut def = ToolDefinition::new(
            tool.name.to_string(),
            tool.description.clone().unwrap_or_default().to_string(),
            schema_val,
        );
        if let Some(title) = &tool.title {
            def = def.with_title(title.clone());
        }
        if let Some(annotations) = &tool.annotations {
            let mapped = Self::annotations_to_llm(annotations);
            if !mapped.is_empty() {
                def = def.with_annotations(mapped);
            }
        }
        if let Some(schema) = &tool.output_schema {
            def = def.with_output_schema(serde_json::to_value(&**schema).unwrap_or(Value::Null));
        }
        def
    }

    /// Converts a list of MCP `Tool`s into a list of LLM `ToolDefinition`s.
    pub fn mcp_to_llm_tools(tools: &[Tool]) -> Vec<ToolDefinition> {
        tools.iter().map(Self::mcp_to_llm_tool).collect()
    }

    /// MCP's hints as `a2a-llm`'s. The MCP struct also carries a `title`,
    /// which belongs on the definition and not on the hints; it is dropped
    /// here and read by [`mcp_to_llm_tool`](Self::mcp_to_llm_tool) instead.
    pub fn annotations_to_llm(
        annotations: &rmcp::model::ToolAnnotations,
    ) -> a2a_llm::ToolAnnotations {
        let mut out = a2a_llm::ToolAnnotations::new();
        out.read_only = annotations.read_only_hint;
        out.destructive = annotations.destructive_hint;
        out.idempotent = annotations.idempotent_hint;
        out.open_world = annotations.open_world_hint;
        out
    }

    /// `a2a-llm`'s hints as MCP's, for a tool the bridge serves.
    pub fn annotations_to_mcp(
        annotations: &a2a_llm::ToolAnnotations,
    ) -> rmcp::model::ToolAnnotations {
        rmcp::model::ToolAnnotations::from_raw(
            None,
            annotations.read_only,
            annotations.destructive,
            annotations.idempotent,
            annotations.open_world,
        )
    }

    /// What an MCP tool answered, as the result the LLM loop hands the model:
    /// the text content joined, and `structuredContent` kept beside it rather
    /// than flattened into the text.
    pub fn mcp_result_to_llm(result: &CallToolResult) -> ToolResult {
        let mut out = ToolResult::new(MessageConverter::extract_text_from_content(&result.content));
        if let Some(structured) = &result.structured_content {
            out = out.with_structured(structured.clone());
        }
        out
    }

    /// Converts an LLM `ToolCall` into an MCP `CallToolRequestParams`.
    pub fn llm_tool_call_to_mcp_request(tool_call: &ToolCall) -> Result<CallToolRequestParams> {
        let mut params = CallToolRequestParams::new(tool_call.name.clone());

        if !tool_call.arguments.trim().is_empty() {
            match serde_json::from_str::<serde_json::Map<String, Value>>(&tool_call.arguments) {
                Ok(args) => {
                    params = params.with_arguments(args);
                }
                Err(e) => {
                    return Err(A2aMcpError::InvalidToolCall(format!(
                        "Failed to parse tool call arguments as JSON Object for {}: {}",
                        tool_call.name, e
                    )));
                }
            }
        }

        Ok(params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_mcp_to_llm_tool() {
        let schema = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "param1": { "type": "string" }
            }
        }))
        .unwrap();

        let tool = Tool::new("my_tool", "My description", std::sync::Arc::new(schema));
        let llm_tool = LlmToolConverter::mcp_to_llm_tool(&tool);

        assert_eq!(llm_tool.name, "my_tool");
        assert_eq!(llm_tool.description, "My description");
        assert_eq!(
            llm_tool.parameters["properties"]["param1"]["type"]
                .as_str()
                .unwrap(),
            "string"
        );
    }

    #[test]
    fn test_llm_tool_call_to_mcp_request() {
        let tool_call = ToolCall {
            id: "call_123".to_string(),
            name: "calculator".to_string(),
            arguments: r#"{"a": 5, "b": 3}"#.to_string(),
        };

        let request = LlmToolConverter::llm_tool_call_to_mcp_request(&tool_call).unwrap();
        assert_eq!(request.name, "calculator");
        assert_eq!(
            request
                .arguments
                .unwrap()
                .get("a")
                .unwrap()
                .as_i64()
                .unwrap(),
            5
        );
    }

    #[test]
    fn test_llm_tool_call_invalid_json() {
        let tool_call = ToolCall {
            id: "call_123".to_string(),
            name: "calculator".to_string(),
            arguments: "not json".to_string(),
        };

        let result = LlmToolConverter::llm_tool_call_to_mcp_request(&tool_call);
        assert!(result.is_err());
    }

    /// The title, the hints and the output schema the server sent are on the
    /// definition; a consumer that needs to know a tool is read-only can.
    #[test]
    fn what_the_server_said_about_a_tool_is_kept() {
        let schema = serde_json::from_value(json!({"type": "object"})).unwrap();
        let output =
            serde_json::from_value(json!({"type": "object", "properties": {"rows": {}}})).unwrap();
        let mut tool = Tool::new("query", "Runs a query", std::sync::Arc::new(schema))
            .with_title("Query")
            .annotate(
                rmcp::model::ToolAnnotations::new()
                    .read_only(true)
                    .open_world(false),
            );
        tool.output_schema = Some(std::sync::Arc::new(output));
        let def = LlmToolConverter::mcp_to_llm_tool(&tool);
        assert_eq!(def.title.as_deref(), Some("Query"));
        let hints = def.annotations.expect("annotations kept");
        assert_eq!(hints.read_only, Some(true));
        assert_eq!(hints.open_world, Some(false));
        assert_eq!(hints.destructive, None);
        assert_eq!(def.output_schema.unwrap()["properties"]["rows"], json!({}));

        // And a server that said nothing leaves nothing behind.
        let plain = Tool::new("plain", "d", std::sync::Arc::new(serde_json::Map::new()));
        let def = LlmToolConverter::mcp_to_llm_tool(&plain);
        assert_eq!(def.title, None);
        assert_eq!(def.annotations, None);
        assert_eq!(def.output_schema, None);
    }

    /// A structured result is beside the text, not flattened into it.
    #[test]
    fn a_structured_result_is_kept_beside_its_text() {
        let mut result = CallToolResult::success(vec![rmcp::model::ContentBlock::text("3 rows")]);
        result.structured_content = Some(json!({"rows": 3}));
        let llm = LlmToolConverter::mcp_result_to_llm(&result);
        assert_eq!(llm.text, "3 rows");
        assert_eq!(llm.structured, Some(json!({"rows": 3})));
    }
}
