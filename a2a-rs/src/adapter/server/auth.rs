//! Authentication middleware for server adapters

// This module is already conditionally compiled with #[cfg(feature = "server")] in mod.rs

use std::sync::Arc;

use async_trait::async_trait;
#[cfg(feature = "http-server")]
use axum::{
    extract::State,
    http::{Request, StatusCode, header},
    middleware::Next,
    response::Response,
};

use crate::domain::A2AError;

/// Interface for authentication handlers
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Authenticate a request based on the provided token
    async fn authenticate(&self, token: &str) -> Result<(), A2AError>;

    /// Get the authentication scheme used by this authenticator
    fn scheme(&self) -> &str;
}

/// Simple token-based authenticator
#[derive(Clone)]
pub struct TokenAuthenticator {
    /// The valid tokens
    tokens: Vec<String>,
    /// The authentication scheme
    scheme: String,
}

impl TokenAuthenticator {
    /// Create a new token authenticator with the given tokens
    pub fn new(tokens: Vec<String>) -> Self {
        Self {
            tokens,
            scheme: "Bearer".to_string(),
        }
    }

    /// Create a new token authenticator with a specific scheme
    pub fn with_scheme(tokens: Vec<String>, scheme: String) -> Self {
        Self { tokens, scheme }
    }
}

#[async_trait]
impl Authenticator for TokenAuthenticator {
    async fn authenticate(&self, token: &str) -> Result<(), A2AError> {
        if self.tokens.contains(&token.to_string()) {
            Ok(())
        } else {
            Err(A2AError::Internal(
                "Invalid authentication token".to_string(),
            ))
        }
    }

    fn scheme(&self) -> &str {
        &self.scheme
    }
}

/// No-op authenticator that allows all requests
#[derive(Clone)]
pub struct NoopAuthenticator;

impl NoopAuthenticator {
    /// Create a new no-op authenticator
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Authenticator for NoopAuthenticator {
    async fn authenticate(&self, _token: &str) -> Result<(), A2AError> {
        Ok(())
    }

    fn scheme(&self) -> &str {
        "None"
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
    }

    impl AuthState {
        /// Create a new authentication state
        pub fn new(authenticator: impl Authenticator + 'static) -> Self {
            Self {
                authenticator: Arc::new(authenticator),
            }
        }
    }

    /// Authentication middleware for Axum
    pub async fn http_auth_middleware(
        State(state): State<AuthState>,
        req: Request<axum::body::Body>,
        next: Next,
    ) -> Result<Response, StatusCode> {
        // Extract the token from the Authorization header
        let auth_header = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|header| header.to_str().ok());

        if let Some(auth) = auth_header {
            // Split the scheme and token
            let parts: Vec<&str> = auth.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let scheme = parts[0];
                let token = parts[1];

                // Verify the scheme matches
                if scheme.to_lowercase() == state.authenticator.scheme().to_lowercase() {
                    // Authenticate the token
                    match state.authenticator.authenticate(token).await {
                        Ok(_) => {
                            // Authentication successful, proceed with the request
                            return Ok(next.run(req).await);
                        }
                        Err(_) => {
                            // Authentication failed
                            return Err(StatusCode::UNAUTHORIZED);
                        }
                    }
                }
            }
        }

        // No valid authentication header found
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
