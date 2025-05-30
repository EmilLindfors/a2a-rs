//! Ports (interfaces) for the A2A protocol
//! 
//! Ports define the interfaces that our application needs, independent of implementation details.
//! They represent the "what" - what operations our application needs to perform.
//!
//! ## Organization
//! 
//! - **Business capability ports**: Focused interfaces for specific business capabilities
//!   - `message_handler`: Message processing
//!   - `task_manager`: Task lifecycle management  
//!   - `notification_manager`: Push notifications
//!   - `streaming_handler`: Real-time updates

// Business capability ports (focused domain interfaces)  
pub mod message_handler;
pub mod notification_manager;
pub mod streaming_handler;
pub mod task_manager;

// Re-export business capability interfaces
pub use message_handler::{MessageHandler, AsyncMessageHandler};
pub use notification_manager::{NotificationManager, AsyncNotificationManager};
pub use streaming_handler::{StreamingHandler, AsyncStreamingHandler, Subscriber as StreamingSubscriber, UpdateEvent};
pub use task_manager::{TaskManager, AsyncTaskManager};
