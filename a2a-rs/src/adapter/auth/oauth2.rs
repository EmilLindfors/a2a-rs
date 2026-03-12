//! OAuth2 and OpenID Connect authentication implementations

#[cfg(feature = "auth")]
use oauth2::{
    AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenUrl, basic::BasicClient,
};
#[cfg(feature = "auth")]
use openidconnect::{
    IssuerUrl, Nonce,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    domain::{
        A2AError,
        core::agent::{
            AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow, OAuthFlows, SecurityScheme,
        },
    },
    port::authenticator::{AuthContext, AuthContextExtractor, AuthPrincipal, Authenticator},
};

/// OAuth2 token information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Token {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

/// OAuth2 authenticator using the oauth2 crate.
///
/// Stores OAuth2 configuration and constructs typed clients on demand, since
/// oauth2 5.0 uses a type-state pattern where each `.set_*()` call changes
/// the generic type.
#[cfg(feature = "auth")]
#[derive(Clone)]
pub struct OAuth2Authenticator {
    /// Client ID
    client_id: ClientId,
    /// Optional client secret
    client_secret: Option<ClientSecret>,
    /// Authorization URL
    auth_url: AuthUrl,
    /// Token URL (used for token exchange, not for authorize URL generation)
    #[allow(dead_code)]
    token_url: Option<TokenUrl>,
    /// Redirect URL
    redirect_url: Option<RedirectUrl>,
    /// Security scheme configuration
    scheme: SecurityScheme,
    /// Valid access tokens (in a real implementation, you'd validate against the OAuth2 server)
    valid_tokens: Vec<String>,
}

#[cfg(feature = "auth")]
impl OAuth2Authenticator {
    /// Create a new OAuth2 authenticator for authorization code flow
    pub fn new_authorization_code(
        client_id: ClientId,
        client_secret: Option<ClientSecret>,
        auth_url: AuthUrl,
        token_url: TokenUrl,
        redirect_url: RedirectUrl,
        scopes: HashMap<String, String>,
    ) -> Self {
        let flow = AuthorizationCodeOAuthFlow {
            authorization_url: auth_url.as_str().to_string(),
            token_url: token_url.as_str().to_string(),
            refresh_url: None,
            scopes,
        };

        let scheme = SecurityScheme::OAuth2 {
            flows: Box::new(OAuthFlows {
                authorization_code: Some(flow),
                ..Default::default()
            }),
            description: Some("OAuth2 Authorization Code Flow".to_string()),
            metadata_url: None,
        };

        Self {
            client_id,
            client_secret,
            auth_url,
            token_url: Some(token_url),
            redirect_url: Some(redirect_url),
            scheme,
            valid_tokens: Vec::new(),
        }
    }

    /// Create a new OAuth2 authenticator for client credentials flow
    pub fn new_client_credentials(
        client_id: ClientId,
        client_secret: ClientSecret,
        token_url: TokenUrl,
        scopes: HashMap<String, String>,
    ) -> Self {
        // Use a placeholder auth URL since client credentials flow doesn't need it
        let auth_url = AuthUrl::new("http://localhost".to_string())
            .expect("localhost URL should always be valid");

        let flow = ClientCredentialsOAuthFlow {
            token_url: token_url.as_str().to_string(),
            refresh_url: None,
            scopes,
        };

        let scheme = SecurityScheme::OAuth2 {
            flows: Box::new(OAuthFlows {
                client_credentials: Some(flow),
                ..Default::default()
            }),
            description: Some("OAuth2 Client Credentials Flow".to_string()),
            metadata_url: None,
        };

        Self {
            client_id,
            client_secret: Some(client_secret),
            auth_url,
            token_url: Some(token_url),
            redirect_url: None,
            scheme,
            valid_tokens: Vec::new(),
        }
    }

    /// Add valid tokens (for testing/development)
    pub fn with_valid_tokens(mut self, tokens: Vec<String>) -> Self {
        self.valid_tokens = tokens;
        self
    }

    /// Generate authorization URL for authorization code flow
    pub fn authorize_url(&self) -> (String, CsrfToken) {
        // Only set auth_uri here; token_uri is not needed for generating the
        // authorize URL and would change the client's type-state parameter.
        let mut client = BasicClient::new(self.client_id.clone())
            .set_auth_uri(self.auth_url.clone());
        if let Some(ref secret) = self.client_secret {
            client = client.set_client_secret(secret.clone());
        }
        if let Some(ref redirect_url) = self.redirect_url {
            client = client.set_redirect_uri(redirect_url.clone());
        }

        let (auth_url, csrf_token) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("read".to_string()))
            .url();

        (auth_url.to_string(), csrf_token)
    }
}

#[cfg(feature = "auth")]
#[async_trait]
impl Authenticator for OAuth2Authenticator {
    async fn authenticate(&self, context: &AuthContext) -> Result<AuthPrincipal, A2AError> {
        self.validate_context(context)?;

        let token = &context.credential;

        // In a real implementation, you would validate the token against the OAuth2 server
        // For now, we'll just check if it's in our list of valid tokens
        if self.valid_tokens.contains(token) {
            let mut principal =
                AuthPrincipal::new(format!("oauth2:{}", token), "oauth2".to_string());

            // Add OAuth2-specific attributes
            if let Some(scope) = context.get_metadata("scope") {
                principal = principal.with_attribute("scope".to_string(), scope.clone());
            }

            Ok(principal)
        } else {
            Err(A2AError::Internal(
                "Invalid OAuth2 access token".to_string(),
            ))
        }
    }

    fn security_scheme(&self) -> &SecurityScheme {
        &self.scheme
    }

    fn validate_context(&self, context: &AuthContext) -> Result<(), A2AError> {
        if context.scheme_type != "oauth2" {
            return Err(A2AError::Internal(format!(
                "Invalid authentication scheme: expected 'oauth2', got '{}'",
                context.scheme_type
            )));
        }
        Ok(())
    }
}

/// OpenID Connect authenticator
#[cfg(feature = "auth")]
#[derive(Clone)]
pub struct OpenIdConnectAuthenticator {
    /// Client ID (stored for authorize_url reconstruction)
    client_id: ClientId,
    /// Optional client secret
    client_secret: Option<ClientSecret>,
    /// Provider metadata (contains all OIDC endpoints)
    provider_metadata: CoreProviderMetadata,
    /// Redirect URL
    redirect_url: RedirectUrl,
    /// Security scheme configuration
    scheme: SecurityScheme,
    /// Valid ID tokens (in a real implementation, you'd validate against the OIDC provider)
    valid_tokens: Vec<String>,
}

#[cfg(feature = "auth")]
impl OpenIdConnectAuthenticator {
    /// Create a new OpenID Connect authenticator
    pub async fn new(
        issuer_url: IssuerUrl,
        client_id: ClientId,
        client_secret: Option<ClientSecret>,
        redirect_url: RedirectUrl,
    ) -> Result<Self, A2AError> {
        // Discover OpenID Connect provider metadata.
        // Disable redirects to prevent SSRF during OIDC discovery.
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| A2AError::Internal(format!("Failed to build HTTP client: {}", e)))?;
        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url.clone(), &http_client)
                .await
                .map_err(|e| {
                    A2AError::Internal(format!("Failed to discover OIDC provider: {}", e))
                })?;

        let scheme = SecurityScheme::OpenIdConnect {
            open_id_connect_url: issuer_url.as_str().to_string(),
            description: Some("OpenID Connect authentication".to_string()),
        };

        Ok(Self {
            client_id,
            client_secret,
            provider_metadata,
            redirect_url,
            scheme,
            valid_tokens: Vec::new(),
        })
    }

    /// Add valid tokens (for testing/development)
    pub fn with_valid_tokens(mut self, tokens: Vec<String>) -> Self {
        self.valid_tokens = tokens;
        self
    }

    /// Generate authorization URL for OpenID Connect
    pub fn authorize_url(&self) -> (String, CsrfToken, Nonce) {
        let client = CoreClient::from_provider_metadata(
            self.provider_metadata.clone(),
            self.client_id.clone(),
            self.client_secret.clone(),
        )
        .set_redirect_uri(self.redirect_url.clone());

        let (auth_url, csrf_token, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .url();

        (auth_url.to_string(), csrf_token, nonce)
    }
}

#[cfg(feature = "auth")]
#[async_trait]
impl Authenticator for OpenIdConnectAuthenticator {
    async fn authenticate(&self, context: &AuthContext) -> Result<AuthPrincipal, A2AError> {
        self.validate_context(context)?;

        let token = &context.credential;

        // In a real implementation, you would validate the ID token
        // For now, we'll just check if it's in our list of valid tokens
        if self.valid_tokens.contains(token) {
            let principal =
                AuthPrincipal::new(format!("oidc:{}", token), "openidconnect".to_string());

            Ok(principal)
        } else {
            Err(A2AError::Internal(
                "Invalid OpenID Connect ID token".to_string(),
            ))
        }
    }

    fn security_scheme(&self) -> &SecurityScheme {
        &self.scheme
    }

    fn validate_context(&self, context: &AuthContext) -> Result<(), A2AError> {
        if context.scheme_type != "openidconnect" {
            return Err(A2AError::Internal(format!(
                "Invalid authentication scheme: expected 'openidconnect', got '{}'",
                context.scheme_type
            )));
        }
        Ok(())
    }
}

/// OAuth2/OIDC token extractor
#[derive(Clone)]
pub struct OAuth2Extractor;

#[async_trait]
impl AuthContextExtractor for OAuth2Extractor {
    #[cfg(feature = "http-server")]
    async fn extract_from_headers(&self, headers: &axum::http::HeaderMap) -> Option<AuthContext> {
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|auth| {
                let parts: Vec<&str> = auth.splitn(2, ' ').collect();
                if parts.len() == 2 && parts[0].to_lowercase() == "bearer" {
                    Some(AuthContext::new("oauth2".to_string(), parts[1].to_string()))
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
                    Some(AuthContext::new("oauth2".to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
    }

    async fn extract_from_query(&self, params: &HashMap<String, String>) -> Option<AuthContext> {
        // OAuth2 tokens can be passed as access_token query parameter
        params.get("access_token").map(|token| {
            AuthContext::new("oauth2".to_string(), token.clone())
                .with_metadata("location".to_string(), "query".to_string())
        })
    }

    async fn extract_from_cookies(&self, _cookies: &str) -> Option<AuthContext> {
        // OAuth2 tokens can be stored in cookies, but we'll keep this simple
        None
    }
}

// Placeholder implementations when auth feature is not enabled
#[cfg(not(feature = "auth"))]
pub struct OAuth2Authenticator;

#[cfg(not(feature = "auth"))]
pub struct OpenIdConnectAuthenticator;

#[cfg(not(feature = "auth"))]
impl OAuth2Authenticator {
    pub fn new_authorization_code(
        _client_id: String,
        _auth_url: String,
        _token_url: String,
    ) -> Self {
        compile_error!("OAuth2 authentication requires the 'auth' feature");
    }
}
