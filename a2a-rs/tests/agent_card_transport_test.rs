//! The served agent card must advertise the transport the server actually mounts.
//!
//! [`HttpServer`] mounts exactly one protocol — ConnectRPC — but a card built
//! from `SimpleAgentInfo::new` defaults its primary interface to the spec's
//! `JSONRPC`. Before this was fixed, every `HttpServer` published a card that
//! lied about itself, so `connect`/`auto_connect` negotiated to a JSON-RPC
//! endpoint that was never mounted and failed with a decode error.
//!
//! The regression guard is that the agent info below is built **plainly** — no
//! `with_preferred_transport` — exactly as a caller who doesn't know about the
//! footgun would write it.

#![cfg(all(feature = "http-server", feature = "http-client"))]

use a2a_rs::adapter::{ConnectRpcAdapter, HttpServer, SimpleAgentInfo};
use a2a_rs::domain::PROTOCOL_BINDING_CONNECTRPC;

mod common;
use common::TestBusinessHandler;

#[tokio::test]
async fn served_card_advertises_the_mounted_transport() {
    // Bind first: the card has to carry the url, so the port must be known
    // before the agent info is built — and a listener handed to `serve_on` is
    // already accepting, so there is nothing to wait for afterwards.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{addr}");

    let agent_info = SimpleAgentInfo::new("Card Transport Agent".to_string(), base_url.clone());
    let processor = ConnectRpcAdapter::with_handler(TestBusinessHandler::new(), agent_info.clone());
    let server = HttpServer::new(processor, agent_info, addr.to_string());

    let handle = tokio::spawn(async move { server.serve_on(listener).await });

    let card = a2a_rs::fetch_agent_card(&base_url)
        .await
        .expect("the bound server serves its card");

    assert_eq!(
        card.preferred_transport(),
        PROTOCOL_BINDING_CONNECTRPC,
        "HttpServer mounts ConnectRPC, so the card it serves must say so"
    );
    assert_eq!(
        card.url(),
        base_url,
        "stamping the binding must not clobber the primary interface's url"
    );

    // The payoff: a card-driven client negotiates to a transport that exists.
    let transport = a2a_rs::default_registry()
        .negotiate(&card)
        .await
        .expect("negotiation should find a compatible transport");
    assert_eq!(transport.protocol(), PROTOCOL_BINDING_CONNECTRPC);

    handle.abort();
}
