//! Agent runtime for managing server lifecycle
//!
//! The runtime handles starting HTTP/WebSocket servers, wiring components,
//! and managing the agent lifecycle based on configuration.

#[cfg(feature = "mcp-client")]
use crate::core::McpClientManager;
use crate::core::config::{AgentConfig, AuthConfig, StorageConfig};
use a2a_rs::adapter::{
    BearerTokenAuthenticator, ConnectRpcAdapter, HttpServer, SimpleAgentInfo,
};
use a2a_rs::port::{
    AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskLifecycle,
    AsyncTaskQuery,
};
use std::sync::Arc;
use tracing::{info, warn};

#[cfg(feature = "auth")]
use a2a_rs::adapter::{JwtAuthenticator, OAuth2Authenticator};
#[cfg(feature = "auth")]
use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};
#[cfg(feature = "auth")]
use std::collections::HashMap;

/// Agent runtime that manages the server lifecycle
pub struct AgentRuntime<H, S> {
    config: AgentConfig,
    handler: Arc<H>,
    storage: Arc<S>,
    #[cfg(feature = "mcp-client")]
    mcp_client: Option<McpClientManager>,
}

impl<H, S> AgentRuntime<H, S>
where
    H: AsyncMessageHandler + Clone + Send + Sync + 'static,
    S: AsyncTaskLifecycle
        + AsyncTaskQuery
        + AsyncNotificationManager
        + Clone
        + Send
        + Sync
        + 'static,
{
    /// Create a new runtime
    pub fn new(config: AgentConfig, handler: Arc<H>, storage: Arc<S>) -> Self {
        Self {
            config,
            handler,
            storage,
            #[cfg(feature = "mcp-client")]
            mcp_client: None,
        }
    }

    /// Create a new runtime with MCP client
    #[cfg(feature = "mcp-client")]
    pub fn with_mcp_client(
        config: AgentConfig,
        handler: Arc<H>,
        storage: Arc<S>,
        mcp_client: McpClientManager,
    ) -> Self {
        Self {
            config,
            handler,
            storage,
            mcp_client: Some(mcp_client),
        }
    }

    /// Get the MCP client manager (if enabled)
    #[cfg(feature = "mcp-client")]
    pub fn mcp_client(&self) -> Option<&McpClientManager> {
        self.mcp_client.as_ref()
    }

    /// Build agent info from configuration
    fn build_agent_info(&self, base_url: String) -> SimpleAgentInfo {
        let mut agent_info = SimpleAgentInfo::new(self.config.agent.name.clone(), base_url);

        if let Some(ref description) = self.config.agent.description {
            agent_info = agent_info.with_description(description.clone());
        }

        if let Some(ref provider) = self.config.agent.provider {
            agent_info = agent_info.with_provider(provider.name.clone(), provider.url.clone());
        }

        if let Some(ref doc_url) = self.config.agent.documentation_url {
            agent_info = agent_info.with_documentation_url(doc_url.clone());
        }

        if let Some(ref version) = self.config.agent.version {
            agent_info = agent_info.with_version(version.clone());
        }

        // Map AuthConfig into AgentCard security schemes
        let mut schemes = std::collections::HashMap::new();
        match &self.config.server.auth {
            crate::core::config::AuthConfig::None => {}
            crate::core::config::AuthConfig::Bearer { format, .. } => {
                schemes.insert(
                    "bearer".to_string(),
                    a2a_rs::domain::SecurityScheme::http(
                        "bearer".to_string(),
                        format.clone(),
                        Some("Bearer token authentication".to_string()),
                    ),
                );
            }
            crate::core::config::AuthConfig::ApiKey { location, name, .. } => {
                schemes.insert(
                    "api_key".to_string(),
                    a2a_rs::domain::SecurityScheme::api_key(
                        name.clone(),
                        location.clone(),
                        Some("API Key authentication".to_string()),
                    ),
                );
            }
            crate::core::config::AuthConfig::Jwt {
                issuer, audience, ..
            } => {
                schemes.insert(
                    "jwt".to_string(),
                    a2a_rs::domain::SecurityScheme::http(
                        "bearer".to_string(),
                        Some("JWT".to_string()),
                        Some(format!(
                            "JWT authentication (issuer: {:?}, audience: {:?})",
                            issuer, audience
                        )),
                    ),
                );
            }
            crate::core::config::AuthConfig::OAuth2 {
                flow,
                token_url,
                authorization_url,
                scopes,
                ..
            } => {
                let scopes_map: std::collections::HashMap<String, String> =
                    scopes.iter().map(|s| (s.clone(), s.clone())).collect();
                let flows = if flow == "client_credentials" {
                    a2a_rs::domain::OAuthFlows::client_credentials(
                        a2a_rs::domain::ClientCredentialsOAuthFlow {
                            token_url: token_url.clone(),
                            refresh_url: String::new(),
                            scopes: scopes_map,
                            ..Default::default()
                        },
                    )
                } else {
                    a2a_rs::domain::OAuthFlows::authorization_code(
                        a2a_rs::domain::AuthorizationCodeOAuthFlow {
                            authorization_url: authorization_url.clone(),
                            token_url: token_url.clone(),
                            refresh_url: String::new(),
                            scopes: scopes_map,
                            ..Default::default()
                        },
                    )
                };
                schemes.insert(
                    "oauth2".to_string(),
                    a2a_rs::domain::SecurityScheme::oauth2(
                        flows,
                        Some("OAuth2 authentication".to_string()),
                        None,
                    ),
                );
            }
        }
        if !schemes.is_empty() {
            agent_info = agent_info.with_security_schemes(schemes);
        }

        // Add features
        if self.config.features.streaming {
            agent_info = agent_info.with_streaming();
        }

        if self.config.features.push_notifications {
            agent_info = agent_info.with_push_notifications();
        }

        if self.config.features.state_history {
            agent_info = agent_info.with_state_transition_history();
        }

        if self.config.features.authenticated_card {
            agent_info = agent_info.with_authenticated_extended_card();
        }

        // Add extensions
        if let Some(ref ap2_config) = self.config.features.extensions.ap2 {
            let roles_json: Vec<serde_json::Value> = ap2_config
                .roles
                .iter()
                .map(|r| serde_json::Value::String(r.clone()))
                .collect();

            let mut params = std::collections::HashMap::new();
            params.insert("roles".to_string(), serde_json::Value::Array(roles_json));
            let params_val = serde_json::Value::Object(params.into_iter().collect());
            let params_struct: buffa_types::google::protobuf::Struct =
                serde_json::from_value(params_val).unwrap_or_default();

            let ext = a2a_rs::domain::AgentExtension {
                uri: "https://github.com/google-agentic-commerce/ap2/tree/v0.1".to_string(),
                description: "Agent Payments Protocol (AP2) v0.1".to_string(),
                required: ap2_config.required,
                params: buffa::MessageField::some(params_struct),
                ..Default::default()
            };

            agent_info = agent_info.add_extension(ext);
            info!("💳 AP2 extension enabled (roles: {:?})", ap2_config.roles);
        }

        // Add skills
        for skill in &self.config.skills {
            agent_info = agent_info.add_comprehensive_skill(
                skill.id.clone(),
                skill.name.clone(),
                skill.description.clone(),
                if skill.keywords.is_empty() {
                    None
                } else {
                    Some(skill.keywords.clone())
                },
                if skill.examples.is_empty() {
                    None
                } else {
                    Some(skill.examples.clone())
                },
                Some(skill.input_formats.clone()),
                Some(skill.output_formats.clone()),
            );
        }

        agent_info
    }

    /// Start HTTP server
    pub async fn start_http(&self) -> Result<(), RuntimeError> {
        if self.config.server.http_port == 0 {
            return Err(RuntimeError::ServerNotConfigured(
                "HTTP port is 0".to_string(),
            ));
        }

        let base_url = format!(
            "http://{}:{}",
            self.config.server.host, self.config.server.http_port
        );
        let agent_info = self.build_agent_info(base_url);

        let processor = ConnectRpcAdapter::new(
            (*self.handler).clone(),
            (*self.storage).clone(),
            (*self.storage).clone(),
            agent_info.clone(),
        );

        let bind_address = format!(
            "{}:{}",
            self.config.server.host, self.config.server.http_port
        );

        info!("🌐 Starting HTTP server on {}", bind_address);
        self.print_agent_info("HTTP", &self.config.server.http_port.to_string());

        match &self.config.server.auth {
            AuthConfig::None => {
                let server = HttpServer::new(processor, agent_info, bind_address);
                server
                    .start()
                    .await
                    .map_err(|e| RuntimeError::ServerError(e.to_string()))
            }
            AuthConfig::Bearer { tokens, format } => {
                info!(
                    "🔐 Authentication: Bearer token ({} token(s){})",
                    tokens.len(),
                    format
                        .as_ref()
                        .map(|f| format!(", format: {}", f))
                        .unwrap_or_default()
                );
                let authenticator = BearerTokenAuthenticator::new(tokens.clone());
                let server =
                    HttpServer::with_auth(processor, agent_info, bind_address, authenticator);
                server
                    .start()
                    .await
                    .map_err(|e| RuntimeError::ServerError(e.to_string()))
            }
            AuthConfig::ApiKey {
                keys,
                location,
                name,
            } => {
                warn!(
                    "🔐 API key authentication configured ({} {}, {} key(s)) but not yet supported, using no auth",
                    location,
                    name,
                    keys.len()
                );
                let server = HttpServer::new(processor, agent_info, bind_address);
                server
                    .start()
                    .await
                    .map_err(|e| RuntimeError::ServerError(e.to_string()))
            }
            #[cfg(feature = "auth")]
            AuthConfig::Jwt {
                secret,
                rsa_pem_path,
                algorithm,
                issuer,
                audience,
            } => {
                info!("🔐 Authentication: JWT (algorithm: {})", algorithm);

                let mut authenticator = if let Some(secret) = secret {
                    JwtAuthenticator::new_with_secret(secret.as_bytes())
                } else if let Some(pem_path) = rsa_pem_path {
                    let pem_data = std::fs::read(pem_path).map_err(|e| {
                        RuntimeError::ServerError(format!("Failed to read RSA PEM file: {}", e))
                    })?;
                    JwtAuthenticator::new_with_rsa_pem(&pem_data).map_err(|e| {
                        RuntimeError::ServerError(format!(
                            "Failed to create JWT authenticator: {}",
                            e
                        ))
                    })?
                } else {
                    return Err(RuntimeError::ServerError(
                        "JWT authentication requires either 'secret' or 'rsa_pem_path'".to_string(),
                    ));
                };

                if let Some(iss) = issuer {
                    authenticator = authenticator.with_issuer(iss.clone());
                    info!("   Issuer: {}", iss);
                }
                if let Some(aud) = audience {
                    authenticator = authenticator.with_audience(aud.clone());
                    info!("   Audience: {}", aud);
                }

                let server =
                    HttpServer::with_auth(processor, agent_info, bind_address, authenticator);
                server
                    .start()
                    .await
                    .map_err(|e| RuntimeError::ServerError(e.to_string()))
            }
            #[cfg(not(feature = "auth"))]
            AuthConfig::Jwt { .. } => Err(RuntimeError::ServerError(
                "JWT authentication requires the 'auth' feature to be enabled".to_string(),
            )),
            #[cfg(feature = "auth")]
            AuthConfig::OAuth2 {
                client_id,
                client_secret,
                authorization_url,
                token_url,
                redirect_url,
                flow,
                scopes,
            } => {
                info!("🔐 Authentication: OAuth2 (flow: {})", flow);
                info!("   Authorization URL: {}", authorization_url);
                info!("   Token URL: {}", token_url);

                let client_id = ClientId::new(client_id.clone());
                let client_secret = ClientSecret::new(client_secret.clone());
                let auth_url = AuthUrl::new(authorization_url.clone()).map_err(|e| {
                    RuntimeError::ServerError(format!("Invalid authorization URL: {}", e))
                })?;
                let token_url = TokenUrl::new(token_url.clone())
                    .map_err(|e| RuntimeError::ServerError(format!("Invalid token URL: {}", e)))?;

                let scopes_map: HashMap<String, String> =
                    scopes.iter().map(|s| (s.clone(), s.clone())).collect();

                let authenticator = if flow == "client_credentials" {
                    OAuth2Authenticator::new_client_credentials(
                        client_id,
                        client_secret,
                        token_url,
                        scopes_map,
                    )
                } else {
                    // Authorization code flow
                    let redirect_url = RedirectUrl::new(
                        redirect_url
                            .clone()
                            .unwrap_or_else(|| "http://localhost:8080/callback".to_string()),
                    )
                    .map_err(|e| {
                        RuntimeError::ServerError(format!("Invalid redirect URL: {}", e))
                    })?;

                    info!("   Redirect URL: {}", redirect_url.as_str());

                    OAuth2Authenticator::new_authorization_code(
                        client_id,
                        Some(client_secret),
                        auth_url,
                        token_url,
                        redirect_url,
                        scopes_map,
                    )
                };

                let server =
                    HttpServer::with_auth(processor, agent_info, bind_address, authenticator);
                server
                    .start()
                    .await
                    .map_err(|e| RuntimeError::ServerError(e.to_string()))
            }
            #[cfg(not(feature = "auth"))]
            AuthConfig::OAuth2 { .. } => Err(RuntimeError::ServerError(
                "OAuth2 authentication requires the 'auth' feature to be enabled".to_string(),
            )),
        }
    }

    /// Start the appropriate server(s) based on configuration
    pub async fn run(self) -> Result<(), RuntimeError>
    where
        S: AsyncStreamingHandler,
    {
        // Check if MCP server mode is enabled
        if self.config.features.mcp_server.enabled {
            return self.run_as_mcp_server().await;
        }

        // Normal A2A server mode
        if self.config.server.http_port > 0 {
            self.start_http().await
        } else {
            Err(RuntimeError::ServerNotConfigured(
                "No servers configured".to_string(),
            ))
        }
    }

    /// Run agent as MCP server.
    ///
    /// The MCP bridge calls the configured [`AsyncMessageHandler`] in-process,
    /// so no HTTP server is spawned. HTTP-only concerns like
    /// [`AuthConfig`] are not applicable here — if you also want a secured
    /// HTTP surface, run a normal `start_http()` instance in a separate
    /// process or task.
    async fn run_as_mcp_server(self) -> Result<(), RuntimeError> {
        use crate::core::mcp;
        use a2a_rs::services::AgentInfoProvider;

        info!("🔌 Running agent in MCP server mode");

        // `base_url` is what gets stamped onto the agent card (and therefore
        // into the MCP tool namespace). MCP stdio has no listener of its own,
        // so this is purely advertising — it doesn't need to be reachable.
        let base_url = format!(
            "http://{}:{}",
            self.config.server.host, self.config.server.http_port
        );

        let agent_info = self.build_agent_info(base_url);
        let agent_card = agent_info
            .get_agent_card()
            .await
            .map_err(|e| RuntimeError::ServerError(format!("Failed to get agent card: {}", e)))?;

        let handler = (*self.handler).clone();
        mcp::run_mcp_server(&self.config.features.mcp_server, agent_card, handler)
            .await
            .map_err(|e| RuntimeError::ServerError(format!("MCP server error: {}", e)))
    }

    /// Print agent information
    fn print_agent_info(&self, server_type: &str, port: &str) {
        info!("📋 Agent: {}", self.config.agent.name);
        if let Some(ref desc) = self.config.agent.description {
            info!("   Description: {}", desc);
        }
        info!("   {} port: {}", server_type, port);

        match &self.config.server.storage {
            StorageConfig::InMemory => info!("💾 Storage: In-memory (non-persistent)"),
            StorageConfig::Sqlx { url, .. } => info!("💾 Storage: SQLx ({})", url),
        }

        if !self.config.skills.is_empty() {
            info!("🛠️  Skills: {}", self.config.skills.len());
            for skill in &self.config.skills {
                info!("   - {} ({})", skill.name, skill.id);
            }
        }
    }
}

/// Runtime errors
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Server not configured: {0}")]
    ServerNotConfigured(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Storage error: {0}")]
    StorageError(String),
}
