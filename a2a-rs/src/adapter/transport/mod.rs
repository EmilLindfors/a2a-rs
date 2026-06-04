//! Transport protocol adapter implementations

/// Shared client-side wire decoding (`StreamResponse` → `StreamItem`).
#[cfg(feature = "client")]
pub mod codec;
/// ConnectRPC transport adapter (`impl A2aService`) over the application service.
#[cfg(feature = "server")]
pub mod connectrpc;
#[cfg(any(feature = "http-client", feature = "http-server"))]
pub mod http;
/// Wire-compatible JSON-RPC 2.0 + HTTP+JSON (REST) transport adapter.
#[cfg(feature = "jsonrpc-server")]
pub mod jsonrpc;
/// Wire-compatible JSON-RPC 2.0 client adapter (`impl Transport`).
#[cfg(feature = "jsonrpc-client")]
pub mod jsonrpc_client;
/// Client-side transport negotiation from an agent card.
#[cfg(feature = "client")]
pub mod negotiation;
/// Resilient streaming: reconnect-with-backoff over the `Transport` port.
#[cfg(feature = "client")]
pub mod retry;
/// Shared JSON-RPC 2.0 wire vocabulary (method names, error codes, envelopes,
/// error maps) — the byte-for-byte contract between the JSON-RPC server and
/// client adapters.
#[cfg(any(feature = "jsonrpc-server", feature = "jsonrpc-client"))]
pub mod jsonrpc_wire;

#[cfg(feature = "server")]
pub use connectrpc::ConnectRpcAdapter;
#[cfg(feature = "jsonrpc-server")]
pub use jsonrpc::{JsonRpcAdapter, jsonrpc_router, rest_router};
#[cfg(feature = "jsonrpc-client")]
pub use jsonrpc_client::JsonRpcClient;
#[cfg(feature = "client")]
pub use negotiation::{TransportFactory, TransportNegotiator, default_registry};
#[cfg(feature = "client")]
pub use retry::{RetryingTransport, subscribe_resilient};
