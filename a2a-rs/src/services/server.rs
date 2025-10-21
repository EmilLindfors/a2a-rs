//! Server service traits

use async_trait::async_trait;

use crate::{
    application::{json_rpc::A2ARequest, JSONRPCResponse},
    domain::{A2AError, AgentCard, AgentSkill},
};

/// A trait for providing agent information
#[async_trait]
pub trait AgentInfoProvider: Send + Sync {
    /// Get the agent card
    async fn get_agent_card(&self) -> Result<AgentCard, A2AError>;

    /// Get the list of skills
    async fn get_skills(&self) -> Result<Vec<AgentSkill>, A2AError> {
        let card = self.get_agent_card().await?;
        Ok(card.skills)
    }

    /// Get a specific skill by ID
    async fn get_skill_by_id(&self, id: &str) -> Result<Option<AgentSkill>, A2AError> {
        let skills = self.get_skills().await?;
        Ok(skills.into_iter().find(|s| s.id == id))
    }

    /// Check if a skill exists
    async fn has_skill(&self, id: &str) -> Result<bool, A2AError> {
        let skill = self.get_skill_by_id(id).await?;
        Ok(skill.is_some())
    }

    /// Get the authenticated extended agent card (v0.3.0)
    ///
    /// Returns an extended version of the agent card with authenticated-only information.
    /// By default, returns AuthenticatedExtendedCardNotConfigured error.
    /// Override this method to provide authenticated extended card support.
    async fn get_authenticated_extended_card(&self) -> Result<AgentCard, A2AError> {
        Err(A2AError::AuthenticatedExtendedCardNotConfigured)
    }
}

/// An async trait for processing A2A protocol requests
#[async_trait]
pub trait AsyncA2ARequestProcessor: Send + Sync {
    /// Process a raw JSON-RPC request string
    async fn process_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError>;

    /// Process a parsed A2A request
    async fn process_request<'a>(
        &self,
        request: &'a A2ARequest,
    ) -> Result<JSONRPCResponse, A2AError>;
}
