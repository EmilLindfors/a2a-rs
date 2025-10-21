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
//! # #[cfg(feature = "http-client")]
//! # {
//! use a2a_rs::{HttpClient, Message};
//! use a2a_rs::services::AsyncA2AClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a client
//!     let client = HttpClient::new("https://example.com/api".to_string());
//!
//!     // Send a task message
//!     let message = Message::user_text("Hello, world!".to_string(), "msg-123".to_string());
//!     let task = client.send_task_message("task-123", &message, None, None).await?;
//!
//!     println!("Task: {:?}", task);
//!     Ok(())
//! }
//! # }
//! ```
//!
//! ## Creating a server
//!
//! ```rust,ignore
//! use a2a_rs::{HttpServer, SimpleAgentInfo, DefaultRequestProcessor};
//! use my_app::{MyMessageHandler, MyTaskManager, MyNotificationManager};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create custom handlers that implement the required traits
//!     let message_handler = MyMessageHandler::new();
//!     let task_manager = MyTaskManager::new();
//!     let notification_manager = MyNotificationManager::new();
//!     let agent_info = SimpleAgentInfo::new("my-agent".to_string(), "https://api.example.com".to_string());
//!
//!     // Create a request processor with your handlers
//!     let processor = DefaultRequestProcessor::new(
//!         message_handler,
//!         task_manager,
//!         notification_manager,
//!         agent_info.clone(),
//!     );
//!
//!     // Create and start the server
//!     let server = HttpServer::new(
//!         processor,
//!         agent_info,
//!         "127.0.0.1:8080".to_string(),
//!     );
//!     server.start().await?;
//!     Ok(())
//! }
//! ```

// Re-export key modules and types
pub mod adapter;
pub mod application;
pub mod domain;
pub mod port;
pub mod services;

#[cfg(feature = "tracing")]
pub mod observability;

// Public API exports
pub use domain::{
    A2AError, AgentCapabilities, AgentCard, AgentCardSignature, AgentExtension, AgentInterface,
    AgentProvider, AgentSkill, Artifact, AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow,
    DeleteTaskPushNotificationConfigParams, FileContent, GetTaskPushNotificationConfigParams,
    ImplicitOAuthFlow, ListTaskPushNotificationConfigParams, ListTasksParams, ListTasksResult,
    Message, MessageSendConfiguration, MessageSendParams, OAuthFlows, Part, PasswordOAuthFlow,
    PushNotificationAuthenticationInfo, PushNotificationConfig, Role, SecurityScheme, Task,
    TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams,
    TaskSendParams, TaskState, TaskStatus, TaskStatusUpdateEvent, TransportProtocol,
};

// Port traits for better separation of concerns
pub use port::{
    AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskManager,
    MessageHandler, NotificationManager, StreamingHandler, StreamingSubscriber, TaskManager,
    UpdateEvent,
};

#[cfg(feature = "http-client")]
pub use adapter::HttpClient;

#[cfg(feature = "ws-client")]
pub use adapter::WebSocketClient;

#[cfg(feature = "http-server")]
pub use adapter::HttpServer;

#[cfg(feature = "ws-server")]
pub use adapter::WebSocketServer;

#[cfg(feature = "server")]
pub use adapter::{
    DefaultRequestProcessor, InMemoryTaskStorage, NoopPushNotificationSender,
    PushNotificationRegistry, PushNotificationSender, SimpleAgentInfo,
};

#[cfg(all(feature = "server", feature = "http-client"))]
pub use adapter::HttpPushNotificationSender;

#[cfg(any(feature = "http-server", feature = "ws-server"))]
pub use adapter::{ApiKeyAuthenticator, BearerTokenAuthenticator, NoopAuthenticator};
#[cfg(feature = "auth")]
pub use adapter::{JwtAuthenticator, OAuth2Authenticator, OpenIdConnectAuthenticator};
#[cfg(any(feature = "http-server", feature = "ws-server"))]
pub use port::Authenticator;
