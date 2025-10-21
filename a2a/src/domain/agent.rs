use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// JSON Web Signature for AgentCard integrity verification (RFC 7515)
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

/// OAuth2 authorization code flow configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthFlowAuthorizationCode {
    #[serde(rename = "authorizationUrl")]
    pub authorization_url: String,
    #[serde(rename = "tokenUrl")]
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, String>>,
}

/// OAuth2 implicit flow configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthFlowImplicit {
    #[serde(rename = "authorizationUrl")]
    pub authorization_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, String>>,
}

/// OAuth2 password flow configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthFlowPassword {
    #[serde(rename = "tokenUrl")]
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, String>>,
}

/// OAuth2 client credentials flow configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthFlowClientCredentials {
    #[serde(rename = "tokenUrl")]
    pub token_url: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshUrl")]
    pub refresh_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<HashMap<String, String>>,
}

/// OAuth2 flows configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthFlows {
    #[serde(skip_serializing_if = "Option::is_none", rename = "authorizationCode")]
    pub authorization_code: Option<OAuthFlowAuthorizationCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implicit: Option<OAuthFlowImplicit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<OAuthFlowPassword>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "clientCredentials")]
    pub client_credentials: Option<OAuthFlowClientCredentials>,
}

/// Security scheme types as per A2A Protocol v0.3.0 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SecurityScheme {
    /// API Key authentication
    #[serde(rename = "apiKey")]
    ApiKey {
        #[serde(rename = "in")]
        location: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// HTTP authentication (Basic, Bearer, etc.)
    #[serde(rename = "http")]
    Http {
        scheme: String,
        #[serde(skip_serializing_if = "Option::is_none", rename = "bearerFormat")]
        bearer_format: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// OAuth 2.0 authentication with optional metadata URL (v0.3.0)
    #[serde(rename = "oauth2")]
    OAuth2 {
        flows: Box<OAuthFlows>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// OAuth2 metadata discovery endpoint per RFC 8414 (v0.3.0)
        #[serde(skip_serializing_if = "Option::is_none", rename = "metadataUrl")]
        metadata_url: Option<String>,
    },
    /// OpenID Connect authentication
    #[serde(rename = "openIdConnect")]
    OpenIdConnect {
        #[serde(rename = "openIdConnectUrl")]
        open_id_connect_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// Mutual TLS authentication (v0.3.0)
    #[serde(rename = "mutualTls")]
    MutualTls {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

/// Information about an agent provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProvider {
    pub organization: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Authentication information for an agent (legacy - use SecurityScheme for v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAuthentication {
    pub schemes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
}

/// Security schemes available for the agent (v0.3.0)
pub type SecuritySchemes = HashMap<String, SecurityScheme>;

/// Security requirements for the agent (v0.3.0)
/// Each item maps scheme names to required scopes
pub type SecurityRequirement = HashMap<String, Vec<String>>;

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
    /// Per-skill security requirements (v0.3.0) - maps security scheme names to required scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<HashMap<String, Vec<String>>>>,
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
    /// Legacy authentication (deprecated - use securitySchemes and security instead)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AgentAuthentication>,
    #[serde(default = "default_input_modes", rename = "defaultInputModes")]
    pub default_input_modes: Vec<String>,
    #[serde(default = "default_output_modes", rename = "defaultOutputModes")]
    pub default_output_modes: Vec<String>,
    pub skills: Vec<AgentSkill>,
    /// Available security schemes (v0.3.0)
    #[serde(skip_serializing_if = "Option::is_none", rename = "securitySchemes")]
    pub security_schemes: Option<SecuritySchemes>,
    /// Security requirements for the agent (v0.3.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<SecurityRequirement>>,
    /// Optional signature for card integrity verification (v0.3.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<AgentCardSignature>,
    /// Whether this agent supports authenticated extended card retrieval (v0.3.0)
    #[serde(skip_serializing_if = "Option::is_none", rename = "supportsAuthenticatedExtendedCard")]
    pub supports_authenticated_extended_card: Option<bool>,
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
        assert_eq!(json_value["type"], "mutualTls");
        assert_eq!(json_value["description"], "Mutual TLS authentication");
    }

    #[test]
    fn test_security_scheme_oauth2_with_metadata() {
        let flows = OAuthFlows {
            authorization_code: Some(OAuthFlowAuthorizationCode {
                authorization_url: "https://example.com/oauth/authorize".to_string(),
                token_url: "https://example.com/oauth/token".to_string(),
                refresh_url: None,
                scopes: None,
            }),
            implicit: None,
            password: None,
            client_credentials: None,
        };

        let scheme = SecurityScheme::OAuth2 {
            flows: Box::new(flows),
            description: Some("OAuth2 authentication".to_string()),
            metadata_url: Some("https://example.com/.well-known/oauth-authorization-server".to_string()),
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
            description: None,
            url: "https://example.com".to_string(),
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
            signature: Some(signature),
            supports_authenticated_extended_card: Some(true),
        };

        let json_value = serde_json::to_value(&card).unwrap();
        assert!(json_value["signature"].is_object());
        assert_eq!(json_value["signature"]["protected"], "eyJhbGciOiJSUzI1NiJ9");
        assert_eq!(json_value["supportsAuthenticatedExtendedCard"], true);
    }

    #[test]
    fn test_agent_skill_with_security() {
        let mut security_req = HashMap::new();
        security_req.insert("oauth2".to_string(), vec!["read:users".to_string()]);

        let skill = AgentSkill {
            id: "test-skill".to_string(),
            name: "Test Skill".to_string(),
            description: Some("A test skill".to_string()),
            tags: None,
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
            description: None,
            url: "https://example.com".to_string(),
            provider: None,
            version: "1.0.0".to_string(),
            documentation_url: None,
            capabilities: AgentCapabilities::default(),
            authentication: None,
            default_input_modes: vec!["text".to_string()],
            default_output_modes: vec!["text".to_string()],
            skills: Vec::new(),
            security_schemes: Some(security_schemes),
            security: Some(vec![security_req]),
            signature: None,
            supports_authenticated_extended_card: None,
        };

        let json_value = serde_json::to_value(&card).unwrap();
        assert!(json_value["securitySchemes"].is_object());
        assert_eq!(json_value["securitySchemes"]["bearer"]["type"], "http");
        assert_eq!(json_value["securitySchemes"]["mtls"]["type"], "mutualTls");
        assert!(json_value["security"].is_array());
    }
}