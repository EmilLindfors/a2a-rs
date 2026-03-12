//! Basic HTTP client example
//!
//! This example demonstrates how to use the WebA2AClient to send messages
//! to an A2A agent and retrieve task information.
//!
//! # Running the Example
//!
//! 1. Start an A2A agent (e.g., the reimbursement agent):
//!    ```bash
//!    cd ../a2a-agents
//!    cargo run --bin reimbursement_demo
//!    ```
//!
//! 2. Run this example:
//!    ```bash
//!    cargo run --example basic_client
//!    ```

use a2a_client::WebA2AClient;
use a2a_rs::domain::{Message, Part};
use a2a_rs::services::AsyncA2AClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting basic A2A client example");

    // Create a client using the builder pattern
    let client = WebA2AClient::builder()
        .http_url("http://localhost:8080")
        .build();

    println!("Created HTTP client for http://localhost:8080");

    // Create a simple message using the builder
    let message = Message::builder()
        .message_id(uuid::Uuid::new_v4().to_string())
        .role(a2a_rs::domain::Role::User)
        .parts(vec![Part::text(
            "Hello! I'd like to submit an expense reimbursement.".to_string(),
        )])
        .build();

    println!("Sending message to agent...");

    // We need to use send_task_message with a new task ID
    // For a new conversation, we create a unique task ID
    let task_id = uuid::Uuid::new_v4().to_string();

    match client
        .http
        .send_task_message(&task_id, &message, None, None)
        .await
    {
        Ok(task) => {
            println!("✓ Message sent successfully!");
            println!("  Task ID: {}", task.id);
            println!("  State: {:?}", task.status.state);
            println!(
                "  Message count: {}",
                task.history.as_ref().map(|h| h.len()).unwrap_or(0)
            );

            // Retrieve the task to see the agent's response
            println!("Retrieving task to see agent response...");
            match client.http.get_task(&task.id, None).await {
                Ok(updated_task) => {
                    println!("✓ Retrieved task successfully!");
                    if let Some(history) = &updated_task.history {
                        println!("  Conversation has {} messages", history.len());
                        if let Some(last_msg) = history.last() {
                            println!("  Last message role: {:?}", last_msg.role);
                            if let Some(a2a_rs::domain::Part::Text { text, .. }) =
                                last_msg.parts.first()
                            {
                                println!("  Agent says: {}", text);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to retrieve task: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to send message: {}", e);
            return Err(e.into());
        }
    }

    println!("Example completed successfully!");

    Ok(())
}
