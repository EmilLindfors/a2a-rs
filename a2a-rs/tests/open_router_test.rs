//! Routes handed to `HttpServer::with_open_router` are served outside the
//! authenticator; everything else stays behind it.
//!
//! The case this exists for is a callback endpoint: a webhook receiver that
//! validates its own per-task token. Mounting it inside the auth middleware
//! would mean the caller-back has to hold the agent's own credentials — so the
//! open router is merged *after* the middleware is applied, and this test pins
//! both halves: the open route answers with no credentials, and the agent's
//! RPCs still refuse exactly as they did before the merge. The public card is
//! merged the same way, so it is read without credentials too.

#![cfg(all(feature = "http-server", feature = "http-client"))]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{Router, http::StatusCode, routing::post};

use a2a_rs::adapter::{BearerTokenAuthenticator, ConnectRpcAdapter, HttpServer, SimpleAgentInfo};

mod common;
use common::TestBusinessHandler;

/// A callback route that counts how often it was reached.
fn counting_router(hits: Arc<AtomicUsize>) -> Router {
    Router::new().route(
        "/callback",
        post(move || {
            hits.fetch_add(1, Ordering::SeqCst);
            async { StatusCode::ACCEPTED }
        }),
    )
}

#[tokio::test]
async fn an_open_route_bypasses_the_authenticator_and_the_agent_routes_do_not() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{addr}");

    let agent_info = SimpleAgentInfo::new("Open Router Agent".to_string(), base_url.clone());
    let processor = ConnectRpcAdapter::with_handler(TestBusinessHandler::new(), agent_info.clone());
    let hits = Arc::new(AtomicUsize::new(0));
    let server = HttpServer::with_auth(
        processor,
        agent_info,
        addr.to_string(),
        BearerTokenAuthenticator::new(vec!["agent-secret".to_string()]),
    )
    .with_open_router(counting_router(hits.clone()));

    let _serving = tokio::spawn(async move { server.serve_on(listener).await });
    let client = reqwest::Client::new();

    // The open route answers with no credentials at all.
    let callback = client
        .post(format!("{base_url}/callback"))
        .send()
        .await
        .expect("the open route is reachable");
    assert_eq!(callback.status(), reqwest::StatusCode::ACCEPTED);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "the handler actually ran");

    // The agent's own RPCs still sit behind the authenticator.
    let list = |token: Option<&str>| {
        let mut request = client
            .post(format!("{base_url}/lf.a2a.v1.A2AService/ListTasks"))
            .header("content-type", "application/json")
            .body("{}");
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        request.send()
    };
    let refused = list(None).await.expect("the RPC route is reachable");
    assert_eq!(
        refused.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "merging the open router must not lift the auth middleware off the agent's routes"
    );
    let answered = list(Some("agent-secret"))
        .await
        .expect("the RPC route is reachable");
    assert_ne!(answered.status(), reqwest::StatusCode::UNAUTHORIZED);
}

/// The public card and its skills are read with no credential on a server
/// behind an authenticator, so a client can learn the scheme it signs in
/// with, and a fleet can tell the agent is up.
#[tokio::test]
async fn the_public_card_is_read_without_signing_in() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{addr}");

    let agent_info = SimpleAgentInfo::new("Guarded Agent".to_string(), base_url.clone());
    let processor = ConnectRpcAdapter::with_handler(TestBusinessHandler::new(), agent_info.clone());
    let server = HttpServer::with_auth(
        processor,
        agent_info,
        addr.to_string(),
        BearerTokenAuthenticator::new(vec!["agent-secret".to_string()]),
    );
    let _serving = tokio::spawn(async move { server.serve_on(listener).await });
    let client = reqwest::Client::new();

    for path in ["/.well-known/agent-card.json", "/agent-card", "/skills"] {
        let res = client
            .get(format!("{base_url}{path}"))
            .send()
            .await
            .expect("the route is reachable");
        assert_eq!(res.status(), reqwest::StatusCode::OK, "{path}");
    }
    let card: serde_json::Value = client
        .get(format!("{base_url}/.well-known/agent-card.json"))
        .send()
        .await
        .expect("the card is reachable")
        .json()
        .await
        .expect("the card is JSON");
    assert_eq!(card["name"], "Guarded Agent");
}

/// The unauthenticated constructor path serves the open routes too — the
/// feature is about *where* the routes mount, not about auth being configured.
#[tokio::test]
async fn an_open_route_is_served_on_an_unauthenticated_server_too() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{addr}");

    let agent_info = SimpleAgentInfo::new("Open Router Agent".to_string(), base_url.clone());
    let processor = ConnectRpcAdapter::with_handler(TestBusinessHandler::new(), agent_info.clone());
    let hits = Arc::new(AtomicUsize::new(0));
    let server = HttpServer::new(processor, agent_info, addr.to_string())
        .with_open_router(counting_router(hits.clone()));

    let _serving = tokio::spawn(async move { server.serve_on(listener).await });

    let callback = reqwest::Client::new()
        .post(format!("{base_url}/callback"))
        .send()
        .await
        .expect("the open route is reachable");
    assert_eq!(callback.status(), reqwest::StatusCode::ACCEPTED);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}
