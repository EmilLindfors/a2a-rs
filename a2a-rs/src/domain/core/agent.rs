use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Supported A2A transport protocols (v0.3.0).
///
/// Defines the transport protocols that agents can use for communication.
/// Agents can support multiple transport protocols simultaneously.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportProtocol {
    #[serde(rename = "JSONRPC")]
    JsonRpc,
    #[serde(rename = "GRPC")]
    Grpc,
    #[serde(rename = "HTTP+JSON")]
    HttpJson,
}

/// Declares a combination of URL and transport protocol for interacting with the agent (v0.3.0).
///
/// This allows agents to expose the same functionality over multiple transport mechanisms.
/// Clients can select any interface based on their transport capabilities and preferences.
///
/// # Example
/// ```rust
/// use a2a_rs::AgentInterface;
///
/// let interface = AgentInterface {
///     url: "https://api.example.com/grpc".to_string(),
///     transport: "GRPC".to_string(),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInterface {
    /// The URL where this interface is available
    pub url: String,
    /// The transport protocol supported at this URL
    pub transport: String,
}

/// Declaration of a protocol extension supported by an agent (v0.3.0).
///
/// Extensions provide a mechanism for agents to declare support for
/// additional capabilities beyond the core A2A protocol specification.
///
/// # Example
/// ```rust
/// use a2a_rs::AgentExtension;
/// use std::collections::HashMap;
///
/// let extension = AgentExtension {
///     uri: "https://example.com/extensions/custom-auth".to_string(),
///     description: Some("Custom authentication extension".to_string()),
///     required: Some(false),
///     params: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExtension {
    /// Unique URI identifying the extension
    pub uri: String,
    /// Human-readable description of the extension
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the client must understand this extension to interact with the agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Extension-specific configuration parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, serde_json::Value>>,
}

/// JSON Web Signature for AgentCard integrity verification (RFC 7515).
///
/// This structure represents a digital signature that can be used to verify
/// the integrity and authenticity of an AgentCard. It follows the JSON Web
/// Signature (JWS) standard as defined in RFC 7515.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCardSignature {
    /// Base64url-encoded protected JWS header
    pub protected: String,
    /// Base64url-encoded signature
    pub signature: String,
    /// Optional unprotected JWS header values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<HashMap<String, serde_json::Value>>,
}

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
        /// OAuth2 metadata discovery endpoint per RFC 8414 (v0.3.0)
        #[serde(skip_serializing_if = "Option::is_none", rename = "metadataUrl")]
        metadata_url: Option<String>,
    },
    #[serde(rename = "openIdConnect")]
    OpenIdConnect {
        #[serde(rename = "openIdConnectUrl")]
        open_id_connect_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// Mutual TLS authentication (v0.3.0)
    #[serde(rename = "mutualTLS")]
    MutualTls {
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
/// - `extensions`: List of protocol extensions supported by the agent (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentCapabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default, rename = "pushNotifications")]
    pub push_notifications: bool,
    #[serde(default, rename = "stateTransitionHistory")]
    pub state_transition_history: bool,
    /// List of protocol extensions supported by the agent (v0.3.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<AgentExtension>>,
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
    /// Per-skill security requirements (v0.3.0) - maps security scheme names to required scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<HashMap<String, Vec<String>>>>,
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
            security: None,
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

    /// Add security requirements to the skill (v0.3.0)
    pub fn with_security(mut self, security: Vec<HashMap<String, Vec<String>>>) -> Self {
        self.security = Some(security);
        self
    }

    /// Create a comprehensive skill with all details in one call
    #[allow(clippy::too_many_arguments)]
    pub fn comprehensive(
        id: String,
        name: String,
        description: String,
        tags: Vec<String>,
        examples: Option<Vec<String>>,
        input_modes: Option<Vec<String>>,
        output_modes: Option<Vec<String>>,
        security: Option<Vec<HashMap<String, Vec<String>>>>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            tags,
            examples,
            input_modes,
            output_modes,
            security,
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
    /// The version of the A2A protocol this agent supports (v0.3.0 required field)
    #[serde(default = "default_protocol_version", rename = "protocolVersion")]
    #[builder(default = default_protocol_version())]
    pub protocol_version: String,
    /// The transport protocol for the preferred endpoint (v0.3.0)
    #[serde(default = "default_preferred_transport", rename = "preferredTransport")]
    #[builder(default = default_preferred_transport())]
    pub preferred_transport: String,
    /// Additional supported interfaces (transport and URL combinations) (v0.3.0)
    #[serde(skip_serializing_if = "Option::is_none", rename = "additionalInterfaces")]
    pub additional_interfaces: Option<Vec<AgentInterface>>,
    /// Optional URL to an icon for the agent (v0.3.0)
    #[serde(skip_serializing_if = "Option::is_none", rename = "iconUrl")]
    pub icon_url: Option<String>,
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
    /// JSON Web Signatures for this AgentCard (v0.3.0 - changed from singular to plural)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Vec<AgentCardSignature>>,
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

fn default_protocol_version() -> String {
    "0.3.0".to_string()
}

fn default_preferred_transport() -> String {
    "JSONRPC".to_string()
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
///     id: Some("config-123".to_string()),
///     url: "https://client.example.com/notifications".to_string(),
///     token: Some("bearer-token-123".to_string()),
///     authentication: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNotificationConfig {
    /// Unique identifier for the push notification configuration (v0.3.0)
    /// Allows multiple notification callbacks per task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub url: String,
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<PushNotificationAuthenticationInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_security_scheme_api_key_serialization() {
        let scheme = SecurityScheme::ApiKey {
            location: "header".to_string(),
            name: "X-API-Key".to_string(),
            description: Some("API Key authentication".to_string()),
        };

        let json_value = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json_value["type"], "apiKey");
        assert_eq!(json_value["in"], "header");
        assert_eq!(json_value["name"], "X-API-Key");
    }

    #[test]
    fn test_security_scheme_http_serialization() {
        let scheme = SecurityScheme::Http {
            scheme: "bearer".to_string(),
            bearer_format: Some("JWT".to_string()),
            description: Some("Bearer token authentication".to_string()),
        };

        let json_value = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json_value["type"], "http");
        assert_eq!(json_value["scheme"], "bearer");
        assert_eq!(json_value["bearerFormat"], "JWT");
    }

    #[test]
    fn test_security_scheme_mtls_serialization() {
        let scheme = SecurityScheme::MutualTls {
            description: Some("Mutual TLS authentication".to_string()),
        };

        let json_value = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json_value["type"], "mutualTLS");
        assert_eq!(json_value["description"], "Mutual TLS authentication");
    }

    #[test]
    fn test_security_scheme_oauth2_with_metadata() {
        let flows = OAuthFlows {
            authorization_code: Some(AuthorizationCodeOAuthFlow {
                authorization_url: "https://example.com/oauth/authorize".to_string(),
                token_url: "https://example.com/oauth/token".to_string(),
                refresh_url: None,
                scopes: HashMap::new(),
            }),
            client_credentials: None,
            implicit: None,
            password: None,
        };

        let scheme = SecurityScheme::OAuth2 {
            flows: Box::new(flows),
            description: Some("OAuth2 authentication".to_string()),
            metadata_url: Some(
                "https://example.com/.well-known/oauth-authorization-server".to_string(),
            ),
        };

        let json_value = serde_json::to_value(&scheme).unwrap();
        assert_eq!(json_value["type"], "oauth2");
        assert_eq!(
            json_value["metadataUrl"],
            "https://example.com/.well-known/oauth-authorization-server"
        );
    }

    #[test]
    fn test_agent_card_with_signature() {
        let mut signature_header = HashMap::new();
        signature_header.insert("alg".to_string(), json!("RS256"));

        let signature = AgentCardSignature {
            protected: "eyJhbGciOiJSUzI1NiJ9".to_string(),
            signature: "dGVzdF9zaWduYXR1cmU".to_string(),
            header: Some(signature_header),
        };

        let card = AgentCard {
            name: "Test Agent".to_string(),
            description: "Test description".to_string(),
            url: "https://example.com".to_string(),
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
            signatures: Some(vec![signature]),
            supports_authenticated_extended_card: Some(true),
        };

        let json_value = serde_json::to_value(&card).unwrap();
        assert!(json_value["signatures"].is_array());
        assert_eq!(json_value["signatures"][0]["protected"], "eyJhbGciOiJSUzI1NiJ9");
        assert_eq!(json_value["supportsAuthenticatedExtendedCard"], true);
        assert_eq!(json_value["protocolVersion"], "0.3.0");
    }

    #[test]
    fn test_agent_skill_with_security() {
        let mut security_req = HashMap::new();
        security_req.insert("oauth2".to_string(), vec!["read:users".to_string()]);

        let skill = AgentSkill {
            id: "test-skill".to_string(),
            name: "Test Skill".to_string(),
            description: "A test skill".to_string(),
            tags: Vec::new(),
            examples: None,
            input_modes: None,
            output_modes: None,
            security: Some(vec![security_req]),
        };

        let json_value = serde_json::to_value(&skill).unwrap();
        assert!(json_value["security"].is_array());
        assert_eq!(json_value["security"][0]["oauth2"][0], "read:users");
    }

    #[test]
    fn test_agent_card_with_security_schemes() {
        let mut security_schemes = HashMap::new();
        security_schemes.insert(
            "bearer".to_string(),
            SecurityScheme::Http {
                scheme: "bearer".to_string(),
                bearer_format: Some("JWT".to_string()),
                description: None,
            },
        );
        security_schemes.insert(
            "mtls".to_string(),
            SecurityScheme::MutualTls {
                description: Some("Client certificate authentication".to_string()),
            },
        );

        let mut security_req = HashMap::new();
        security_req.insert("bearer".to_string(), Vec::new());

        let card = AgentCard {
            name: "Secure Agent".to_string(),
            description: "Secure agent description".to_string(),
            url: "https://example.com".to_string(),
            provider: None,
            version: "1.0.0".to_string(),
            protocol_version: "0.3.0".to_string(),
            preferred_transport: "JSONRPC".to_string(),
            additional_interfaces: None,
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities::default(),
            security_schemes: Some(security_schemes),
            security: Some(vec![security_req]),
            default_input_modes: vec!["text".to_string()],
            default_output_modes: vec!["text".to_string()],
            skills: Vec::new(),
            signatures: None,
            supports_authenticated_extended_card: None,
        };

        let json_value = serde_json::to_value(&card).unwrap();
        assert!(json_value["securitySchemes"].is_object());
        assert_eq!(json_value["securitySchemes"]["bearer"]["type"], "http");
        assert_eq!(json_value["securitySchemes"]["mtls"]["type"], "mutualTLS");
        assert!(json_value["security"].is_array());
    }
}
