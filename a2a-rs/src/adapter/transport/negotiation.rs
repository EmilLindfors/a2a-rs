//! Client-side transport negotiation.
//!
//! A [`TransportFactory`] knows how to build a [`Transport`] for one wire
//! protocol from an agent interface. A [`TransportNegotiator`] holds an ordered
//! set of factories and, given an [`AgentCard`], picks the first interface it can
//! satisfy — ranked by **client preference** (factory registration order), which
//! dominates the card's own `preferred_transport`.
//!
//! This is composition-at-the-edge: the application assembles a negotiator with
//! exactly the transports it compiled in, then calls [`connect`] (or
//! [`TransportNegotiator::negotiate`]) to obtain a ready `Box<dyn Transport>`.

use async_trait::async_trait;

use crate::domain::{A2AError, AgentCard, AgentInterface};
use crate::port::Transport;

/// Builds a [`Transport`] for a single wire protocol from an agent interface.
#[async_trait]
pub trait TransportFactory: Send + Sync {
    /// The protocol this factory handles, matching `AgentInterface::protocol_binding`
    /// (e.g. `"JSONRPC"`, `"CONNECTRPC"`).
    fn protocol(&self) -> &str;

    /// Construct a transport for `iface`. Returning `Err` lets the negotiator
    /// fall through to the next compatible interface/factory.
    async fn create(
        &self,
        card: &AgentCard,
        iface: &AgentInterface,
    ) -> Result<Box<dyn Transport>, A2AError>;
}

/// Factory for the wire-compatible JSON-RPC 2.0 transport.
#[cfg(feature = "jsonrpc-client")]
pub struct JsonRpcTransportFactory;

#[cfg(feature = "jsonrpc-client")]
#[async_trait]
impl TransportFactory for JsonRpcTransportFactory {
    fn protocol(&self) -> &str {
        "JSONRPC"
    }

    async fn create(
        &self,
        _card: &AgentCard,
        iface: &AgentInterface,
    ) -> Result<Box<dyn Transport>, A2AError> {
        Ok(Box::new(super::jsonrpc_client::JsonRpcClient::new(
            iface.url.clone(),
        )))
    }
}

/// Factory for the ConnectRPC transport.
#[cfg(feature = "http-client")]
pub struct ConnectRpcTransportFactory;

#[cfg(feature = "http-client")]
#[async_trait]
impl TransportFactory for ConnectRpcTransportFactory {
    fn protocol(&self) -> &str {
        "CONNECTRPC"
    }

    async fn create(
        &self,
        _card: &AgentCard,
        iface: &AgentInterface,
    ) -> Result<Box<dyn Transport>, A2AError> {
        // `HttpClient::new` panics on an unparseable URL; validate first so a bad
        // interface is a recoverable negotiation miss, not a crash.
        iface.url.parse::<http::Uri>().map_err(|e| {
            A2AError::InvalidParams(format!("invalid interface url {}: {e}", iface.url))
        })?;
        Ok(Box::new(super::http::HttpClient::new(iface.url.clone())))
    }
}

/// An ordered registry of [`TransportFactory`]s that negotiates a transport from
/// an agent card. Registration order is the client's preference order.
#[derive(Default)]
pub struct TransportNegotiator {
    factories: Vec<Box<dyn TransportFactory>>,
}

impl TransportNegotiator {
    /// An empty negotiator. Add factories with [`with`](Self::with).
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a factory (appended at lowest preference).
    pub fn with(mut self, factory: impl TransportFactory + 'static) -> Self {
        self.factories.push(Box::new(factory));
        self
    }

    /// The protocols this negotiator can construct, in preference order.
    pub fn supported(&self) -> impl Iterator<Item = &str> {
        self.factories.iter().map(|f| f.protocol())
    }

    /// Pick and construct the first transport that matches a card interface,
    /// ranked by client preference (registration order).
    pub async fn negotiate(&self, card: &AgentCard) -> Result<Box<dyn Transport>, A2AError> {
        for factory in &self.factories {
            for iface in &card.supported_interfaces {
                if iface.protocol_binding == factory.protocol()
                    && version_compatible(&iface.protocol_version)
                {
                    match factory.create(card, iface).await {
                        Ok(transport) => return Ok(transport),
                        Err(_err) => continue,
                    }
                }
            }
        }
        Err(A2AError::UnsupportedOperation(format!(
            "no compatible transport: client supports [{}], card offers [{}]",
            self.supported().collect::<Vec<_>>().join(", "),
            card.supported_interfaces
                .iter()
                .map(|i| i.protocol_binding.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )))
    }
}

/// Permissive major-version check: accept the v1.x line (or an unspecified
/// version). A future major version on an interface is skipped, not errored.
fn version_compatible(version: &str) -> bool {
    version.is_empty() || version.split('.').next() == Some("1")
}

/// The default registry, holding every transport compiled into this build.
///
/// Preference order is **CONNECTRPC then JSONRPC**: ConnectRPC is the in-tree,
/// first-class streaming transport, with JSON-RPC 2.0 as the interoperable
/// fallback. Flip the two `with` lines below for spec-default JSONRPC-first.
pub fn default_registry() -> TransportNegotiator {
    #[allow(unused_mut)]
    let mut negotiator = TransportNegotiator::new();
    #[cfg(feature = "http-client")]
    {
        negotiator = negotiator.with(ConnectRpcTransportFactory);
    }
    #[cfg(feature = "jsonrpc-client")]
    {
        negotiator = negotiator.with(JsonRpcTransportFactory);
    }
    negotiator
}

/// Fetch an agent's card and negotiate a transport in one step.
///
/// Fetches `/.well-known/agent-card.json` (falling back to `/agent-card`) from
/// `base_url`, then runs [`TransportNegotiator::negotiate`].
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub async fn connect(
    base_url: &str,
    negotiator: &TransportNegotiator,
) -> Result<Box<dyn Transport>, A2AError> {
    let card = fetch_agent_card(base_url).await?;
    negotiator.negotiate(&card).await
}

/// Validate `base_url`, negotiate a transport from the agent card, and fall back
/// to a direct client when the card can't be fetched or none of its interfaces
/// match a compiled-in transport.
///
/// This is the one-call ergonomic entry point shared by the CLI and the web
/// client: it validates the URL up front (so a malformed URL is a hard error),
/// tries [`connect`] with the [`default_registry`], and on any negotiation miss
/// falls back to a direct client on `base_url` so the call still works against a
/// bare agent URL with no published card. The fallback prefers the in-tree
/// ConnectRPC transport, using JSON-RPC 2.0 when ConnectRPC isn't compiled in.
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub async fn auto_connect(base_url: &str) -> Result<Box<dyn Transport>, A2AError> {
    // Validate URL format up front so a malformed URL is a hard error rather
    // than a silent fallback to a client that will fail on first request.
    reqwest::Url::parse(base_url)
        .map_err(|e| A2AError::InvalidParams(format!("invalid url {base_url}: {e}")))?;

    match connect(base_url, &default_registry()).await {
        Ok(transport) => Ok(transport),
        // Card fetch / negotiation failed — fall back to a direct client.
        Err(_) => Ok(direct_transport(base_url)),
    }
}

/// Build a direct client on `base_url`, preferring ConnectRPC when compiled in.
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
fn direct_transport(base_url: &str) -> Box<dyn Transport> {
    #[cfg(feature = "http-client")]
    {
        Box::new(super::http::HttpClient::new(base_url.to_string()))
    }
    #[cfg(all(not(feature = "http-client"), feature = "jsonrpc-client"))]
    {
        Box::new(super::jsonrpc_client::JsonRpcClient::new(
            base_url.to_string(),
        ))
    }
}

/// Fetch an [`AgentCard`] from the agent's well-known endpoint (plain HTTP GET).
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub async fn fetch_agent_card(base_url: &str) -> Result<AgentCard, A2AError> {
    use crate::adapter::error::HttpClientError;

    let client = reqwest::Client::new();
    let base = base_url.trim_end_matches('/');
    for path in [".well-known/agent-card.json", "agent-card"] {
        let url = format!("{base}/{path}");
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(HttpClientError::Reqwest)?;
        if resp.status().is_success() {
            return resp
                .json::<AgentCard>()
                .await
                .map_err(|e| A2AError::Internal(format!("Failed to parse agent card JSON: {e}")));
        }
    }
    Err(A2AError::Internal(format!(
        "Agent card not found at {base_url}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_v2_interface() {
        assert!(version_compatible("1.0"));
        assert!(version_compatible("")); // unspecified accepted
        assert!(!version_compatible("2.0"));
    }

    #[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
    #[tokio::test]
    async fn auto_connect_rejects_malformed_url() {
        // `Box<dyn Transport>` isn't `Debug`, so match rather than `unwrap_err`.
        match auto_connect("not-a-url").await {
            Err(A2AError::InvalidParams(_)) => {}
            Err(other) => panic!("wrong error: {other:?}"),
            Ok(_) => panic!("expected an error for a malformed url"),
        }
    }

    // A well-formed URL with no agent published there: card fetch fails, so
    // `auto_connect` must fall back to a direct client rather than erroring.
    // Port 1 is reserved/unroutable, so the GET fails fast.
    #[cfg(feature = "http-client")]
    #[tokio::test]
    async fn auto_connect_falls_back_to_direct_connectrpc() {
        let transport = auto_connect("http://127.0.0.1:1")
            .await
            .expect("fallback yields a direct transport");
        assert_eq!(transport.protocol(), "CONNECTRPC");
    }

    #[cfg(all(not(feature = "http-client"), feature = "jsonrpc-client"))]
    #[tokio::test]
    async fn auto_connect_falls_back_to_direct_jsonrpc() {
        let transport = auto_connect("http://127.0.0.1:1")
            .await
            .expect("fallback yields a direct transport");
        assert_eq!(transport.protocol(), "JSONRPC");
    }
}
