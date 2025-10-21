//! Core domain types for the A2A protocol

pub mod agent;
pub mod message;
pub mod task;

pub use agent::{
    AgentCapabilities, AgentCard, AgentCardSignature, AgentProvider, AgentSkill,
    AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow, ImplicitOAuthFlow, OAuthFlows,
    PasswordOAuthFlow, PushNotificationAuthenticationInfo, PushNotificationConfig, SecurityScheme,
};
pub use message::{Artifact, FileContent, Message, Part, Role};
pub use task::{
    MessageSendConfiguration, MessageSendParams, Task, TaskIdParams, TaskPushNotificationConfig,
    TaskQueryParams, TaskSendParams, TaskState, TaskStatus,
};
