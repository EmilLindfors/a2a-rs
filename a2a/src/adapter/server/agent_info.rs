//! A simple agent info provider implementation

#![cfg(feature = "server")]

use async_trait::async_trait;

use crate::{
    domain::{
        A2AError, AgentAuthentication, AgentCapabilities, AgentCard, AgentCardSignature,
        AgentProvider, AgentSkill, SecurityRequirement, SecuritySchemes,
    },
    port::server::AgentInfoProvider,
};

/// A simple agent info provider that returns a fixed agent card
#[derive(Clone)]
pub struct SimpleAgentInfo {
    /// The agent card to return
    card: AgentCard,
}

impl SimpleAgentInfo {
    /// Create a new agent info provider with the given name and URL
    pub fn new(name: String, url: String) -> Self {
        Self {
            card: AgentCard {
                name,
                description: None,
                url,
                provider: None,
                version: "1.0.0".to_string(),
                documentation_url: None,
                capabilities: AgentCapabilities::default(),
                authentication: None,
                default_input_modes: vec!["text".to_string()],
                default_output_modes: vec!["text".to_string()],
                skills: Vec::new(),
                security_schemes: None,
                security: None,
                signature: None,
                supports_authenticated_extended_card: None,
            },
        }
    }

    /// Set the description of the agent
    pub fn with_description(mut self, description: String) -> Self {
        self.card.description = Some(description);
        self
    }

    /// Set the provider of the agent
    pub fn with_provider(mut self, organization: String, url: Option<String>) -> Self {
        self.card.provider = Some(AgentProvider {
            organization,
            url,
        });
        self
    }

    /// Set the version of the agent
    pub fn with_version(mut self, version: String) -> Self {
        self.card.version = version;
        self
    }

    /// Set the documentation URL of the agent
    pub fn with_documentation_url(mut self, url: String) -> Self {
        self.card.documentation_url = Some(url);
        self
    }

    /// Enable streaming capability
    pub fn with_streaming(mut self) -> Self {
        self.card.capabilities.streaming = true;
        self
    }

    /// Enable push notifications capability
    pub fn with_push_notifications(mut self) -> Self {
        self.card.capabilities.push_notifications = true;
        self
    }

    /// Enable state transition history capability
    pub fn with_state_transition_history(mut self) -> Self {
        self.card.capabilities.state_transition_history = true;
        self
    }

    /// Set the authentication schemes
    pub fn with_authentication(mut self, schemes: Vec<String>) -> Self {
        self.card.authentication = Some(AgentAuthentication {
            schemes,
            credentials: None,
        });
        self
    }

    /// Add an input mode
    pub fn add_input_mode(mut self, mode: String) -> Self {
        self.card.default_input_modes.push(mode);
        self
    }

    /// Add an output mode
    pub fn add_output_mode(mut self, mode: String) -> Self {
        self.card.default_output_modes.push(mode);
        self
    }

    /// Add a skill
    pub fn add_skill(mut self, id: String, name: String, description: Option<String>) -> Self {
        self.card.skills.push(AgentSkill {
            id,
            name,
            description,
            tags: None,
            examples: None,
            input_modes: None,
            output_modes: None,
            security: None,
        });
        self
    }

    /// Set the security schemes for the agent (v0.3.0)
    pub fn with_security_schemes(mut self, schemes: SecuritySchemes) -> Self {
        self.card.security_schemes = Some(schemes);
        self
    }

    /// Set the security requirements for the agent (v0.3.0)
    pub fn with_security(mut self, requirements: Vec<SecurityRequirement>) -> Self {
        self.card.security = Some(requirements);
        self
    }

    /// Set the signature for the agent card (v0.3.0)
    pub fn with_signature(mut self, signature: AgentCardSignature) -> Self {
        self.card.signature = Some(signature);
        self
    }

    /// Enable authenticated extended card support (v0.3.0)
    pub fn with_extended_card_support(mut self, enabled: bool) -> Self {
        self.card.supports_authenticated_extended_card = Some(enabled);
        self
    }
}

#[async_trait]
impl AgentInfoProvider for SimpleAgentInfo {
    async fn get_agent_card(&self) -> Result<AgentCard, A2AError> {
        Ok(self.card.clone())
    }
}