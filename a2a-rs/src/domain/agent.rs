use serde::{Deserialize, Serialize};

/// Information about an agent provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProvider {
    pub organization: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Authentication information for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAuthentication {
    pub schemes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
}

/// Capabilities supported by an agent
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentCapabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default, rename = "pushNotifications")]
    pub push_notifications: bool,
    #[serde(default, rename = "stateTransitionHistory")]
    pub state_transition_history: bool,
}

/// A skill provided by an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "inputModes")]
    pub input_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "outputModes")]
    pub output_modes: Option<Vec<String>>,
}

impl AgentSkill {
    /// Create a new skill with the minimum required fields
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            description: None,
            tags: None,
            examples: None,
            input_modes: None,
            output_modes: None,
        }
    }

    /// Add a description to the skill
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Add tags to the skill
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Add examples to the skill
    pub fn with_examples(mut self, examples: Vec<String>) -> Self {
        self.examples = Some(examples);
        self
    }

    /// Add input modes to the skill
    pub fn with_input_modes(mut self, input_modes: Vec<String>) -> Self {
        self.input_modes = Some(input_modes);
        self
    }

    /// Add output modes to the skill
    pub fn with_output_modes(mut self, output_modes: Vec<String>) -> Self {
        self.output_modes = Some(output_modes);
        self
    }

    /// Create a comprehensive skill with all details in one call
    pub fn comprehensive(
        id: String,
        name: String,
        description: Option<String>,
        tags: Option<Vec<String>>,
        examples: Option<Vec<String>>,
        input_modes: Option<Vec<String>>,
        output_modes: Option<Vec<String>>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            tags,
            examples,
            input_modes,
            output_modes,
        }
    }
}

/// Card describing an agent's capabilities and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<AgentProvider>,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "documentationUrl")]
    pub documentation_url: Option<String>,
    pub capabilities: AgentCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AgentAuthentication>,
    #[serde(default = "default_input_modes", rename = "defaultInputModes")]
    pub default_input_modes: Vec<String>,
    #[serde(default = "default_output_modes", rename = "defaultOutputModes")]
    pub default_output_modes: Vec<String>,
    pub skills: Vec<AgentSkill>,
}

fn default_input_modes() -> Vec<String> {
    vec!["text".to_string()]
}

fn default_output_modes() -> Vec<String> {
    vec!["text".to_string()]
}

/// Authentication information with extensibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationInfo {
    pub schemes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
    // Support for additional properties as specified in schema
    #[serde(flatten)]
    pub additional_properties: serde_json::Map<String, serde_json::Value>,
}

/// Configuration for push notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNotificationConfig {
    pub url: String,
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AuthenticationInfo>,
}
