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
    AgentCapabilities, AgentCard, AgentCardBuilder, AgentCardSignature, AgentExtension, AgentInterface,
    AgentProvider, AgentSkill, Artifact, AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow,
    DeviceCodeOAuthFlow, ListTasksParams, ListTasksResult, GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigsParams, DeleteTaskPushNotificationConfigParams,
    TaskIdParams, TaskQueryParams, TaskSendParams,
    Message, MessageSendConfiguration, MessageSendParams, OAuthFlows, Part, PartBuilder, FilePartBuilder, part,
    PushNotificationAuthenticationInfo, Role, SecurityScheme, SecurityRequirement,
    StringList, Task, TaskPushNotificationConfig, TaskState, TaskStatus, TaskStateExt,
};
pub use generated::{security_scheme, o_auth_flows};
pub use error::A2AError;
pub use events::{TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use validation::{Validate, ValidationResult};
