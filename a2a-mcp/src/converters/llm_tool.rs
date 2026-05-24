//! Converter between MCP Tools and LLM Tool Primitives

use crate::error::{A2aMcpError, Result};
use a2a_agents_common::llm::{ToolCall, ToolDefinition};
use rmcp::model::{CallToolRequestParams, Tool};
use serde_json::Value;

/// Converts between MCP Tools and A2A LLM Tool Primitives
pub struct LlmToolConverter;

impl LlmToolConverter {
    /// Convert an MCP `Tool` into an LLM `ToolDefinition`.
    /// 
    /// This directly copies the MCP tool's name, description, and JSON schema input properties.
    pub fn mcp_to_llm_tool(tool: &Tool) -> ToolDefinition {
        let schema_val = serde_json::to_value(&*tool.input_schema).unwrap_or(Value::Null);

        ToolDefinition {
            name: tool.name.to_string(),
            description: tool.description.clone().unwrap_or_default().to_string(),
            parameters: schema_val,
        }
    }

    /// Converts a list of MCP `Tool`s into a list of LLM `ToolDefinition`s.
    pub fn mcp_to_llm_tools(tools: &[Tool]) -> Vec<ToolDefinition> {
        tools.iter().map(Self::mcp_to_llm_tool).collect()
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
        })).unwrap();

        let tool = Tool::new("my_tool", "My description", std::sync::Arc::new(schema));
        let llm_tool = LlmToolConverter::mcp_to_llm_tool(&tool);

        assert_eq!(llm_tool.name, "my_tool");
        assert_eq!(llm_tool.description, "My description");
        assert_eq!(
            llm_tool.parameters["properties"]["param1"]["type"].as_str().unwrap(),
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
            request.arguments.unwrap().get("a").unwrap().as_i64().unwrap(),
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
}
