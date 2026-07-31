//! Core domain types for the A2A protocol

pub mod agent;
pub mod message;
pub mod task;

pub use agent::{
    AgentCapabilities, AgentCard, AgentCardBuilder, AgentCardSignature, AgentExtension,
    AgentInterface, AgentProvider, AgentSkill, AuthorizationCodeOAuthFlow,
    ClientCredentialsOAuthFlow, DeviceCodeOAuthFlow, OAuthFlows, PROTOCOL_BINDING_CONNECTRPC,
    PROTOCOL_BINDING_HTTP_JSON, PROTOCOL_BINDING_JSONRPC, PushNotificationAuthenticationInfo,
    SecurityRequirement, SecurityScheme, StringList,
};
pub use message::{Artifact, FilePartBuilder, Message, Part, PartBuilder, Role, part};
pub use task::{
    DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigsParams, ListTasksParams, ListTasksResult,
    MessageSendConfiguration, MessageSendParams, SendCompletion, Task, TaskIdParams,
    TaskPushNotificationConfig, TaskQueryParams, TaskSendParams, TaskState, TaskStateExt,
    TaskStatus, VersionedTask,
};
