//! A complete WebSocket example that runs both server and client together

use futures::StreamExt;
use std::time::Duration;
use tokio::time::sleep;

use a2a_rs::adapter::{
    DefaultRequestProcessor, InMemoryTaskStorage, NoopPushNotificationSender, SimpleAgentInfo,
    WebSocketClient, WebSocketServer,
};

mod common;
use a2a_rs::domain::{Message, Part, Role};
use a2a_rs::observability;
use a2a_rs::services::{AsyncA2AClient, StreamItem};
use common::SimpleAgentHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for better observability
    observability::init_tracing();

    println!("🚀 Starting WebSocket Full Example");
    println!("===================================");

    // Start the server in a background task
    let server_handle = tokio::spawn(async {
        run_server().await.expect("Server failed");
    });

    // Give the server time to start
    sleep(Duration::from_millis(500)).await;

    // Run the client
    match run_client().await {
        Ok(_) => println!("✅ Client completed successfully"),
        Err(e) => println!("❌ Client failed: {}", e),
    }

    // Let the server run a bit longer
    sleep(Duration::from_millis(1000)).await;

    // Abort the server
    server_handle.abort();

    println!("🏁 WebSocket Full Example completed");
    Ok(())
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 Starting WebSocket server...");

    // Create server components
    let push_sender = NoopPushNotificationSender;
    let storage = InMemoryTaskStorage::with_push_sender(push_sender);
    let handler = SimpleAgentHandler::with_storage(storage.clone());
    let test_agent_info =
        SimpleAgentInfo::new("test-agent".to_string(), "ws://localhost:8081".to_string());
    let processor = DefaultRequestProcessor::with_handler(handler.clone(), test_agent_info);

    // Create agent info
    let agent_info = SimpleAgentInfo::new(
        "Example A2A WebSocket Agent".to_string(),
        "ws://localhost:8081".to_string(),
    )
    .with_description("An example A2A WebSocket agent with streaming support".to_string())
    .with_provider(
        "Example Organization".to_string(),
        "https://example.org".to_string(),
    )
    .with_documentation_url("https://example.org/docs".to_string())
    .with_streaming()
    .with_push_notifications()
    .add_comprehensive_skill(
        "echo".to_string(),
        "Echo Skill".to_string(),
        Some("Echoes back the user's message".to_string()),
        Some(vec!["echo".to_string(), "respond".to_string()]),
        Some(vec!["Echo: Hello World".to_string()]),
        Some(vec!["text".to_string()]),
        Some(vec!["text".to_string()]),
    );

    // Using the default no-authentication server for the example
    let server = WebSocketServer::new(processor, agent_info, handler, "127.0.0.1:8081".to_string());

    println!("🔗 WebSocket server listening on ws://127.0.0.1:8081");
    server
        .start()
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

async fn run_client() -> Result<(), Box<dyn std::error::Error>> {
    println!("📱 Starting WebSocket client...");

    // Create WebSocket client
    let client = WebSocketClient::new("ws://127.0.0.1:8081".to_string());

    // Note: WebSocket client connects automatically on first request
    println!("🔌 WebSocket client ready");

    // Test 2: Create and send message to task
    println!("📨 Testing task creation and messaging...");

    let task_id = uuid::Uuid::new_v4().to_string();
    let task_id = format!("task-{}", task_id);

    let message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text(
            "Hello from WebSocket client! Please echo this message with streaming.".to_string(),
        )])
        .message_id(uuid::Uuid::new_v4().to_string())
        .build();

    match client
        .send_task_message(&task_id, &message, None, None)
        .await
    {
        Ok(response) => {
            println!("✅ Task created with ID: {}", task_id);
            println!("   Status: {:?}", response.status.state);
        }
        Err(e) => {
            println!("❌ Failed to send message: {}", e);
            return Err(e.into());
        }
    }

    // Test 3: Subscribe to streaming updates
    println!("📡 Testing streaming updates...");

    let mut stream = client.subscribe_to_task(&task_id, None).await?;
    println!("✅ Subscribed to task updates");

    // Collect a few streaming updates with timeout
    let mut update_count = 0;
    let max_updates = 3;
    let timeout_duration = Duration::from_secs(5);

    println!(
        "🎯 Waiting for streaming updates (max {} seconds)...",
        timeout_duration.as_secs()
    );

    let timeout_future = sleep(timeout_duration);
    tokio::pin!(timeout_future);

    loop {
        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(stream_item)) => {
                        match stream_item {
                            StreamItem::Task(task) => {
                                println!("📦 Task update: {} - {:?}", task.id, task.status.state);
                                update_count += 1;
                            }
                            StreamItem::StatusUpdate(update) => {
                                println!("📈 Status update #{}: {:?}", update_count + 1, update.status.state);
                                update_count += 1;

                                // Check if this is the final update
                                if update.final_ {
                                    println!("🏁 Final update received");
                                    break;
                                }
                            }
                            StreamItem::ArtifactUpdate(artifact_event) => {
                                println!("🎁 Artifact update: {}", artifact_event.artifact.artifact_id);
                                update_count += 1;

                                // Check if this is the last chunk
                                if artifact_event.last_chunk.unwrap_or(false) {
                                    println!("🏁 Final artifact chunk received");
                                    break;
                                }
                            }
                        }

                        if update_count >= max_updates {
                            println!("📊 Received {} updates, stopping", update_count);
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        println!("❌ Stream error: {}", e);
                        break;
                    }
                    None => {
                        println!("📡 Stream ended");
                        break;
                    }
                }
            }
            _ = &mut timeout_future => {
                println!("⏰ Timeout reached, stopping stream");
                break;
            }
        }
    }

    // Test 4: Get task back
    println!("📤 Testing task retrieval...");
    match client.get_task(&task_id, None).await {
        Ok(task) => {
            println!("✅ Retrieved task: {}", task.id);
            println!("   Status: {:?}", task.status.state);
            if let Some(history) = &task.history {
                println!("   History entries: {}", history.len());
            }
        }
        Err(e) => {
            println!("❌ Failed to get task: {}", e);
            return Err(e.into());
        }
    }

    // Test 5: Cancel task (if not already completed)
    println!("🛑 Testing task cancellation...");
    match client.cancel_task(&task_id).await {
        Ok(task) => {
            println!("✅ Task canceled: {}", task.id);
            println!("   Final status: {:?}", task.status.state);
        }
        Err(e) => {
            // Task might already be completed, which is fine
            println!("ℹ️  Task cancellation result: {}", e);
        }
    }

    // Note: WebSocket client will disconnect automatically when dropped
    println!("🔌 WebSocket client finished");

    println!("🎉 All WebSocket client tests completed!");
    Ok(())
}
