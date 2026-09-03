//! OAuth2 and OpenID Connect authentication implementations

#[cfg(feature = "auth")]
use oauth2::{
    AuthUrl, ClientId, ClientSecret, CsrfToken, IntrospectionUrl, RedirectUrl, Scope, TokenUrl,
    basic::BasicClient,
};
#[cfg(feature = "auth")]
use openidconnect::{
    ClaimsVerificationError, IssuerUrl, JsonWebKeySetUrl, Nonce, SignatureVerificationError,
    core::{
        CoreAuthenticationFlow, CoreClient, CoreIdToken, CoreIdTokenClaims, CoreIdTokenVerifier,
        CoreJsonWebKeySet, CoreProviderMetadata,
    },
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(feature = "auth")]
use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

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

/// What an authorization server says about a token (RFC 7662 §2.2).
///
/// Only the fields that decide whether the token is good and who presented it.
/// Unknown members are ignored — every server adds its own.
#[cfg(feature = "auth")]
#[derive(Debug, Clone, Deserialize)]
struct IntrospectionResponse {
    /// Whether the token is currently valid. The only required member, and the
    /// only one that may be trusted when it is `false`.
    active: bool,
    /// Subject: the end user the token was issued for.
    #[serde(default)]
    sub: Option<String>,
    /// Human-readable identifier for that user, where the server publishes one.
    #[serde(default)]
    username: Option<String>,
    /// The client the token was issued to. This is the subject for a
    /// client-credentials token, which has no end user.
    #[serde(default)]
    client_id: Option<String>,
    /// Space-delimited scopes.
    #[serde(default)]
    scope: Option<String>,
    /// Expiry, as a Unix timestamp.
    #[serde(default)]
    exp: Option<i64>,
}

/// The `oauth2` / `openidconnect` HTTP client, over the workspace's reqwest.
///
/// Both crates ship their own reqwest integration behind a `reqwest` feature,
/// and that feature pins reqwest 0.12 while the workspace is on 0.13 — taking
/// it would put two reqwests and two TLS stacks in every binary with `auth`
/// on. Their `AsyncHttpClient` is a plain `Fn(http::Request<Vec<u8>>) ->
/// Future<Result<http::Response<Vec<u8>>, E>>`, so this is the whole
/// integration: buffer the request in, buffer the response out. Redirect
/// policy and everything else stay on the `reqwest::Client` the caller built.
#[cfg(feature = "auth")]
fn oauth_http(
    client: reqwest::Client,
) -> impl Fn(
    oauth2::HttpRequest,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<oauth2::HttpResponse, crate::adapter::error::HttpClientError>,
            > + Send,
    >,
> {
    use crate::adapter::error::HttpClientError;

    move |request| {
        let client = client.clone();
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let response = client
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .body(body)
                .send()
                .await?;
            let mut out = http::Response::builder().status(response.status());
            if let Some(headers) = out.headers_mut() {
                *headers = response.headers().clone();
            }
            let body = response.bytes().await?.to_vec();
            out.body(body)
                .map_err(|e| HttpClientError::Request(format!("invalid response: {e}")))
        })
    }
}

/// Where and how to ask the authorization server about a token.
#[cfg(feature = "auth")]
#[derive(Clone)]
struct Introspection {
    url: IntrospectionUrl,
    http: reqwest::Client,
}

/// OAuth2 authenticator using the oauth2 crate.
///
/// Stores OAuth2 configuration and constructs typed clients on demand, since
/// oauth2 5.0 uses a type-state pattern where each `.set_*()` call changes
/// the generic type.
///
/// Give it an introspection endpoint ([`with_introspection`]) for anything but a
/// test: an access token is opaque, so the authorization server is the only
/// thing that can say whether it is still valid and whose it is. Without one the
/// authenticator can do no better than match the token against
/// [`with_valid_tokens`], and the identity it reports is the credential itself —
/// which rotates on every refresh, so nothing an agent stores per caller
/// survives one.
///
/// [`with_introspection`]: Self::with_introspection
/// [`with_valid_tokens`]: Self::with_valid_tokens
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
    /// The authorization server's introspection endpoint, when it has one.
    introspection: Option<Introspection>,
    /// Statically accepted tokens, for tests and local development. Consulted
    /// only when there is no introspection endpoint.
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
            refresh_url: String::new(),
            scopes,
            ..Default::default()
        };

        let scheme = SecurityScheme::oauth2(
            OAuthFlows::authorization_code(flow),
            Some("OAuth2 Authorization Code Flow".to_string()),
            None,
        );

        Self {
            client_id,
            client_secret,
            auth_url,
            token_url: Some(token_url),
            redirect_url: Some(redirect_url),
            scheme,
            introspection: None,
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
            refresh_url: String::new(),
            scopes,
            ..Default::default()
        };

        let scheme = SecurityScheme::oauth2(
            OAuthFlows::client_credentials(flow),
            Some("OAuth2 Client Credentials Flow".to_string()),
            None,
        );

        Self {
            client_id,
            client_secret: Some(client_secret),
            auth_url,
            token_url: Some(token_url),
            redirect_url: None,
            scheme,
            introspection: None,
            valid_tokens: Vec::new(),
        }
    }

    /// Validate presented tokens against the authorization server's
    /// introspection endpoint (RFC 7662).
    ///
    /// This is what makes the authenticator name a *subject* rather than a
    /// credential: the response carries `sub`, which survives a refresh, while
    /// the access token does not. An agent that owns anything per caller — a
    /// conversation, a quota — needs the first.
    ///
    /// Redirects are not followed, for the same reason OIDC discovery does not
    /// follow them: the endpoint is named by configuration and a redirect would
    /// send the client credentials somewhere else.
    pub fn with_introspection(mut self, url: IntrospectionUrl) -> Result<Self, A2AError> {
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| A2AError::Internal(format!("Failed to build HTTP client: {}", e)))?;
        self.introspection = Some(Introspection { url, http });
        Ok(self)
    }

    /// Accept these tokens without asking anyone — for tests and local
    /// development only.
    ///
    /// Ignored once [`with_introspection`](Self::with_introspection) is set: a
    /// deployment that can ask the authorization server has no reason to trust
    /// a list, and a static token that outlives its revocation is exactly what
    /// introspection exists to catch.
    pub fn with_valid_tokens(mut self, tokens: Vec<String>) -> Self {
        self.valid_tokens = tokens;
        self
    }

    /// Ask the authorization server about `token` and turn its answer into a
    /// principal.
    async fn introspect(
        &self,
        introspection: &Introspection,
        token: &str,
    ) -> Result<AuthPrincipal, A2AError> {
        // RFC 7662 §2.1: the caller authenticates as a client. HTTP Basic is
        // the form every server accepts.
        let response = introspection
            .http
            .post(introspection.url.as_str())
            .basic_auth(
                self.client_id.as_str(),
                self.client_secret.as_ref().map(|secret| secret.secret()),
            )
            .form(&[("token", token), ("token_type_hint", "access_token")])
            .send()
            .await
            .map_err(|e| {
                A2AError::Internal(format!(
                    "OAuth2 token introspection failed: {}",
                    crate::adapter::error::describe_transport_error(&e)
                ))
            })?;

        let status = response.status();
        if !status.is_success() {
            // A failing endpoint is our problem, not the caller's, and saying
            // "invalid token" here sends whoever is debugging to the wrong side.
            return Err(A2AError::Internal(format!(
                "OAuth2 token introspection endpoint answered {}",
                status
            )));
        }

        let claims: IntrospectionResponse = response.json().await.map_err(|e| {
            A2AError::Internal(format!(
                "OAuth2 token introspection returned an unreadable body: {}",
                e
            ))
        })?;

        if !claims.active {
            return Err(A2AError::Internal(
                "Invalid OAuth2 access token".to_string(),
            ));
        }

        // `sub` first, since that is the identity that outlives the token.
        // `client_id` is the honest subject of a client-credentials token,
        // which has no end user at all.
        let subject = claims
            .sub
            .clone()
            .or_else(|| claims.username.clone())
            .or_else(|| claims.client_id.clone())
            .ok_or_else(|| {
                A2AError::Internal(
                    "OAuth2 token introspection named no subject (`sub`, `username` or \
                     `client_id`) — there is nothing to attribute the request to"
                        .to_string(),
                )
            })?;

        let mut principal = AuthPrincipal::new(subject, "oauth2".to_string());
        if let Some(scope) = claims.scope {
            principal = principal.with_attribute("scope".to_string(), scope);
        }
        if let Some(client_id) = claims.client_id {
            principal = principal.with_attribute("client_id".to_string(), client_id);
        }
        if let Some(exp) = claims.exp {
            principal = principal.with_attribute("exp".to_string(), exp.to_string());
        }
        Ok(principal)
    }

    /// Generate authorization URL for authorization code flow
    pub fn authorize_url(&self) -> (String, CsrfToken) {
        // Only set auth_uri here; token_uri is not needed for generating the
        // authorize URL and would change the client's type-state parameter.
        let mut client =
            BasicClient::new(self.client_id.clone()).set_auth_uri(self.auth_url.clone());
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

        let mut principal = match &self.introspection {
            Some(introspection) => self.introspect(introspection, token).await?,
            // Nobody to ask: the token can only be matched against the static
            // list, and the only identity available is the credential. Prefixed
            // so it cannot be mistaken for a subject the server vouched for.
            None if self.valid_tokens.contains(token) => {
                AuthPrincipal::new(format!("oauth2:{}", token), "oauth2".to_string())
            }
            None => {
                return Err(A2AError::Internal(
                    "Invalid OAuth2 access token".to_string(),
                ));
            }
        };

        // Whatever the transport observed, and only where the authorization
        // server said nothing: what the server returns is the authority on a
        // token's scope, and the request cannot be allowed to widen it.
        if let Some(scope) = context.get_metadata("scope")
            && !principal.attributes.contains_key("scope")
        {
            principal = principal.with_attribute("scope".to_string(), scope.clone());
        }

        Ok(principal)
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

/// The provider's published signing keys.
///
/// Discovery fetches the set once, at construction. Providers rotate signing
/// keys, and a token signed with one issued after that would fail verification
/// for as long as the process ran — so a verification that fails for want of a
/// key refetches. Rate-limited, because the same failure is what an attacker
/// gets by presenting garbage, and that must not turn into a request to the
/// provider per attempt.
#[cfg(feature = "auth")]
struct SigningKeys {
    jwks_uri: JsonWebKeySetUrl,
    http: reqwest::Client,
    state: RwLock<KeyState>,
}

#[cfg(feature = "auth")]
struct KeyState {
    set: CoreJsonWebKeySet,
    fetched_at: Instant,
}

#[cfg(feature = "auth")]
impl SigningKeys {
    fn current(&self) -> CoreJsonWebKeySet {
        self.read().set.clone()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, KeyState> {
        // A panic while holding the lock leaves the last fetched set behind,
        // which is still the best answer available.
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Fetch the set again, unless that happened within `min_interval`. `None`
    /// means the caller should keep the answer it has.
    async fn refetch(&self, min_interval: Duration) -> Result<Option<CoreJsonWebKeySet>, A2AError> {
        if self.read().fetched_at.elapsed() < min_interval {
            return Ok(None);
        }

        let set = CoreJsonWebKeySet::fetch_async(&self.jwks_uri, &oauth_http(self.http.clone()))
            .await
            .map_err(|e| {
                A2AError::Internal(format!("Failed to fetch the OIDC provider's keys: {}", e))
            })?;

        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        state.set = set.clone();
        state.fetched_at = Instant::now();
        Ok(Some(set))
    }
}

/// A verification failure the provider's current keys might fix.
#[cfg(feature = "auth")]
fn is_missing_key(error: &ClaimsVerificationError) -> bool {
    matches!(
        error,
        ClaimsVerificationError::SignatureVerification(
            SignatureVerificationError::NoMatchingKey
                | SignatureVerificationError::AmbiguousKeyId(_)
        )
    )
}

/// The nonce binds an ID token to the authentication request that asked for it.
/// An agent receiving a token someone else obtained never made that request, so
/// it has no nonce to compare against; `aud` is what ties the token to this
/// client here.
#[cfg(feature = "auth")]
fn any_nonce(_: Option<&Nonce>) -> Result<(), String> {
    Ok(())
}

/// OpenID Connect authenticator.
///
/// Verifies the ID token presented against the provider's published signing
/// keys, and reports the `sub` it carries — the identity that outlives the
/// token, so a caller who logs in again is the same caller.
///
/// The token must name this `client_id` in `aud`. An ID token is issued to one
/// client, and taking one issued to another would let any application the user
/// has signed into speak for them here.
///
/// This is for a caller presenting an **ID token**. A caller presenting an
/// opaque access token needs [`OAuth2Authenticator::with_introspection`]
/// instead — the same OIDC provider serves both, and only the authorization
/// server can say anything about an opaque token.
#[cfg(feature = "auth")]
#[derive(Clone)]
pub struct OpenIdConnectAuthenticator {
    /// Client ID: the audience an ID token has to name, and stored for
    /// authorize_url reconstruction.
    client_id: ClientId,
    /// Optional client secret. Its presence decides whether the verifier is a
    /// confidential-client one, which is what admits shared-secret signatures.
    client_secret: Option<ClientSecret>,
    /// The issuer an ID token has to name.
    issuer: IssuerUrl,
    /// Provider metadata (contains all OIDC endpoints)
    provider_metadata: CoreProviderMetadata,
    /// Redirect URL
    redirect_url: RedirectUrl,
    /// Security scheme configuration
    scheme: SecurityScheme,
    /// The keys tokens are verified against.
    keys: Arc<SigningKeys>,
    /// The shortest gap between two fetches of those keys.
    key_refetch_interval: Duration,
}

#[cfg(feature = "auth")]
impl OpenIdConnectAuthenticator {
    /// Nothing refetches the provider's keys more often than this.
    pub const DEFAULT_KEY_REFETCH_INTERVAL: Duration = Duration::from_secs(60);

    /// Create a new OpenID Connect authenticator.
    ///
    /// Discovery runs here: the issuer's metadata names the JWKS endpoint, and
    /// the keys are fetched with it.
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
        let provider_metadata = CoreProviderMetadata::discover_async(
            issuer_url.clone(),
            &oauth_http(http_client.clone()),
        )
        .await
        .map_err(|e| A2AError::Internal(format!("Failed to discover OIDC provider: {}", e)))?;

        let scheme = SecurityScheme::open_id_connect(
            issuer_url.as_str().to_string(),
            Some("OpenID Connect authentication".to_string()),
        );

        let keys = SigningKeys {
            jwks_uri: provider_metadata.jwks_uri().clone(),
            http: http_client,
            state: RwLock::new(KeyState {
                set: provider_metadata.jwks().clone(),
                fetched_at: Instant::now(),
            }),
        };

        Ok(Self {
            client_id,
            client_secret,
            issuer: issuer_url,
            provider_metadata,
            redirect_url,
            scheme,
            keys: Arc::new(keys),
            key_refetch_interval: Self::DEFAULT_KEY_REFETCH_INTERVAL,
        })
    }

    /// How long to wait before fetching the provider's signing keys again.
    ///
    /// The refetch is triggered by a token naming a key the agent does not
    /// have, which is what a rotation looks like — and also what a stream of
    /// junk tokens looks like, hence the floor. A provider that rotates on a
    /// schedule tighter than the default is the reason to lower it.
    pub fn with_key_refetch_interval(mut self, interval: Duration) -> Self {
        self.key_refetch_interval = interval;
        self
    }

    /// Verify `id_token` against `keys`, as this client.
    fn verified<'t>(
        &self,
        id_token: &'t CoreIdToken,
        keys: CoreJsonWebKeySet,
    ) -> Result<&'t CoreIdTokenClaims, ClaimsVerificationError> {
        let verifier = match &self.client_secret {
            Some(secret) => CoreIdTokenVerifier::new_confidential_client(
                self.client_id.clone(),
                secret.clone(),
                self.issuer.clone(),
                keys,
            ),
            None => CoreIdTokenVerifier::new_public_client(
                self.client_id.clone(),
                self.issuer.clone(),
                keys,
            ),
        };

        id_token.claims(&verifier, any_nonce)
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

        let id_token: CoreIdToken = context.credential.parse().map_err(|e| {
            A2AError::Internal(format!(
                "Invalid OpenID Connect ID token: not a well-formed ID token ({})",
                e
            ))
        })?;

        let mut failure = match self.verified(&id_token, self.keys.current()) {
            Ok(claims) => return Ok(principal_from(claims)),
            Err(e) => e,
        };

        // Signed by a key we have not seen: the provider may have rotated since
        // discovery. Ask once for the current set, and let the second answer
        // stand whichever way it goes.
        if is_missing_key(&failure)
            && let Some(rotated) = self.keys.refetch(self.key_refetch_interval).await?
        {
            match self.verified(&id_token, rotated) {
                Ok(claims) => return Ok(principal_from(claims)),
                Err(e) => failure = e,
            }
        }

        Err(A2AError::Internal(format!(
            "Invalid OpenID Connect ID token: {}",
            failure
        )))
    }

    fn security_scheme(&self) -> &SecurityScheme {
        &self.scheme
    }

    fn validate_context(&self, context: &AuthContext) -> Result<(), A2AError> {
        // `oauth2` is accepted because that is what the bearer extractor labels
        // an `Authorization: Bearer …` header, and an ID token arrives in one.
        if context.scheme_type != "openidconnect" && context.scheme_type != "oauth2" {
            return Err(A2AError::Internal(format!(
                "Invalid authentication scheme: expected 'openidconnect', got '{}'",
                context.scheme_type
            )));
        }
        Ok(())
    }
}

/// What the agent knows about the caller, from the claims the provider signed.
#[cfg(feature = "auth")]
fn principal_from(claims: &CoreIdTokenClaims) -> AuthPrincipal {
    let mut principal = AuthPrincipal::new(
        claims.subject().as_str().to_string(),
        "openidconnect".to_string(),
    );

    principal = principal.with_attribute("iss".to_string(), claims.issuer().as_str().to_string());
    principal = principal.with_attribute(
        "exp".to_string(),
        claims.expiration().timestamp().to_string(),
    );
    if let Some(email) = claims.email() {
        principal = principal.with_attribute("email".to_string(), email.as_str().to_string());
    }
    if let Some(username) = claims.preferred_username() {
        principal =
            principal.with_attribute("preferred_username".to_string(), username.to_string());
    }
    principal
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
