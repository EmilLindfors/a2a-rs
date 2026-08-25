//! Domain models for the A2A protocol

pub mod conversation;
pub mod core;
pub mod error;
pub mod error_details;
pub mod events;
pub mod generated;
pub mod ids;
pub mod retention;
pub mod retry;
pub mod state;
#[cfg(test)]
mod tests;
pub mod validation;

// Re-export key types for convenience
pub use conversation::{Conversation, Digest, Seq, SequencedMessage};
pub use core::{
    AgentCapabilities, AgentCard, AgentCardBuilder, AgentCardSignature, AgentExtension,
    AgentInterface, AgentProvider, AgentSkill, Artifact, AuthorizationCodeOAuthFlow,
    ClientCredentialsOAuthFlow, DeleteTaskPushNotificationConfigParams, DeviceCodeOAuthFlow,
    FilePartBuilder, GetTaskPushNotificationConfigParams, ListTaskPushNotificationConfigsParams,
    ListTasksParams, ListTasksResult, Message, OAuthFlows, PROTOCOL_BINDING_CONNECTRPC,
    PROTOCOL_BINDING_HTTP_JSON, PROTOCOL_BINDING_JSONRPC, Part, PartBuilder,
    PushNotificationAuthenticationInfo, Role, SecurityRequirement, SecurityScheme, SendCompletion,
    StringList, Task, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams, TaskState,
    TaskStateExt, TaskStatus, VersionedTask, part,
};
pub use error::{A2AError, Result};
pub use error_details::{ErrorDetail, ErrorInfo, FieldViolation};
pub use events::{TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use generated::{o_auth_flows, security_scheme};
pub use ids::{ContextId, PushConfigId, TaskId};
pub use retention::{ReadRefresh, RetentionPolicy, Swept};
pub use retry::RetryPolicy;
pub use state::{ContextState, Remembered, StateKey, StateKeyError, StateScope};
pub use validation::{Validate, ValidationResult};
