//! Live end-to-end test of agent-to-agent delegation.
//!
//! Spins up a real A2A agent (echo) on an ephemeral socket, then drives an
//! [`A2aAgentToolSource`] against it through the JSON-RPC [`Transport`]. This
//! proves the multi-agent keystone over the wire: the tool source sends an A2A
//! task to a peer agent, waits for it to finish, and returns the reply — exactly
//! what the LLM handler does when the model calls an `ask_<agent>` tool.

#![cfg(feature = "mcp-server")]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use a2a_agents::A2aAgentToolSource;
use a2a_agents::handlers::tools::ToolSource;
use a2a_agents_common::llm::ToolCall;

use a2a_rs::adapter::business::{EchoResponder, Responder, ResponderMessageHandler};
use a2a_rs::adapter::{JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router};
use a2a_rs::domain::{A2AError, Message, Part, Role, Task, TaskState};
use a2a_rs::{InMemoryStreamingHandler, InMemoryTaskStorage, JsonRpcClient, Transport};

/// Echo responder that drives the task to a terminal `Completed` state (the
/// built-in [`EchoResponder`] deliberately stays `Working`, which models the
/// "acknowledge now, finish later" case — used by the timeout test below).
struct CompletingEcho;

#[async_trait]
impl Responder for CompletingEcho {
    async fn respond(
        &self,
        message: &Message,
        task: &Task,
    ) -> Result<(Message, TaskState), A2AError> {
        let echoed = message
            .parts
            .iter()
            .filter_map(|p| p.get_text())
            .collect::<Vec<_>>()
            .join(" ");
        let reply = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("Echo: {echoed}"))])
            .message_id(uuid::Uuid::new_v4().to_string())
            .task_id(task.id.clone())
            .build();
        Ok((reply, TaskState::Completed))
    }
}

/// Stand up a JSON-RPC A2A agent on an ephemeral port and return its base URL.
async fn spawn_agent(responder: impl Responder + 'static) -> String {
    let storage = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();
    let push = storage.push_notifier();
    let handler = ResponderMessageHandler::new(storage.clone(), streaming.clone(), push, responder);
    let agent_info = SimpleAgentInfo::new("echo".to_string(), "http://localhost".to_string());
    let adapter = Arc::new(
        JsonRpcAdapter::new(handler, storage.clone(), storage.clone(), agent_info)
            .with_streaming_handler(streaming.clone()),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = jsonrpc_router(adapter);
    tokio::spawn(async move {
        axum8::serve(listener, app).await.unwrap();
    });
    base
}

fn tool_call(message: &str) -> ToolCall {
    ToolCall {
        id: "call-1".to_string(),
        name: "ask_echo".to_string(),
        arguments: serde_json::json!({ "message": message }).to_string(),
    }
}

#[tokio::test]
async fn delegates_to_remote_agent_over_the_wire() {
    let base = spawn_agent(CompletingEcho).await;
    let transport: Arc<dyn Transport> = Arc::new(JsonRpcClient::new(base));
    let source = A2aAgentToolSource::new("echo", "Echoes the input back.".to_string(), transport);

    assert_eq!(source.tool_name(), "ask_echo");

    let reply = source
        .invoke("local-orchestrator-task", &tool_call("hello world"))
        .await
        .expect("delegation should succeed");
    assert_eq!(reply, "Echo: hello world");
}

#[tokio::test]
async fn times_out_when_remote_never_reaches_terminal_state() {
    // EchoResponder keeps the task Working forever, so the tool source must give
    // up once its deadline elapses rather than hang.
    let base = spawn_agent(EchoResponder).await;
    let transport: Arc<dyn Transport> = Arc::new(JsonRpcClient::new(base));
    let source = A2aAgentToolSource::new("echo", "Echoes the input back.".to_string(), transport)
        .with_deadline(Duration::from_millis(400));

    let err = source
        .invoke("local-orchestrator-task", &tool_call("hello"))
        .await
        .expect_err("a never-completing remote should time out");
    match err {
        A2AError::Internal(m) => assert!(m.contains("did not finish"), "unexpected message: {m}"),
        other => panic!("expected Internal timeout error, got {other:?}"),
    }
}
