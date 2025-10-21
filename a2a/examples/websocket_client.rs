//! A WebSocket client example with streaming and reconnection support

use futures::StreamExt;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

use a2a::{
    adapter::client::WebSocketClient,
    domain::{Message, Part, Role, TaskState},
    port::client::{AsyncA2AClient, StreamItem},
};

// Custom configuration for WebSocket client
fn create_client_config() -> a2a::adapter::client::ws::WebSocketClientConfig {
    a2a::adapter::client::ws::WebSocketClientConfig {
        connect_timeout: Duration::from_secs(5),
        response_timeout: Duration::from_secs(30),
        ping_interval: Duration::from_secs(15),
        idle_timeout: Duration::from_secs(60),
        max_reconnect_attempts: 3,
        reconnect_base_delay: Duration::from_millis(500),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure tracing for better debug output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Create a WebSocket client with custom configuration
    let client = WebSocketClient::with_config(
        "ws://localhost:8081".to_string(), 
        create_client_config()
    );
    
    println!("Connecting to WebSocket server...");
    
    // Generate a unique task ID
    let task_id = format!("task-{}", Uuid::new_v4());
    println!("Created task with ID: {}", task_id);

    // Create a message with multiple parts
    let message = Message {
        role: Role::User,
        parts: vec![
            Part::Text {
                text: "Hello, A2A agent! Please demonstrate streaming response capabilities.".to_string(),
                metadata: None,
            }
        ],
        metadata: None,
    };

    // Subscribe to task updates with streaming
    println!("Subscribing to task updates...");
    let mut stream = client
        .subscribe_to_task(&task_id, &message, Some("demo-session"), Some(10))
        .await?;

    // Process streaming updates
    println!("Waiting for streaming updates (press Ctrl+C to stop)...");
    
    // Track state for display purposes
    let mut last_status: Option<TaskState> = None;
    let mut received_final = false;
    let mut artifact_count = 0;
    
    // Flag for shutdown
    let mut shutdown = false;
    
    // Spawn a task to handle Ctrl+C
    let shutdown_task_id = task_id.clone();
    let shutdown_client = client.clone();
    tokio::spawn(async move {
        // This will complete when Ctrl+C is pressed
        if let Err(e) = tokio::signal::ctrl_c().await {
            eprintln!("Failed to listen for Ctrl+C: {}", e);
            return;
        }
        
        println!("\nInterrupt received, canceling task...");
        
        // Attempt to cancel the task before exiting
        match shutdown_client.cancel_task(&shutdown_task_id).await {
            Ok(task) => println!("Task canceled with state: {:?}", task.status.state),
            Err(e) => eprintln!("Failed to cancel task: {}", e),
        }
        
        // Signal the main loop to exit
        std::process::exit(0);
    });
    
    // Main event loop
    while !shutdown {
        match stream.next().await {
            Some(Ok(StreamItem::StatusUpdate(update))) => {
                // Only print status changes to avoid spam
                if last_status.as_ref() != Some(&update.status.state) {
                    println!("Status update: {:?}", update.status.state);
                    last_status = Some(update.status.state);
                }
                
                // Print message if available
                if let Some(message) = &update.status.message {
                    println!("Message:");
                    for part in &message.parts {
                        match part {
                            Part::Text { text, .. } => println!("  {}", text),
                            _ => println!("  [Non-text content]"),
                        }
                    }
                }
                
                // Check if this is the final update
                if update.final_ {
                    println!("Received final update, stream should complete soon");
                    received_final = true;
                }
            },
            Some(Ok(StreamItem::ArtifactUpdate(update))) => {
                artifact_count += 1;
                println!("Artifact update #{} for task {}", artifact_count, update.id);
                println!("  Name: {:?}", update.artifact.name);
                
                // Print sample of artifact content
                for (i, part) in update.artifact.parts.iter().take(1).enumerate() {
                    match part {
                        Part::Text { text, .. } => {
                            let preview = if text.len() > 50 {
                                format!("{}...", &text[..50])
                            } else {
                                text.clone()
                            };
                            println!("  Part {}: {}", i, preview);
                        },
                        _ => println!("  Part {}: [Non-text content]", i),
                    }
                }
                
                if update.artifact.parts.len() > 1 {
                    println!("  ... and {} more parts", update.artifact.parts.len() - 1);
                }
            },
            Some(Err(e)) => {
                eprintln!("Stream error: {}", e);
                // Wait briefly before attempting to continue
                sleep(Duration::from_millis(500)).await;
            },
            None => {
                // Stream ended
                println!("Stream has ended");
                if !received_final {
                    // If we didn't get a final update, the task might still be running
                    println!("No final update received, checking task status...");
                    match client.get_task(&task_id, None).await {
                        Ok(task) => {
                            println!("Task status: {:?}", task.status.state);
                            if task.status.state == TaskState::Working {
                                // Task is still working but stream ended - try to reconnect
                                println!("Task is still working, attempting to resubscribe...");
                                match client.resubscribe_to_task(&task_id, None).await {
                                    Ok(new_stream) => {
                                        println!("Successfully resubscribed to task");
                                        stream = new_stream;
                                        continue;
                                    },
                                    Err(e) => {
                                        eprintln!("Failed to resubscribe: {}", e);
                                        break;
                                    }
                                }
                            } else {
                                // Task has completed or is in another terminal state
                                println!("Task is in terminal state: {:?}", task.status.state);
                                break;
                            }
                        },
                        Err(e) => {
                            eprintln!("Failed to get task status: {}", e);
                            break;
                        }
                    }
                } else {
                    // We received a final update, so the stream ending is expected
                    println!("Stream completed normally");
                    break;
                }
            }
        }
    }
    
    // Before exiting, let's get the final task state with history
    println!("\nRetrieving full task details with history...");
    match client.get_task(&task_id, Some(10)).await {
        Ok(task) => {
            println!("Final task state: {:?}", task.status.state);
            
            // Show conversation history if available
            if let Some(history) = task.history {
                println!("\nConversation history ({} messages):", history.len());
                for (i, msg) in history.iter().enumerate() {
                    println!("  Message {}: {} with {} parts", 
                             i + 1, 
                             if msg.role == Role::User { "User" } else { "Agent" },
                             msg.parts.len());
                }
            } else {
                println!("No conversation history available");
            }
            
            // Show artifacts if available
            if let Some(artifacts) = task.artifacts {
                println!("\nTask has {} artifacts", artifacts.len());
            } else {
                println!("No artifacts available");
            }
        },
        Err(e) => eprintln!("Failed to retrieve task: {}", e),
    }
    
    println!("Client example completed");
    Ok(())
}