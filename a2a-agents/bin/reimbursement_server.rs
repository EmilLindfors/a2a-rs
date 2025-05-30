use a2a_agents::reimbursement_agent::{AuthConfig, ModernReimbursementServer, ServerConfig};
use clap::Parser;

/// Command-line arguments for the reimbursement server
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Host to bind the server to
    #[clap(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on (HTTP server)
    #[clap(long, default_value = "8080")]
    http_port: u16,

    /// WebSocket port
    #[clap(long, default_value = "8081")]
    ws_port: u16,

    /// Configuration file path (JSON format)
    #[clap(long)]
    config: Option<String>,

    /// Server mode: http, websocket, or both
    #[clap(long, default_value = "both")]
    mode: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with better formatting
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Parse command-line arguments
    let args = Args::parse();

    // Load configuration
    let mut config = if let Some(config_path) = args.config {
        println!("📄 Loading config from: {}", config_path);
        std::env::set_var("CONFIG_FILE", config_path);
        ServerConfig::load()?
    } else {
        ServerConfig::from_env()
    };

    // Override config with command-line arguments
    config.host = args.host;
    config.http_port = args.http_port;
    config.ws_port = args.ws_port;

    println!("🚀 Starting Modern Reimbursement Agent Server");
    println!("===============================================");
    println!("📍 Host: {}", config.host);
    println!("🔌 HTTP Port: {}", config.http_port);
    println!("📡 WebSocket Port: {}", config.ws_port);
    println!("⚙️  Mode: {}", args.mode);
    match &config.storage {
        a2a_agents::reimbursement_agent::StorageConfig::InMemory => {
            println!("💾 Storage: In-memory (non-persistent)");
        }
        a2a_agents::reimbursement_agent::StorageConfig::Sqlx { url, .. } => {
            println!("💾 Storage: SQLx ({})", url);
        }
    }
    match &config.auth {
        AuthConfig::None => {
            println!("🔓 Authentication: None (public access)");
        }
        AuthConfig::BearerToken { tokens, format } => {
            println!("🔐 Authentication: Bearer token ({} token(s){})", 
                tokens.len(),
                format.as_ref().map(|f| format!(", format: {}", f)).unwrap_or_default()
            );
        }
        AuthConfig::ApiKey { keys, location, name } => {
            println!("🔐 Authentication: API key ({} {} '{}', {} key(s))", 
                location, name, name, keys.len()
            );
        }
    }
    println!();

    // Create the modern server
    let server = ModernReimbursementServer::from_config(config);

    // Start the server based on mode
    match args.mode.as_str() {
        "http" => {
            println!("🌐 Starting HTTP server only...");
            server.start_http().await?;
        }
        "websocket" | "ws" => {
            println!("🔌 Starting WebSocket server only...");
            server.start_websocket().await?;
        }
        "both" | "all" => {
            println!("🔄 Starting both HTTP and WebSocket servers...");
            server.start_all().await?;
        }
        _ => {
            eprintln!("❌ Invalid mode: {}. Use 'http', 'websocket', or 'both'", args.mode);
            std::process::exit(1);
        }
    }

    Ok(())
}
