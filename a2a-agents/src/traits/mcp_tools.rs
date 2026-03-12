//! Traits and helpers for using MCP tools in message handlers

#[cfg(feature = "mcp-client")]
use crate::core::McpClientManager;
#[cfg(feature = "mcp-client")]
use rmcp::model::CallToolResult;
#[cfg(feature = "mcp-client")]
use serde_json::Value;

/// Extension trait for message handlers to easily call MCP tools
#[cfg(feature = "mcp-client")]
pub trait McpToolsExt {
    /// Get the MCP client manager
    fn mcp_client(&self) -> &McpClientManager;

    /// Call an MCP tool with JSON arguments
    async fn call_mcp_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<CallToolResult, Box<dyn std::error::Error>> {
        self.mcp_client()
            .call_tool(server_name, tool_name, arguments)
            .await
    }

    /// Call an MCP tool with no arguments
    async fn call_mcp_tool_simple(
        &self,
        server_name: &str,
        tool_name: &str,
    ) -> Result<CallToolResult, Box<dyn std::error::Error>> {
        self.call_mcp_tool(server_name, tool_name, None).await
    }

    /// List all available MCP tools
    async fn list_mcp_tools(&self) -> Vec<(String, String)> {
        self.mcp_client()
            .list_all_tools()
            .await
            .iter()
            .map(|(server, tool)| (server.clone(), tool.name.to_string()))
            .collect()
    }

    /// Check if an MCP server is connected
    async fn is_mcp_server_connected(&self, server_name: &str) -> bool {
        self.mcp_client().is_connected(server_name).await
    }
}

/// Helper to extract text from MCP tool call result
#[cfg(feature = "mcp-client")]
pub fn extract_tool_result_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|content| {
            if let rmcp::model::RawContent::Text(text) = &**content {
                Some(text.text.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Helper to check if tool call was successful
#[cfg(feature = "mcp-client")]
pub fn is_tool_call_successful(result: &CallToolResult) -> bool {
    !result.is_error.unwrap_or(false)
}

#[cfg(not(feature = "mcp-client"))]
pub trait McpToolsExt {}
