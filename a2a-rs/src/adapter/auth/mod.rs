//! Authentication implementations

#[cfg(any(feature = "http-server", feature = "ws-server"))]
pub mod authenticator;

// Re-export authentication types
#[cfg(any(feature = "http-server", feature = "ws-server"))]
pub use authenticator::{Authenticator, NoopAuthenticator, TokenAuthenticator};

#[cfg(feature = "http-server")]
pub use authenticator::with_auth;