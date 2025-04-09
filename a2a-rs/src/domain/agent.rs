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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default, rename = "pushNotifications")]
    pub push_notifications: bool,
    #[serde(default, rename = "stateTransitionHistory")]
    pub state_transition_history: bool,
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            streaming: false,
            push_notifications: false,
            state_transition_history: false,
        }
    }
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