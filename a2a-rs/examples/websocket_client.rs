//! A WebSocket client example with streaming

use futures::StreamExt;

use a2a_rs::{
    adapter::client::WebSocketClient,
    domain::{Message, Part},
    port::client::{AsyncA2AClient, StreamItem},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a WebSocket client connected to our example server
    let client = WebSocketClient::new("ws://localhost:8081".to_string());

    // Generate a task ID
    let task_id = format!("task-{}", uuid::Uuid::new_v4());
    println!("Created task with ID: {}", task_id);

    // Create a message
    let message = Message::user_text("Hello, A2A agent! Please stream your response.".to_string());
    
    // Optional: Add a data part
    // let mut data = serde_json::Map::new();
    // data.insert("key".to_string(), serde_json::Value::String("value".to_string()));
    // let data_part = Part::data(data);
    // message.add_part(data_part);

    // Subscribe to task updates
    println!("Subscribing to task updates...");
    let mut stream = client
        .subscribe_to_task(&task_id, &message, None, None)
        .await?;

    // Process streaming updates
    println!("Waiting for streaming updates...");
    let mut final_received = false;
    
    while let Some(result) = stream.next().await {
        match result {
            Ok(StreamItem::StatusUpdate(update)) => {
                println!("Status update: {:?}", update.status.state);
                
                if let Some(message) = &update.status.message {
                    println!("  Message:");
                    for part in &message.parts {
                        match part {
                            Part::Text { text, .. } => println!("    {}", text),
                            _ => println!("    [Non-text content]"),
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
                println!("Artifact update for task {}", update.id);
                println!("  Artifact name: {:?}", update.artifact.name);
                println!("  Parts: {} item(s)", update.artifact.parts.len());
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    if !final_received {
        // If we didn't get a final update, cancel the task
        println!("\nCanceling task...");
        let task = client.cancel_task(&task_id).await?;
        println!("Task canceled with state: {:?}", task.status.state);
        
        // Check if we have task history
        if let Some(history) = &task.history {
            println!("\nTask history:");
            for (i, msg) in history.iter().enumerate() {
                println!("  Message {}: Role: {:?}", i+1, msg.role);
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