//! Authentication adapter implementations

use std::collections::HashMap;
#[cfg(feature = "http-server")]
use std::sync::Arc;

use async_trait::async_trait;
#[cfg(feature = "http-server")]
use axum::{
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::Response,
};

#[cfg(not(feature = "http-server"))]
type HeaderMap = std::collections::HashMap<String, String>;

use crate::{
    domain::{core::agent::SecurityScheme, A2AError},
    port::authenticator::{AuthContext, AuthContextExtractor, AuthPrincipal, Authenticator},
};

/// HTTP Bearer token authenticator
#[derive(Clone)]
pub struct BearerTokenAuthenticator {
    /// The valid tokens
    tokens: Vec<String>,
    /// The security scheme configuration
    scheme: SecurityScheme,
}

impl BearerTokenAuthenticator {
    /// Create a new bearer token authenticator with the given tokens
    pub fn new(tokens: Vec<String>) -> Self {
        Self {
            tokens,
            scheme: SecurityScheme::Http {
                scheme: "bearer".to_string(),
                bearer_format: None,
                description: Some("Bearer token authentication".to_string()),
            },
        }
    }

    /// Create with a specific bearer format
    pub fn with_format(tokens: Vec<String>, format: String) -> Self {
        Self {
            tokens,
            scheme: SecurityScheme::Http {
                scheme: "bearer".to_string(),
                bearer_format: Some(format),
                description: Some("Bearer token authentication".to_string()),
            },
        }
    }
}

#[async_trait]
impl Authenticator for BearerTokenAuthenticator {
    async fn authenticate(&self, context: &AuthContext) -> Result<AuthPrincipal, A2AError> {
        self.validate_context(context)?;

        if self.tokens.contains(&context.credential) {
            Ok(AuthPrincipal::new(
                context.credential.clone(),
                "bearer".to_string(),
            ))
        } else {
            Err(A2AError::Internal(
                "Invalid authentication token".to_string(),
            ))
        }
    }

    fn security_scheme(&self) -> &SecurityScheme {
        &self.scheme
    }

    fn validate_context(&self, context: &AuthContext) -> Result<(), A2AError> {
        if context.scheme_type != "bearer" {
            return Err(A2AError::Internal(format!(
                "Invalid authentication scheme: expected 'bearer', got '{}'",
                context.scheme_type
            )));
        }
        Ok(())
    }
}

/// HTTP context extractor for Bearer tokens
#[derive(Clone)]
pub struct BearerTokenExtractor;

#[async_trait]
impl AuthContextExtractor for BearerTokenExtractor {
    #[cfg(feature = "http-server")]
    async fn extract_from_headers(&self, headers: &HeaderMap) -> Option<AuthContext> {
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|auth| {
                let parts: Vec<&str> = auth.splitn(2, ' ').collect();
                if parts.len() == 2 && parts[0].to_lowercase() == "bearer" {
                    Some(AuthContext::new("bearer".to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
    }

    #[cfg(not(feature = "http-server"))]
    async fn extract_from_headers(&self, headers: &HeaderMap) -> Option<AuthContext> {
        headers
            .get("authorization")
            .or_else(|| headers.get("Authorization"))
            .and_then(|auth| {
                let parts: Vec<&str> = auth.splitn(2, ' ').collect();
                if parts.len() == 2 && parts[0].to_lowercase() == "bearer" {
                    Some(AuthContext::new("bearer".to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
    }

    async fn extract_from_query(&self, _params: &HashMap<String, String>) -> Option<AuthContext> {
        // Bearer tokens are not typically passed in query parameters
        None
    }

    async fn extract_from_cookies(&self, _cookies: &str) -> Option<AuthContext> {
        // Bearer tokens are not typically passed in cookies
        None
    }
}

/// API Key authenticator
#[derive(Clone)]
pub struct ApiKeyAuthenticator {
    /// Valid API keys
    api_keys: Vec<String>,
    /// The security scheme configuration
    scheme: SecurityScheme,
}

impl ApiKeyAuthenticator {
    /// Create a new API key authenticator
    pub fn new(api_keys: Vec<String>, location: String, name: String) -> Self {
        Self {
            api_keys,
            scheme: SecurityScheme::ApiKey {
                location,
                name,
                description: Some("API key authentication".to_string()),
            },
        }
    }

    /// Create for header-based API key
    pub fn header(api_keys: Vec<String>, header_name: String) -> Self {
        Self::new(api_keys, "header".to_string(), header_name)
    }

    /// Create for query parameter-based API key
    pub fn query(api_keys: Vec<String>, param_name: String) -> Self {
        Self::new(api_keys, "query".to_string(), param_name)
    }

    /// Create for cookie-based API key
    pub fn cookie(api_keys: Vec<String>, cookie_name: String) -> Self {
        Self::new(api_keys, "cookie".to_string(), cookie_name)
    }
}

#[async_trait]
impl Authenticator for ApiKeyAuthenticator {
    async fn authenticate(&self, context: &AuthContext) -> Result<AuthPrincipal, A2AError> {
        self.validate_context(context)?;

        if self.api_keys.contains(&context.credential) {
            Ok(
                AuthPrincipal::new(context.credential.clone(), "apikey".to_string())
                    .with_attribute(
                        "location".to_string(),
                        context
                            .metadata
                            .get("location")
                            .unwrap_or(&String::new())
                            .clone(),
                    ),
            )
        } else {
            Err(A2AError::Internal("Invalid API key".to_string()))
        }
    }

    fn security_scheme(&self) -> &SecurityScheme {
        &self.scheme
    }

    fn validate_context(&self, context: &AuthContext) -> Result<(), A2AError> {
        if context.scheme_type != "apikey" {
            return Err(A2AError::Internal(format!(
                "Invalid authentication scheme: expected 'apikey', got '{}'",
                context.scheme_type
            )));
        }
        Ok(())
    }
}

/// API Key context extractor
#[derive(Clone)]
pub struct ApiKeyExtractor {
    location: String,
    name: String,
}

impl ApiKeyExtractor {
    pub fn new(location: String, name: String) -> Self {
        Self { location, name }
    }
}

#[async_trait]
impl AuthContextExtractor for ApiKeyExtractor {
    #[cfg(feature = "http-server")]
    async fn extract_from_headers(&self, headers: &HeaderMap) -> Option<AuthContext> {
        if self.location != "header" {
            return None;
        }

        headers
            .get(axum::http::HeaderName::from_bytes(self.name.as_bytes()).ok()?)
            .and_then(|h| h.to_str().ok())
            .map(|value| {
                AuthContext::new("apikey".to_string(), value.to_string())
                    .with_metadata("location".to_string(), "header".to_string())
                    .with_metadata("name".to_string(), self.name.clone())
            })
    }

    #[cfg(not(feature = "http-server"))]
    async fn extract_from_headers(&self, headers: &HeaderMap) -> Option<AuthContext> {
        if self.location != "header" {
            return None;
        }

        headers.get(&self.name).map(|value| {
            AuthContext::new("apikey".to_string(), value.clone())
                .with_metadata("location".to_string(), "header".to_string())
                .with_metadata("name".to_string(), self.name.clone())
        })
    }

    async fn extract_from_query(&self, params: &HashMap<String, String>) -> Option<AuthContext> {
        if self.location != "query" {
            return None;
        }

        params.get(&self.name).map(|value| {
            AuthContext::new("apikey".to_string(), value.clone())
                .with_metadata("location".to_string(), "query".to_string())
                .with_metadata("name".to_string(), self.name.clone())
        })
    }

    async fn extract_from_cookies(&self, cookies: &str) -> Option<AuthContext> {
        if self.location != "cookie" {
            return None;
        }

        // Simple cookie parsing - in production, use a proper cookie parser
        cookies
            .split(';')
            .map(|cookie| cookie.trim())
            .find_map(|cookie| {
                let parts: Vec<&str> = cookie.splitn(2, '=').collect();
                if parts.len() == 2 && parts[0] == self.name {
                    Some(
                        AuthContext::new("apikey".to_string(), parts[1].to_string())
                            .with_metadata("location".to_string(), "cookie".to_string())
                            .with_metadata("name".to_string(), self.name.clone()),
                    )
                } else {
                    None
                }
            })
    }
}

/// No-op authenticator that allows all requests
#[derive(Clone)]
pub struct NoopAuthenticator {
    scheme: SecurityScheme,
}

impl NoopAuthenticator {
    /// Create a new no-op authenticator
    pub fn new() -> Self {
        Self {
            scheme: SecurityScheme::Http {
                scheme: "none".to_string(),
                bearer_format: None,
                description: Some("No authentication required".to_string()),
            },
        }
    }
}

impl Default for NoopAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Authenticator for NoopAuthenticator {
    async fn authenticate(&self, _context: &AuthContext) -> Result<AuthPrincipal, A2AError> {
        Ok(AuthPrincipal::new(
            "anonymous".to_string(),
            "none".to_string(),
        ))
    }

    fn security_scheme(&self) -> &SecurityScheme {
        &self.scheme
    }

    fn validate_context(&self, _context: &AuthContext) -> Result<(), A2AError> {
        // No validation needed for no-op
        Ok(())
    }
}

#[cfg(feature = "http-server")]
mod http_auth {
    use super::*;

    /// Authentication middleware state
    #[derive(Clone)]
    pub struct AuthState {
        /// The authenticator to use
        authenticator: Arc<dyn Authenticator>,
        /// Context extractors
        extractors: Vec<Arc<dyn AuthContextExtractor>>,
    }

    impl AuthState {
        /// Create a new authentication state
        pub fn new(authenticator: impl Authenticator + 'static) -> Self {
            Self {
                authenticator: Arc::new(authenticator),
                extractors: vec![Arc::new(BearerTokenExtractor)],
            }
        }

        /// Create with custom extractors
        #[allow(dead_code)]
        pub fn with_extractors(
            authenticator: impl Authenticator + 'static,
            extractors: Vec<Arc<dyn AuthContextExtractor>>,
        ) -> Self {
            Self {
                authenticator: Arc::new(authenticator),
                extractors,
            }
        }
    }

    /// Authentication middleware for Axum
    pub async fn http_auth_middleware(
        State(state): State<AuthState>,
        req: Request<axum::body::Body>,
        next: Next,
    ) -> Result<Response, StatusCode> {
        let headers = req.headers();

        // Try to extract auth context using available extractors
        for extractor in &state.extractors {
            if let Some(context) = extractor.extract_from_headers(headers).await {
                // Try to authenticate with the extracted context
                match state.authenticator.authenticate(&context).await {
                    Ok(_principal) => {
                        // Authentication successful, we could add the principal to request extensions
                        // For now, just proceed with the request
                        return Ok(next.run(req).await);
                    }
                    Err(_) => {
                        // This extractor found credentials but they were invalid
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                }
            }
        }

        // No valid authentication context found
        Err(StatusCode::UNAUTHORIZED)
    }

    /// Helper function to apply authentication middleware to a router
    pub fn with_auth<R>(router: R, authenticator: impl Authenticator + 'static) -> axum::Router
    where
        R: Into<axum::Router>,
    {
        let auth_state = AuthState::new(authenticator);
        let router = router.into();

        router.layer(axum::middleware::from_fn_with_state(
            auth_state,
            http_auth_middleware,
        ))
    }
}

#[cfg(feature = "http-server")]
pub use http_auth::with_auth;
