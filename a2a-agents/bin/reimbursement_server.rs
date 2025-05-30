use a2a_agents::reimbursement_agent::modern_server::ModernReimbursementServer;
use clap::Parser;

/// Command-line arguments for the reimbursement server
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Host to bind the server to
    #[clap(long, default_value = "localhost")]
    host: String,

    /// Port to listen on (HTTP server, WebSocket will use port+1)
    #[clap(long, default_value = "10002")]
    port: u16,

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

    println!("🚀 Starting Modern Reimbursement Agent Server");
    println!("===============================================");
    println!("📍 Host: {}", args.host);
    println!("🔌 HTTP Port: {}", args.port);
    println!("📡 WebSocket Port: {}", args.port + 1);
    println!("⚙️  Mode: {}", args.mode);
    println!();

    // Create the modern server
    let server = ModernReimbursementServer::new(args.host, args.port);

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
