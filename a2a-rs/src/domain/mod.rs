//! Domain models for the A2A protocol

pub mod core;
pub mod error;
pub mod events;
pub mod protocols;
#[cfg(test)]
mod tests;
pub mod validation;

// Re-export key types for convenience
pub use core::{
    AgentCapabilities, AgentCard, AgentProvider, AgentSkill, Artifact, AuthorizationCodeOAuthFlow,
    ClientCredentialsOAuthFlow, FileContent, ImplicitOAuthFlow, Message, MessageSendConfiguration,
    MessageSendParams, OAuthFlows, Part, PasswordOAuthFlow, PushNotificationAuthenticationInfo,
    PushNotificationConfig, Role, SecurityScheme, Task, TaskIdParams, TaskPushNotificationConfig,
    TaskQueryParams, TaskSendParams, TaskState, TaskStatus,
};
pub use error::A2AError;
pub use events::{TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use protocols::{
    JSONRPCError, JSONRPCMessage, JSONRPCNotification, JSONRPCRequest, JSONRPCResponse,
};
pub use validation::{Validate, ValidationResult};
