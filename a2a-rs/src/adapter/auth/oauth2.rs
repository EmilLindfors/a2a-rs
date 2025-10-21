//! OAuth2 and OpenID Connect authentication implementations

#[cfg(feature = "auth")]
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenUrl,
};
#[cfg(feature = "auth")]
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    IssuerUrl, Nonce,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    domain::{
        core::agent::{
            AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow, OAuthFlows, SecurityScheme,
        },
        A2AError,
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

/// OAuth2 authenticator using the oauth2 crate
#[cfg(feature = "auth")]
#[derive(Clone)]
pub struct OAuth2Authenticator {
    /// OAuth2 client
    client: BasicClient,
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
        let client = BasicClient::new(client_id, client_secret, auth_url, Some(token_url))
            .set_redirect_uri(redirect_url);

        let flow = AuthorizationCodeOAuthFlow {
            authorization_url: client.auth_url().url().to_string(),
            token_url: client.token_url().unwrap().url().to_string(),
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
            client,
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
        let client = BasicClient::new(
            client_id,
            Some(client_secret),
            AuthUrl::new("".to_string()).unwrap(),
            Some(token_url),
        );

        let flow = ClientCredentialsOAuthFlow {
            token_url: client.token_url().unwrap().url().to_string(),
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
            client,
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
        let (auth_url, csrf_token) = self
            .client
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
    /// OpenID Connect client
    client: CoreClient,
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
        // Discover OpenID Connect provider metadata
        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url.clone(), async_http_client)
                .await
                .map_err(|e| {
                    A2AError::Internal(format!("Failed to discover OIDC provider: {}", e))
                })?;

        // Create OpenID Connect client
        let client =
            CoreClient::from_provider_metadata(provider_metadata, client_id, client_secret)
                .set_redirect_uri(redirect_url);

        let scheme = SecurityScheme::OpenIdConnect {
            open_id_connect_url: issuer_url.url().to_string(),
            description: Some("OpenID Connect authentication".to_string()),
        };

        Ok(Self {
            client,
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
        let (auth_url, csrf_token, nonce) = self
            .client
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

#[cfg(feature = "auth")]
async fn async_http_client(
    request: openidconnect::HttpRequest,
) -> Result<openidconnect::HttpResponse, openidconnect::reqwest::Error<reqwest::Error>> {
    use openidconnect::reqwest::async_http_client;
    async_http_client(request).await
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
