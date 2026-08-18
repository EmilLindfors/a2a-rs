//! `OAuth2Authenticator` against a live introspection endpoint.
//!
//! The property under test is the one that decides whether an agent can own
//! anything per caller: two different access tokens for the same user have to
//! authenticate as the *same* principal, because a refresh replaces the token
//! and nothing else. That only works if the identity comes from the
//! authorization server's `sub` rather than from the bearer string.

#![cfg(all(feature = "auth", feature = "http-server"))]

use std::collections::HashMap;

use a2a_rs::adapter::OAuth2Authenticator;
use a2a_rs::port::{AuthContext, Authenticator};
use axum::{Form, Json, Router, routing::post};
use oauth2::{ClientId, ClientSecret, IntrospectionUrl, TokenUrl};
use serde::Deserialize;

#[derive(Deserialize)]
struct IntrospectRequest {
    token: String,
}

/// Stands in for the authorization server. Every token beginning with `live-`
/// belongs to the same user, which is what a refresh looks like from here; the
/// rest are answers a real server gives that the authenticator has to handle.
async fn introspect(Form(request): Form<IntrospectRequest>) -> Json<serde_json::Value> {
    let body = match request.token.as_str() {
        token if token.starts_with("live-") => serde_json::json!({
            "active": true,
            "sub": "user-42",
            "username": "kari",
            "client_id": "agent-client",
            "scope": "a2a:read a2a:write",
            "exp": 4_102_444_800i64,
        }),
        // A machine token: no end user, and `client_id` is the honest subject.
        "machine" => serde_json::json!({ "active": true, "client_id": "batch-runner" }),
        // Active, and the server will not say whose it is.
        "anonymous" => serde_json::json!({ "active": true }),
        _ => serde_json::json!({ "active": false }),
    };
    Json(body)
}

/// A running introspection endpoint, plus an authenticator pointed at it.
async fn authenticator() -> OAuth2Authenticator {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/introspect", listener.local_addr().unwrap());
    let app = Router::new().route("/introspect", post(introspect));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    OAuth2Authenticator::new_client_credentials(
        ClientId::new("agent-client".to_string()),
        ClientSecret::new("shh".to_string()),
        TokenUrl::new("http://localhost/token".to_string()).unwrap(),
        HashMap::new(),
    )
    .with_introspection(IntrospectionUrl::new(url).unwrap())
    .expect("the introspection client builds")
}

fn presenting(token: &str) -> AuthContext {
    AuthContext::new("oauth2".to_string(), token.to_string())
}

/// The whole point: the principal is the subject the server named, and the
/// credential is not in it.
#[tokio::test]
async fn the_principal_is_the_subject_not_the_token() {
    let principal = authenticator()
        .await
        .authenticate(&presenting("live-abc"))
        .await
        .expect("an active token authenticates");

    assert_eq!(principal.id, "user-42");
    assert_eq!(principal.scheme, "oauth2");
    assert_eq!(
        principal.attributes.get("scope").map(String::as_str),
        Some("a2a:read a2a:write")
    );
}

/// A refreshed token is a different string for the same person. An agent that
/// keyed anything on the credential would hand them a clean slate here.
#[tokio::test]
async fn a_refreshed_token_is_the_same_caller() {
    let authenticator = authenticator().await;

    let before = authenticator
        .authenticate(&presenting("live-first"))
        .await
        .unwrap();
    let after = authenticator
        .authenticate(&presenting("live-second"))
        .await
        .unwrap();

    assert_eq!(before.id, after.id);
}

/// A client-credentials token has no end user, and the client it was issued to
/// is a stable identity that outlives the token just as well.
#[tokio::test]
async fn a_machine_token_is_identified_by_its_client() {
    let principal = authenticator()
        .await
        .authenticate(&presenting("machine"))
        .await
        .expect("an active machine token authenticates");

    assert_eq!(principal.id, "batch-runner");
}

#[tokio::test]
async fn an_inactive_token_is_refused() {
    let error = authenticator()
        .await
        .authenticate(&presenting("revoked"))
        .await
        .expect_err("an inactive token must not authenticate");

    assert!(error.to_string().contains("Invalid OAuth2 access token"));
}

/// Falling back to the token here would put the credential back in the
/// principal id — the thing this whole path exists to avoid — and it would do
/// it silently, on the one server that gave us no identity to use.
#[tokio::test]
async fn a_response_with_no_subject_is_an_error() {
    let error = authenticator()
        .await
        .authenticate(&presenting("anonymous"))
        .await
        .expect_err("a token nobody will claim must not authenticate");

    let message = error.to_string();
    assert!(message.contains("no subject"), "{message}");
}

/// The static token list is a development convenience, and it is not consulted
/// once there is a server to ask: a list cannot know about a revocation.
#[tokio::test]
async fn a_statically_allowed_token_does_not_bypass_introspection() {
    let authenticator = authenticator()
        .await
        .with_valid_tokens(vec!["revoked".to_string()]);

    assert!(
        authenticator
            .authenticate(&presenting("revoked"))
            .await
            .is_err()
    );
}
