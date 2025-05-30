//! Transport protocol adapter implementations

#[cfg(any(feature = "http-client", feature = "http-server"))]
pub mod http;

#[cfg(any(feature = "ws-client", feature = "ws-server"))]
pub mod websocket;
