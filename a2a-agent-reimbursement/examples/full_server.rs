//! Complete working example of the reimbursement agent with HTTP/WebSocket servers
//!
//! This example shows how to set up and run a full reimbursement agent server
//! using the a2a-agents framework.
//!
//! Run with: cargo run --example full_server

use a2a_agent_reimbursement::ReimbursementHandler;
use a2a_agents::core::{AgentBuilder, AgentConfig};
use a2a_rs::InMemoryTaskStorage;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,a2a_rs=debug,a2a_agent_reimbursement=debug")),
        )
        .init();

    tracing::info!("🚀 Starting Reimbursement Agent Server");

    // Load configuration
    let config = AgentConfig::from_file("reimbursement.toml")?;
    tracing::info!("📄 Configuration loaded: {}", config.agent.name);
    tracing::info!("🔌 HTTP Port: {}", config.server.http_port);
    tracing::info!("📡 WebSocket Port: {}", config.server.ws_port);

    // Create storage
    let storage = InMemoryTaskStorage::default();
    tracing::info!("💾 Using in-memory storage");

    // Create reimbursement handler
    let handler = ReimbursementHandler::new(storage.clone());
    tracing::info!("✅ Reimbursement handler created");

    // Build the agent using AgentBuilder
    // Note: The current builder API expects handlers that don't need storage passed in
    // For now, we'll use the lower-level server setup from the reimbursement server module

    // Use the ReimbursementServer from the crate
    use a2a_agent_reimbursement::server::ReimbursementServer;
    use a2a_agent_reimbursement::config::ServerConfig;

    let server_config = ServerConfig {
        host: "127.0.0.1".to_string(),
        http_port: config.server.http_port,
        ws_port: config.server.ws_port,
        storage: a2a_agent_reimbursement::config::StorageConfig::InMemory,
        auth: a2a_agent_reimbursement::config::AuthConfig::None,
    };

    let server = ReimbursementServer::from_config(server_config);

    tracing::info!("🎯 Starting agent servers...");
    tracing::info!("   HTTP:      http://127.0.0.1:{}", config.server.http_port);
    tracing::info!("   WebSocket: ws://127.0.0.1:{}", config.server.ws_port);
    tracing::info!("");
    tracing::info!("💡 Try sending a message:");
    tracing::info!("   curl -X POST http://127.0.0.1:{}/message/send \\", config.server.http_port);
    tracing::info!("     -H 'Content-Type: application/json' \\");
    tracing::info!("     -d '{{");
    tracing::info!("       \"message\": {{");
    tracing::info!("         \"role\": \"user\",");
    tracing::info!("         \"parts\": [{{\"type\": \"text\", \"text\": \"I need to submit a reimbursement\"}}]");
    tracing::info!("       }}");
    tracing::info!("     }}'");
    tracing::info!("");

    // Run the server
    server.start_all().await?;

    Ok(())
}
