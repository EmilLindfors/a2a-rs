//! Bridge that provides MCP tools as capabilities to A2A agents

use crate::{
    converters::MessageConverter,
    error::{A2aMcpError, Result},
};
use a2a_rs::{
    domain::{Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use rmcp::{model::*, Peer, RoleClient};
use std::sync::Arc;
use tracing::{debug, error, info};

/// Bridge that provides MCP tools as additional capabilities to A2A agents
///
/// This allows A2A agents to call MCP tools by sending specially formatted messages.
/// Tool requests are detected in incoming messages and routed to the MCP server.
///
/// # Message Format for Tool Calls
///
/// To call an MCP tool, the A2A message should contain:
/// - A text part starting with `TOOL_CALL: <tool_name>`
/// - Followed by a JSON data part with the tool arguments
///
/// Example:
/// ```text
/// Message {
///     role: "user",
///     parts: [
///         Text { text: "TOOL_CALL: calculator_add" },
///         Data { data: {"a": 5, "b": 3}, mime_type: "application/json" }
///     ]
/// }
/// ```
#[derive(Clone)]
pub struct McpToA2ABridge<H: AsyncMessageHandler> {
    /// The MCP client peer for calling tools
    mcp_peer: Arc<Peer<RoleClient>>,
    /// Available MCP tools
    tools: Arc<Vec<Tool>>,
    /// The underlying A2A message handler to delegate non-tool messages
    inner_handler: Arc<H>,
}

impl<H: AsyncMessageHandler + Clone + Send + Sync + 'static> McpToA2ABridge<H> {
    /// Create a new MCP → A2A bridge
    ///
    /// # Arguments
    ///
    /// * `mcp_peer` - MCP client peer for calling tools
    /// * `inner_handler` - Underlying A2A handler for non-tool messages
    pub async fn new(mcp_peer: Peer<RoleClient>, inner_handler: H) -> Result<Self> {
        // Fetch available tools from MCP server
        let tools = mcp_peer
            .list_tools(None)
            .await
            .map_err(|e| A2aMcpError::McpServer(format!("Failed to list tools: {:?}", e)))?
            .tools;

        info!("McpToA2ABridge initialized with {} MCP tools", tools.len());

        Ok(Self {
            mcp_peer: Arc::new(mcp_peer),
            tools: Arc::new(tools),
            inner_handler: Arc::new(inner_handler),
        })
    }

    /// Check if a message is a tool call request
    fn is_tool_call(message: &Message) -> Option<(String, serde_json::Value)> {
        // Look for a text part that starts with "TOOL_CALL:"
        let tool_name = message.parts.iter().find_map(|part| {
            if let Part::Text { text, .. } = part {
                if let Some(name) = text.strip_prefix("TOOL_CALL:") {
                    return Some(name.trim().to_string());
                }
            }
            None
        })?;

        // Look for a data part containing the arguments
        let args = message
            .parts
            .iter()
            .find_map(|part| {
                if let Part::Data { data, .. } = part {
                    return Some(serde_json::Value::Object(data.clone()));
                }
                None
            })
            .unwrap_or(serde_json::json!({}));

        Some((tool_name, args))
    }

    /// Call an MCP tool
    async fn call_mcp_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult> {
        debug!("Calling MCP tool: {} with args: {}", tool_name, arguments);

        // Verify tool exists
        if !self.tools.iter().any(|t| t.name == tool_name) {
            return Err(A2aMcpError::ToolNotFound(tool_name.to_string()));
        }

        // Call the MCP tool via the peer
        let result = self
            .mcp_peer
            .call_tool(CallToolRequestParam {
                name: tool_name.to_string().into(),
                arguments: if let serde_json::Value::Object(map) = arguments {
                    Some(map)
                } else {
                    None
                },
            })
            .await
            .map_err(|e| A2aMcpError::McpServer(format!("Tool call failed: {:?}", e)))?;

        info!("MCP tool '{}' returned result", tool_name);

        Ok(result)
    }
}

#[async_trait]
impl<H: AsyncMessageHandler + Clone + Send + Sync + 'static> AsyncMessageHandler
    for McpToA2ABridge<H>
{
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> std::result::Result<Task, a2a_rs::domain::error::A2AError> {
        // Check if this is a tool call request
        if let Some((tool_name, args)) = Self::is_tool_call(message) {
            info!("Detected MCP tool call request for tool: {}", tool_name);

            // Call the MCP tool
            match self.call_mcp_tool(&tool_name, args).await {
                Ok(result) => {
                    // Convert MCP result to A2A task
                    let task_state = if result.is_error.unwrap_or(false) {
                        TaskState::Failed
                    } else {
                        TaskState::Completed
                    };

                    let message_text = MessageConverter::extract_text_from_content(&result.content);

                    // Create agent response message
                    let agent_message = Message::builder()
                        .role(Role::Agent)
                        .parts(vec![Part::Text {
                            text: message_text,
                            metadata: None,
                        }])
                        .message_id(uuid::Uuid::new_v4().to_string())
                        .build();

                    Ok(Task::builder()
                        .id(task_id.to_string())
                        .context_id(uuid::Uuid::new_v4().to_string())
                        .status(TaskStatus {
                            state: task_state,
                            message: None,
                            timestamp: Some(chrono::Utc::now()),
                        })
                        .history(vec![message.clone(), agent_message])
                        .build())
                }
                Err(e) => {
                    error!("MCP tool call failed: {}", e);
                    Err(e.to_a2a_error())
                }
            }
        } else {
            // Not a tool call, delegate to inner handler
            debug!("Message is not a tool call, delegating to inner handler");
            self.inner_handler
                .process_message(task_id, message, session_id)
                .await
        }
    }
}

/// Helper to create a tool call message
pub fn create_tool_call_message(tool_name: &str, arguments: serde_json::Value) -> Message {
    let data_map = if let serde_json::Value::Object(map) = arguments {
        map
    } else {
        serde_json::Map::new()
    };

    Message::builder()
        .role(Role::User)
        .parts(vec![
            Part::Text {
                text: format!("TOOL_CALL: {}", tool_name),
                metadata: None,
            },
            Part::Data {
                data: data_map,
                metadata: None,
            },
        ])
        .message_id(uuid::Uuid::new_v4().to_string())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tool_call_detection() {
        let mut data_map = serde_json::Map::new();
        data_map.insert(
            "param".to_string(),
            serde_json::Value::String("value".to_string()),
        );

        let tool_message = Message::builder()
            .role(Role::User)
            .parts(vec![
                Part::Text {
                    text: "TOOL_CALL: my_tool".to_string(),
                    metadata: None,
                },
                Part::Data {
                    data: data_map,
                    metadata: None,
                },
            ])
            .message_id("test".to_string())
            .build();

        let result = McpToA2ABridge::<NoOpHandler>::is_tool_call(&tool_message);
        assert!(result.is_some());
        let (name, args) = result.unwrap();
        assert_eq!(name, "my_tool");
        assert_eq!(args["param"], "value");
    }

    #[test]
    fn test_is_not_tool_call() {
        let normal_message = Message::builder()
            .role(Role::User)
            .parts(vec![Part::Text {
                text: "Just a normal message".to_string(),
                metadata: None,
            }])
            .message_id("test".to_string())
            .build();

        let result = McpToA2ABridge::<NoOpHandler>::is_tool_call(&normal_message);
        assert!(result.is_none());
    }

    #[test]
    fn test_create_tool_call_message() {
        let msg = create_tool_call_message("test_tool", serde_json::json!({"x": 42}));
        assert_eq!(msg.parts.len(), 2);
        assert_eq!(msg.role, Role::User);

        if let Part::Text { text, .. } = &msg.parts[0] {
            assert!(text.contains("TOOL_CALL: test_tool"));
        } else {
            panic!("Expected text part");
        }
    }

    // Mock handler for testing
    #[derive(Clone)]
    struct NoOpHandler;

    #[async_trait]
    impl AsyncMessageHandler for NoOpHandler {
        async fn process_message(
            &self,
            task_id: &str,
            message: &Message,
            _session_id: Option<&str>,
        ) -> std::result::Result<Task, a2a_rs::domain::error::A2AError> {
            Ok(Task::builder()
                .id(task_id.to_string())
                .context_id(uuid::Uuid::new_v4().to_string())
                .status(TaskStatus {
                    state: TaskState::Completed,
                    message: None,
                    timestamp: None,
                })
                .history(vec![message.clone()])
                .build())
        }
    }
}
