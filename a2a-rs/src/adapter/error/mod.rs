//! Error types for adapter implementations

#[cfg(any(feature = "http-client", feature = "jsonrpc-client", feature = "auth"))]
pub mod client;

/// A `reqwest::Error` and everything under it, as one line.
///
/// `reqwest::Error`'s `Display` omits its source chain, so a DNS failure, a
/// refused connection and an untrusted certificate all read as `error sending
/// request for url (…)`. Telling a TLS-intercepting proxy from the network
/// being down cost a full investigation once (see `NOTES.md`); the cause was
/// there the whole time, one `source()` away.
#[cfg(any(feature = "http-client", feature = "jsonrpc-client", feature = "auth"))]
pub(crate) fn describe_transport_error(error: &reqwest::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(feature = "server")]
pub mod server;

// Re-export client error types
#[cfg(any(feature = "http-client", feature = "jsonrpc-client", feature = "auth"))]
pub use client::HttpClientError;
#[cfg(any(feature = "http-client", feature = "jsonrpc-client", feature = "auth"))]
pub(crate) use client::http_client;

// Re-export server error types
#[cfg(feature = "http-server")]
pub use server::HttpServerError;
