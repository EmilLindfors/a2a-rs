# a2a-rs

[![Crates.io](https://img.shields.io/crates/v/a2a-rs.svg)](https://crates.io/crates/a2a-rs)
[![Documentation](https://docs.rs/a2a-rs/badge.svg)](https://docs.rs/a2a-rs)
[![CI](https://github.com/emillindfors/a2a-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/emillindfors/a2a-rs/actions/workflows/rust.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A Rust implementation of the [Agent-to-Agent (A2A) Protocol](https://google.github.io/A2A/) v0.3.0. Provides a modular framework for building agents that communicate over a standardized JSON-RPC 2.0 protocol, following hexagonal architecture principles.

## Overview

The workspace is organized into several crates:

| Crate | Description |
|-------|-------------|
| [a2a-rs](./a2a-rs/) | Core protocol library — types, traits, transports, storage |
| [a2a-ap2](./a2a-ap2/) | Agent Payments Protocol (AP2) extension |
| [a2a-agents](./a2a-agents/) | Declarative agent framework with TOML configuration |
| [a2a-agents-common](./a2a-agents-common/) | Shared utilities (NLP, formatting, testing fixtures) |
| [a2a-client](./a2a-client/) | Web client library for building agent frontends |

## Quick start

```bash
git clone https://github.com/emillindfors/a2a-rs.git
cd a2a-rs

# Run the reimbursement agent demo (agent + web UI on http://localhost:3000)
cd a2a-agents && cargo run --bin reimbursement_demo
```

### Add to your project

```toml
[dependencies]
# Server with default features (in-memory storage, tracing)
a2a-rs = "0.2.0"

# HTTP client
a2a-rs = { version = "0.2.0", features = ["http-client"] }

# HTTP server with Axum
a2a-rs = { version = "0.2.0", features = ["http-server"] }

# All transports, auth, SQLite + PostgreSQL storage
a2a-rs = { version = "0.2.0", features = ["full"] }
```

## Features

The core library uses Cargo feature flags so you only compile what you need:

| Feature | Description |
|---------|-------------|
| `server` (default) | Async server traits and in-memory storage |
| `tracing` (default) | Structured logging via `tracing` |
| `http-server` | Axum-based HTTP server |
| `ws-server` | WebSocket server via tokio-tungstenite |
| `http-client` | HTTP client via reqwest |
| `ws-client` | WebSocket client |
| `auth` | JWT, OAuth2, OpenID Connect authentication |
| `sqlite` | SQLite storage via SQLx |
| `postgres` | PostgreSQL storage via SQLx |
| `full` | All of the above |

## Usage

### Client

```rust
use a2a_rs::{HttpClient, Message};
use a2a_rs::port::AsyncA2AClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = HttpClient::new("http://localhost:3030".to_string());

    let message = Message::user_text("I need to submit a $50 lunch expense".to_string());
    let task = client.send_task_message("task-123", &message, None, None).await?;

    println!("Task state: {:?}", task.status.state);
    Ok(())
}
```

### Server

```rust
use a2a_rs::{HttpServer, SimpleAgentInfo, DefaultRequestProcessor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = HttpServer::new(
        DefaultRequestProcessor::new(),
        SimpleAgentInfo::new("my-agent".to_string(), "1.0.0".to_string()),
        "127.0.0.1:3030".to_string(),
    );

    server.start().await?;
    Ok(())
}
```

### Streaming with WebSocket

```rust
use a2a_rs::{WebSocketClient, Message};
use a2a_rs::services::StreamItem;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = WebSocketClient::new("ws://localhost:3030/ws".to_string());

    let message = Message::user_text("Process my request".to_string());
    let mut stream = client.subscribe_to_task("task-456", &message, None, None).await?;

    while let Some(result) = stream.next().await {
        match result? {
            StreamItem::Task(task) => println!("Task: {:?}", task),
            StreamItem::StatusUpdate(update) => {
                println!("Status: {:?}", update);
                if update.final_ { break; }
            }
            StreamItem::ArtifactUpdate(artifact) => {
                println!("Artifact: {:?}", artifact);
            }
        }
    }

    Ok(())
}
```

### Declarative agent (TOML-based)

The `a2a-agents` framework lets you define agents with minimal boilerplate:

```rust
use a2a_agents::AgentBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    AgentBuilder::from_file("agent.toml")?
        .with_handler(MyHandler)
        .build_with_auto_storage()
        .await?
        .run()
        .await?;
    Ok(())
}
```

See [a2a-agents/examples/](./a2a-agents/examples/) for complete examples.

## Architecture

The core library follows hexagonal architecture with clear layer separation:

```
                        Application Layer
            ┌──────────────────┬─────────────────────┐
            │  JSON-RPC        │  HTTP / WebSocket    │
            │  Handlers        │  Transport           │
            └────────┬─────────┴──────────┬──────────┘
                     │                    │
                     v                    v
                         Port Layer
            ┌──────────────────┬─────────────────────┐
            │  MessageHandler  │  StreamingHandler    │
            │  TaskManager     │  NotificationManager │
            │  Authenticator   │  RequestProcessor    │
            └────────┬─────────┴──────────┬──────────┘
                     │                    │
                     v                    v
                        Domain Layer
            ┌──────────────────┬─────────────────────┐
            │  Message, Part   │  AgentCard           │
            │  Task, Artifact  │  Capabilities        │
            │  TaskStatus      │  SecurityScheme      │
            └──────────────────┴─────────────────────┘
```

Port traits define the contracts between layers. Implement `AsyncMessageHandler` to handle incoming messages; implement `AsyncTaskManager` for task persistence. The framework provides default implementations (in-memory storage, SQLx backends) that can be swapped without changing business logic.

## Protocol coverage

Implements the full A2A v0.3.0 specification:

- `message/send` and `message/stream` (blocking and streaming message exchange)
- `tasks/get`, `tasks/list`, `tasks/cancel`, `tasks/resubscribe`
- Push notification CRUD (set, get, list, delete)
- `agent/getAuthenticatedExtendedCard`
- Security schemes: HTTP bearer, API key, OAuth2, OpenID Connect, mTLS
- Task states: submitted, working, input-required, completed, canceled, failed, rejected, auth-required

## Testing

```bash
# Full workspace
cargo test --workspace

# Core library with all features
cd a2a-rs && cargo test --all-features

# Agent framework
cd a2a-agents && cargo test
```

The test suite includes unit tests, integration tests, property-based tests, and spec compliance tests.

## Contributing

Contributions are welcome. To get started:

```bash
git clone https://github.com/emillindfors/a2a-rs.git
cd a2a-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

See [ISSUES.md](./ISSUES.md) for known issues and areas where help is appreciated.

## License

MIT
