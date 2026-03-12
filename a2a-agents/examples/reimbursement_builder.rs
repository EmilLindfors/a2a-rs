//! Reimbursement agent using the new declarative builder API
//!
//! This example shows how to create a full-featured agent using TOML configuration
//! with much less boilerplate than the traditional approach.

use a2a_agents::{
    agents::reimbursement::ReimbursementHandler,
    core::{AgentBuilder, BuildError},
};
use a2a_rs::adapter::storage::SqlxTaskStorage;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    println!("🚀 Starting Reimbursement Agent with Builder API");
    println!();

    // Get database URL from environment or use default
    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:reimbursement_tasks.db".to_string());

    // Create storage with custom migrations
    let migrations = &[include_str!("../migrations/001_create_reimbursements.sql")];
    let storage = SqlxTaskStorage::with_migrations(&database_url, migrations)
        .await
        .map_err(|e| format!("Failed to create storage: {}", e))?;

    // Create the handler
    let handler = ReimbursementHandler::new(storage.clone());

    // Build and run the agent - this is where the magic happens!
    // The configuration file defines all the metadata, skills, and features
    // The builder wires everything together automatically
    AgentBuilder::from_file("reimbursement.toml")?
        .with_config(|config| {
            // Override config from environment if needed
            if let Ok(port) = env::var("HTTP_PORT") {
                if let Ok(port_num) = port.parse() {
                    config.server.http_port = port_num;
                }
            }
            if let Ok(port) = env::var("WS_PORT") {
                if let Ok(port_num) = port.parse() {
                    config.server.ws_port = port_num;
                }
            }
        })
        .with_handler(handler)
        .with_storage(storage)
        .build()?
        .run()
        .await
        .map_err(|e| BuildError::RuntimeError(e.to_string()))?;

    Ok(())
}
