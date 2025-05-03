//! A simple WebSocket server example

use a2a_rs::adapter::server::{
        DefaultRequestProcessor, InMemoryTaskStorage, SimpleAgentInfo, WebSocketServer, HttpPushNotificationSender,
    };

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a custom push notification sender
    let push_sender = HttpPushNotificationSender::new();
    
    // Create task storage with the push notification sender
    let storage = InMemoryTaskStorage::with_push_sender(push_sender);
    
    // Create request processor
    let processor = DefaultRequestProcessor::new(storage.clone());
    
    // Create agent info provider
    let agent_info = SimpleAgentInfo::new(
        "Example A2A WebSocket Agent".to_string(),
        "ws://localhost:8081".to_string(),
    )
    .with_description("An example A2A WebSocket agent with streaming support".to_string())
    .with_provider("Example Organization".to_string(), Some("https://example.org".to_string()))
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
    
    // Uncomment the following lines to enable token-based authentication
    // let tokens = vec!["secret-token".to_string()];
    // let authenticator = TokenAuthenticator::new(tokens);
    // let server = WebSocketServer::with_auth(processor, agent_info, storage, "127.0.0.1:8081".to_string(), authenticator);
    
    // Using the default no-authentication server for the example
    let server = WebSocketServer::new(processor, agent_info, storage, "127.0.0.1:8081".to_string());
    
    println!("Starting WebSocket server on ws://127.0.0.1:8081");
    println!("This server supports streaming responses");
    println!("Push notifications are configured but require a valid endpoint");
    println!("Press Ctrl+C to stop");
    
    // Start the server
    server.start().await?;
    
    Ok(())
}