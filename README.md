# A2A Protocol for Rust

A Rust implementation of the Agent-to-Agent (A2A) Protocol that follows idiomatic Rust practices and hexagonal architecture principles.

## Features

- Complete implementation of the A2A protocol as specified in the specification
- Support for both client and server roles
- Multiple transport options:
  - HTTP client and server
  - WebSocket client and server with streaming support
- Comprehensive Agent Skills management
- Task history tracking and management
- Push notifications for task updates
- Async interfaces with Tokio
- Feature flags for optional dependencies
- Built-in test suite and examples
- Pure Rust TLS implementation with rustls

## Architecture

The project follows a hexagonal architecture with:

- **Domain**: Core business logic and models
- **Ports**: Interfaces for the outside world to interact with the domain
- **Adapters**: Implementations of the ports for specific technologies
- **Application**: Services that coordinate the domain logic

## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
a2a-protocol = { version = "0.1.0", features = ["http-client"] }
```

## Feature Flags

- `client`: Base client functionality
- `http-client`: HTTP client implementation
- `ws-client`: WebSocket client implementation with streaming support
- `server`: Base server functionality
- `http-server`: HTTP server implementation
- `ws-server`: WebSocket server implementation with streaming support
- `full`: All available features

## TLS Configuration

This library uses `rustls` as its TLS backend rather than the more common `native-tls` or `openssl` crates. This eliminates the dependency on OpenSSL development libraries and simplifies cross-platform builds.

### Benefits of rustls:

1. **Pure Rust Implementation**: No need for system OpenSSL libraries
2. **Security Focus**: Modern TLS implementation with security as a primary goal
3. **Performance**: Often faster than OpenSSL-based solutions
4. **Cross-Platform**: Easier to build on different platforms without system dependencies

### Usage Notes:

- All HTTP and WebSocket clients use `rustls` by default
- No additional configuration is needed to use TLS connections
- Standard certificate verification is automatically handled

For custom certificate roots or client certificates, you can configure the reqwest or tungstenite clients directly and pass them to the library.

## Examples and Testing

The project includes several examples to help you get started:

- `http_server.rs` - A simple HTTP server implementation
- `http_client.rs` - A client that connects to the HTTP server
- `websocket_server.rs` - A WebSocket server with streaming support
- `websocket_client.rs` - A client that connects to the WebSocket server for streaming updates

To run the examples:

```bash
# Start the HTTP server
cargo run --example http_server --features http-server

# In a different terminal, run the client
cargo run --example http_client --features http-client

# For WebSocket examples
cargo run --example websocket_server --features ws-server
cargo run --example websocket_client --features ws-client
```

The project also includes integration tests that verify compliance with the A2A specification:

```bash
# Run all tests
cargo test --all-features

# Run specific tests
cargo test --test integration_test
cargo test --test websocket_test
cargo test --test push_notification_test
```

## Usage Examples

### Client Example

```rust
use a2a_protocol::{
    adapter::client::HttpClient,
    domain::{Message, Part},
    port::client::AsyncA2AClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client
    let client = HttpClient::new("https://example.com/api".to_string());

    // Create a message
    let message = Message::user_text("Hello, world!".to_string());

    // Send a task message
    let task = client.send_task_message("task-123", &message, None, None).await?;

    println!("Task: {:?}", task);
    Ok(())
}
```

### Streaming Client Example

```rust
use a2a_protocol::{
    adapter::client::WebSocketClient,
    domain::{Message, Part},
    port::client::{AsyncA2AClient, StreamItem},
};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a WebSocket client
    let client = WebSocketClient::new("wss://example.com/ws".to_string());

    // Create a message
    let message = Message::user_text("Hello, world!".to_string());

    // Subscribe to task updates
    let mut stream = client.subscribe_to_task("task-123", &message, None, None).await?;

    // Process streaming updates
    while let Some(result) = stream.next().await {
        match result {
            Ok(StreamItem::StatusUpdate(update)) => {
                println!("Status update: {:?}", update);
                if update.final_ {
                    break;
                }
            }
            Ok(StreamItem::ArtifactUpdate(update)) => {
                println!("Artifact update: {:?}", update);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
```

### Server Example

```rust
use a2a_protocol::{
    adapter::server::HttpServer,
    application::json_rpc::{A2ARequest, JSONRPCResponse},
    domain::{A2AError, AgentCard, AgentCapabilities, Message, Task, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams},
    port::server::{AgentInfoProvider, AsyncA2ARequestProcessor, AsyncTaskHandler},
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

// In-memory task storage
struct TaskStorage {
    tasks: Mutex<HashMap<String, Task>>,
}

impl TaskStorage {
    fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
        }
    }
}

// Task handler implementation
struct MyTaskHandler {
    storage: Arc<TaskStorage>,
}

#[async_trait]
impl AsyncTaskHandler for MyTaskHandler {
    async fn handle_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        let mut task = Task::new(task_id.to_string());
        
        // Add the message to the task and update status
        task.update_status(TaskState::Working, Some(message.clone()));
        
        // Store the task
        let mut tasks = self.storage.tasks.lock().unwrap();
        tasks.insert(task_id.to_string(), task.clone());
        
        Ok(task)
    }
    
    // Implement other required methods
    // ...
}

// Agent info provider implementation
struct MyAgentInfo;

#[async_trait]
impl AgentInfoProvider for MyAgentInfo {
    async fn get_agent_card(&self) -> Result<AgentCard, A2AError> {
        Ok(AgentCard {
            name: "My Agent".to_string(),
            description: Some("A sample A2A agent".to_string()),
            url: "https://example.com/agent".to_string(),
            provider: None,
            version: "1.0.0".to_string(),
            documentation_url: Some("https://example.com/docs".to_string()),
            capabilities: AgentCapabilities {
                streaming: true,
                push_notifications: false,
                state_transition_history: true,
            },
            authentication: None,
            default_input_modes: vec!["text".to_string()],
            default_output_modes: vec!["text".to_string()],
            skills: vec![],
        })
    }
}

// Request processor implementation
struct MyRequestProcessor {
    task_handler: Arc<MyTaskHandler>,
}

#[async_trait]
impl AsyncA2ARequestProcessor for MyRequestProcessor {
    async fn process_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError> {
        // Parse the request
        let request: A2ARequest = serde_json::from_str(request)?;
        
        // Process the request
        let response = self.process_request(&request).await?;
        
        // Serialize the response
        let json = serde_json::to_string(&response)?;
        Ok(json)
    }
    
    async fn process_request<'a>(&self, request: &'a A2ARequest) -> Result<JSONRPCResponse, A2AError> {
        // Process the request based on its type
        // ...
        
        // Return a response
        Ok(JSONRPCResponse::success(None, serde_json::json!({})))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create task storage
    let storage = Arc::new(TaskStorage::new());
    
    // Create task handler
    let task_handler = Arc::new(MyTaskHandler { storage: storage.clone() });
    
    // Create request processor
    let processor = MyRequestProcessor { task_handler: task_handler.clone() };
    
    // Create agent info provider
    let agent_info = MyAgentInfo;
    
    // Create HTTP server
    let server = HttpServer::new(processor, agent_info, "127.0.0.1:8080".to_string());
    
    // Start the server
    println!("Starting server on 127.0.0.1:8080");
    server.start().await?;
    
    Ok(())
}
```

## Project Status

This project is fully compliant with the A2A specification and includes:

- ✅ Functional HTTP and WebSocket transports
- ✅ Full task history tracking
- ✅ Comprehensive skills management
- ✅ Push notification support
- ✅ File content handling and validation
- ✅ Integration tests covering core functionality
- ✅ Example implementations for both client and server

### Implementation Note

The codebase has been thoroughly refactored to improve compliance with the A2A specification. Recent improvements include:

1. Migrated from OpenSSL to rustls for better cross-platform support
2. Enhanced task history functionality to properly track state transitions
3. Improved skills handling with comprehensive builder patterns
4. Added robust push notification support with retry mechanisms
5. Expanded test coverage including integration tests

Check the `A2A_COMPLIANCE_ASSESSMENT.md` file for a detailed assessment of the implementation against the specification.

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

For more detailed implementation information, see the `IMPLEMENTATION_GUIDE.md` file in the a2a-rs directory.