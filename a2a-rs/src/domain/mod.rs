//! Domain models for the A2A protocol

pub mod core;
pub mod error;
pub mod error_details;
pub mod events;
pub mod generated;
pub mod ids;
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
    TaskSendParams, TaskState, TaskStateExt, TaskStatus, VersionedTask, part,
};
pub use error::{A2AError, Result};
pub use error_details::{ErrorDetail, ErrorInfo, FieldViolation};
pub use events::{TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use ids::{ContextId, PushConfigId, TaskId};
pub use generated::{o_auth_flows, security_scheme};
pub use validation::{Validate, ValidationResult};
