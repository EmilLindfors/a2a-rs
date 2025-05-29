//! Domain models for the A2A protocol

pub mod core;
pub mod error;
pub mod events;
pub mod protocols;
pub mod validation;
#[cfg(test)]
mod tests;

// Re-export key types for convenience
pub use core::{
    AgentCapabilities, AgentCard, AgentProvider, AgentSkill,
    PushNotificationAuthenticationInfo, PushNotificationConfig, SecurityScheme,
    OAuthFlows, AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow,
    ImplicitOAuthFlow, PasswordOAuthFlow,
    Artifact, FileContent, Message, Part, Role,
    MessageSendConfiguration, MessageSendParams, Task, TaskIdParams,
    TaskPushNotificationConfig, TaskQueryParams, TaskSendParams, TaskState, TaskStatus,
};
pub use error::A2AError;
pub use events::{TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use protocols::{JSONRPCError, JSONRPCMessage, JSONRPCNotification, JSONRPCRequest, JSONRPCResponse};
pub use validation::{Validate, ValidationResult};
