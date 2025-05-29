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
    let message_id = format!("msg-{}", uuid::Uuid::new_v4());
    let mut message = Message::user_text("Hello, A2A agent! How are you today?".to_string(), message_id);

    // Add a file part (properly validated)
    let file_part = Part::file_from_bytes(
        "SGVsbG8sIHdvcmxkIQ==".to_string(), // Base64 encoded "Hello, world!"
        Some("greeting.txt".to_string()),
        Some("text/plain".to_string()),
    );
    message.add_part_validated(file_part).unwrap();

    // Optional: Set up push notifications
    // let push_config = TaskPushNotificationConfig {
    //     id: task_id.clone(),
    //     push_notification_config: PushNotificationConfig {
    //         url: "https://example.com/webhook".to_string(),
    //         token: Some("secret-token".to_string()),
    //         authentication: None,
    //     },
    // };
    // client.set_task_push_notification(&push_config).await?;

    // Send a task message with retries
    println!("Sending message to task...");

    // Try to connect with retries
    let max_retries = 5;
    let mut task = None;

    for retry in 0..max_retries {
        match client
            .send_task_message(&task_id, &message, None, None)
            .await
        {
            Ok(t) => {
                task = Some(t);
                break;
            }
            Err(e) => {
                if retry < max_retries - 1 {
                    println!(
                        "Connection attempt {} failed: {}. Retrying in 1 second...",
                        retry + 1,
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                } else {
                    return Err(e.into());
                }
            }
        }
    }

    let task = task.unwrap();
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

    // Get the task again to verify it's stored (with history)
    println!("\nRetrieving task with full history...");
    let task = client.get_task(&task_id, None).await?;
    println!(
        "Retrieved task with ID: {} and state: {:?}",
        task.id, task.status.state
    );

    // Also try getting the task with limited history
    println!("\nRetrieving task with limited history (1 item)...");
    let task_limited = client.get_task(&task_id, Some(1)).await?;
    println!(
        "Retrieved task with ID: {} and state: {:?}",
        task_limited.id, task_limited.status.state
    );

    // Check for task history
    if let Some(history) = &task.history {
        println!("\nTask full history ({} messages):", history.len());
        for (i, msg) in history.iter().enumerate() {
            println!("  Message {}: Role: {:?}", i + 1, msg.role);
            for part in &msg.parts {
                match part {
                    Part::Text { text, .. } => println!("    Text: {}", text),
                    Part::File { .. } => println!("    [File content]"),
                    Part::Data { .. } => println!("    [Data content]"),
                }
            }
        }
    }

    // Check for limited task history
    if let Some(history) = &task_limited.history {
        println!("\nTask limited history ({} messages):", history.len());
        for (i, msg) in history.iter().enumerate() {
            println!("  Message {}: Role: {:?}", i + 1, msg.role);
            for part in &msg.parts {
                match part {
                    Part::Text { text, .. } => println!("    Text: {}", text),
                    Part::File { .. } => println!("    [File content]"),
                    Part::Data { .. } => println!("    [Data content]"),
                }
            }
        }
    }

    // Cancel the task
    println!("\nCanceling task...");
    let task = client.cancel_task(&task_id).await?;
    println!("Task canceled with state: {:?}", task.status.state);

    Ok(())
}
