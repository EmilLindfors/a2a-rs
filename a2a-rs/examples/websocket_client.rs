//! A WebSocket client example with streaming

use futures::StreamExt;

use a2a_rs::{
    adapter::WebSocketClient,
    domain::{Message, Part},
    observability,
    services::client::{AsyncA2AClient, StreamItem},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for better observability
    observability::init_tracing();
    // Create a WebSocket client connected to our example server
    let client = WebSocketClient::new("ws://localhost:8081".to_string());

    // Generate a task ID
    let task_id = format!("task-{}", uuid::Uuid::new_v4());
    println!("Created task with ID: {}", task_id);

    // Create a message
    let message_id = format!("msg-{}", uuid::Uuid::new_v4());
    let mut message = Message::user_text(
        "Hello, A2A agent! Please stream your response.".to_string(),
        message_id,
    );

    // Add a data part to test multiple content types
    let mut data = serde_json::Map::new();
    data.insert(
        "action".to_string(),
        serde_json::Value::String("streaming_test".to_string()),
    );
    data.insert(
        "timestamp".to_string(),
        serde_json::Value::String(format!("{}", chrono::Utc::now())),
    );
    let data_part = Part::Data {
        data,
        metadata: None,
    };
    message.add_part(data_part);

    // Subscribe to task updates
    println!("Subscribing to task updates...");
    // First send the message
    let task = client
        .send_task_message(&task_id, &message, None, None)
        .await?;
    println!("Task created: {:#?}", task);

    // Then subscribe to updates
    let mut stream = client.subscribe_to_task(&task_id, None).await?;

    // Process streaming updates with timeout
    println!("Waiting for streaming updates...");
    let mut final_received = false;
    let mut status_updates = 0;
    let mut artifact_updates = 0;

    // Set a timeout for receiving streaming updates (12 seconds to allow server time)
    let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(12));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            result = stream.next() => {
                if let Some(result) = result {
                    match result {
                        Ok(StreamItem::Task(task)) => {
                            println!("Received initial task response for task {}", task.id);
                            println!("  Status: {:?}", task.status.state);

                            if let Some(message) = &task.status.message {
                                println!("  Message:");
                                for part in &message.parts {
                                    match part {
                                        Part::Text { text, .. } => println!("    {}", text),
                                        Part::Data { .. } => println!("    [Data content]"),
                                        _ => println!("    [Other content]"),
                                    }
                                }
                            }
                        }
                        Ok(StreamItem::StatusUpdate(update)) => {
                            status_updates += 1;
                            println!(
                                "Status update #{}: {:?}",
                                status_updates, update.status.state
                            );

                            if let Some(message) = &update.status.message {
                                println!("  Message:");
                                for part in &message.parts {
                                    match part {
                                        Part::Text { text, .. } => println!("    {}", text),
                                        Part::Data { .. } => println!("    [Data content]"),
                                        _ => println!("    [Other content]"),
                                    }
                                }
                            }

                            if update.final_ {
                                println!("Received final update");
                                final_received = true;
                                break;
                            }
                        }
                        Ok(StreamItem::ArtifactUpdate(update)) => {
                            artifact_updates += 1;
                            println!(
                                "Artifact update #{} for task {}",
                                artifact_updates, update.task_id
                            );
                            println!("  Artifact name: {:?}", update.artifact.name);
                            println!("  Parts: {} item(s)", update.artifact.parts.len());

                            // Display artifact parts
                            for (i, part) in update.artifact.parts.iter().enumerate() {
                                match part {
                                    Part::Text { text, .. } => println!("    Part {}: {}", i + 1, text),
                                    Part::Data { .. } => println!("    Part {}: [Data content]", i + 1),
                                    _ => println!("    Part {}: [Other content]", i + 1),
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Stream error: {}", e);
                            break;
                        }
                    }
                } else {
                    // Stream ended
                    println!("Stream ended");
                    break;
                }
            }
            _ = &mut timeout => {
                println!("Timeout reached, stopping streaming");
                break;
            }
        }
    }

    println!("\nSummary of streaming:");
    println!("  Total status updates: {}", status_updates);
    println!("  Total artifact updates: {}", artifact_updates);
    println!("  Final update received: {}", final_received);

    if !final_received {
        // If we didn't get a final update, cancel the task
        println!("\nCanceling task...");
        let task = client.cancel_task(&task_id).await?;
        println!("Task canceled with state: {:?}", task.status.state);

        // Check if we have task history
        if let Some(history) = &task.history {
            println!("\nTask history:");
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
    }

    Ok(())
}
