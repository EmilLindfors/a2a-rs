//! Authentication adapter implementations

#[cfg(any(feature = "http-server", feature = "ws-server"))]
pub mod authenticator;

#[cfg(feature = "auth")]
pub mod jwt;

#[cfg(feature = "auth")]
pub mod oauth2;

// Re-export authentication types
#[cfg(any(feature = "http-server", feature = "ws-server"))]
pub use authenticator::{
    ApiKeyAuthenticator, ApiKeyExtractor, BearerTokenAuthenticator, BearerTokenExtractor,
    NoopAuthenticator,
};

#[cfg(feature = "auth")]
pub use jwt::{JwtAuthenticator, JwtExtractor};

#[cfg(feature = "auth")]
pub use oauth2::{OAuth2Authenticator, OAuth2Extractor, OpenIdConnectAuthenticator};

#[cfg(feature = "http-server")]
pub use authenticator::with_auth;
