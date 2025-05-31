//! Business logic adapter implementations

#[cfg(feature = "server")]
pub mod agent_info;
#[cfg(feature = "server")]
pub mod message_handler;
#[cfg(feature = "server")]
pub mod push_notification;
#[cfg(feature = "server")]
pub mod request_processor;

// Re-export business implementations
#[cfg(feature = "server")]
pub use agent_info::SimpleAgentInfo;
#[cfg(feature = "server")]
pub use message_handler::DefaultMessageHandler;
#[cfg(all(feature = "server", feature = "http-client"))]
pub use push_notification::HttpPushNotificationSender;
#[cfg(feature = "server")]
pub use push_notification::{
    NoopPushNotificationSender, PushNotificationRegistry, PushNotificationSender,
};
#[cfg(feature = "server")]
pub use request_processor::DefaultRequestProcessor;
