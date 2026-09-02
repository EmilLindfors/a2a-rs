//! Transport protocol adapter implementations

/// Shared client-side wire decoding (`StreamResponse` → `StreamItem`).
#[cfg(feature = "client")]
pub mod codec;
/// The `A2AError` ⇄ `ConnectError` mapping, shared by the ConnectRPC server
/// adapter and the `HttpClient` so the two directions cannot drift.
#[cfg(any(feature = "server", feature = "http-client"))]
pub mod connect_wire;
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
/// Shared JSON-RPC 2.0 wire vocabulary (method names, error codes, envelopes,
/// error maps) — the byte-for-byte contract between the JSON-RPC server and
/// client adapters. The error map is also what the ConnectRPC binding carries
/// in its error detail, so it is built whenever either side of any transport is.
#[cfg(any(feature = "server", feature = "client"))]
pub mod jsonrpc_wire;
/// Client-side transport negotiation from an agent card.
#[cfg(feature = "client")]
pub mod negotiation;
/// Wire details of the streaming-resumption enhancement, shared by the
/// transports that implement it.
#[cfg(any(feature = "server", feature = "http-client"))]
mod resume;
/// Resilient streaming: reconnect-with-backoff over the `Transport` port.
#[cfg(feature = "client")]
pub mod retry;

#[cfg(feature = "server")]
pub use connectrpc::ConnectRpcAdapter;
#[cfg(feature = "jsonrpc-server")]
pub use jsonrpc::{JsonRpcAdapter, jsonrpc_router, rest_router};
#[cfg(feature = "jsonrpc-client")]
pub use jsonrpc_client::JsonRpcClient;
#[cfg(feature = "client")]
pub use negotiation::{ClientConfig, TransportFactory, TransportNegotiator, default_registry};
#[cfg(feature = "client")]
pub use retry::{RetryingTransport, subscribe_resilient};
