//! Domain models for the A2A protocol

pub mod core;
pub mod error;
pub mod events;
pub mod generated;
#[cfg(test)]
mod tests;
pub mod validation;

// Re-export key types for convenience
pub use core::{
    AgentCapabilities, AgentCard, AgentCardBuilder, AgentCardSignature, AgentExtension,
    AgentInterface, AgentProvider, AgentSkill, Artifact, AuthorizationCodeOAuthFlow,
    ClientCredentialsOAuthFlow, DeleteTaskPushNotificationConfigParams, DeviceCodeOAuthFlow,
    FilePartBuilder, GetTaskPushNotificationConfigParams, ListTaskPushNotificationConfigsParams,
    ListTasksParams, ListTasksResult, Message, MessageSendConfiguration, MessageSendParams,
    OAuthFlows, Part, PartBuilder, PushNotificationAuthenticationInfo, Role, SecurityRequirement,
    SecurityScheme, StringList, Task, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams,
    TaskSendParams, TaskState, TaskStateExt, TaskStatus, part,
};
pub use error::A2AError;
pub use events::{TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use generated::{o_auth_flows, security_scheme};
pub use validation::{Validate, ValidationResult};
