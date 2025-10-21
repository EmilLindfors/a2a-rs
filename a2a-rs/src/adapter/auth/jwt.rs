//! JWT authentication implementation using jsonwebtoken crate

#[cfg(feature = "auth")]
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;

use crate::{
    domain::{core::agent::SecurityScheme, A2AError},
    port::authenticator::{AuthContext, AuthContextExtractor, AuthPrincipal, Authenticator},
};

/// JWT Claims structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// Expiration time (Unix timestamp)
    pub exp: i64,
    /// Issued at (Unix timestamp)
    pub iat: i64,
    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    /// Audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    /// Additional claims
    #[serde(flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// JWT authenticator using the jsonwebtoken crate
#[cfg(feature = "auth")]
#[derive(Clone)]
pub struct JwtAuthenticator {
    /// Decoding key for JWT verification
    decoding_key: DecodingKey,
    /// Validation rules
    validation: Validation,
    /// Security scheme configuration
    scheme: SecurityScheme,
}

#[cfg(feature = "auth")]
impl JwtAuthenticator {
    /// Create a new JWT authenticator with a secret
    pub fn new_with_secret(secret: &[u8]) -> Self {
        Self {
            decoding_key: DecodingKey::from_secret(secret),
            validation: Validation::new(Algorithm::HS256),
            scheme: SecurityScheme::Http {
                scheme: "bearer".to_string(),
                bearer_format: Some("JWT".to_string()),
                description: Some("JWT Bearer token authentication".to_string()),
            },
        }
    }

    /// Create a new JWT authenticator with an RSA public key
    pub fn new_with_rsa_pem(pem: &[u8]) -> Result<Self, A2AError> {
        let decoding_key = DecodingKey::from_rsa_pem(pem)
            .map_err(|e| A2AError::Internal(format!("Invalid RSA PEM: {}", e)))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = true;

        Ok(Self {
            decoding_key,
            validation,
            scheme: SecurityScheme::Http {
                scheme: "bearer".to_string(),
                bearer_format: Some("JWT".to_string()),
                description: Some("JWT Bearer token authentication with RSA".to_string()),
            },
        })
    }

    /// Create with custom validation rules
    pub fn with_validation(mut self, validation: Validation) -> Self {
        self.validation = validation;
        self
    }

    /// Set required issuer
    pub fn with_issuer(mut self, issuer: String) -> Self {
        self.validation.iss = Some(std::collections::HashSet::from([issuer]));
        self
    }

    /// Set required audience
    pub fn with_audience(mut self, audience: String) -> Self {
        self.validation.aud = Some(std::collections::HashSet::from([audience]));
        self
    }
}

#[cfg(feature = "auth")]
#[async_trait]
impl Authenticator for JwtAuthenticator {
    async fn authenticate(&self, context: &AuthContext) -> Result<AuthPrincipal, A2AError> {
        self.validate_context(context)?;

        let token = &context.credential;

        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)
            .map_err(|e| A2AError::Internal(format!("JWT validation failed: {}", e)))?;

        let mut principal = AuthPrincipal::new(token_data.claims.sub, "jwt".to_string());

        // Add JWT claims as attributes
        if let Some(iss) = token_data.claims.iss {
            principal = principal.with_attribute("issuer".to_string(), iss);
        }
        if let Some(aud) = token_data.claims.aud {
            principal = principal.with_attribute("audience".to_string(), aud);
        }
        principal = principal.with_attribute("exp".to_string(), token_data.claims.exp.to_string());
        principal = principal.with_attribute("iat".to_string(), token_data.claims.iat.to_string());

        // Add additional claims
        for (key, value) in token_data.claims.additional {
            if let Ok(string_value) = serde_json::to_string(&value) {
                principal = principal.with_attribute(key, string_value);
            }
        }

        Ok(principal)
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

/// JWT extractor for Bearer tokens
#[derive(Clone)]
pub struct JwtExtractor;

#[async_trait]
impl AuthContextExtractor for JwtExtractor {
    #[cfg(feature = "http-server")]
    async fn extract_from_headers(&self, headers: &axum::http::HeaderMap) -> Option<AuthContext> {
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|auth| {
                let parts: Vec<&str> = auth.splitn(2, ' ').collect();
                if parts.len() == 2 && parts[0].to_lowercase() == "bearer" {
                    Some(
                        AuthContext::new("bearer".to_string(), parts[1].to_string())
                            .with_metadata("format".to_string(), "JWT".to_string()),
                    )
                } else {
                    None
                }
            })
    }

    #[cfg(not(feature = "http-server"))]
    async fn extract_from_headers(&self, headers: &HashMap<String, String>) -> Option<AuthContext> {
        headers
            .get("authorization")
            .or_else(|| headers.get("Authorization"))
            .and_then(|auth| {
                let parts: Vec<&str> = auth.splitn(2, ' ').collect();
                if parts.len() == 2 && parts[0].to_lowercase() == "bearer" {
                    Some(
                        AuthContext::new("bearer".to_string(), parts[1].to_string())
                            .with_metadata("format".to_string(), "JWT".to_string()),
                    )
                } else {
                    None
                }
            })
    }

    async fn extract_from_query(&self, _params: &HashMap<String, String>) -> Option<AuthContext> {
        // JWTs are typically not passed in query parameters for security reasons
        None
    }

    async fn extract_from_cookies(&self, _cookies: &str) -> Option<AuthContext> {
        // JWTs can be passed in cookies, but we'll keep this simple for now
        None
    }
}

#[cfg(not(feature = "auth"))]
/// Placeholder when auth feature is not enabled
pub struct JwtAuthenticator;

#[cfg(not(feature = "auth"))]
impl JwtAuthenticator {
    pub fn new_with_secret(_secret: &[u8]) -> Self {
        compile_error!("JWT authentication requires the 'auth' feature");
    }
}
