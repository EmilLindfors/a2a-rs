# A2A Protocol for Rust

A Rust implementation of the Agent-to-Agent (A2A) Protocol that follows idiomatic Rust practices and hexagonal architecture principles.

## Protocol Compliance

**✅ A2A Protocol v0.3.0 Compliant**

This implementation fully supports the A2A Protocol v0.3.0 specification, including all new security features:

- ✅ **Enhanced Security Schemes**: Support for API Key, HTTP Bearer, OAuth2, OpenID Connect, and Mutual TLS (mTLS)
- ✅ **Agent Card Signatures**: RFC 7515 (JSON Web Signature) support for card integrity verification
- ✅ **Per-Skill Security**: Skills can specify their own authentication requirements
- ✅ **OAuth2 Metadata Discovery**: RFC 8414 support for OAuth2 metadata URLs
- ✅ **Extended Card Support**: Authenticated clients can request extended agent information
- ✅ **Well-Known URIs**: RFC 8615 compliant discovery endpoint (`/.well-known/agent-card.json`)
- ✅ **Backward Compatible**: Full compatibility with v0.2.x clients and servers

See [examples/v03_security_example.rs](a2a/examples/v03_security_example.rs) for a complete demonstration of v0.3.0 security features.

## Features

- Complete implementation of the A2A protocol v0.2.x and v0.3.0
- Support for both client and server roles
- Multiple transport options:
  - HTTP client and server
  - WebSocket client and server with streaming support
- Async and sync interfaces
- Feature flags for optional dependencies
- Comprehensive security scheme support
- Agent card digital signatures

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

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.