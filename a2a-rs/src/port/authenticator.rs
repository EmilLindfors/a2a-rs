//! Authentication port - defines the interface for authentication in the domain

use async_trait::async_trait;
use std::collections::HashMap;

use crate::domain::{core::agent::SecurityScheme, A2AError};

/// Authentication context containing credentials and metadata
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// The authentication scheme type (e.g., "bearer", "apikey", "oauth2")
    pub scheme_type: String,
    /// The credential value (token, api key, etc)
    pub credential: String,
    /// Additional context (e.g., location for API key, scopes for OAuth2)
    pub metadata: HashMap<String, String>,
}

impl AuthContext {
    /// Create a new authentication context
    pub fn new(scheme_type: String, credential: String) -> Self {
        Self {
            scheme_type,
            credential,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get a metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

/// Port interface for authentication handlers
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Authenticate a request based on the provided context
    async fn authenticate(&self, context: &AuthContext) -> Result<AuthPrincipal, A2AError>;

    /// Get the security scheme configuration
    fn security_scheme(&self) -> &SecurityScheme;

    /// Validate that the context matches this authenticator's scheme
    fn validate_context(&self, context: &AuthContext) -> Result<(), A2AError>;
}

/// Represents an authenticated principal
#[derive(Debug, Clone)]
pub struct AuthPrincipal {
    /// Unique identifier for the authenticated entity
    pub id: String,
    /// The authentication scheme used
    pub scheme: String,
    /// Additional claims or attributes
    pub attributes: HashMap<String, String>,
}

impl AuthPrincipal {
    /// Create a new authenticated principal
    pub fn new(id: String, scheme: String) -> Self {
        Self {
            id,
            scheme,
            attributes: HashMap::new(),
        }
    }

    /// Add an attribute to the principal
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.insert(key, value);
        self
    }
}

/// Port interface for authentication context extraction
#[async_trait]
pub trait AuthContextExtractor: Send + Sync {
    /// Extract authentication context from HTTP headers
    #[cfg(feature = "http-server")]
    async fn extract_from_headers(&self, headers: &axum::http::HeaderMap) -> Option<AuthContext>;

    /// Extract authentication context from headers (generic version)
    #[cfg(not(feature = "http-server"))]
    async fn extract_from_headers(
        &self,
        headers: &std::collections::HashMap<String, String>,
    ) -> Option<AuthContext>;

    /// Extract authentication context from query parameters
    async fn extract_from_query(&self, params: &HashMap<String, String>) -> Option<AuthContext>;

    /// Extract authentication context from cookies
    async fn extract_from_cookies(&self, cookies: &str) -> Option<AuthContext>;
}

/// Composite authenticator that tries multiple authentication methods
#[async_trait]
pub trait CompositeAuthenticator: Send + Sync {
    /// Try to authenticate using any available scheme
    async fn authenticate_any(&self, contexts: Vec<AuthContext>)
        -> Result<AuthPrincipal, A2AError>;

    /// Get all supported security schemes
    fn supported_schemes(&self) -> Vec<&SecurityScheme>;
}
