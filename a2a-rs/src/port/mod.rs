//! Ports (interfaces) for the A2A protocol
//!
//! Ports define the interfaces that our application needs, independent of implementation details.
//! They represent the "what" - what operations our application needs to perform.
//!
//! ## Organization
//!
//! - **Business capability ports**: Focused interfaces for specific business capabilities
//!   - `authenticator`: Authentication and authorization
//!   - `message_handler`: Message processing
//!   - `task_manager`: Task lifecycle management  
//!   - `notification_manager`: Push notifications
//!   - `streaming_handler`: Real-time updates

// Business capability ports (focused domain interfaces)
pub mod authenticator;
pub mod message_handler;
pub mod notification_manager;
pub mod streaming_handler;
pub mod task_manager;

// Re-export business capability interfaces
pub use authenticator::{
    AuthContext, AuthContextExtractor, AuthPrincipal, Authenticator, CompositeAuthenticator,
};
pub use message_handler::{AsyncMessageHandler, MessageHandler};
pub use notification_manager::{AsyncNotificationManager, NotificationManager};
pub use streaming_handler::{
    AsyncStreamingHandler, StreamingHandler, Subscriber as StreamingSubscriber, UpdateEvent,
};
pub use task_manager::{AsyncTaskManager, TaskManager};
