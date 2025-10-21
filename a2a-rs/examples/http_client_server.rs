//! A complete HTTP example that runs both server and client together

use std::time::Duration;
use tokio::time::sleep;

use a2a_rs::adapter::{
    BearerTokenAuthenticator, DefaultRequestProcessor, HttpClient, HttpServer, InMemoryTaskStorage,
    NoopPushNotificationSender, SimpleAgentInfo,
};

mod common;
use a2a_rs::domain::{Message, Part, Role};
use a2a_rs::observability;
use a2a_rs::services::AsyncA2AClient;
use common::SimpleAgentHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for better observability
    observability::init_tracing();

    println!("🚀 Starting HTTP Full Example");
    println!("==============================");

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

    println!("🏁 HTTP Full Example completed");
    Ok(())
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 Starting HTTP server...");

    // Create server components
    let push_sender = NoopPushNotificationSender;
    let storage = InMemoryTaskStorage::with_push_sender(push_sender);
    let handler = SimpleAgentHandler::with_storage(storage);
    let test_agent_info = SimpleAgentInfo::new(
        "test-agent".to_string(),
        "http://localhost:8080".to_string(),
    );
    let processor = DefaultRequestProcessor::with_handler(handler, test_agent_info);

    // Create agent info
    let agent_info = SimpleAgentInfo::new(
        "Example A2A Agent".to_string(),
        "http://localhost:8080".to_string(),
    )
    .with_description("An example A2A agent using the a2a-protocol crate".to_string())
    .with_provider(
        "Example Organization".to_string(),
        "https://example.org".to_string(),
    )
    .with_documentation_url("https://example.org/docs".to_string())
    .with_streaming()
    .add_comprehensive_skill(
        "echo".to_string(),
        "Echo Skill".to_string(),
        Some("Echoes back the user's message".to_string()),
        Some(vec!["echo".to_string(), "respond".to_string()]),
        Some(vec!["Echo: Hello World".to_string()]),
        Some(vec!["text".to_string()]),
        Some(vec!["text".to_string()]),
    );

    // Server with bearer token authentication
    let tokens = vec!["secret-token".to_string()];
    let authenticator = BearerTokenAuthenticator::new(tokens);
    let server = HttpServer::with_auth(
        processor,
        agent_info,
        "127.0.0.1:8080".to_string(),
        authenticator,
    );

    println!("🔗 HTTP server listening on http://127.0.0.1:8080");
    server
        .start()
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

async fn run_client() -> Result<(), Box<dyn std::error::Error>> {
    println!("📱 Starting HTTP client...");

    // Create HTTP client with authentication
    let client = HttpClient::with_auth(
        "http://127.0.0.1:8080".to_string(),
        "secret-token".to_string(),
    );

    // Note: HTTP client communicates via JSON-RPC, not direct REST endpoints
    println!("📋 HTTP client connected successfully");

    // Test 3: Create and send message to task
    println!("📨 Testing task creation and messaging...");

    let task_id = uuid::Uuid::new_v4().to_string();
    let task_id = format!("task-{}", task_id);

    let message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text(
            "Hello from HTTP client! Please echo this message.".to_string(),
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

    // Test 5: Cancel task
    println!("🛑 Testing task cancellation...");
    match client.cancel_task(&task_id).await {
        Ok(task) => {
            println!("✅ Task canceled: {}", task.id);
            println!("   Final status: {:?}", task.status.state);
        }
        Err(e) => {
            println!("❌ Failed to cancel task: {}", e);
            return Err(e.into());
        }
    }

    println!("🎉 All HTTP client tests passed!");
    Ok(())
}
