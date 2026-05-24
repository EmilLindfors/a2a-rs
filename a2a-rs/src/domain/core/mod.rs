//! Core domain types for the A2A protocol

pub mod agent;
pub mod message;
pub mod task;

pub use agent::{
    AgentCapabilities, AgentCard, AgentCardBuilder, AgentCardSignature, AgentExtension, AgentInterface,
    AgentProvider, AgentSkill, AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow,
    DeviceCodeOAuthFlow, OAuthFlows, PushNotificationAuthenticationInfo,
    SecurityScheme, SecurityRequirement, StringList,
};
pub use message::{Artifact, Message, Part, PartBuilder, FilePartBuilder, Role, part};
pub use task::{
    DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigsParams, ListTasksParams, ListTasksResult,
    MessageSendConfiguration, MessageSendParams, Task, TaskIdParams, TaskPushNotificationConfig,
    TaskQueryParams, TaskSendParams, TaskState, TaskStatus, TaskStateExt,
};
