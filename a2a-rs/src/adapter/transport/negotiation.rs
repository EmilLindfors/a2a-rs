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
//!
//! A [`ClientConfig`] carries the settings that are not derivable from the card —
//! credentials and request timeout — through negotiation, so a negotiated client
//! is configured exactly like a hand-built one. Without it, authentication would
//! only be reachable by bypassing negotiation entirely.

use async_trait::async_trait;

#[cfg(feature = "http-client")]
use crate::domain::PROTOCOL_BINDING_CONNECTRPC;
#[cfg(feature = "jsonrpc-client")]
use crate::domain::PROTOCOL_BINDING_JSONRPC;
use crate::domain::{A2AError, AgentCard, AgentInterface};
use crate::port::Transport;

/// Client-side connection settings applied to every transport built during
/// negotiation, and to the agent-card fetch that precedes it.
///
/// The agent card describes *where and how* to reach an agent; this describes
/// what the caller brings to the call. Both are needed to build a usable client,
/// and only the card comes off the wire.
#[derive(Clone, Default)]
pub struct ClientConfig {
    auth_token: Option<String>,
    timeout_secs: Option<u64>,
}

/// Redacts the token. A derived `Debug` would print the credential verbatim,
/// and this type is exactly the thing a caller reaches for when tracing why a
/// connection did not authenticate — so the one line most likely to be written
/// is the one that must not carry the secret.
impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field(
                "auth_token",
                match &self.auth_token {
                    Some(_) => &"<redacted>",
                    None => &"None",
                },
            )
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

impl ClientConfig {
    /// Unauthenticated, transport-default timeout.
    pub fn new() -> Self {
        Self::default()
    }

    /// Send `token` as an HTTP `Authorization: Bearer` credential.
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    /// Bound each request to `secs` seconds.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// The bearer token, when one was set.
    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    /// The per-request timeout in seconds, when one was set.
    pub fn timeout_secs(&self) -> Option<u64> {
        self.timeout_secs
    }
}

/// Builds a [`Transport`] for a single wire protocol from an agent interface.
#[async_trait]
pub trait TransportFactory: Send + Sync {
    /// The protocol this factory handles, matching `AgentInterface::protocol_binding`
    /// (e.g. `"JSONRPC"`, `"CONNECTRPC"`).
    fn protocol(&self) -> &str;

    /// Construct a transport for `iface`, configured with `config`. Returning
    /// `Err` lets the negotiator fall through to the next compatible
    /// interface/factory.
    async fn create(
        &self,
        card: &AgentCard,
        iface: &AgentInterface,
        config: &ClientConfig,
    ) -> Result<Box<dyn Transport>, A2AError>;
}

/// Build a JSON-RPC client on `url` with `config` applied.
#[cfg(feature = "jsonrpc-client")]
fn jsonrpc_client(url: String, config: &ClientConfig) -> super::jsonrpc_client::JsonRpcClient {
    use super::jsonrpc_client::JsonRpcClient;

    let mut client = match config.auth_token() {
        Some(token) => JsonRpcClient::with_auth(url, token.to_string()),
        None => JsonRpcClient::new(url),
    };
    if let Some(secs) = config.timeout_secs() {
        client = client.with_timeout(secs);
    }
    client
}

/// Build a ConnectRPC client on `url` with `config` applied, reporting a URL
/// `http::Uri` cannot represent rather than panicking on it.
#[cfg(feature = "http-client")]
fn connect_rpc_client(
    url: String,
    config: &ClientConfig,
) -> Result<super::http::HttpClient, A2AError> {
    use super::http::HttpClient;

    let mut client = match config.auth_token() {
        Some(token) => HttpClient::try_with_auth(url, token.to_string())?,
        None => HttpClient::try_new(url)?,
    };
    if let Some(secs) = config.timeout_secs() {
        client = client.with_timeout(secs);
    }
    Ok(client)
}

/// Factory for the wire-compatible JSON-RPC 2.0 transport.
#[cfg(feature = "jsonrpc-client")]
pub struct JsonRpcTransportFactory;

#[cfg(feature = "jsonrpc-client")]
#[async_trait]
impl TransportFactory for JsonRpcTransportFactory {
    fn protocol(&self) -> &str {
        PROTOCOL_BINDING_JSONRPC
    }

    async fn create(
        &self,
        _card: &AgentCard,
        iface: &AgentInterface,
        config: &ClientConfig,
    ) -> Result<Box<dyn Transport>, A2AError> {
        Ok(Box::new(jsonrpc_client(iface.url.clone(), config)))
    }
}

/// Factory for the ConnectRPC transport.
#[cfg(feature = "http-client")]
pub struct ConnectRpcTransportFactory;

#[cfg(feature = "http-client")]
#[async_trait]
impl TransportFactory for ConnectRpcTransportFactory {
    fn protocol(&self) -> &str {
        PROTOCOL_BINDING_CONNECTRPC
    }

    async fn create(
        &self,
        _card: &AgentCard,
        iface: &AgentInterface,
        config: &ClientConfig,
    ) -> Result<Box<dyn Transport>, A2AError> {
        // A URL the ConnectRPC client cannot represent is a recoverable
        // negotiation miss — the negotiator falls through to the next
        // interface — rather than a crash.
        Ok(Box::new(connect_rpc_client(iface.url.clone(), config)?))
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
    /// ranked by client preference (registration order), with default settings.
    pub async fn negotiate(&self, card: &AgentCard) -> Result<Box<dyn Transport>, A2AError> {
        self.negotiate_with(card, &ClientConfig::default()).await
    }

    /// As [`negotiate`](Self::negotiate), configuring the chosen transport with
    /// `config`.
    pub async fn negotiate_with(
        &self,
        card: &AgentCard,
        config: &ClientConfig,
    ) -> Result<Box<dyn Transport>, A2AError> {
        self.select(card, config)
            .await
            .map(|(transport, _iface)| transport)
    }

    /// [`negotiate_with`](Self::negotiate_with), also returning the interface
    /// the transport was built from — which is where it will send every
    /// request, and the one fact a caller holding a *different* address for
    /// the agent needs in order to explain a failure.
    async fn select<'c>(
        &self,
        card: &'c AgentCard,
        config: &ClientConfig,
    ) -> Result<(Box<dyn Transport>, &'c AgentInterface), A2AError> {
        for factory in &self.factories {
            for iface in &card.supported_interfaces {
                if iface.protocol_binding == factory.protocol()
                    && version_compatible(&iface.protocol_version)
                {
                    match factory.create(card, iface, config).await {
                        Ok(transport) => return Ok((transport, iface)),
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
    connect_with(base_url, negotiator, &ClientConfig::default()).await
}

/// As [`connect`], applying `config` to both the card fetch and the negotiated
/// transport.
///
/// The card fetch carries the credentials too: an agent that authenticates its
/// RPC endpoints usually authenticates the card endpoint as well, and a fetch
/// that 401s would otherwise sink the whole negotiation.
///
/// # Which URL the transport dials
///
/// `base_url` is where the *card* is fetched from. The transport dials the
/// interface URL the card advertises, which is the card's to decide — an
/// agent may serve JSON-RPC on a sub-path, or its card through a gateway that
/// does not forward RPC — so the address a caller holds does not override it.
/// When the two disagree in origin, an agent whose advertised URL is wrong
/// (korps' `advertised_url`, say) fails every request even for a caller
/// holding its real address; that is logged here, at the one moment both
/// URLs are known, because the failure that follows names only the one that
/// was dialed.
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub async fn connect_with(
    base_url: &str,
    negotiator: &TransportNegotiator,
    config: &ClientConfig,
) -> Result<Box<dyn Transport>, A2AError> {
    let card = fetch_agent_card_with(base_url, config).await?;
    let (transport, iface) = negotiator.select(&card, config).await?;
    if !same_origin(base_url, &iface.url) {
        #[cfg(feature = "tracing")]
        tracing::warn!(
            card_url = %base_url,
            interface_url = %iface.url,
            protocol = %iface.protocol_binding,
            "the agent card advertises its interface at a different origin than it was fetched from; requests go to the advertised URL"
        );
    }
    Ok(transport)
}

/// Whether two URLs name the same scheme, host and port — the part of an
/// address that decides which server answers. Paths are the card's business
/// (an interface on a sub-path is normal); an unparseable URL is reported as a
/// mismatch, since it cannot be the same server as a parseable one.
fn same_origin(a: &str, b: &str) -> bool {
    match (reqwest::Url::parse(a), reqwest::Url::parse(b)) {
        (Ok(a), Ok(b)) => {
            a.scheme() == b.scheme()
                && a.host_str() == b.host_str()
                && a.port_or_known_default() == b.port_or_known_default()
        }
        _ => false,
    }
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
///
/// The fallback is for a card that cannot be *fetched or negotiated*, not for
/// one that is wrong: when the card is served and names an interface this
/// client speaks, requests go to the URL the card advertises, whatever
/// `base_url` was — see [`connect_with`] for why, and for the warning logged
/// when the two disagree.
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub async fn auto_connect(base_url: &str) -> Result<Box<dyn Transport>, A2AError> {
    auto_connect_with(base_url, &ClientConfig::default()).await
}

/// As [`auto_connect`], applying `config` to the card fetch, the negotiated
/// transport, **and** the direct-client fallback.
///
/// This is the entry point a CLI or a web client wants whenever credentials are
/// in play: every path out of it yields a configured client, so `--auth` behaves
/// the same whether the card negotiated or the fallback fired.
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub async fn auto_connect_with(
    base_url: &str,
    config: &ClientConfig,
) -> Result<Box<dyn Transport>, A2AError> {
    // Validate URL format up front so a malformed URL is a hard error rather
    // than a silent fallback to a client that will fail on first request.
    reqwest::Url::parse(base_url)
        .map_err(|e| A2AError::InvalidParams(format!("invalid url {base_url}: {e}")))?;

    match connect_with(base_url, &default_registry(), config).await {
        Ok(transport) => Ok(transport),
        // Card fetch / negotiation failed — fall back to a direct client.
        Err(_) => direct_transport(base_url, config),
    }
}

/// Build a direct client on `base_url`, preferring ConnectRPC when compiled in.
///
/// Fallible because `reqwest::Url` — which [`auto_connect_with`] validates with
/// — is *more* permissive than the `http::Uri` the ConnectRPC client needs:
/// `http://münchen.de` parses as the former and not the latter. Building
/// infallibly here turned that gap into a panic on the fallback path, which is
/// the path a bad URL is most likely to reach in the first place.
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
fn direct_transport(base_url: &str, config: &ClientConfig) -> Result<Box<dyn Transport>, A2AError> {
    #[cfg(feature = "http-client")]
    {
        Ok(Box::new(connect_rpc_client(base_url.to_string(), config)?))
    }
    #[cfg(all(not(feature = "http-client"), feature = "jsonrpc-client"))]
    {
        Ok(Box::new(jsonrpc_client(base_url.to_string(), config)))
    }
}

/// Fetch an [`AgentCard`] from the agent's well-known endpoint (plain HTTP GET).
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub async fn fetch_agent_card(base_url: &str) -> Result<AgentCard, A2AError> {
    fetch_agent_card_with(base_url, &ClientConfig::default()).await
}

/// As [`fetch_agent_card`], sending `config`'s credentials and honouring its
/// timeout — for agents that guard the card endpoint too.
#[cfg(any(feature = "http-client", feature = "jsonrpc-client"))]
pub async fn fetch_agent_card_with(
    base_url: &str,
    config: &ClientConfig,
) -> Result<AgentCard, A2AError> {
    use crate::adapter::error::HttpClientError;

    let client = reqwest::Client::new();
    let base = base_url.trim_end_matches('/');
    for path in [".well-known/agent-card.json", "agent-card"] {
        let url = format!("{base}/{path}");
        let mut request = client.get(&url);
        if let Some(token) = config.auth_token() {
            request = request.bearer_auth(token);
        }
        if let Some(secs) = config.timeout_secs() {
            request = request.timeout(std::time::Duration::from_secs(secs));
        }
        let resp = request.send().await.map_err(HttpClientError::Reqwest)?;
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

    /// The origin is what decides which server answers; the path is the
    /// card's to choose and a port left implicit is the same port.
    #[test]
    fn same_origin_compares_scheme_host_and_port_only() {
        assert!(same_origin(
            "http://127.0.0.1:8081",
            "http://127.0.0.1:8081/jsonrpc"
        ));
        assert!(same_origin(
            "https://agent.example",
            "https://agent.example:443/"
        ));
        assert!(!same_origin(
            "http://127.0.0.1:8081",
            "http://agent.internal:8081"
        ));
        assert!(!same_origin(
            "http://127.0.0.1:8081",
            "http://127.0.0.1:8082"
        ));
        assert!(!same_origin(
            "http://127.0.0.1:8081",
            "https://127.0.0.1:8081"
        ));
        assert!(!same_origin("http://127.0.0.1:8081", "not a url"));
    }

    #[test]
    fn rejects_v2_interface() {
        assert!(version_compatible("1.0"));
        assert!(version_compatible("")); // unspecified accepted
        assert!(!version_compatible("2.0"));
    }

    /// A `Debug` line must not carry the credential. `ClientConfig` is what a
    /// caller reaches for when tracing an authentication failure, so the most
    /// likely line to be logged is the one that would have leaked the token.
    #[test]
    fn debug_redacts_the_token() {
        let rendered = format!("{:?}", ClientConfig::new().with_auth_token("s3cret"));
        assert!(!rendered.contains("s3cret"), "token leaked: {rendered}");
        assert!(rendered.contains("redacted"), "{rendered}");

        // …and says so only when there is one to hide.
        let rendered = format!("{:?}", ClientConfig::new());
        assert!(!rendered.contains("redacted"), "{rendered}");
    }

    /// `reqwest::Url` accepts an IDN host and normalizes it to punycode;
    /// `http::Uri` rejects the raw bytes. `auto_connect_with` validates with the
    /// former and the fallback built with the latter, so a URL that had just
    /// been declared valid panicked one line later.
    #[cfg(feature = "http-client")]
    #[tokio::test]
    async fn idn_url_is_an_error_not_a_panic() {
        assert!(
            reqwest::Url::parse("http://münchen.de").is_ok(),
            "premise: the up-front validation accepts this"
        );
        match auto_connect("http://münchen.de").await {
            Err(A2AError::InvalidParams(_)) => {}
            Err(other) => panic!("wrong error: {other:?}"),
            Ok(_) => panic!("expected an error for a url http::Uri cannot represent"),
        }
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
