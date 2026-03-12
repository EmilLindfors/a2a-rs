//! Example A2A agent that uses MCP tools
//!
//! This example demonstrates how an A2A agent can connect to MCP servers
//! and use their tools to accomplish tasks.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example mcp_client_agent --features mcp-client
//! ```

use a2a_agents::core::{AgentBuilder, McpClientManager};
use a2a_agents::traits::{McpToolsExt, extract_tool_result_text, is_tool_call_successful};
use a2a_rs::domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus};
use a2a_rs::port::AsyncMessageHandler;
use async_trait::async_trait;

/// Agent handler that can call MCP tools
#[derive(Clone)]
struct McpToolAgent {
    mcp_client: McpClientManager,
}

impl McpToolAgent {
    fn new(mcp_client: McpClientManager) -> Self {
        Self { mcp_client }
    }
}

// Implement the McpToolsExt trait to get helper methods
impl McpToolsExt for McpToolAgent {
    fn mcp_client(&self) -> &McpClientManager {
        &self.mcp_client
    }
}

#[async_trait]
impl AsyncMessageHandler for McpToolAgent {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        // Extract text from the message
        let text = message
            .parts
            .iter()
            .filter_map(|part| {
                if let Part::Text { text, .. } = part {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        tracing::info!("Received message: {}", text);

        // Check what MCP servers are connected
        let connected_servers = self.mcp_client.connected_servers().await;
        tracing::info!("Connected MCP servers: {:?}", connected_servers);

        // List all available tools
        let available_tools = self.list_mcp_tools().await;
        tracing::info!("Available MCP tools: {:?}", available_tools);

        // Simple routing based on message content
        let response_text = if text.to_lowercase().contains("list tools") {
            // List all available MCP tools
            if available_tools.is_empty() {
                "No MCP tools are currently available. Make sure MCP servers are configured and running.".to_string()
            } else {
                let mut tool_list = String::from("Available MCP tools:\n\n");
                for (server, tool) in available_tools {
                    tool_list.push_str(&format!("- {} (from server: {})\n", tool, server));
                }
                tool_list
            }
        } else if text.to_lowercase().contains("greet") || text.to_lowercase().contains("hello") {
            // Try to call the greet tool from the example-agent server
            if self.is_mcp_server_connected("example-agent").await {
                match self
                    .call_mcp_tool(
                        "example-agent",
                        "example_com_greet",
                        Some(serde_json::json!({ "message": "greet me" })),
                    )
                    .await
                {
                    Ok(result) if is_tool_call_successful(&result) => {
                        let tool_response = extract_tool_result_text(&result);
                        format!("MCP Tool Response:\n{}", tool_response)
                    }
                    Ok(result) => {
                        format!("Tool call failed: {}", extract_tool_result_text(&result))
                    }
                    Err(e) => {
                        format!("Error calling MCP tool: {}", e)
                    }
                }
            } else {
                "example-agent MCP server is not connected".to_string()
            }
        } else if text.to_lowercase().contains("calculate") {
            // Try to call the calculate tool
            if self.is_mcp_server_connected("example-agent").await {
                match self
                    .call_mcp_tool(
                        "example-agent",
                        "example_com_calculate",
                        Some(serde_json::json!({ "message": &text })),
                    )
                    .await
                {
                    Ok(result) if is_tool_call_successful(&result) => {
                        let tool_response = extract_tool_result_text(&result);
                        format!("MCP Tool Response:\n{}", tool_response)
                    }
                    Ok(result) => {
                        format!("Tool call failed: {}", extract_tool_result_text(&result))
                    }
                    Err(e) => {
                        format!("Error calling MCP tool: {}", e)
                    }
                }
            } else {
                "example-agent MCP server is not connected".to_string()
            }
        } else {
            format!(
                "I'm an MCP tool orchestrator. I have access to {} MCP tools from {} servers.\n\
                Try asking me to:\n\
                - 'list tools' to see available tools\n\
                - 'greet me' to call the greeting tool\n\
                - 'calculate 2 + 2' to use the calculator tool",
                available_tools.len(),
                connected_servers.len()
            )
        };

        // Build response message
        let response_message = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::Text {
                text: response_text,
                metadata: None,
            }])
            .message_id(format!("{}-response", task_id))
            .build();

        // Return completed task
        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id(session_id.unwrap_or("default").to_string())
            .status(TaskStatus {
                state: TaskState::Completed,
                message: None,
                timestamp: Some(chrono::Utc::now()),
            })
            .history(vec![message.clone(), response_message])
            .build())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Create MCP client manager and initialize from config
    let mcp_client = McpClientManager::new();
    let config = a2a_agents::core::AgentConfig::from_file("examples/mcp_client_agent.toml")?;

    tracing::info!("Initializing MCP client connections...");
    mcp_client.initialize(&config.features.mcp_client).await?;

    // Create handler with MCP client
    let handler = McpToolAgent::new(mcp_client);

    // Build and run the agent
    // Note: The builder also auto-initializes MCP client, but since our handler
    // already has one, we use the manual approach here for demonstration
    AgentBuilder::from_file("examples/mcp_client_agent.toml")?
        .with_handler(handler)
        .build_with_auto_storage()
        .await?
        .run()
        .await?;

    Ok(())
}
