//! A Rust implementation of the Agent-to-Agent (A2A) Protocol
//!
//! This library provides a type-safe, idiomatic Rust implementation of the A2A protocol,
//! with support for both client and server roles. The implementation follows a hexagonal
//! architecture with clear separation between domains, ports, and adapters.
//!
//! # Features
//!
//! - Complete implementation of the A2A protocol
//! - Support for HTTP and WebSocket transport
//! - Support for streaming updates
//! - Async and sync interfaces
//! - Feature flags for optional dependencies
//!
//! # Examples
//!
//! ## Creating a client
//!
//! ```rust,no_run
//! #[cfg(feature = "http-client")]
//! use a2a_protocol::{
//!     adapter::client::HttpClient,
//!     domain::{Message, Part},
//!     port::client::AsyncA2AClient,
//! };
//!
//! #[cfg(feature = "http-client")]
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a client
//!     let client = HttpClient::new("https://example.com/api".to_string());
//!
//!     // Send a task message
//!     let message = Message::user_text("Hello, world!".to_string());
//!     let task = client.send_task_message("task-123", &message, None, None).await?;
//!
//!     println!("Task: {:?}", task);
//!     Ok(())
//! }
//! ```
//!
//! ## Creating a server
//!
//! ```rust,no_run
//! #[cfg(feature = "http-server")]
//! use a2a_protocol::{
//!     adapter::server::HttpServer,
//!     domain::{A2AError, AgentCard, Message, Task, TaskIdParams, TaskPushNotificationConfig},
//!     port::server::{AgentInfoProvider, AsyncA2ARequestProcessor, AsyncTaskHandler},
//! };
//!
//! #[cfg(feature = "http-server")]
//! struct MyTaskHandler;
//!
//! #[cfg(feature = "http-server")]
//! #[async_trait::async_trait]
//! impl AsyncTaskHandler for MyTaskHandler {
//!     async fn handle_message<'a>(
//!         &self,
//!         task_id: &'a str,
//!         message: &'a Message,
//!         session_id: Option<&'a str>,
//!     ) -> Result<Task, A2AError> {
//!         // Implement message handling
//!         Ok(Task::new(task_id.to_string()))
//!     }
//!
//!     // Implement other required methods
//!     // ...
//! }
//!
//! #[cfg(feature = "http-server")]
//! struct MyAgentInfo;
//!
//! #[cfg(feature = "http-server")]
//! #[async_trait::async_trait]
//! impl AgentInfoProvider for MyAgentInfo {
//!     async fn get_agent_card(&self) -> Result<AgentCard, A2AError> {
//!         // Return agent card
//!         // ...
//!     }
//! }
//!
//! #[cfg(feature = "http-server")]
//! struct MyRequestProcessor;
//!
//! #[cfg(feature = "http-server")]
//! #[async_trait::async_trait]
//! impl AsyncA2ARequestProcessor for MyRequestProcessor {
//!     // Implement request processing
//!     // ...
//! }
//!
//! #[cfg(feature = "http-server")]
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a server
//!     let server = HttpServer::new(
//!         MyRequestProcessor,
//!         MyAgentInfo,
//!         "127.0.0.1:8080".to_string(),
//!     );
//!
//!     // Start the server
//!     server.start().await?;
//!     Ok(())
//! }
//! ```

// Re-export key modules and types
pub mod adapter;
pub mod application;
pub mod domain;
pub mod port;

// Public API exports
pub use domain::{
    agent::{
        AgentAuthentication, AgentCapabilities, AgentCard, AgentProvider, AgentSkill,
        AuthenticationInfo, PushNotificationConfig,
    },
    error::A2AError,
    message::{Artifact, FileContent, Message, Part, Role},
    task::{
        Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams,
        TaskSendParams, TaskState, TaskStatus, TaskStatusUpdateEvent,
    },
};

#[cfg(feature = "client")]
pub use port::client::{A2AClient, AsyncA2AClient, StreamItem};

#[cfg(feature = "server")]
pub use port::server::{
    A2ARequestProcessor, AgentInfoProvider, AsyncA2ARequestProcessor, AsyncTaskHandler, Subscriber,
    TaskHandler,
};

#[cfg(feature = "http-client")]
pub use adapter::client::http::HttpClient;

#[cfg(feature = "ws-client")]
pub use adapter::client::ws::WebSocketClient;

#[cfg(feature = "http-server")]
pub use adapter::server::http::HttpServer;

#[cfg(feature = "ws-server")]
pub use adapter::server::ws::WebSocketServer;