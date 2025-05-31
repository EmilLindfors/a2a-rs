use a2a_agents::reimbursement_agent::{
    config::{AuthConfig, ServerConfig, StorageConfig},
    server::ReimbursementServer,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== A2A Agents Configuration Demo ===\n");

    // Example 1: In-memory storage (default)
    println!("1. In-Memory Storage Configuration:");
    let config1 = ServerConfig {
        host: "127.0.0.1".to_string(),
        http_port: 8080,
        ws_port: 8081,
        storage: StorageConfig::InMemory,
        auth: AuthConfig::None,
    };
    println!("   Storage: {:?}", config1.storage);
    println!("   Auth: {:?}", config1.auth);
    println!();

    // Example 2: SQLx storage configuration
    println!("2. SQLx Storage Configuration:");
    let config2 = ServerConfig {
        host: "127.0.0.1".to_string(),
        http_port: 8080,
        ws_port: 8081,
        storage: StorageConfig::Sqlx {
            url: "sqlite://reimbursement.db".to_string(),
            max_connections: 10,
            enable_logging: true,
        },
        auth: AuthConfig::None,
    };
    println!("   Storage: {:?}", config2.storage);
    println!();

    // Example 3: Bearer token authentication
    println!("3. Bearer Token Authentication:");
    let config3 = ServerConfig {
        host: "127.0.0.1".to_string(),
        http_port: 8080,
        ws_port: 8081,
        storage: StorageConfig::InMemory,
        auth: AuthConfig::BearerToken {
            tokens: vec![
                "secret_token_123".to_string(),
                "another_token_456".to_string(),
            ],
            format: Some("Bearer {}".to_string()),
        },
    };
    println!("   Auth: {:?}", config3.auth);
    println!();

    // Example 4: Combined SQLx + Auth
    println!("4. Production Configuration (SQLx + Auth):");
    let config4 = ServerConfig {
        host: "0.0.0.0".to_string(),
        http_port: 8080,
        ws_port: 8081,
        storage: StorageConfig::Sqlx {
            url: "postgres://user:password@localhost/reimbursement_prod".to_string(),
            max_connections: 50,
            enable_logging: false,
        },
        auth: AuthConfig::BearerToken {
            tokens: vec!["prod_token_abc123".to_string()],
            format: Some("A2A-Token {}".to_string()),
        },
    };
    println!("   Storage: {:?}", config4.storage);
    println!("   Auth: {:?}", config4.auth);
    println!();

    // Demonstrate server creation
    println!("5. Server Creation Examples:");

    // Simple server
    let _simple_server = ReimbursementServer::new("127.0.0.1".to_string(), 8080);
    println!("   Simple server created (in-memory storage, no auth)");

    // Config-based server
    let _config_server = ReimbursementServer::from_config(config1);
    println!("   Config-based server created");

    // Another config-based server
    let _another_server = ReimbursementServer::from_config(config2);
    println!("   SQLx config server created");

    println!("\n=== Configuration Files ===");
    println!("Available configuration examples:");
    println!("  - config.example.json (basic in-memory)");
    println!("  - config.sqlx.example.json (SQLx storage)");
    println!("  - config.auth.example.json (with authentication)");
    println!("  - config.apikey.example.json (API key auth)");

    println!("\n=== Usage Commands ===");
    println!("Start server with different configs:");
    println!("  cargo run --bin reimbursement_server -- --config config.example.json");
    println!("  cargo run --bin reimbursement_server -- --config config.sqlx.example.json");
    println!("  cargo run --bin reimbursement_server -- --config config.auth.example.json");

    println!("\n=== Automatic Migrations ===");
    println!("SQLx storage automatically handles migrations:");
    println!("  ✅ Base A2A framework tables (tasks, task_history, etc.)");
    println!("  ✅ Reimbursement-specific tables (reimbursement_requests, receipts, etc.)");
    println!("  ✅ No manual migration commands needed!");
    println!("  ✅ Database file created automatically if it doesn't exist");

    Ok(())
}
