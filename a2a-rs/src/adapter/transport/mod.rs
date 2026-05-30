//! Transport protocol adapter implementations

/// ConnectRPC transport adapter (`impl A2aService`) over the application service.
#[cfg(feature = "server")]
pub mod connectrpc;
#[cfg(any(feature = "http-client", feature = "http-server"))]
pub mod http;

#[cfg(feature = "server")]
pub use connectrpc::ConnectRpcAdapter;
