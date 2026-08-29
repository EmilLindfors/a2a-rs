//! The middleware reads the credential the authenticator says it takes.
//!
//! An `Authenticator` declares a security scheme — the port requires
//! `security_scheme()`, and the same value is what goes on the agent card — and
//! then refuses any context labelled with a different one. The middleware used
//! to extract with a hard-coded `BearerTokenExtractor` regardless, so three of
//! the five authenticators could not be reached through `with_auth` at all:
//! an API key, an OAuth2 access token and an OIDC ID token were each extracted
//! as `bearer` and refused by the authenticator that had just been configured
//! to accept them. A server wired that way answered 401 to every request,
//! valid credentials included, and looked from the outside exactly like a
//! client sending the wrong token.
//!
//! Each test here is the pair that catches it: the right credential is
//! accepted *and* a plausible wrong one is refused. Either alone passes on a
//! server that refuses everybody.

#![cfg(all(feature = "jsonrpc-server", feature = "http-server"))]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode, header::CONTENT_TYPE};
use serde_json::json;
use tower::ServiceExt;

use a2a_rs::adapter::streaming::InMemoryStreamingHandler;
use a2a_rs::adapter::{
    ApiKeyAuthenticator, BearerTokenAuthenticator, InMemoryTaskStorage, JsonRpcAdapter,
    SimpleAgentInfo, jsonrpc_router, with_auth,
};
use a2a_rs::domain::{A2AError, Message, Task, TaskState, TaskStatus};
use a2a_rs::port::{AsyncMessageHandler, Authenticator, RequestContext};

/// Records the caller of every call that reached it, so a test can tell "the
/// request was authenticated" from "the request was refused".
#[derive(Clone, Default)]
struct RecordingHandler {
    callers: Arc<Mutex<Vec<Option<String>>>>,
}

impl RecordingHandler {
    fn callers(&self) -> Vec<Option<String>> {
        self.callers.lock().expect("not poisoned").clone()
    }
}

#[async_trait]
impl AsyncMessageHandler for RecordingHandler {
    async fn process_message(
        &self,
        task_id: &str,
        _message: &Message,
        ctx: &RequestContext,
    ) -> Result<Task, A2AError> {
        self.callers
            .lock()
            .expect("not poisoned")
            .push(ctx.caller().map(str::to_string));
        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id("ctx-1".to_string())
            .status(TaskStatus::new(TaskState::Completed, None))
            .build())
    }
}

/// A router serving `handler` behind `authenticator`.
fn served(handler: RecordingHandler, authenticator: impl Authenticator + 'static) -> axum::Router {
    let storage = InMemoryTaskStorage::new();
    let adapter = Arc::new(
        JsonRpcAdapter::new(
            handler,
            storage.clone(),
            storage,
            SimpleAgentInfo::new("scheme-test".to_string(), "http://localhost".to_string()),
        )
        .with_streaming_handler(InMemoryStreamingHandler::new()),
    );
    with_auth(jsonrpc_router(adapter), authenticator)
}

/// A `SendMessage` carrying the given headers.
fn post(headers: &[(&str, &str)]) -> Request<Body> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "SendMessage",
        "params": {
            "message": {
                "messageId": "m1",
                "taskId": "t1",
                "contextId": "ctx-1",
                "role": "ROLE_USER",
                "parts": [{ "text": "hello" }],
            }
        }
    });
    let mut req = Request::post("/").header(CONTENT_TYPE, "application/json");
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    req.body(Body::from(body.to_string())).expect("request")
}

// --- API key -----------------------------------------------------------------

/// The header the config names is the header that is read. This is the case
/// that was impossible before: the key never left `X-API-Key`, and the
/// `Authorization` header the middleware did read was empty.
#[tokio::test]
async fn an_api_key_in_its_own_header_authenticates() {
    let handler = RecordingHandler::default();
    let app = served(
        handler.clone(),
        ApiKeyAuthenticator::header(vec!["k-alice".to_string()], "X-API-Key".to_string()),
    );

    let response = app
        .oneshot(post(&[("x-api-key", "k-alice")]))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        handler.callers(),
        vec![Some("k-alice".to_string())],
        "the key the middleware accepted has to reach the handler as the caller"
    );
}

/// And a wrong key in the right header is still refused, so the test above is
/// about the credential rather than about a server that accepts anything.
#[tokio::test]
async fn a_wrong_api_key_is_refused() {
    let handler = RecordingHandler::default();
    let app = served(
        handler.clone(),
        ApiKeyAuthenticator::header(vec!["k-alice".to_string()], "X-API-Key".to_string()),
    );

    let response = app
        .oneshot(post(&[("x-api-key", "k-mallory")]))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        handler.callers().is_empty(),
        "the call must not be dispatched"
    );
}

/// An API key server does not accept a bearer token that happens to hold the
/// key. The scheme is part of the credential, not decoration.
#[tokio::test]
async fn an_api_key_server_refuses_a_bearer_token() {
    let handler = RecordingHandler::default();
    let app = served(
        handler.clone(),
        ApiKeyAuthenticator::header(vec!["k-alice".to_string()], "X-API-Key".to_string()),
    );

    let response = app
        .oneshot(post(&[("authorization", "Bearer k-alice")]))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        handler.callers().is_empty(),
        "the call must not be dispatched"
    );
}

// --- bearer ------------------------------------------------------------------

/// The scheme that always worked, kept working: deriving the extractor must
/// not have moved the case everyone is already deployed on.
#[tokio::test]
async fn a_bearer_token_still_authenticates() {
    let handler = RecordingHandler::default();
    let app = served(
        handler.clone(),
        BearerTokenAuthenticator::new(vec!["t-alice".to_string()]),
    );

    let response = app
        .oneshot(post(&[("authorization", "Bearer t-alice")]))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(handler.callers(), vec![Some("t-alice".to_string())]);
}

// --- OAuth2 ------------------------------------------------------------------

/// An OAuth2 access token reaches an `OAuth2Authenticator`.
///
/// `with_valid_tokens` stands in for an authorization server, which is what it
/// is for: what is under test is the extraction, not the introspection.
#[cfg(feature = "auth")]
#[tokio::test]
async fn an_oauth2_access_token_authenticates() {
    use a2a_rs::adapter::OAuth2Authenticator;
    use oauth2::{ClientId, ClientSecret, TokenUrl};

    let handler = RecordingHandler::default();
    let authenticator = OAuth2Authenticator::new_client_credentials(
        ClientId::new("client".to_string()),
        ClientSecret::new("secret".to_string()),
        TokenUrl::new("https://issuer.example.com/token".to_string()).expect("valid URL"),
        Default::default(),
    )
    .with_valid_tokens(vec!["access-alice".to_string()]);
    let app = served(handler.clone(), authenticator);

    let response = app
        .oneshot(post(&[("authorization", "Bearer access-alice")]))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        handler.callers(),
        vec![Some("oauth2:access-alice".to_string())],
        "an OAuth2 server with no introspection names the credential itself"
    );
}

/// And an unknown access token is refused, so the test above is about the
/// token rather than about a server that lets everyone in.
#[cfg(feature = "auth")]
#[tokio::test]
async fn an_unknown_oauth2_access_token_is_refused() {
    use a2a_rs::adapter::OAuth2Authenticator;
    use oauth2::{ClientId, ClientSecret, TokenUrl};

    let handler = RecordingHandler::default();
    let authenticator = OAuth2Authenticator::new_client_credentials(
        ClientId::new("client".to_string()),
        ClientSecret::new("secret".to_string()),
        TokenUrl::new("https://issuer.example.com/token".to_string()).expect("valid URL"),
        Default::default(),
    )
    .with_valid_tokens(vec!["access-alice".to_string()]);
    let app = served(handler.clone(), authenticator);

    let response = app
        .oneshot(post(&[("authorization", "Bearer access-mallory")]))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        handler.callers().is_empty(),
        "the call must not be dispatched"
    );
}
