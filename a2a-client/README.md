# a2a-client

A reusable Rust library for building web-based frontends for A2A (Agent-to-Agent) Protocol agents.

## Overview

`a2a-client` provides reusable components, utilities, and client wrappers for creating web applications that interact with A2A protocol agents. It's designed to be integrated into your own web applications, not used as a standalone application.

## Features

- 🔌 **Unified Client API** - Single interface for HTTP and WebSocket transports
- 📡 **SSE Streaming** - Server-Sent Events with automatic fallback to polling
- 🎨 **View Models** - Ready-to-use view models for tasks and messages
- 🔄 **Auto-reconnection** - Resilient WebSocket connections with retry logic
- 🧩 **Modular Components** - Use only what you need via feature flags
- 🦀 **Type-safe** - Leverages Rust's type system for protocol correctness

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
a2a-client = { path = "../a2a-client" }

# For Axum integration with SSE streaming components
a2a-client = { path = "../a2a-client", features = ["axum-components"] }
```

## Quick Start

### Basic HTTP Client

```rust
use a2a_client::WebA2AClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a client connected to your A2A agent
    let client = WebA2AClient::new_http("http://localhost:8080".to_string());

    // Send a message and get a task
    let message = a2a_rs::domain::Message::builder()
        .text("Hello, agent!")
        .build();

    let task = client.http.send_message(&message, None).await?;
    println!("Task ID: {}", task.id);

    Ok(())
}
```

### With WebSocket Support

```rust
use a2a_client::WebA2AClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Auto-detect available transports
    let client = WebA2AClient::auto_connect("http://localhost:8080").await?;

    if client.has_websocket() {
        println!("WebSocket support detected!");
    }

    Ok(())
}
```

### SSE Streaming with Axum

```rust
use a2a_client::{WebA2AClient, components::create_sse_stream};
use axum::{Router, routing::get, extract::{State, Path}};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = Arc::new(
        WebA2AClient::new_with_websocket(
            "http://localhost:8080".to_string(),
            "ws://localhost:8080/ws".to_string()
        )
    );

    let app = Router::new()
        .route("/stream/:task_id", get(stream_handler))
        .with_state(client);

    // Start server...
    Ok(())
}

async fn stream_handler(
    State(client): State<Arc<WebA2AClient>>,
    Path(task_id): Path<String>,
) -> axum::response::sse::Sse<impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    create_sse_stream(client, task_id)
}
```

## Architecture

```
a2a-client/
├── src/
│   ├── lib.rs              # Core client API
│   ├── components/         # Reusable UI components
│   │   ├── streaming.rs    # SSE streaming helpers
│   │   └── task_viewer.rs  # View models for tasks/messages
│   └── utils/              # Utilities
│       └── formatters.rs   # Display formatting
└── examples/               # Usage examples (coming soon)
```

## Core Components

### `WebA2AClient`

The main client struct that wraps both HTTP and WebSocket clients:

```rust
pub struct WebA2AClient {
    pub http: HttpClient,
    pub ws: Option<Arc<WebSocketClient>>,
}
```

**Methods:**
- `new_http(base_url)` - HTTP-only client
- `new_with_websocket(http_url, ws_url)` - Client with both transports
- `auto_connect(base_url)` - Auto-detect available transports
- `has_websocket()` - Check if WebSocket is available
- `websocket()` - Get WebSocket client reference

### View Models

#### `TaskView`

Display model for task lists:

```rust
pub struct TaskView {
    pub task_id: String,
    pub state: String,
    pub message_count: usize,
    pub last_message_preview: Option<String>,
}
```

#### `MessageView`

Display model for individual messages:

```rust
pub struct MessageView {
    pub id: String,
    pub role: String,
    pub content: String,
}
```

### Streaming Components

#### `create_sse_stream`

Creates a Server-Sent Events stream for task updates:

```rust
pub fn create_sse_stream(
    client: Arc<WebA2AClient>,
    task_id: String,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>>
```

**Features:**
- Automatic WebSocket connection with retry logic
- Graceful fallback to HTTP polling
- Emits `task-update`, `task-status`, and `artifact` events
- Keep-alive heartbeat

### Formatters

```rust
// Format task state for display
pub fn format_task_state(state: &TaskState) -> String;

// Extract text from message parts
pub fn format_message_content(parts: &[Part]) -> String;

// Truncate text for previews
pub fn truncate_preview(text: &str, max_len: usize) -> String;
```

## Features

### `axum-components` (default)

Enables Axum-specific components like SSE streaming support.

```toml
[dependencies]
a2a-client = { path = "../a2a-client", default-features = false }  # Minimal
a2a-client = { path = "../a2a-client" }  # With Axum components
```

## Integration with A2A Agents

This library works seamlessly with agents built using:
- `a2a-rs` - Core protocol implementation
- `a2a-agents` - Declarative agent framework
- `a2a-agent-reimbursement` - Example agent implementation

## Development

### Building

```bash
cargo build --all-features
```

### Testing

```bash
# Unit tests
cargo test

# Integration tests (requires running agent)
cargo test --features integration-tests
```

## Examples

See the `examples/` directory for complete working examples:

- `basic_client.rs` - Simple HTTP client usage (coming soon)
- `sse_streaming.rs` - SSE streaming with Axum (coming soon)
- `websocket_client.rs` - WebSocket integration (coming soon)

## Roadmap

See [TODO.md](TODO.md) for planned features and improvements.

## Contributing

This library is part of the [a2a-rs](../README.md) workspace. Contributions are welcome!

## License

MIT

## See Also

- [a2a-rs](../a2a-rs) - Core A2A protocol implementation
- [a2a-agents](../a2a-agents) - Declarative agent framework
- [A2A Protocol Specification](../spec) - Protocol documentation