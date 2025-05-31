use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Information about an agent provider, including organization details and contact URL.
///
/// This structure contains metadata about the organization or entity that provides
/// the agent service, including contact information and organizational details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProvider {
    pub organization: String,
    pub url: String,
}

/// Security scheme configurations for agent authentication.
///
/// Defines the various authentication methods supported by an agent,
/// including API keys, HTTP authentication, and OAuth 2.0 flows.
/// Each scheme specifies the required parameters and configuration
/// for successful authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SecurityScheme {
    #[serde(rename = "apiKey")]
    ApiKey {
        #[serde(rename = "in")]
        location: String, // "query" | "header" | "cookie"
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
        flows: Box<OAuthFlows>,
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

/// OAuth flow configurations supporting multiple authentication flows.
///
/// This structure contains optional configurations for different OAuth 2.0 flows
/// that an agent may support. Each flow type has specific requirements and use cases:
/// - Authorization Code: Most secure, requires user interaction
/// - Client Credentials: For server-to-server authentication  
/// - Implicit: For client-side applications (deprecated in OAuth 2.1)
/// - Password: For trusted applications with user credentials
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

/// Configuration for OAuth 2.0 authorization code flow.
///
/// The authorization code flow is the most secure OAuth flow, involving
/// a two-step process where the user is redirected to authorize the application,
/// and then an authorization code is exchanged for an access token.
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

/// Configuration for OAuth 2.0 client credentials flow.
///
/// The client credentials flow is used for server-to-server authentication
/// where no user interaction is required. The client authenticates using
/// its own credentials to obtain an access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCredentialsOAuthFlow {
    #[serde(rename = "tokenUrl")]
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    pub scopes: HashMap<String, String>,
}

/// Configuration for OAuth 2.0 implicit flow.
///
/// The implicit flow is designed for client-side applications that cannot
/// securely store client secrets. Access tokens are returned directly
/// from the authorization endpoint. Note: This flow is deprecated in OAuth 2.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplicitOAuthFlow {
    #[serde(rename = "authorizationUrl")]
    pub authorization_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    pub scopes: HashMap<String, String>,
}

/// Configuration for OAuth 2.0 password flow.
///
/// The password flow allows the application to exchange the user's username
/// and password for an access token. This flow should only be used by
/// highly trusted applications as it requires handling user credentials directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordOAuthFlow {
    #[serde(rename = "tokenUrl")]
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    pub scopes: HashMap<String, String>,
}

/// Capabilities supported by an agent, including streaming and push notifications.
///
/// This structure defines what features an agent supports:
/// - `streaming`: Whether the agent supports real-time streaming updates
/// - `push_notifications`: Whether the agent can send push notifications
/// - `state_transition_history`: Whether the agent maintains task state history
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentCapabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default, rename = "pushNotifications")]
    pub push_notifications: bool,
    #[serde(default, rename = "stateTransitionHistory")]
    pub state_transition_history: bool,
}

/// A skill provided by an agent with metadata and examples.\n///\n/// Skills define specific capabilities that an agent can perform,\n/// including natural language descriptions, categorization tags,\n/// usage examples, and supported input/output modes.\n///\n/// # Example\n/// ```rust\n/// use a2a_rs::AgentSkill;\n/// \n/// let skill = AgentSkill::new(\n///     \"text-generation\".to_string(),\n///     \"Text Generation\".to_string(), \n///     \"Generate natural language text based on prompts\".to_string(),\n///     vec![\"nlp\".to_string(), \"generation\".to_string()]\n/// );\n/// ```
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

/// Card describing an agent's capabilities, metadata, and available skills.\n///\n/// The AgentCard is the primary descriptor for an agent, containing all the\n/// information needed for clients to understand what the agent can do and\n/// how to interact with it. This includes basic metadata like name and version,\n/// capabilities like streaming support, available skills, and security requirements.\n///\n/// # Example\n/// ```rust\n/// use a2a_rs::{AgentCard, AgentCapabilities, AgentSkill};\n/// \n/// let card = AgentCard::builder()\n///     .name(\"My Agent\".to_string())\n///     .description(\"A helpful AI agent\".to_string())\n///     .url(\"https://agent.example.com\".to_string())\n///     .version(\"1.0.0\".to_string())\n///     .capabilities(AgentCapabilities::default())\n///     .build();\n/// ```
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
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "supportsAuthenticatedExtendedCard"
    )]
    pub supports_authenticated_extended_card: Option<bool>,
}

fn default_input_modes() -> Vec<String> {
    vec!["text".to_string()]
}

fn default_output_modes() -> Vec<String> {
    vec!["text".to_string()]
}

/// Authentication information for push notification endpoints.
///
/// Specifies the authentication schemes and credentials required
/// to send push notifications to a client endpoint. This allows
/// agents to securely deliver notifications to authenticated endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNotificationAuthenticationInfo {
    pub schemes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
}

/// Configuration for push notification delivery including URL and authentication.
///
/// Contains all the information needed to send push notifications to a client,
/// including the destination URL, optional authentication token, and
/// authentication scheme details.
///
/// # Example
/// ```rust
/// use a2a_rs::PushNotificationConfig;
/// 
/// let config = PushNotificationConfig {
///     url: "https://client.example.com/notifications".to_string(),
///     token: Some("bearer-token-123".to_string()),
///     authentication: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNotificationConfig {
    pub url: String,
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<PushNotificationAuthenticationInfo>,
}
