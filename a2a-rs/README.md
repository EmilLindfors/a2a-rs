# a2a-rs

[![Crates.io](https://img.shields.io/crates/v/a2a-rs.svg)](https://crates.io/crates/a2a-rs)
[![Documentation](https://docs.rs/a2a-rs/badge.svg)](https://docs.rs/a2a-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A Rust implementation of the Agent-to-Agent (A2A) Protocol v0.3.0, providing a type-safe, idiomatic way to build agent communication systems.

## Features

- 🚀 **A2A Protocol v0.3.0** - Full support for the latest A2A specification including:
  - Enhanced push notification management with listing and deletion
  - Task listing with comprehensive filtering and pagination
  - Authenticated extended card support
  - Protocol extensions framework
  - Multi-transport support (JSONRPC, GRPC, HTTP+JSON)
- 🔄 **Multiple Transport Options** - HTTP and WebSocket support
- 📡 **Streaming Updates** - Real-time task and artifact updates
- 🔐 **Authentication & Security** - JWT, OAuth2, OpenID Connect support with agent card signatures
- 💾 **Persistent Storage** - SQLx integration for task persistence
- 🎯 **Async-First Design** - Built on Tokio with async/await throughout
- 🧩 **Modular Architecture** - Use only the features you need
- ✅ **Type Safety** - Leverages Rust's type system for protocol compliance

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
a2a-rs = "0.1.0"

# For HTTP client
a2a-rs = { version = "0.1.0", features = ["http-client"] }

# For HTTP server
a2a-rs = { version = "0.1.0", features = ["http-server"] }

# Full feature set
a2a-rs = { version = "0.1.0", features = ["full"] }
```

### Client Example

```rust
use a2a_rs::{HttpClient, Message};
use a2a_rs::services::AsyncA2AClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = HttpClient::new("https://api.example.com".to_string());

    let message = Message::user_text("Hello, agent!".to_string(), "msg-123".to_string());
    let task = client.send_task_message("task-123", &message, None, None).await?;

    println!("Task created: {:?}", task);
    Ok(())
}
```

### Server Example

```rust
use a2a_rs::{HttpServer, Message, Task, A2AError};
use a2a_rs::port::{AsyncTaskHandler, AgentInfoProvider};

struct MyAgent;

#[async_trait::async_trait]
impl AsyncTaskHandler for MyAgent {
    async fn handle_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        // Process the message and return updated task
        Ok(Task::new(task_id.to_string()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = HttpServer::new(
        MyAgent,
        AgentInfo::default(),
        "127.0.0.1:8080".to_string(),
    );
    
    server.start().await?;
    Ok(())
}
```

## Architecture

This library follows a hexagonal architecture pattern:

- **Domain**: Core business logic and types
- **Ports**: Trait definitions for external dependencies
- **Adapters**: Concrete implementations for different transports and storage

## Feature Flags

- `client` - Client-side functionality
- `server` - Server-side functionality  
- `http-client` - HTTP client implementation
- `http-server` - HTTP server implementation
- `ws-client` - WebSocket client implementation
- `ws-server` - WebSocket server implementation
- `auth` - Authentication support (JWT, OAuth2, OpenID Connect)
- `sqlx-storage` - SQLx-based persistent storage
- `sqlite` - SQLite database support
- `postgres` - PostgreSQL database support
- `mysql` - MySQL database support
- `tracing` - Structured logging and tracing
- `full` - All features enabled

## Examples

See the [examples](examples/) directory for complete working examples:

- [HTTP Client/Server](examples/http_client_server.rs)
- [WebSocket Client/Server](examples/websocket_client_server.rs)
- [SQLx Storage Demo](examples/sqlx_storage_demo.rs)
- [Storage Comparison](examples/storage_comparison.rs)

## Documentation

Full API documentation is available on [docs.rs](https://docs.rs/a2a-rs).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.