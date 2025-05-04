//! Server port (interface) for the A2A protocol

#[cfg(feature = "server")]
use async_trait::async_trait;

use crate::{
    application::json_rpc::{A2ARequest, JSONRPCResponse},
    domain::{
        A2AError, Message, Task, TaskPushNotificationConfig,
    },
};

/// A trait defining the methods a task handler should implement
pub trait TaskHandler {
    /// Handle a task message
    fn handle_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
    ) -> Result<Task, A2AError>;

    /// Get a task by ID
    fn get_task(&self, task_id: &str, history_length: Option<u32>) -> Result<Task, A2AError>;

    /// Cancel a task
    fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError>;

    /// Set up push notifications for a task
    fn set_push_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Get the push notification configuration for a task
    fn get_push_notification(&self, task_id: &str) -> Result<TaskPushNotificationConfig, A2AError>;
}

#[cfg(feature = "server")]
#[async_trait]
/// An async trait defining the methods an async task handler should implement
pub trait AsyncTaskHandler: Send + Sync {
    /// Handle a task message
    async fn handle_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError>;

    /// Get a task by ID
    async fn get_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError>;

    /// Cancel a task
    async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError>;

    /// Set up push notifications for a task
    async fn set_push_notification<'a>(
        &self,
        config: &'a TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Get the push notification configuration for a task
    async fn get_push_notification<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Add a status update subscriber for streaming
    async fn add_status_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<crate::domain::TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError>;

    /// Add an artifact update subscriber for streaming
    async fn add_artifact_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn Subscriber<crate::domain::TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError>;

    /// Remove subscribers for a task
    async fn remove_subscribers<'a>(&self, task_id: &'a str) -> Result<(), A2AError>;
}

/// A trait for processing A2A requests
pub trait A2ARequestProcessor {
    /// Process a raw A2A request
    fn process_raw_request(&self, request: &str) -> Result<String, A2AError>;

    /// Process a structured A2A request
    fn process_request(&self, request: &A2ARequest) -> Result<JSONRPCResponse, A2AError>;
}

#[cfg(feature = "server")]
#[async_trait]
/// An async trait for processing A2A requests
pub trait AsyncA2ARequestProcessor: Send + Sync {
    /// Process a raw A2A request
    async fn process_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError>;

    /// Process a structured A2A request
    async fn process_request<'a>(
        &self,
        request: &'a A2ARequest,
    ) -> Result<JSONRPCResponse, A2AError>;
}

#[cfg(feature = "server")]
#[async_trait]
/// A trait for getting information about an agent
pub trait AgentInfoProvider: Send + Sync {
    /// Get the agent card
    async fn get_agent_card(&self) -> Result<crate::domain::AgentCard, A2AError>;

    /// Get all skills provided by the agent
    async fn get_skills(&self) -> Result<Vec<crate::AgentSkill>, A2AError> {
        // Default implementation that gets skills from the agent card
        let card = self.get_agent_card().await?;
        Ok(card.skills)
    }

    /// Get a specific skill by ID
    async fn get_skill_by_id(&self, id: &str) -> Result<Option<crate::AgentSkill>, A2AError> {
        // Default implementation that finds the skill by ID
        let skills = self.get_skills().await?;
        Ok(skills.into_iter().find(|skill| skill.id == id))
    }

    /// Check if the agent has a specific skill
    async fn has_skill(&self, id: &str) -> Result<bool, A2AError> {
        // Default implementation that checks if the skill exists
        let skill = self.get_skill_by_id(id).await?;
        Ok(skill.is_some())
    }
}

#[cfg(feature = "server")]
#[async_trait]
/// A trait for subscribing to updates
pub trait Subscriber<T>: Send + Sync {
    /// Handle an update
    async fn on_update(&self, update: T) -> Result<(), A2AError>;
}
