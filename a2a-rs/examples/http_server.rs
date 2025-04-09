//! A simple HTTP server example

use a2a_rs::{
    adapter::server::{DefaultRequestProcessor, HttpServer, InMemoryTaskStorage, SimpleAgentInfo},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create task storage
    let storage = InMemoryTaskStorage::new();
    
    // Create request processor
    let processor = DefaultRequestProcessor::new(storage);
    
    // Create agent info provider
    let agent_info = SimpleAgentInfo::new(
        "Example A2A Agent".to_string(),
        "http://localhost:8080".to_string(),
    )
    .with_description("An example A2A agent using the a2a-protocol crate".to_string())
    .with_provider("Example Organization".to_string(), Some("https://example.org".to_string()))
    .with_documentation_url("https://example.org/docs".to_string())
    .with_streaming()
    .add_skill(
        "echo".to_string(),
        "Echo Skill".to_string(),
        Some("Echoes back the user's message".to_string()),
    );
    
    // Create HTTP server
    let server = HttpServer::new(processor, agent_info, "127.0.0.1:8080".to_string());
    
    println!("Starting HTTP server on http://127.0.0.1:8080");
    println!("Try accessing the agent card at http://127.0.0.1:8080/agent-card");
    println!("Press Ctrl+C to stop");
    
    // Start the server
    server.start().await?;
    
    Ok(())
}