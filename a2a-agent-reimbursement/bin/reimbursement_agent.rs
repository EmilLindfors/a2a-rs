//! Reimbursement Agent Binary
//!
//! Standalone executable for running the expense reimbursement agent server.
//!
//! Usage:
//!   reimbursement-agent [OPTIONS]
//!
//! Options:
//!   -c, --config <FILE>    Path to configuration file [default: reimbursement.toml]
//!   -p, --port <PORT>      Override HTTP port
//!   --ws-port <PORT>       Override WebSocket port
//!   -h, --help             Print help
//!   -V, --version          Print version

use a2a_agent_reimbursement::config::{AuthConfig, ServerConfig, StorageConfig};
use a2a_agent_reimbursement::server::ReimbursementServer;
use clap::Parser;
use tracing_subscriber::EnvFilter;

/// Expense Reimbursement Agent for A2A Protocol
#[derive(Parser, Debug)]
#[clap(
    name = "reimbursement-agent",
    about = "Intelligent expense reimbursement assistant",
    version
)]
struct Args {
    /// Path to configuration file
    #[clap(short, long, default_value = "reimbursement.toml")]
    config: String,

    /// Override HTTP port
    #[clap(short = 'p', long)]
    port: Option<u16>,

    /// Override WebSocket port
    #[clap(long)]
    ws_port: Option<u16>,
}

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

    // Parse command line arguments
    let args = Args::parse();

    tracing::info!("🚀 Starting Reimbursement Agent");
    tracing::info!("📄 Configuration file: {}", args.config);

    // Load configuration or use defaults
    let mut server_config = if std::path::Path::new(&args.config).exists() {
        // Try to load from TOML
        match std::fs::read_to_string(&args.config) {
            Ok(content) => match toml::from_str::<toml::Value>(&content) {
                Ok(config) => {
                    let http_port = config
                        .get("server")
                        .and_then(|s| s.get("http_port"))
                        .and_then(|p| p.as_integer())
                        .unwrap_or(8080) as u16;
                    let ws_port = config
                        .get("server")
                        .and_then(|s| s.get("ws_port"))
                        .and_then(|p| p.as_integer())
                        .unwrap_or(8081) as u16;

                    ServerConfig {
                        host: "127.0.0.1".to_string(),
                        http_port,
                        ws_port,
                        storage: StorageConfig::InMemory,
                        auth: AuthConfig::None,
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to parse config file: {}", e);
                    tracing::warn!("Using default configuration");
                    ServerConfig::default()
                }
            },
            Err(_) => {
                tracing::warn!("Config file not found, using defaults");
                ServerConfig::default()
            }
        }
    } else {
        tracing::warn!("Config file '{}' not found, using defaults", args.config);
        ServerConfig::default()
    };

    // Override ports if specified
    if let Some(port) = args.port {
        tracing::info!("Overriding HTTP port: {}", port);
        server_config.http_port = port;
    }
    if let Some(ws_port) = args.ws_port {
        tracing::info!("Overriding WebSocket port: {}", ws_port);
        server_config.ws_port = ws_port;
    }

    let http_port = server_config.http_port;
    let ws_port = server_config.ws_port;

    tracing::info!("🔌 HTTP Port: {}", http_port);
    tracing::info!("📡 WebSocket Port: {}", ws_port);
    tracing::info!("💾 Storage: In-Memory");

    // Create and run the server
    let server = ReimbursementServer::from_config(server_config);

    tracing::info!("");
    tracing::info!("✅ Reimbursement Agent Server Ready!");
    tracing::info!("   HTTP:      http://127.0.0.1:{}", http_port);
    tracing::info!("   WebSocket: ws://127.0.0.1:{}", ws_port);
    tracing::info!("");
    tracing::info!("💡 Test with curl:");
    tracing::info!("   curl -X POST http://127.0.0.1:{}/message/send \\", http_port);
    tracing::info!("     -H 'Content-Type: application/json' \\");
    tracing::info!("     -d '{{\"message\":{{\"role\":\"user\",\"parts\":[{{\"type\":\"text\",\"text\":\"I need help\"}}]}}}}'");
    tracing::info!("");

    // Run the server
    server.start_all().await?;

    Ok(())
}
