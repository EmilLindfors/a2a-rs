//! Domain models for the A2A protocol

pub mod agent;
pub mod error;
pub mod message;
pub mod task;

// Re-export key types for convenience
pub use agent::{
    AgentAuthentication, AgentCapabilities, AgentCard, AgentCardSignature, AgentProvider,
    AgentSkill, AuthenticationInfo, OAuthFlowAuthorizationCode, OAuthFlowClientCredentials,
    OAuthFlowImplicit, OAuthFlowPassword, OAuthFlows, PushNotificationConfig, SecurityRequirement,
    SecurityScheme, SecuritySchemes,
};
pub use error::A2AError;
pub use message::{Artifact, FileContent, Message, Part, Role};
pub use task::{
    Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams,
    TaskSendParams, TaskState, TaskStatus, TaskStatusUpdateEvent,
};