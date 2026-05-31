//! Transport protocol adapter implementations

/// ConnectRPC transport adapter (`impl A2aService`) over the application service.
#[cfg(feature = "server")]
pub mod connectrpc;
#[cfg(any(feature = "http-client", feature = "http-server"))]
pub mod http;
/// Wire-compatible JSON-RPC 2.0 + HTTP+JSON (REST) transport adapter.
#[cfg(feature = "jsonrpc-server")]
pub mod jsonrpc;

#[cfg(feature = "server")]
pub use connectrpc::ConnectRpcAdapter;
#[cfg(feature = "jsonrpc-server")]
pub use jsonrpc::{JsonRpcAdapter, jsonrpc_router, rest_router};
