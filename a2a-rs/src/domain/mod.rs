//! Domain models for the A2A protocol

pub mod agent;
pub mod error;
pub mod message;
pub mod task;
#[cfg(test)]
mod tests;

// Re-export key types for convenience
pub use agent::{
    AgentAuthentication, AgentCapabilities, AgentCard, AgentProvider, AgentSkill,
    AuthenticationInfo, PushNotificationConfig,
};
pub use error::A2AError;
pub use message::{Artifact, FileContent, Message, Part, Role};
pub use task::{
    Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams,
    TaskSendParams, TaskState, TaskStatus, TaskStatusUpdateEvent,
};
