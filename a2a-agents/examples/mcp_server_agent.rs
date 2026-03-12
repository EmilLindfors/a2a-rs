//! Example A2A agent exposed as an MCP server
//!
//! This example demonstrates how to expose an A2A agent as an MCP server,
//! making it usable in Claude Desktop and other MCP clients.
//!
//! # Usage
//!
//! Run the agent:
//! ```bash
//! cargo run --example mcp_server_agent --features mcp-server
//! ```
//!
//! The agent will start in MCP stdio mode, waiting for JSON-RPC messages on stdin.
//!
//! # Claude Desktop Integration
//!
//! Add this to your `claude_desktop_config.json`:
//! ```json
//! {
//!   "mcpServers": {
//!     "my-a2a-agent": {
//!       "command": "cargo",
//!       "args": ["run", "--example", "mcp_server_agent", "--features", "mcp-server"],
//!       "cwd": "/path/to/a2a-rs/a2a-agents"
//!     }
//!   }
//! }
//! ```

use a2a_agents::core::AgentBuilder;
use a2a_rs::domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus};
use a2a_rs::port::AsyncMessageHandler;
use async_trait::async_trait;

/// Simple message handler that responds to basic queries
#[derive(Clone)]
struct SimpleHandler;

#[async_trait]
impl AsyncMessageHandler for SimpleHandler {
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
            .join(" ")
            .to_lowercase();

        // Simple pattern matching based on message content
        let response = if text.contains("greet") || text.contains("hello") || text.contains("hi") {
            "Hello! I'm an A2A agent exposed as an MCP server. How can I help you today?"
        } else if text.contains("calculate") || text.contains("math") || text.contains("+") || text.contains("*") {
            // Very simple calculator example
            if let Some(result) = simple_calculate(&text) {
                &format!("The result is: {}", result)
            } else {
                "I can help with simple calculations like '2 + 2' or '10 * 5'"
            }
        } else if text.contains("echo") {
            &format!("You said: {}", text.replace("echo", "").trim())
        } else {
            "I can help with greetings, simple calculations, or echo your messages. Try asking me to greet you or calculate something!"
        };

        // Build response message
        let response_message = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::Text {
                text: response.to_string(),
                metadata: None,
            }])
            .message_id(format!("{}-response", task_id))
            .build();

        // Return a completed task with the response
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

/// Very simple calculator for demonstration
fn simple_calculate(text: &str) -> Option<String> {
    // Extract numbers and operator
    let parts: Vec<&str> = text.split_whitespace().collect();

    for i in 0..parts.len().saturating_sub(2) {
        if let (Ok(a), Ok(b)) = (parts[i].parse::<f64>(), parts.get(i + 2)?.parse::<f64>()) {
            let result = match parts[i + 1] {
                "+" | "plus" | "add" => Some(a + b),
                "-" | "minus" | "subtract" => Some(a - b),
                "*" | "times" | "multiply" => Some(a * b),
                "/" | "divide" | "divided" => Some(a / b),
                _ => None,
            };

            if let Some(r) = result {
                return Some(format!("{}", r));
            }
        }
    }

    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Build and run the agent from configuration
    AgentBuilder::from_file("examples/mcp_server_agent.toml")?
        .with_handler(SimpleHandler)
        .build_with_auto_storage()
        .await?
        .run()
        .await?;

    Ok(())
}
