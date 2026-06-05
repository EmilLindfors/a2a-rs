//! Agent configuration with TOML support
//!
//! This module provides declarative configuration for A2A agents via TOML files.
//! It supports environment variable interpolation and sensible defaults.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to parse TOML: {0}")]
    TomlError(#[from] toml::de::Error),
    #[error("Environment variable not found: {0}")]
    EnvVarError(String),
    #[error("Invalid configuration: {0}")]
    ValidationError(String),
}

/// Complete agent configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Agent metadata
    pub agent: AgentMetadata,

    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Skills exposed by the agent
    #[serde(default)]
    pub skills: Vec<SkillConfig>,

    /// Features enabled for the agent
    #[serde(default)]
    pub features: FeaturesConfig,

    /// LLM Configuration
    #[serde(default)]
    pub llm: Option<LlmConfig>,
}

/// LLM Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// LLM Provider (e.g. "openai", "gemini")
    pub provider: String,
    /// API key for the LLM
    pub api_key: Option<String>,
    /// Model to use
    pub model: Option<String>,
    /// Base URL (for providers like openai that support local LLMs like ollama)
    pub base_url: Option<String>,
}

impl AgentConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    /// Parse configuration from TOML string
    pub fn from_toml(content: &str) -> Result<Self, ConfigError> {
        // Expand environment variables
        let expanded = expand_env_vars(content)?;
        let config: AgentConfig = toml::from_str(&expanded)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.agent.name.is_empty() {
            return Err(ConfigError::ValidationError(
                "Agent name cannot be empty".to_string(),
            ));
        }

        if !self.features.mcp_server.enabled && self.server.http_port == 0 {
            return Err(ConfigError::ValidationError(
                "The HTTP server port must be configured when MCP server is disabled".to_string(),
            ));
        }

        // Validate skills
        for skill in &self.skills {
            if skill.id.is_empty() {
                return Err(ConfigError::ValidationError(
                    "Skill ID cannot be empty".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Build agent card URL from server config
    pub fn agent_url(&self) -> String {
        format!("http://{}:{}", self.server.host, self.server.http_port)
    }
}

/// Agent metadata and identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    /// Agent name
    pub name: String,

    /// Agent description
    #[serde(default)]
    pub description: Option<String>,

    /// Agent version
    #[serde(default)]
    pub version: Option<String>,

    /// Provider information
    #[serde(default)]
    pub provider: Option<ProviderInfo>,

    /// Documentation URL
    #[serde(default)]
    pub documentation_url: Option<String>,

    /// The implementation handler to use for this agent (e.g. 'reimbursement', 'echo')
    /// Used primarily by the generic a2a binary.
    #[serde(default)]
    pub implementation: Option<String>,
}

/// Provider information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub url: String,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to
    #[serde(default = "default_host")]
    pub host: String,

    /// HTTP server port (0 to disable)
    #[serde(default = "default_http_port")]
    pub http_port: u16,

    /// Storage configuration
    #[serde(default)]
    pub storage: StorageConfig,

    /// Authentication configuration
    #[serde(default)]
    pub auth: AuthConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            http_port: default_http_port(),
            storage: StorageConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

/// Storage backend configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageConfig {
    /// In-memory storage (default)
    #[default]
    InMemory,

    /// SQLx-based persistent storage
    Sqlx {
        /// Database URL (supports env vars like ${DATABASE_URL})
        url: String,

        /// Maximum number of connections in the pool
        #[serde(default = "default_max_connections")]
        max_connections: u32,

        /// Enable SQL query logging
        #[serde(default)]
        enable_logging: bool,
    },
}

/// Authentication configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthConfig {
    /// No authentication (default for development)
    #[default]
    None,

    /// Bearer token authentication
    Bearer {
        /// List of valid tokens (supports env vars)
        tokens: Vec<String>,

        /// Optional bearer format description (e.g., "JWT")
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    /// API Key authentication
    ApiKey {
        /// Valid API keys
        keys: Vec<String>,

        /// Location of the API key: "header", "query", or "cookie"
        #[serde(default = "default_api_key_location")]
        location: String,

        /// Name of the header/query param/cookie
        #[serde(default = "default_api_key_name")]
        name: String,
    },

    /// JWT (JSON Web Token) authentication
    Jwt {
        /// JWT secret for HMAC algorithms (HS256, HS384, HS512)
        /// Use ${ENV_VAR} for environment variables
        #[serde(skip_serializing_if = "Option::is_none")]
        secret: Option<String>,

        /// RSA public key in PEM format for RSA algorithms (RS256, RS384, RS512)
        #[serde(skip_serializing_if = "Option::is_none")]
        rsa_pem_path: Option<String>,

        /// Algorithm to use (HS256, HS384, HS512, RS256, RS384, RS512)
        #[serde(default = "default_jwt_algorithm")]
        algorithm: String,

        /// Required issuer (iss claim)
        #[serde(skip_serializing_if = "Option::is_none")]
        issuer: Option<String>,

        /// Required audience (aud claim)
        #[serde(skip_serializing_if = "Option::is_none")]
        audience: Option<String>,
    },

    /// OAuth2 authentication
    OAuth2 {
        /// Client ID
        client_id: String,

        /// Client secret (use ${ENV_VAR} for environment variables)
        client_secret: String,

        /// Authorization URL
        authorization_url: String,

        /// Token URL
        token_url: String,

        /// Redirect URL for authorization code flow
        #[serde(skip_serializing_if = "Option::is_none")]
        redirect_url: Option<String>,

        /// OAuth2 flow type: "authorization_code" or "client_credentials"
        #[serde(default = "default_oauth2_flow")]
        flow: String,

        /// Required scopes
        #[serde(default)]
        scopes: Vec<String>,
    },
}

/// Skill configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    /// Unique skill identifier
    pub id: String,

    /// Human-readable skill name
    pub name: String,

    /// Skill description
    #[serde(default)]
    pub description: Option<String>,

    /// Keywords for skill discovery
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Example queries for this skill
    #[serde(default)]
    pub examples: Vec<String>,

    /// Supported input formats
    #[serde(default = "default_formats")]
    pub input_formats: Vec<String>,

    /// Supported output formats
    #[serde(default = "default_formats")]
    pub output_formats: Vec<String>,
}

/// Features configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfig {
    /// Enable streaming updates
    #[serde(default)]
    pub streaming: bool,

    /// Enable push notifications
    #[serde(default)]
    pub push_notifications: bool,

    /// Enable state transition history
    #[serde(default)]
    pub state_history: bool,

    /// Enable authenticated extended card
    #[serde(default)]
    pub authenticated_card: bool,

    /// Protocol extensions (AP2, etc.)
    #[serde(default)]
    pub extensions: ExtensionsConfig,

    /// MCP server configuration (expose agent as MCP server)
    #[serde(default)]
    pub mcp_server: McpServerConfig,

    /// MCP client configuration (connect to MCP servers to use their tools)
    #[serde(default)]
    pub mcp_client: McpClientConfig,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            streaming: true,
            push_notifications: true,
            state_history: true,
            authenticated_card: false,
            extensions: ExtensionsConfig::default(),
            mcp_server: McpServerConfig::default(),
            mcp_client: McpClientConfig::default(),
        }
    }
}

/// Protocol extensions configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtensionsConfig {
    /// AP2 (Agent Payments Protocol) extension
    #[serde(default)]
    pub ap2: Option<Ap2ExtensionConfig>,
}

/// AP2 extension configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ap2ExtensionConfig {
    /// AP2 roles this agent performs (merchant, shopper, credentials-provider, payment-processor)
    pub roles: Vec<String>,

    /// Whether clients must understand AP2 to interact with this agent
    #[serde(default)]
    pub required: bool,
}

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Enable MCP server (expose agent as MCP tools)
    #[serde(default)]
    pub enabled: bool,

    /// Use stdio transport (for Claude Desktop integration).
    ///
    /// Ignored when [`http.enabled`](McpHttpConfig::enabled) is set — the HTTP
    /// (Streamable HTTP) transport takes precedence, since a single process
    /// cannot own stdin/stdout for stdio and bind a socket at the same time.
    #[serde(default = "default_true")]
    pub stdio: bool,

    /// Streamable HTTP transport (for networked MCP clients).
    #[serde(default)]
    pub http: McpHttpConfig,

    /// Server name (defaults to agent name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Server version (defaults to agent version)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            stdio: true,
            http: McpHttpConfig::default(),
            name: None,
            version: None,
        }
    }
}

/// Streamable HTTP transport configuration for the MCP server.
///
/// When [`enabled`](Self::enabled), the agent is served over MCP's Streamable
/// HTTP transport (`rmcp`'s `StreamableHttpService`) instead of stdio, mounted
/// at [`path`](Self::path) on `host:port`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpHttpConfig {
    /// Serve the MCP server over Streamable HTTP rather than stdio.
    #[serde(default)]
    pub enabled: bool,

    /// Host/interface to bind to.
    #[serde(default = "default_mcp_http_host")]
    pub host: String,

    /// TCP port to bind to.
    #[serde(default = "default_mcp_http_port")]
    pub port: u16,

    /// URL path the Streamable HTTP endpoint is mounted at.
    #[serde(default = "default_mcp_http_path")]
    pub path: String,

    /// Hostnames / `host:port` authorities accepted in the inbound `Host`
    /// header (DNS-rebinding protection).
    ///
    /// * Omitted → the secure default: loopback only (`localhost`, `127.0.0.1`,
    ///   `::1`).
    /// * `[]` → disable `Host` validation entirely (allow any host — required
    ///   for public binds, but **not recommended** without an upstream proxy).
    /// * Non-empty → only the listed authorities are accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_hosts: Option<Vec<String>>,

    /// Browser `Origin` values accepted on inbound requests.
    ///
    /// * Omitted (or `[]`) → `Origin` validation disabled (the rmcp default).
    /// * Non-empty → requests carrying an `Origin` must match one of these per
    ///   RFC 6454 `(scheme, host, port)`; entries must include a scheme (e.g.
    ///   `https://app.example.com`). Requests without an `Origin` still pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<String>>,
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_mcp_http_host(),
            port: default_mcp_http_port(),
            path: default_mcp_http_path(),
            allowed_hosts: None,
            allowed_origins: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_mcp_http_host() -> String {
    "127.0.0.1".to_string()
}

fn default_mcp_http_port() -> u16 {
    8000
}

fn default_mcp_http_path() -> String {
    "/mcp".to_string()
}

/// MCP client configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpClientConfig {
    /// Enable MCP client (connect to MCP servers to use their tools)
    #[serde(default)]
    pub enabled: bool,

    /// MCP servers to connect to
    #[serde(default)]
    pub servers: Vec<McpServerConnection>,
}

/// Configuration for connecting to an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConnection {
    /// Unique name for this MCP server
    pub name: String,

    /// Command to run to start the MCP server
    pub command: String,

    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables to set
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    /// Working directory for the command
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

// Default value functions

fn default_host() -> String {
    std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

fn default_http_port() -> u16 {
    std::env::var("HTTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080)
}

fn default_max_connections() -> u32 {
    10
}

fn default_jwt_algorithm() -> String {
    "HS256".to_string()
}

fn default_oauth2_flow() -> String {
    "authorization_code".to_string()
}

fn default_api_key_location() -> String {
    "header".to_string()
}

fn default_api_key_name() -> String {
    "X-API-Key".to_string()
}

fn default_formats() -> Vec<String> {
    vec!["text".to_string(), "data".to_string()]
}

/// Expand environment variables in the config string
/// Supports ${VAR_NAME} and ${VAR_NAME:-default} syntax
fn expand_env_vars(content: &str) -> Result<String, ConfigError> {
    use std::sync::LazyLock;
    static ENV_VAR_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap());

    let mut result = content.to_string();
    let re = &*ENV_VAR_RE;

    for cap in re.captures_iter(content) {
        let full_match = &cap[0];
        let var_name = &cap[1];

        let value =
            std::env::var(var_name).map_err(|_| ConfigError::EnvVarError(var_name.to_string()))?;

        result = result.replace(full_match, &value);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_config() {
        let toml = r#"
            [agent]
            name = "Test Agent"
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(config.agent.name, "Test Agent");
        assert_eq!(config.server.http_port, 8080);
    }

    #[test]
    fn test_complete_config() {
        let toml = r#"
            [agent]
            name = "Reimbursement Agent"
            description = "Handles employee reimbursements"
            version = "1.0.0"

            [agent.provider]
            name = "Example Corp"
            url = "https://example.com"

            [server]
            host = "0.0.0.0"
            http_port = 3000

            [server.storage]
            type = "sqlx"
            url = "sqlite:test.db"
            max_connections = 5
            enable_logging = true

            [server.auth]
            type = "bearer"
            tokens = ["token123"]
            format = "JWT"

            [[skills]]
            id = "process_expense"
            name = "Process Expense"
            description = "Process expense reimbursements"
            keywords = ["expense", "reimbursement"]
            examples = ["Reimburse my $50 lunch"]
            input_formats = ["text", "data"]
            output_formats = ["text", "data"]

            [features]
            streaming = true
            push_notifications = true
            state_history = true
            authenticated_card = false
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(config.agent.name, "Reimbursement Agent");
        assert_eq!(config.server.http_port, 3000);
        assert_eq!(config.skills.len(), 1);
        assert_eq!(config.skills[0].id, "process_expense");
        assert!(config.features.streaming);
    }

    #[test]
    fn test_env_var_expansion() {
        // SAFETY: This is a test function run in a controlled environment
        // We're setting an environment variable that won't affect other tests
        unsafe {
            std::env::set_var("TEST_TOKEN", "secret123");
        }

        let content = r#"
            [server.auth]
            type = "bearer"
            tokens = ["${TEST_TOKEN}"]
        "#;

        let expanded = expand_env_vars(content).unwrap();
        assert!(expanded.contains("secret123"));
    }

    #[test]
    #[cfg(feature = "auth")]
    fn test_jwt_auth_config() {
        let toml = r#"
            [agent]
            name = "JWT Agent"

            [server.auth]
            type = "jwt"
            secret = "my-jwt-secret"
            algorithm = "HS256"
            issuer = "https://auth.example.com"
            audience = "api://my-agent"
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        match &config.server.auth {
            AuthConfig::Jwt {
                secret,
                algorithm,
                issuer,
                audience,
                ..
            } => {
                assert_eq!(secret.as_ref().unwrap(), "my-jwt-secret");
                assert_eq!(algorithm, "HS256");
                assert_eq!(issuer.as_ref().unwrap(), "https://auth.example.com");
                assert_eq!(audience.as_ref().unwrap(), "api://my-agent");
            }
            _ => panic!("Expected JWT auth config"),
        }
    }

    #[test]
    #[cfg(feature = "auth")]
    fn test_oauth2_auth_config() {
        let toml = r#"
            [agent]
            name = "OAuth2 Agent"

            [server.auth]
            type = "oauth2"
            client_id = "my-client-id"
            client_secret = "my-client-secret"
            authorization_url = "https://provider.com/auth"
            token_url = "https://provider.com/token"
            flow = "authorization_code"
            scopes = ["read", "write"]
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        match &config.server.auth {
            AuthConfig::OAuth2 {
                client_id,
                client_secret,
                flow,
                scopes,
                ..
            } => {
                assert_eq!(client_id, "my-client-id");
                assert_eq!(client_secret, "my-client-secret");
                assert_eq!(flow, "authorization_code");
                assert_eq!(scopes.len(), 2);
                assert_eq!(scopes[0], "read");
            }
            _ => panic!("Expected OAuth2 auth config"),
        }
    }

    #[test]
    fn test_validation_empty_name() {
        let toml = r#"
            [agent]
            name = ""
        "#;

        let result = AgentConfig::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_ap2_extension_config() {
        let toml = r#"
            [agent]
            name = "Merchant Agent"

            [features.extensions.ap2]
            roles = ["merchant", "payment-processor"]
            required = true
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        let ap2 = config.features.extensions.ap2.unwrap();
        assert_eq!(ap2.roles, vec!["merchant", "payment-processor"]);
        assert!(ap2.required);
    }

    #[test]
    fn test_mcp_http_config() {
        let toml = r#"
            [agent]
            name = "HTTP MCP Agent"

            [server]
            http_port = 0

            [features.mcp_server]
            enabled = true
            stdio = false

            [features.mcp_server.http]
            enabled = true
            host = "0.0.0.0"
            port = 9000
            path = "/rpc"
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        let http = &config.features.mcp_server.http;
        assert!(http.enabled);
        assert_eq!(http.host, "0.0.0.0");
        assert_eq!(http.port, 9000);
        assert_eq!(http.path, "/rpc");
        // Security knobs omitted → None (keep rmcp's loopback-only default).
        assert!(http.allowed_hosts.is_none());
        assert!(http.allowed_origins.is_none());
    }

    #[test]
    fn test_mcp_http_security_knobs() {
        let toml = r#"
            [agent]
            name = "Public MCP Agent"

            [server]
            http_port = 0

            [features.mcp_server]
            enabled = true

            [features.mcp_server.http]
            enabled = true
            allowed_hosts = ["mcp.example.com", "mcp.example.com:8000"]
            allowed_origins = ["https://app.example.com"]
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        let http = &config.features.mcp_server.http;
        assert_eq!(
            http.allowed_hosts.as_deref(),
            Some(
                [
                    "mcp.example.com".to_string(),
                    "mcp.example.com:8000".to_string()
                ]
                .as_slice()
            )
        );
        assert_eq!(
            http.allowed_origins.as_deref(),
            Some(["https://app.example.com".to_string()].as_slice())
        );
    }

    #[test]
    fn test_mcp_http_disable_host_validation() {
        // An explicit empty list parses as Some([]) — distinct from omission —
        // and disables Host validation at the transport layer.
        let toml = r#"
            [agent]
            name = "Open MCP Agent"

            [server]
            http_port = 0

            [features.mcp_server]
            enabled = true

            [features.mcp_server.http]
            enabled = true
            allowed_hosts = []
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(
            config.features.mcp_server.http.allowed_hosts.as_deref(),
            Some([].as_slice())
        );
    }

    #[test]
    fn test_mcp_http_config_defaults() {
        // Omitting [features.mcp_server.http] leaves HTTP disabled with sane defaults.
        let toml = r#"
            [agent]
            name = "Stdio MCP Agent"

            [server]
            http_port = 0

            [features.mcp_server]
            enabled = true
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        let mcp = &config.features.mcp_server;
        assert!(mcp.stdio);
        assert!(!mcp.http.enabled);
        assert_eq!(mcp.http.host, "127.0.0.1");
        assert_eq!(mcp.http.port, 8000);
        assert_eq!(mcp.http.path, "/mcp");
    }

    #[test]
    fn test_ap2_extension_config_optional() {
        let toml = r#"
            [agent]
            name = "Plain Agent"
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        assert!(config.features.extensions.ap2.is_none());
    }
}
