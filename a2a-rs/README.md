# a2a-rs

[![Crates.io](https://img.shields.io/crates/v/a2a-rs.svg)](https://crates.io/crates/a2a-rs)
[![Documentation](https://docs.rs/a2a-rs/badge.svg)](https://docs.rs/a2a-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A Rust implementation of the Agent-to-Agent (A2A) Protocol v1.0.0, providing a type-safe, idiomatic way to build agent communication systems.

## Features

- 🚀 **A2A Protocol v1.0.0** - Implements the A2A specification (see [Spec compliance](#spec-compliance) for the small, documented divergences), including:
  - Enhanced push notification management with listing and deletion
  - Task listing with comprehensive filtering and pagination
  - Authenticated extended card support
  - Protocol extensions framework
  - Multi-transport support: spec-compliant JSON-RPC 2.0 and HTTP+JSON, plus ConnectRPC (see [Spec compliance](#spec-compliance))
- 🔄 **Multiple Transport Options** - HTTP support
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
use a2a_rs::Transport;

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

## Spec compliance

`a2a-rs` targets **A2A Protocol v1.0.0** and is wire-compatible with the
specification: the domain types, transports, and `StreamResponse`/JSON-RPC
payloads follow the spec, so off-the-shelf A2A clients and servers interoperate.
There are a couple of small, deliberate divergences, all backward-compatible:

- **`Last-Event-ID` stream resumption is an opt-in enhancement, not a spec
  feature.** The A2A spec reconnects a dropped stream by re-issuing the subscribe
  call (resuming from the task's *current* state). On top of that, `a2a-rs` adds
  gap-free resumption using the **W3C SSE-standard** `id:` field and
  `Last-Event-ID` header (`RetryingTransport` / `WebA2AClient::subscribe_resilient`
  on the client; buffered replay on the server). This is fully interoperable —
  spec clients ignore the `id:` field and never send the header, getting standard
  reconnect-from-current-state behavior — but **gap-free resume only works
  a2a-rs ↔ a2a-rs**, not against third-party agents. For strictly spec-shaped
  streaming, use `WebA2AClient::subscribe` (or `subscribe_to_task` with
  `last_event_id = None`).
- **ConnectRPC is offered as an additional transport.** The spec names three
  transport bindings — `JSONRPC`, `GRPC`, and `HTTP+JSON`. `a2a-rs` adds
  **ConnectRPC** as the in-tree default (advertised in the agent card under the
  non-spec `CONNECTRPC` binding), alongside a spec-compliant **JSON-RPC 2.0**
  transport and HTTP+JSON/REST. For interop with third-party A2A agents use the
  JSON-RPC transport (`JsonRpcClient` / `jsonrpc_router`); ConnectRPC is the
  preferred path a2a-rs ↔ a2a-rs.
- **JSON-RPC method names follow the proto RPC names** (`SubscribeToTask`,
  `SendStreamingMessage`, …) rather than the canonical JSON-RPC strings
  (`tasks/resubscribe`, `message/stream`); the request/response bodies are
  spec-shaped ProtoJSON.

## Feature Flags

- `client` - Client-side functionality
- `server` - Server-side functionality  
- `http-client` - HTTP client implementation
- `http-server` - HTTP server implementation
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
- [SQLx Storage Demo](examples/sqlx_storage_demo.rs)
- [Storage Comparison](examples/storage_comparison.rs)

## Documentation

Full API documentation is available on [docs.rs](https://docs.rs/a2a-rs).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.