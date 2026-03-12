//! A simple agent info provider implementation

// This module is already conditionally compiled with #[cfg(feature = "server")] in mod.rs

use async_trait::async_trait;

use std::collections::HashMap;

use crate::{
    domain::{
        A2AError, AgentCapabilities, AgentCard, AgentExtension, AgentProvider, AgentSkill,
        SecurityScheme,
    },
    services::server::AgentInfoProvider,
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
                description: "Agent description".to_string(),
                url,
                provider: None,
                version: "1.0.0".to_string(),
                protocol_version: "0.3.0".to_string(),
                preferred_transport: "JSONRPC".to_string(),
                additional_interfaces: None,
                icon_url: None,
                documentation_url: None,
                capabilities: AgentCapabilities::default(),
                security_schemes: None,
                security: None,
                default_input_modes: vec!["text".to_string()],
                default_output_modes: vec!["text".to_string()],
                skills: Vec::new(),
                signatures: None,
                supports_authenticated_extended_card: None,
            },
        }
    }

    /// Set the description of the agent
    pub fn with_description(mut self, description: String) -> Self {
        self.card.description = description;
        self
    }

    /// Set the provider of the agent
    pub fn with_provider(mut self, organization: String, url: String) -> Self {
        self.card.provider = Some(AgentProvider { organization, url });
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

    /// Enable authenticated extended card support (v0.3.0)
    pub fn with_authenticated_extended_card(mut self) -> Self {
        self.card.supports_authenticated_extended_card = Some(true);
        self
    }

    /// Set the security schemes for the agent card.
    ///
    /// Accepts a map of scheme names to `SecurityScheme` definitions.
    /// Also sets the agent-level `security` field to require any one of the provided schemes
    /// (with no additional scopes by default).
    ///
    /// # Example
    /// ```rust
    /// use std::collections::HashMap;
    /// use a2a_rs::domain::SecurityScheme;
    /// use a2a_rs::SimpleAgentInfo;
    ///
    /// let mut schemes = HashMap::new();
    /// schemes.insert("bearer".to_string(), SecurityScheme::Http {
    ///     scheme: "bearer".to_string(),
    ///     bearer_format: Some("JWT".to_string()),
    ///     description: None,
    /// });
    ///
    /// let agent = SimpleAgentInfo::new("agent".into(), "http://localhost".into())
    ///     .with_security_schemes(schemes);
    /// ```
    pub fn with_security_schemes(mut self, schemes: HashMap<String, SecurityScheme>) -> Self {
        // Build security requirements: each scheme is an OR alternative (no scopes required by default)
        let security: Vec<HashMap<String, Vec<String>>> = schemes
            .keys()
            .map(|name| {
                let mut req = HashMap::new();
                req.insert(name.clone(), Vec::new());
                req
            })
            .collect();

        self.card.security_schemes = if schemes.is_empty() {
            None
        } else {
            Some(schemes)
        };
        self.card.security = if security.is_empty() {
            None
        } else {
            Some(security)
        };
        self
    }

    /// Set the security schemes with explicit security requirements.
    ///
    /// Unlike `with_security_schemes`, this allows specifying custom security requirements
    /// (e.g., requiring specific OAuth2 scopes).
    pub fn with_security(
        mut self,
        schemes: HashMap<String, SecurityScheme>,
        security: Vec<HashMap<String, Vec<String>>>,
    ) -> Self {
        self.card.security_schemes = if schemes.is_empty() {
            None
        } else {
            Some(schemes)
        };
        self.card.security = if security.is_empty() {
            None
        } else {
            Some(security)
        };
        self
    }

    /// Add a protocol extension to the agent capabilities
    pub fn add_extension(mut self, extension: AgentExtension) -> Self {
        match &mut self.card.capabilities.extensions {
            Some(exts) => exts.push(extension),
            None => self.card.capabilities.extensions = Some(vec![extension]),
        }
        self
    }

    /// Set protocol extensions on the agent capabilities, replacing any existing ones
    pub fn with_extensions(mut self, extensions: Vec<AgentExtension>) -> Self {
        self.card.capabilities.extensions = if extensions.is_empty() {
            None
        } else {
            Some(extensions)
        };
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

    /// Add a basic skill
    pub fn add_skill(mut self, id: String, name: String, description: Option<String>) -> Self {
        let skill = AgentSkill::new(
            id,
            name,
            description.unwrap_or_else(|| "Skill description".to_string()),
            Vec::new(), // empty tags
        );

        self.card.skills.push(skill);
        self
    }

    /// Add a comprehensive skill with all details
    #[allow(clippy::too_many_arguments)]
    pub fn add_comprehensive_skill(
        mut self,
        id: String,
        name: String,
        description: Option<String>,
        tags: Option<Vec<String>>,
        examples: Option<Vec<String>>,
        input_modes: Option<Vec<String>>,
        output_modes: Option<Vec<String>>,
    ) -> Self {
        let skill = AgentSkill::comprehensive(
            id,
            name,
            description.unwrap_or_else(|| "Skill description".to_string()),
            tags.unwrap_or_default(),
            examples,
            input_modes,
            output_modes,
            None, // security - v0.3.0
        );

        self.card.skills.push(skill);
        self
    }

    /// Add a skill using the AgentSkill builder
    pub fn add_skill_object(mut self, skill: AgentSkill) -> Self {
        self.card.skills.push(skill);
        self
    }

    /// Replace all skills with a new set
    pub fn with_skills(mut self, skills: Vec<AgentSkill>) -> Self {
        self.card.skills = skills;
        self
    }

    /// Get all currently defined skills
    pub fn get_skills(&self) -> &Vec<AgentSkill> {
        &self.card.skills
    }

    /// Get a skill by ID
    pub fn get_skill_by_id(&self, id: &str) -> Option<&AgentSkill> {
        self.card.skills.iter().find(|skill| skill.id == id)
    }

    /// Add a new skill or update an existing one
    pub fn add_or_update_skill(&mut self, skill: AgentSkill) -> &mut Self {
        // Check if the skill with this ID already exists
        if let Some(index) = self.card.skills.iter().position(|s| s.id == skill.id) {
            // Update the existing skill
            self.card.skills[index] = skill;
        } else {
            // Add a new skill
            self.card.skills.push(skill);
        }
        self
    }

    /// Remove a skill by ID
    pub fn remove_skill(&mut self, id: &str) -> bool {
        let len_before = self.card.skills.len();
        self.card.skills.retain(|skill| skill.id != id);
        self.card.skills.len() < len_before
    }

    /// Update a skill's details
    #[allow(clippy::too_many_arguments)]
    pub fn update_skill(
        &mut self,
        id: &str,
        name: Option<String>,
        description: Option<Option<String>>,
        tags: Option<Option<Vec<String>>>,
        examples: Option<Option<Vec<String>>>,
        input_modes: Option<Option<Vec<String>>>,
        output_modes: Option<Option<Vec<String>>>,
    ) -> bool {
        if let Some(skill) = self.card.skills.iter_mut().find(|s| s.id == id) {
            if let Some(name_val) = name {
                skill.name = name_val;
            }

            if let Some(desc) = description {
                skill.description = desc.unwrap_or_else(|| "Updated description".to_string());
            }

            if let Some(tags_val) = tags {
                skill.tags = tags_val.unwrap_or_default();
            }

            if let Some(examples_val) = examples {
                skill.examples = examples_val;
            }

            if let Some(input_modes_val) = input_modes {
                skill.input_modes = input_modes_val;
            }

            if let Some(output_modes_val) = output_modes {
                skill.output_modes = output_modes_val;
            }

            true
        } else {
            false
        }
    }
}

#[async_trait]
impl AgentInfoProvider for SimpleAgentInfo {
    async fn get_agent_card(&self) -> Result<AgentCard, A2AError> {
        Ok(self.card.clone())
    }

    // Override the default implementation for better performance
    async fn get_skills(&self) -> Result<Vec<AgentSkill>, A2AError> {
        Ok(self.card.skills.clone())
    }

    // Override the default implementation for better performance
    async fn get_skill_by_id(&self, id: &str) -> Result<Option<AgentSkill>, A2AError> {
        Ok(self
            .card
            .skills
            .iter()
            .find(|skill| skill.id == id)
            .cloned())
    }

    // Override the default implementation for better performance
    async fn has_skill(&self, id: &str) -> Result<bool, A2AError> {
        Ok(self.card.skills.iter().any(|skill| skill.id == id))
    }

    // Override to provide authenticated extended card when configured (v0.3.0)
    async fn get_authenticated_extended_card(&self) -> Result<AgentCard, A2AError> {
        if self
            .card
            .supports_authenticated_extended_card
            .unwrap_or(false)
        {
            // Return the same card for now
            // In a real implementation, this might include additional authenticated-only fields
            Ok(self.card.clone())
        } else {
            Err(A2AError::AuthenticatedExtendedCardNotConfigured)
        }
    }
}
