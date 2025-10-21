//! Core domain types for the A2A protocol

pub mod agent;
pub mod message;
pub mod task;

pub use agent::{
    AgentCapabilities, AgentCard, AgentCardSignature, AgentExtension, AgentInterface,
    AgentProvider, AgentSkill, AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow,
    ImplicitOAuthFlow, OAuthFlows, PasswordOAuthFlow, PushNotificationAuthenticationInfo,
    PushNotificationConfig, SecurityScheme, TransportProtocol,
};
pub use message::{Artifact, FileContent, Message, Part, Role};
pub use task::{
    DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigParams, ListTasksParams, ListTasksResult,
    MessageSendConfiguration, MessageSendParams, Task, TaskIdParams, TaskPushNotificationConfig,
    TaskQueryParams, TaskSendParams, TaskState, TaskStatus,
};
