use std::collections::HashMap;

// Re-export generated types so downstream code gets them from `domain::core::agent`
pub use crate::domain::generated::{
    APIKeySecurityScheme, AgentCapabilities, AgentCard, AgentCardSignature, AgentExtension,
    AgentInterface, AgentProvider, AgentSkill, AuthenticationInfo, AuthorizationCodeOAuthFlow,
    ClientCredentialsOAuthFlow, DeviceCodeOAuthFlow, HTTPAuthSecurityScheme, ImplicitOAuthFlow,
    MutualTlsSecurityScheme, OAuth2SecurityScheme, OAuthFlows, OpenIdConnectSecurityScheme,
    PasswordOAuthFlow, SecurityRequirement, SecurityScheme, StringList, o_auth_flows,
    security_scheme,
};

pub type PushNotificationAuthenticationInfo = AuthenticationInfo;

impl AgentSkill {
    /// Create a new skill with the minimum required fields
    pub fn new(id: String, name: String, description: String, tags: Vec<String>) -> Self {
        Self {
            id,
            name,
            description,
            tags,
            ..Default::default()
        }
    }

    /// Add examples to the skill
    pub fn with_examples(mut self, examples: Vec<String>) -> Self {
        self.examples = examples;
        self
    }

    /// Add input modes to the skill
    pub fn with_input_modes(mut self, input_modes: Vec<String>) -> Self {
        self.input_modes = input_modes;
        self
    }

    /// Add output modes to the skill
    pub fn with_output_modes(mut self, output_modes: Vec<String>) -> Self {
        self.output_modes = output_modes;
        self
    }

    /// Add security requirements to the skill
    pub fn with_security(mut self, security: Vec<HashMap<String, Vec<String>>>) -> Self {
        self.security_requirements = security
            .into_iter()
            .map(|req| {
                let schemes = req
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            StringList {
                                list: v,
                                ..Default::default()
                            },
                        )
                    })
                    .collect();
                SecurityRequirement {
                    schemes,
                    ..Default::default()
                }
            })
            .collect();
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
        let mut skill = Self::new(id, name, description, tags);
        if let Some(ex) = examples {
            skill = skill.with_examples(ex);
        }
        if let Some(im) = input_modes {
            skill = skill.with_input_modes(im);
        }
        if let Some(om) = output_modes {
            skill = skill.with_output_modes(om);
        }
        if let Some(sec) = security {
            skill = skill.with_security(sec);
        }
        skill
    }
}

impl SecurityScheme {
    pub fn api_key(name: String, location: String, description: Option<String>) -> Self {
        Self {
            scheme: Some(security_scheme::Scheme::ApiKeySecurityScheme(Box::new(
                APIKeySecurityScheme {
                    name,
                    location,
                    description: description.unwrap_or_default(),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        }
    }

    pub fn http(
        scheme_name: String,
        bearer_format: Option<String>,
        description: Option<String>,
    ) -> Self {
        Self {
            scheme: Some(security_scheme::Scheme::HttpAuthSecurityScheme(Box::new(
                HTTPAuthSecurityScheme {
                    scheme: scheme_name,
                    bearer_format: bearer_format.unwrap_or_default(),
                    description: description.unwrap_or_default(),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        }
    }

    pub fn oauth2(
        flows: OAuthFlows,
        description: Option<String>,
        oauth2_metadata_url: Option<String>,
    ) -> Self {
        Self {
            scheme: Some(security_scheme::Scheme::Oauth2SecurityScheme(Box::new(
                OAuth2SecurityScheme {
                    flows: ::buffa::MessageField::some(flows),
                    description: description.unwrap_or_default(),
                    oauth2_metadata_url: oauth2_metadata_url.unwrap_or_default(),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        }
    }

    pub fn open_id_connect(open_id_connect_url: String, description: Option<String>) -> Self {
        Self {
            scheme: Some(security_scheme::Scheme::OpenIdConnectSecurityScheme(
                Box::new(OpenIdConnectSecurityScheme {
                    open_id_connect_url,
                    description: description.unwrap_or_default(),
                    ..Default::default()
                }),
            )),
            ..Default::default()
        }
    }

    pub fn mutual_tls(description: Option<String>) -> Self {
        Self {
            scheme: Some(security_scheme::Scheme::MtlsSecurityScheme(Box::new(
                MutualTlsSecurityScheme {
                    description: description.unwrap_or_default(),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        }
    }
}

impl OAuthFlows {
    pub fn authorization_code(flow: AuthorizationCodeOAuthFlow) -> Self {
        Self {
            flow: Some(o_auth_flows::Flow::AuthorizationCode(Box::new(flow))),
            ..Default::default()
        }
    }

    pub fn client_credentials(flow: ClientCredentialsOAuthFlow) -> Self {
        Self {
            flow: Some(o_auth_flows::Flow::ClientCredentials(Box::new(flow))),
            ..Default::default()
        }
    }

    pub fn device_code(flow: DeviceCodeOAuthFlow) -> Self {
        Self {
            flow: Some(o_auth_flows::Flow::DeviceCode(Box::new(flow))),
            ..Default::default()
        }
    }
}

impl AgentCapabilities {
    pub fn streaming(&self) -> bool {
        self.streaming.unwrap_or(false)
    }

    pub fn push_notifications(&self) -> bool {
        self.push_notifications.unwrap_or(false)
    }

    pub fn extended_agent_card(&self) -> bool {
        self.extended_agent_card.unwrap_or(false)
    }
}

impl AgentCardSignature {
    pub fn new(
        protected: String,
        signature: String,
        header: Option<::buffa_types::google::protobuf::Struct>,
    ) -> Self {
        Self {
            protected,
            signature,
            header: header.into(),
            ..Default::default()
        }
    }
}

impl AgentCard {
    pub fn builder() -> AgentCardBuilder {
        AgentCardBuilder::new()
    }

    pub fn url(&self) -> &str {
        self.supported_interfaces
            .first()
            .map(|i| i.url.as_str())
            .unwrap_or("")
    }

    pub fn protocol_version(&self) -> &str {
        self.supported_interfaces
            .first()
            .map(|i| i.protocol_version.as_str())
            .unwrap_or("1.0")
    }

    pub fn preferred_transport(&self) -> &str {
        self.supported_interfaces
            .first()
            .map(|i| i.protocol_binding.as_str())
            .unwrap_or("JSONRPC")
    }

    pub fn supports_extended_agent_card(&self) -> bool {
        self.capabilities.extended_agent_card.unwrap_or(false)
    }
}

pub struct AgentCardBuilder {
    name: String,
    description: String,
    url: String,
    provider: Option<AgentProvider>,
    version: String,
    protocol_version: Option<String>,
    preferred_transport: Option<String>,
    supported_interfaces: Vec<AgentInterface>,
    icon_url: Option<String>,
    documentation_url: Option<String>,
    capabilities: Option<AgentCapabilities>,
    security_schemes: HashMap<String, SecurityScheme>,
    security_requirements: Vec<SecurityRequirement>,
    default_input_modes: Vec<String>,
    default_output_modes: Vec<String>,
    skills: Vec<AgentSkill>,
    signatures: Vec<AgentCardSignature>,
}

impl Default for AgentCardBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentCardBuilder {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            url: String::new(),
            provider: None,
            version: String::new(),
            protocol_version: None,
            preferred_transport: None,
            supported_interfaces: Vec::new(),
            icon_url: None,
            documentation_url: None,
            capabilities: None,
            security_schemes: HashMap::new(),
            security_requirements: Vec::new(),
            default_input_modes: vec!["text".to_string()],
            default_output_modes: vec!["text".to_string()],
            skills: Vec::new(),
            signatures: Vec::new(),
        }
    }

    pub fn name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    pub fn description(mut self, description: String) -> Self {
        self.description = description;
        self
    }

    pub fn url(mut self, url: String) -> Self {
        self.url = url;
        self
    }

    pub fn provider(mut self, provider: AgentProvider) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn version(mut self, version: String) -> Self {
        self.version = version;
        self
    }

    pub fn protocol_version(mut self, protocol_version: String) -> Self {
        self.protocol_version = Some(protocol_version);
        self
    }

    pub fn preferred_transport(mut self, preferred_transport: String) -> Self {
        self.preferred_transport = Some(preferred_transport);
        self
    }

    pub fn additional_interfaces(mut self, interfaces: Vec<AgentInterface>) -> Self {
        self.supported_interfaces.extend(interfaces);
        self
    }

    pub fn icon_url(mut self, icon_url: String) -> Self {
        self.icon_url = Some(icon_url);
        self
    }

    pub fn documentation_url(mut self, documentation_url: String) -> Self {
        self.documentation_url = Some(documentation_url);
        self
    }

    pub fn capabilities(mut self, capabilities: AgentCapabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }

    pub fn security_schemes(mut self, security_schemes: HashMap<String, SecurityScheme>) -> Self {
        self.security_schemes = security_schemes;
        self
    }

    pub fn security(mut self, security: Vec<HashMap<String, Vec<String>>>) -> Self {
        self.security_requirements = security
            .into_iter()
            .map(|req| {
                let schemes = req
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            StringList {
                                list: v,
                                ..Default::default()
                            },
                        )
                    })
                    .collect();
                SecurityRequirement {
                    schemes,
                    ..Default::default()
                }
            })
            .collect();
        self
    }

    pub fn default_input_modes(mut self, default_input_modes: Vec<String>) -> Self {
        self.default_input_modes = default_input_modes;
        self
    }

    pub fn default_output_modes(mut self, default_output_modes: Vec<String>) -> Self {
        self.default_output_modes = default_output_modes;
        self
    }

    pub fn skills(mut self, skills: Vec<AgentSkill>) -> Self {
        self.skills = skills;
        self
    }

    pub fn signatures(mut self, signatures: Vec<AgentCardSignature>) -> Self {
        self.signatures = signatures;
        self
    }

    pub fn supports_extended_agent_card(mut self, val: bool) -> Self {
        let caps = self
            .capabilities
            .get_or_insert_with(AgentCapabilities::default);
        caps.extended_agent_card = Some(val);
        self
    }

    pub fn build(self) -> AgentCard {
        let mut supported_interfaces = self.supported_interfaces;
        // Make sure the primary interface exists and is first
        if !self.url.is_empty() {
            let primary = AgentInterface {
                url: self.url,
                protocol_binding: self
                    .preferred_transport
                    .unwrap_or_else(|| "JSONRPC".to_string()),
                protocol_version: self.protocol_version.unwrap_or_else(|| "1.0".to_string()),
                ..Default::default()
            };
            supported_interfaces.insert(0, primary);
        }

        AgentCard {
            name: self.name,
            description: self.description,
            supported_interfaces,
            provider: self.provider.into(),
            version: self.version,
            documentation_url: self.documentation_url,
            capabilities: self.capabilities.unwrap_or_default().into(),
            security_schemes: self.security_schemes,
            security_requirements: self.security_requirements,
            default_input_modes: self.default_input_modes,
            default_output_modes: self.default_output_modes,
            skills: self.skills,
            signatures: self.signatures,
            icon_url: self.icon_url,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_scheme_api_key_serialization() {
        let scheme = SecurityScheme::api_key(
            "X-API-Key".to_string(),
            "header".to_string(),
            Some("API Key authentication".to_string()),
        );

        let json_value = serde_json::to_value(&scheme).expect("Failed to serialize SecurityScheme");
        // Verify output matches protobuf JSON mappings
        assert_eq!(json_value["apiKeySecurityScheme"]["location"], "header");
        assert_eq!(json_value["apiKeySecurityScheme"]["name"], "X-API-Key");
    }

    #[test]
    fn test_security_scheme_http_serialization() {
        let scheme = SecurityScheme::http(
            "bearer".to_string(),
            Some("JWT".to_string()),
            Some("Bearer token authentication".to_string()),
        );

        let json_value = serde_json::to_value(&scheme).expect("Failed to serialize SecurityScheme");
        assert_eq!(json_value["httpAuthSecurityScheme"]["scheme"], "bearer");
        assert_eq!(json_value["httpAuthSecurityScheme"]["bearerFormat"], "JWT");
    }

    #[test]
    fn test_security_scheme_mtls_serialization() {
        let scheme = SecurityScheme::mutual_tls(Some("Mutual TLS authentication".to_string()));

        let json_value = serde_json::to_value(&scheme).expect("Failed to serialize SecurityScheme");
        assert_eq!(
            json_value["mtlsSecurityScheme"]["description"],
            "Mutual TLS authentication"
        );
    }

    #[test]
    fn test_security_scheme_oauth2_with_metadata() {
        let flows = OAuthFlows::authorization_code(AuthorizationCodeOAuthFlow {
            authorization_url: "https://example.com/oauth/authorize".to_string(),
            token_url: "https://example.com/oauth/token".to_string(),
            refresh_url: String::new(),
            scopes: HashMap::new(),
            ..Default::default()
        });

        let scheme = SecurityScheme::oauth2(
            flows,
            Some("OAuth2 authentication".to_string()),
            Some("https://example.com/.well-known/oauth-authorization-server".to_string()),
        );

        let json_value = serde_json::to_value(&scheme).expect("Failed to serialize SecurityScheme");
        assert_eq!(
            json_value["oauth2SecurityScheme"]["oauth2MetadataUrl"],
            "https://example.com/.well-known/oauth-authorization-server"
        );
    }
}
