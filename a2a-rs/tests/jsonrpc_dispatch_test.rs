//! Behavioral tests for the JSON-RPC adapter's method dispatch.
//!
//! Drives [`JsonRpcAdapter::handle_unary`] against an in-memory handler and
//! asserts the JSON-RPC envelopes + ProtoJSON result bodies that an
//! off-the-shelf A2A client would see.

#![cfg(feature = "jsonrpc-server")]

mod common;

use a2a_rs::adapter::transport::jsonrpc::{JsonRpcId, JsonRpcRequest, error_code, methods};
use a2a_rs::adapter::{InMemoryTaskStorage, JsonRpcAdapter, SimpleAgentInfo};
use a2a_rs::domain::{ContextId, TaskId, TaskState};
use a2a_rs::port::AsyncTaskLifecycle;
use common::TestBusinessHandler;
use serde_json::{Value, json};

fn adapter() -> JsonRpcAdapter {
    adapter_with_handler().0
}

/// As [`adapter`], also handing back the handler — the seam a test needs to put
/// a task into a state the dispatched methods cannot reach on their own.
fn adapter_with_handler() -> (JsonRpcAdapter, TestBusinessHandler) {
    let handler = TestBusinessHandler::with_storage(InMemoryTaskStorage::new());
    let agent_info = SimpleAgentInfo::new("test-agent".to_string(), "http://localhost".to_string());
    (
        JsonRpcAdapter::with_handler(handler.clone(), agent_info),
        handler,
    )
}

fn request(method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Num(1),
        method: method.to_string(),
        params: Some(params),
    }
}

fn send_message_params(task_id: &str) -> Value {
    send_message_params_ids(Some(task_id), None)
}

/// `SendMessage` params carrying only the ids that are `Some`.
///
/// An absent id is *omitted* from the JSON rather than sent as `""`, which is
/// what a client that wants the server to assign one puts on the wire.
fn send_message_params_ids(task_id: Option<&str>, context_id: Option<&str>) -> Value {
    let mut message = json!({
        "messageId": "m1",
        "role": "ROLE_USER",
        "parts": [{ "text": "hello" }],
    });
    if let Some(id) = task_id {
        message["taskId"] = id.into();
    }
    if let Some(id) = context_id {
        message["contextId"] = id.into();
    }
    json!({ "message": message })
}

/// Send `params` and return the `result.task`, failing on a JSON-RPC error.
async fn send_ok(a: &JsonRpcAdapter, params: Value) -> Value {
    let value =
        serde_json::to_value(a.handle_unary(request(methods::SEND_MESSAGE, params), None).await)
            .unwrap();
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    value["result"]["task"].clone()
}

#[tokio::test]
async fn send_message_returns_task_union() {
    let resp = adapter()
        .handle_unary(
            request(methods::SEND_MESSAGE, send_message_params("task-1")),
            None,
        )
        .await;
    let value = serde_json::to_value(&resp).unwrap();

    assert_eq!(value["jsonrpc"], "2.0");
    assert_eq!(value["id"], 1);
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    // Field-presence union: result is `{ "task": { ... } }`, no discriminator.
    let task = &value["result"]["task"];
    assert_eq!(task["id"], "task-1");
    // State is a SCREAMING_SNAKE proto-name string (the exact value depends on
    // the handler; just assert the ProtoJSON enum shape).
    assert!(
        task["status"]["state"]
            .as_str()
            .is_some_and(|s| s.starts_with("TASK_STATE_")),
        "unexpected status: {:?}",
        task["status"],
    );
}

#[tokio::test]
async fn get_task_round_trips() {
    let a = adapter();
    a.handle_unary(
        request(methods::SEND_MESSAGE, send_message_params("task-2")),
        None,
    )
    .await;

    let resp = a
        .handle_unary(request(methods::GET_TASK, json!({ "id": "task-2" })), None)
        .await;
    let value = serde_json::to_value(&resp).unwrap();
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    // GetTask result is a bare Task (not a union).
    assert_eq!(value["result"]["id"], "task-2");
}

#[tokio::test]
async fn cancel_task_returns_canceled_state() {
    // The echo handler finishes what it is sent, and a completed task is
    // correctly not cancelable — so the task is driven to `Working` directly.
    let (a, handler) = adapter_with_handler();
    let id: TaskId = "task-3".parse().unwrap();
    handler
        .create(&id, &"ctx".parse::<ContextId>().unwrap())
        .await
        .unwrap();
    handler
        .update_status(&id, TaskState::Working, None)
        .await
        .unwrap();

    let resp = a
        .handle_unary(
            request(methods::CANCEL_TASK, json!({ "id": "task-3" })),
            None,
        )
        .await;
    let value = serde_json::to_value(&resp).unwrap();
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    assert_eq!(value["result"]["id"], "task-3");
    assert_eq!(value["result"]["status"]["state"], "TASK_STATE_CANCELED");
}

#[tokio::test]
async fn unknown_method_is_method_not_found() {
    let resp = adapter()
        .handle_unary(request("NoSuchMethod", json!({})), None)
        .await;
    let value = serde_json::to_value(&resp).unwrap();
    assert!(value.get("result").is_none());
    assert_eq!(value["error"]["code"], error_code::METHOD_NOT_FOUND);
}

#[tokio::test]
async fn invalid_params_is_invalid_params() {
    // `message` is required on SendMessageRequest's wire shape; an int is invalid.
    let resp = adapter()
        .handle_unary(
            request(methods::SEND_MESSAGE, json!({ "message": 42 })),
            None,
        )
        .await;
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["error"]["code"], error_code::INVALID_PARAMS);
}

#[tokio::test]
async fn missing_message_is_invalid_params() {
    let resp = adapter()
        .handle_unary(request(methods::SEND_MESSAGE, json!({})), None)
        .await;
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["error"]["code"], error_code::INVALID_PARAMS);
}

#[tokio::test]
async fn get_missing_task_is_task_not_found() {
    let resp = adapter()
        .handle_unary(request(methods::GET_TASK, json!({ "id": "nope" })), None)
        .await;
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["error"]["code"], error_code::TASK_NOT_FOUND);
}

#[tokio::test]
async fn list_tasks_returns_response_envelope() {
    let a = adapter();
    a.handle_unary(
        request(methods::SEND_MESSAGE, send_message_params("task-4")),
        None,
    )
    .await;

    let resp = a
        .handle_unary(request(methods::LIST_TASKS, json!({})), None)
        .await;
    let value = serde_json::to_value(&resp).unwrap();
    assert!(value.get("error").is_none(), "unexpected error: {value:?}");
    assert!(value["result"]["tasks"].is_array());
}

// ---------------------------------------------------------------------------
// Server-side CallInterceptor chain
// ---------------------------------------------------------------------------

mod interceptors {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use a2a_rs::domain::A2AError;
    use a2a_rs::port::{CallContext, CallInterceptor, CallSide};
    use async_trait::async_trait;
    use serde_json::json;

    use super::{adapter, methods, request};

    /// Records how often each hook fired and whether `after` saw an error.
    #[derive(Clone, Default)]
    struct Counting {
        before: Arc<AtomicUsize>,
        after_ok: Arc<AtomicUsize>,
        after_err: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl CallInterceptor for Counting {
        async fn before(&self, ctx: &CallContext) -> Result<(), A2AError> {
            assert_eq!(ctx.side, CallSide::Server);
            self.before.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn after(&self, _ctx: &CallContext, outcome: Result<(), &A2AError>) {
            match outcome {
                Ok(()) => self.after_ok.fetch_add(1, Ordering::SeqCst),
                Err(_) => self.after_err.fetch_add(1, Ordering::SeqCst),
            };
        }
    }

    /// A `before` that always short-circuits the call.
    struct Rejecting;

    #[async_trait]
    impl CallInterceptor for Rejecting {
        async fn before(&self, _ctx: &CallContext) -> Result<(), A2AError> {
            Err(A2AError::UnsupportedOperation(
                "rejected by interceptor".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn before_and_after_wrap_each_dispatch() {
        let counter = Counting::default();
        let a = adapter().with_interceptor(counter.clone());

        // A successful call: after observes Ok.
        a.handle_unary(
            request(methods::SEND_MESSAGE, super::send_message_params("ti-1")),
            None,
        )
        .await;
        // A failing call (missing task): after observes Err.
        let resp = a
            .handle_unary(request(methods::GET_TASK, json!({ "id": "ghost" })), None)
            .await;
        let value = serde_json::to_value(&resp).unwrap();
        assert!(value.get("error").is_some(), "expected an error: {value:?}");

        assert_eq!(counter.before.load(Ordering::SeqCst), 2);
        assert_eq!(counter.after_ok.load(Ordering::SeqCst), 1);
        assert_eq!(counter.after_err.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rejecting_before_short_circuits_dispatch() {
        // Rejecting runs first; the real method never executes.
        let a = adapter()
            .with_interceptor(Rejecting)
            .with_interceptor(Counting::default());

        let resp = a
            .handle_unary(request(methods::GET_TASK, json!({ "id": "task-x" })), None)
            .await;
        let value = serde_json::to_value(&resp).unwrap();
        // The short-circuit error surfaces as the JSON-RPC error, not a task.
        assert_eq!(
            value["error"]["message"],
            "Unsupported operation: rejected by interceptor"
        );
        assert!(value.get("result").is_none() || value["result"].is_null());
    }
}

// ---------------------------------------------------------------------------
// Server-assigned ids (issue #51)
//
// `a2a.proto` makes a client message's `task_id` and `context_id` optional, and
// proto3 has no absent scalar — an omitted id arrives as `""`. These pin the
// resolution rule the service applies.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn omitted_task_id_gets_a_server_assigned_one() {
    let task = send_ok(&adapter(), send_message_params_ids(None, None)).await;

    let id = task["id"].as_str().expect("task id");
    let context_id = task["contextId"].as_str().expect("context id");
    assert!(!id.is_empty(), "server must name the task");
    assert!(!context_id.is_empty(), "server must name the context");

    // The ids are stamped onto the stored message too: a client that sent none
    // reads them back off history rather than getting `""`.
    assert_eq!(task["history"][0]["taskId"], id);
    assert_eq!(task["history"][0]["contextId"], context_id);
}

#[tokio::test]
async fn each_id_less_send_starts_its_own_task() {
    let a = adapter();
    let first = send_ok(&a, send_message_params_ids(None, None)).await;
    let second = send_ok(&a, send_message_params_ids(None, None)).await;

    assert_ne!(first["id"], second["id"]);
    assert_ne!(first["contextId"], second["contextId"]);
}

#[tokio::test]
async fn a_supplied_context_id_survives_an_omitted_task_id() {
    let task = send_ok(&adapter(), send_message_params_ids(None, Some("ctx-kept"))).await;

    assert_eq!(task["contextId"], "ctx-kept");
    assert!(!task["id"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn omitted_context_id_is_inferred_from_the_task() {
    let a = adapter();
    send_ok(&a, send_message_params_ids(Some("t-ctx"), Some("ctx-1"))).await;

    // Second turn names only the task. The spec has the server infer the
    // context from it rather than inventing a new one.
    let task = send_ok(&a, send_message_params_ids(Some("t-ctx"), None)).await;
    assert_eq!(task["contextId"], "ctx-1");
}

#[tokio::test]
async fn context_id_contradicting_the_task_is_rejected() {
    let a = adapter();
    send_ok(&a, send_message_params_ids(Some("t-clash"), Some("ctx-1"))).await;

    let resp = a
        .handle_unary(
            request(
                methods::SEND_MESSAGE,
                send_message_params_ids(Some("t-clash"), Some("ctx-2")),
            ),
            None,
        )
        .await;
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["error"]["code"], error_code::INVALID_PARAMS);
    assert!(
        value["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("context_id")),
        "error should name the field: {value:?}",
    );
}

#[tokio::test]
async fn a_client_chosen_task_id_is_kept() {
    let task = send_ok(&adapter(), send_message_params_ids(Some("t-mine"), None)).await;

    assert_eq!(task["id"], "t-mine");
    // Nothing to infer from — the task is new — so the server names the context.
    assert!(!task["contextId"].as_str().unwrap().is_empty());
}
