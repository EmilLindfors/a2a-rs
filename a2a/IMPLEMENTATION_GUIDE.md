# A2A Protocol Implementation Guide

This guide provides instructions for implementing custom servers and clients using the A2A Protocol Rust library.

## Implementing a Custom Server

To implement a custom A2A server, you need to provide three core components:

1. A `TaskHandler` implementation that processes tasks
2. An `AgentInfoProvider` implementation that provides agent metadata
3. A `RequestProcessor` that routes JSON-RPC requests to your handler

### Step 1: Implement a Task Handler

The `TaskHandler` (or `AsyncTaskHandler`) is responsible for processing messages and managing tasks:

```rust
use a2a_protocol::{
    domain::{A2AError, Message, Task, TaskPushNotificationConfig, TaskState},
    port::server::{AsyncTaskHandler, Subscriber},
};
use async_trait::async_trait;

struct MyTaskHandler {
    // Your data structures here
}

#[async_trait]
impl AsyncTaskHandler for MyTaskHandler {
    async fn handle_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Process the message and return a task
        // This is where your agent's core logic goes
        let mut task = Task::new(task_id.to_string());
        
        // Process the message...
        
        // Update the task status with a response
        let response = Message::agent_text("I received your message!".to_string());
        task.update_status(TaskState::Completed, Some(response));
        
        Ok(task)
    }
    
    // Implement other required methods...
}
```

### Step 2: Implement an Agent Info Provider

The `AgentInfoProvider` returns metadata about your agent:

```rust
use a2a_protocol::{
    domain::{A2AError, AgentCard, AgentCapabilities},
    port::server::AgentInfoProvider,
};
use async_trait::async_trait;

struct MyAgentInfo {
    // Your agent info here
}

#[async_trait]
impl AgentInfoProvider for MyAgentInfo {
    async fn get_agent_card(&self) -> Result<AgentCard, A2AError> {
        Ok(AgentCard {
            name: "My Custom Agent".to_string(),
            description: Some("A custom A2A agent".to_string()),
            url: "https://example.com/agent".to_string(),
            provider: None,
            version: "1.0.0".to_string(),
            documentation_url: None,
            capabilities: AgentCapabilities {
                streaming: true,
                push_notifications: false,
                state_transition_history: false,
            },
            authentication: None,
            default_input_modes: vec!["text".to_string()],
            default_output_modes: vec!["text".to_string()],
            skills: vec![],
        })
    }
}
```

### Step 3: Use the Default Request Processor or Create Your Own

You can use the provided `DefaultRequestProcessor` or implement a custom one:

```rust
use a2a_protocol::adapter::server::DefaultRequestProcessor;

// Using the default processor
let processor = DefaultRequestProcessor::new(my_task_handler);

// Or implement your own by implementing the AsyncA2ARequestProcessor trait
```

### Step 4: Create and Start Your Server

Choose an HTTP or WebSocket server implementation:

```rust
use a2a_protocol::adapter::server::{HttpServer, WebSocketServer};

// For HTTP
let http_server = HttpServer::new(processor, agent_info, "127.0.0.1:8080".to_string());
http_server.start().await?;

// For WebSocket with streaming support
let ws_server = WebSocketServer::new(
    processor,
    agent_info,
    task_handler,
    "127.0.0.1:8081".to_string(),
);
ws_server.start().await?;
```

## Implementing a Custom Client

To implement a custom A2A client, you can either use the provided implementations or create your own by implementing the `AsyncA2AClient` trait.

### Option 1: Use Provided Clients

```rust
use a2a_protocol::{
    adapter::client::{HttpClient, WebSocketClient},
    port::client::AsyncA2AClient,
    domain::Message,
};

// For HTTP
let http_client = HttpClient::new("http://localhost:8080".to_string());
let task = http_client.send_task_message("task-123", &message, None, None).await?;

// For WebSocket with streaming
let ws_client = WebSocketClient::new("ws://localhost:8081".to_string());
let mut stream = ws_client.subscribe_to_task("task-123", &message, None, None).await?;
```

### Option 2: Custom Client Implementation

Implement the `AsyncA2AClient` trait for your custom client:

```rust
use a2a_protocol::{
    application::json_rpc::{A2ARequest, JSONRPCResponse},
    domain::{A2AError, Message, Task},
    port::client::{AsyncA2AClient, StreamItem},
};
use async_trait::async_trait;
use futures::stream::Stream;

struct MyCustomClient {
    // Your client implementation details
}

#[async_trait]
impl AsyncA2AClient for MyCustomClient {
    async fn send_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError> {
        // Implement raw request sending
    }
    
    async fn send_request<'a>(&self, request: &'a A2ARequest) -> Result<JSONRPCResponse, A2AError> {
        // Implement structured request sending
    }
    
    async fn send_task_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        // Implement task message sending
    }
    
    // Implement other required methods...
}
```

## Handling Streaming

### Server-Side Streaming

For server-side streaming, you need to implement the `Subscriber` trait and add subscribers to tasks:

```rust
#[async_trait]
impl AsyncTaskHandler for MyTaskHandler {
    // ...
    
    async fn add_status_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        // Store the subscriber and notify it of updates
    }
    
    async fn add_artifact_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        // Store the subscriber and notify it of artifact updates
    }
}
```

### Client-Side Streaming

For client-side streaming, use the `subscribe_to_task` method:

```rust
let mut stream = client.subscribe_to_task("task-123", &message, None, None).await?;

while let Some(result) = stream.next().await {
    match result {
        Ok(StreamItem::StatusUpdate(update)) => {
            // Handle status update
        }
        Ok(StreamItem::ArtifactUpdate(update)) => {
            // Handle artifact update
        }
        Err(e) => {
            // Handle error
        }
    }
}
```

## Best Practices

1. **Error Handling**: Properly propagate errors and handle edge cases
2. **Task Storage**: Use a reliable storage backend for production servers
3. **Authentication**: Implement proper authentication for production systems
4. **Streaming**: For long-running tasks, use WebSocket streaming
5. **Cancellation**: Always handle task cancellation properly
6. **Timeouts**: Implement appropriate timeouts for all operations
7. **Logging**: Add comprehensive logging for easier debugging

## Advanced Topics

### Custom Task Storage

The library provides an `InMemoryTaskStorage` implementation for testing, but for production use, you should implement a custom storage adapter.

### Custom Transport

The library supports HTTP and WebSocket transport, but you can implement custom transport by creating your own client and server adapters.

### Authentication

For production use, implement proper authentication by extending the provided adapters with authentication middleware.