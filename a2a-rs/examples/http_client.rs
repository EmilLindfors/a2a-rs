//! A simple HTTP client example

use a2a_rs::{
    adapter::client::HttpClient,
    domain::{Message, Part},
    port::client::AsyncA2AClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client connected to our example server
    let client = HttpClient::new("http://localhost:8080".to_string());

    // Generate a task ID
    let task_id = format!("task-{}", uuid::Uuid::new_v4());
    println!("Created task with ID: {}", task_id);

    // Create a message
    let message = Message::user_text("Hello, A2A agent! How are you today?".to_string());

    // Send a task message
    println!("Sending message to task...");
    let task = client.send_task_message(&task_id, &message, None, None).await?;
    println!("Got response with status: {:?}", task.status.state);

    if let Some(response_message) = task.status.message {
        println!("Agent response:");
        for part in response_message.parts {
            match part {
                Part::Text { text, .. } => println!("  {}", text),
                _ => println!("  [Non-text content]"),
            }
        }
    }

    // Get the task again to verify it's stored
    println!("\nRetrieving task...");
    let task = client.get_task(&task_id, None).await?;
    println!("Retrieved task with ID: {} and state: {:?}", task.id, task.status.state);

    // Cancel the task
    println!("\nCanceling task...");
    let task = client.cancel_task(&task_id).await?;
    println!("Task canceled with state: {:?}", task.status.state);

    Ok(())
}