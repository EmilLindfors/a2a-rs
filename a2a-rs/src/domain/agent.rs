use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Information about an agent provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProvider {
    pub organization: String,
    pub url: String,
}

/// Security scheme types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SecurityScheme {
    #[serde(rename = "apiKey")]
    ApiKey {
        #[serde(rename = "in")]
        location: String,  // "query" | "header" | "cookie"
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    #[serde(rename = "http")]
    Http {
        scheme: String,
        #[serde(skip_serializing_if = "Option::is_none", rename = "bearerFormat")]
        bearer_format: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    #[serde(rename = "oauth2")]
    OAuth2 {
        flows: OAuthFlows,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    #[serde(rename = "openIdConnect")]
    OpenIdConnect {
        #[serde(rename = "openIdConnectUrl")]
        open_id_connect_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

/// OAuth flow configurations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthFlows {
    #[serde(skip_serializing_if = "Option::is_none", rename = "authorizationCode")]
    pub authorization_code: Option<AuthorizationCodeOAuthFlow>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "clientCredentials")]
    pub client_credentials: Option<ClientCredentialsOAuthFlow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implicit: Option<ImplicitOAuthFlow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<PasswordOAuthFlow>,
}

/// Authorization code OAuth flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationCodeOAuthFlow {
    #[serde(rename = "authorizationUrl")]
    pub authorization_url: String,
    #[serde(rename = "tokenUrl")]
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    pub scopes: HashMap<String, String>,
}

/// Client credentials OAuth flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCredentialsOAuthFlow {
    #[serde(rename = "tokenUrl")]
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    pub scopes: HashMap<String, String>,
}

/// Implicit OAuth flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplicitOAuthFlow {
    #[serde(rename = "authorizationUrl")]
    pub authorization_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    pub scopes: HashMap<String, String>,
}

/// Password OAuth flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordOAuthFlow {
    #[serde(rename = "tokenUrl")]
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    pub scopes: HashMap<String, String>,
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
    pub description: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "inputModes")]
    pub input_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "outputModes")]
    pub output_modes: Option<Vec<String>>,
}

impl AgentSkill {
    /// Create a new skill with the minimum required fields
    pub fn new(id: String, name: String, description: String, tags: Vec<String>) -> Self {
        Self {
            id,
            name,
            description,
            tags,
            examples: None,
            input_modes: None,
            output_modes: None,
        }
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
        description: String,
        tags: Vec<String>,
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
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct AgentCard {
    pub name: String,
    pub description: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<AgentProvider>,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "documentationUrl")]
    pub documentation_url: Option<String>,
    pub capabilities: AgentCapabilities,
    #[serde(skip_serializing_if = "Option::is_none", rename = "securitySchemes")]
    pub security_schemes: Option<HashMap<String, SecurityScheme>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<HashMap<String, Vec<String>>>>,
    #[serde(default = "default_input_modes", rename = "defaultInputModes")]
    pub default_input_modes: Vec<String>,
    #[serde(default = "default_output_modes", rename = "defaultOutputModes")]
    pub default_output_modes: Vec<String>,
    pub skills: Vec<AgentSkill>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "supportsAuthenticatedExtendedCard")]
    pub supports_authenticated_extended_card: Option<bool>,
}

fn default_input_modes() -> Vec<String> {
    vec!["text".to_string()]
}

fn default_output_modes() -> Vec<String> {
    vec!["text".to_string()]
}

/// Push notification authentication information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNotificationAuthenticationInfo {
    pub schemes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
}

/// Configuration for push notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNotificationConfig {
    pub url: String,
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<PushNotificationAuthenticationInfo>,
}
