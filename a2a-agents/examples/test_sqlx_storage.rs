use a2a_agents::reimbursement_agent::{
    config::{ServerConfig, StorageConfig},
    server::ReimbursementServer,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info,a2a_rs=debug,a2a_agents=debug")
        .init();

    // Create configuration with SQLx storage
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        http_port: 8080,
        ws_port: 8081,
        storage: StorageConfig::Sqlx {
            url: "sqlite://reimbursement_test.db".to_string(),
            max_connections: 5,
            enable_logging: true,
        },
        auth: Default::default(),
    };

    println!("🚀 Starting Reimbursement Agent with SQLx Storage");
    println!("📦 Database: sqlite://reimbursement_test.db");
    println!("✨ Automatic Migrations: Both base A2A tables and reimbursement tables will be created automatically!");
    println!("📝 No manual migration needed - SQLx handles everything!");
    println!();

    // Create and start server
    let server = ReimbursementServer::from_config(config);

    println!("📋 Starting HTTP server...");
    println!("   This will automatically:");
    println!("   1. Create the database file if it doesn't exist");
    println!("   2. Run base A2A framework migrations (tasks, task_history, etc.)");
    println!(
        "   3. Run reimbursement-specific migrations (reimbursement_requests, receipts, etc.)"
    );
    println!("   4. Start the HTTP server on port 8080");
    println!();

    server.start_http().await?;

    Ok(())
}
