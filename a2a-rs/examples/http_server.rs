//! A simple HTTP server example

use a2a_rs::adapter::server::{
    DefaultRequestProcessor, NoopPushNotificationSender, HttpServer, InMemoryTaskStorage,
    SimpleAgentInfo,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a custom push notification sender
    let push_sender = NoopPushNotificationSender::default();

    // Create task storage with the push notification sender
    let storage = InMemoryTaskStorage::with_push_sender(push_sender);

    // Create request processor
    let processor = DefaultRequestProcessor::new(storage);

    // Create agent info provider
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

    // Uncomment the following lines to enable token-based authentication
    // let tokens = vec!["secret-token".to_string()];
    // let authenticator = TokenAuthenticator::new(tokens);
    // let server = HttpServer::with_auth(processor, agent_info, "127.0.0.1:8080".to_string(), authenticator);

    // Using the default no-authentication server for the example
    let server = HttpServer::new(processor, agent_info, "127.0.0.1:8080".to_string());

    println!("Starting HTTP server on http://127.0.0.1:8080");
    println!("Try accessing the agent card at http://127.0.0.1:8080/agent-card");
    println!("Try accessing the skills at http://127.0.0.1:8080/skills");
    println!("Try accessing a specific skill at http://127.0.0.1:8080/skills/echo");
    println!("Server will exit after handling requests for 15 seconds");

    // Start the server with a timeout
    let server_future = server.start();
    let timeout_future = tokio::time::sleep(tokio::time::Duration::from_secs(15));

    tokio::select! {
        result = server_future => {
            println!("Server exited: {:?}", result);
            result?;
        }
        _ = timeout_future => {
            println!("Server timeout reached, exiting gracefully");
        }
    }

    Ok(())
}
