//! Simple example of running the reimbursement agent
//!
//! This example shows how to create and run a reimbursement agent
//! with in-memory storage.

use a2a_agent_reimbursement::ReimbursementHandler;
use a2a_rs::InMemoryTaskStorage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info,a2a_rs=debug,a2a_agent_reimbursement=debug")
        .init();

    tracing::info!("🚀 Starting Reimbursement Agent (Simple Example)");

    // Create storage
    let storage = InMemoryTaskStorage::default();

    // Create handler
    let handler = ReimbursementHandler::new(storage.clone());

    // For a real server, you would use a2a-rs server components
    // This is just a simple example showing the handler creation

    tracing::info!("✅ Handler created successfully");
    tracing::info!("💡 Use AgentBuilder from a2a-agents for a full server setup");

    Ok(())
}
