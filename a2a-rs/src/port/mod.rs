//! Ports (interfaces) for the A2A protocol
//! 
//! Ports define the interfaces that our application needs, independent of implementation details.
//! They represent the "what" - what operations our application needs to perform.
//!
//! ## Organization
//! 
//! - **Protocol-level ports** (`client`, `server`): Core A2A protocol communication interfaces
//! - **Business capability ports**: Focused interfaces for specific business capabilities
//!   - `message_handler`: Message processing
//!   - `task_manager`: Task lifecycle management  
//!   - `notification_manager`: Push notifications
//!   - `streaming_handler`: Real-time updates

// Protocol-level ports (core A2A communication)
pub mod client;
pub mod server;

// Business capability ports (focused domain interfaces)  
pub mod message_handler;
pub mod notification_manager;
pub mod streaming_handler;
pub mod task_manager;

// Re-export protocol-level interfaces
pub use client::A2AClient;

#[cfg(feature = "client")]
pub use client::{AsyncA2AClient, StreamItem};

pub use server::{A2ARequestProcessor, TaskHandler};

#[cfg(feature = "server")]
pub use server::{AgentInfoProvider, AsyncA2ARequestProcessor, AsyncTaskHandler, Subscriber};

// Re-export business capability interfaces (recommended for new code)
pub use message_handler::{MessageHandler, AsyncMessageHandler};
pub use notification_manager::{NotificationManager, AsyncNotificationManager};
pub use streaming_handler::{StreamingHandler, AsyncStreamingHandler, Subscriber as StreamingSubscriber, UpdateEvent};
pub use task_manager::{TaskManager, AsyncTaskManager};
