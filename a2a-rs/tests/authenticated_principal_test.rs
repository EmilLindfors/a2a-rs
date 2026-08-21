//! The authenticated principal reaches the message handler.
//!
//! Authentication happens in an axum middleware, the message handler runs three
//! layers below it, and until the [`RequestContext`] was threaded through the
//! two never met: the middleware authenticated and dropped the principal on the
//! floor. A handler keeping per-caller state — the conversation store's context
//! owner is the one in tree — therefore saw every caller as the same anonymous
//! nobody.
//!
//! These tests stand up the real router with the real middleware and assert on
//! what the handler was told, because every intermediate layer here forwards a
//! value it does not itself read, and a forwarding bug is invisible from either
//! end alone.

#![cfg(all(feature = "jsonrpc-server", feature = "auth"))]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header::CONTENT_TYPE};
use serde_json::json;
use tower::ServiceExt;

use a2a_rs::adapter::streaming::InMemoryStreamingHandler;
use a2a_rs::adapter::{
    BearerTokenAuthenticator, InMemoryTaskStorage, JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router,
    with_auth,
};
use a2a_rs::domain::{A2AError, Message, Task, TaskState, TaskStatus};
use a2a_rs::port::{AsyncMessageHandler, RequestContext};

/// What one call was told about who was asking: the principal's id and the
/// session id, both as the handler saw them.
type Seen = (Option<String>, Option<String>);

/// Records what each call was told about its caller.
#[derive(Clone, Default)]
struct RecordingHandler {
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl RecordingHandler {
    /// Every call so far, in order.
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("not poisoned").clone()
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
        self.seen.lock().expect("not poisoned").push((
            ctx.caller().map(str::to_string),
            ctx.session_id().map(str::to_string),
        ));
        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id("ctx-1".to_string())
            .status(TaskStatus::new(TaskState::Completed, None))
            .build())
    }
}

fn adapter(handler: RecordingHandler) -> Arc<JsonRpcAdapter> {
    let storage = InMemoryTaskStorage::new();
    Arc::new(
        JsonRpcAdapter::new(
            handler,
            storage.clone(),
            storage,
            SimpleAgentInfo::new("principal-test".to_string(), "http://localhost".to_string()),
        )
        // A real streaming backend, or the streaming method fails before the
        // handler is ever called and the test would pass on the wrong reason.
        .with_streaming_handler(InMemoryStreamingHandler::new()),
    )
}

fn send_message_body() -> serde_json::Value {
    json!({
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
    })
}

fn post(body: serde_json::Value, bearer: Option<&str>) -> Request<Body> {
    let mut req = Request::post("/").header(CONTENT_TYPE, "application/json");
    if let Some(token) = bearer {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    req.body(Body::from(body.to_string())).expect("request")
}

/// The headline: a token the authenticator accepts becomes a principal id the
/// handler can read.
#[tokio::test]
async fn the_handler_sees_the_authenticated_caller() {
    let handler = RecordingHandler::default();
    let app = with_auth(
        jsonrpc_router(adapter(handler.clone())),
        BearerTokenAuthenticator::new(vec!["alice-token".to_string()]),
    );

    let response = app
        .oneshot(post(send_message_body(), Some("alice-token")))
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(
        handler.seen(),
        vec![(Some("alice-token".to_string()), Some("ctx-1".to_string()))],
        "the principal the middleware authenticated has to reach the handler"
    );
}

/// A rejected token never reaches the handler at all — the middleware answers
/// 401 first. Worth pinning: the principal plumbing must not turn a refusal into
/// a call with no caller.
#[tokio::test]
async fn a_bad_token_never_reaches_the_handler() {
    let handler = RecordingHandler::default();
    let app = with_auth(
        jsonrpc_router(adapter(handler.clone())),
        BearerTokenAuthenticator::new(vec!["alice-token".to_string()]),
    );

    let response = app
        .oneshot(post(send_message_body(), Some("not-a-token")))
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(handler.seen().is_empty(), "the call must not be dispatched");
}

/// Without a middleware there is no principal, and that is a `None` caller
/// rather than a failure: an agent that does not authenticate still serves.
#[tokio::test]
async fn an_unauthenticated_server_names_no_caller() {
    let handler = RecordingHandler::default();
    let app = jsonrpc_router(adapter(handler.clone()));

    let response = app
        .oneshot(post(send_message_body(), None))
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(handler.seen(), vec![(None, Some("ctx-1".to_string()))]);
}

/// Two callers against the same endpoint are two different principals — the
/// distinction the conversation store's owner check depends on.
#[tokio::test]
async fn two_callers_are_told_apart() {
    let handler = RecordingHandler::default();
    let router = jsonrpc_router(adapter(handler.clone()));
    let app = with_auth(
        router,
        BearerTokenAuthenticator::new(vec!["alice-token".to_string(), "bob-token".to_string()]),
    );

    for token in ["alice-token", "bob-token"] {
        let response = app
            .clone()
            .oneshot(post(send_message_body(), Some(token)))
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let callers: Vec<_> = handler.seen().into_iter().map(|(who, _)| who).collect();
    assert_eq!(
        callers,
        vec![
            Some("alice-token".to_string()),
            Some("bob-token".to_string())
        ]
    );
}

/// The streaming path builds its own context and had to be threaded separately.
#[tokio::test]
async fn the_streaming_path_carries_the_caller_too() {
    let handler = RecordingHandler::default();
    let app = with_auth(
        jsonrpc_router(adapter(handler.clone())),
        BearerTokenAuthenticator::new(vec!["alice-token".to_string()]),
    );

    let mut body = send_message_body();
    body["method"] = json!("SendStreamingMessage");

    let response = app
        .oneshot(post(body, Some("alice-token")))
        .await
        .expect("router responds");

    // The stream itself is not the point here; the handler call behind it is.
    // Drain it so the response is complete before the assertion.
    let _ = to_bytes(response.into_body(), 64 * 1024).await;

    assert_eq!(
        handler.seen().first().map(|(who, _)| who.clone()),
        Some(Some("alice-token".to_string())),
        "the SSE path decodes its own request and has its own place to drop the principal"
    );
}

/// The ConnectRPC path, over a real socket.
///
/// This is the transport korps actually serves, and its principal takes a
/// different route than the JSON-RPC one: the middleware puts it in the request
/// extensions, and `connectrpc` moves those onto its own `Context`. Nothing in
/// the unit tests above covers that handoff, and it is exactly the kind of
/// passthrough an upstream bump can drop.
#[cfg(feature = "http-client")]
#[tokio::test]
async fn the_connectrpc_path_carries_the_caller_over_a_socket() {
    use a2a_rs::adapter::transport::http::HttpClient;
    use a2a_rs::adapter::{ConnectRpcAdapter, HttpServer};
    use a2a_rs::domain::SendCompletion;
    use a2a_rs::port::Transport;

    let handler = RecordingHandler::default();
    let storage = InMemoryTaskStorage::new();

    // Bind before serving. A hard-coded port collides with whatever else the
    // workspace run has bound, and sleeping for the server to come up is a
    // guess that gets shorter under load — both of which this test did, and
    // flaked on. The listener queues connections from the bind, so the client
    // below can connect before `serve_on` is first polled.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{addr}");

    let agent_info = SimpleAgentInfo::new("principal-connect-test".to_string(), base_url.clone());
    let processor = ConnectRpcAdapter::new(
        handler.clone(),
        storage.clone(),
        storage,
        agent_info.clone(),
    );
    let server = HttpServer::with_auth(
        processor,
        agent_info,
        addr.to_string(),
        BearerTokenAuthenticator::new(vec!["alice-token".to_string()]),
    );

    let (shutdown, stop) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(async move {
        tokio::select! {
            _ = server.serve_on(listener) => {}
            _ = stop => {}
        }
    });

    let client = HttpClient::with_auth(base_url, "alice-token".to_string());
    let message = Message::user_text("hello".to_string(), "m1".to_string());
    client
        .send_task_message(
            Some("t1"),
            &message,
            None,
            None,
            SendCompletion::WhenCreated,
        )
        .await
        .expect("the authenticated call is served");

    assert_eq!(
        handler.seen().first().map(|(who, _)| who.clone()),
        Some(Some("alice-token".to_string())),
        "the principal has to survive the trip through connectrpc's Context"
    );

    let _ = shutdown.send(());
    let _ = serving.await;
}
