//! Live end-to-end test of discovery-by-skill.
//!
//! Registers a real A2A agent's card (advertising a `weather-lookup` skill) in
//! an [`InMemoryAgentRegistry`], then resolves it **by skill** — not by URL —
//! and delegates to it over the wire via [`A2aAgentToolSource`]. This proves the
//! Pillar 2 keystone: an orchestrator binds to whichever agent provides a
//! capability, found through the registry port.

#![cfg(feature = "mcp-server")]

use std::sync::Arc;

use async_trait::async_trait;

use a2a_agents::handlers::tools::ToolSource;
use a2a_agents::{A2aAgentToolSource, AgentRegistry, InMemoryAgentRegistry};
use a2a_agents_common::llm::ToolCall;

use a2a_rs::adapter::business::{Responder, ResponderMessageHandler};
use a2a_rs::adapter::{JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router};
use a2a_rs::domain::{A2AError, AgentCard, AgentSkill, Message, Part, Role, Task, TaskState};
use a2a_rs::{InMemoryStreamingHandler, InMemoryTaskStorage, JsonRpcClient, Transport};

/// Echo responder that drives the task to a terminal `Completed` state.
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
    let agent_info =
        SimpleAgentInfo::new("Weather Agent".to_string(), "http://localhost".to_string());
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

/// Build a card advertising the `weather-lookup` skill, dialable at `endpoint`.
fn weather_card() -> AgentCard {
    let mut card = AgentCard {
        name: "Weather Agent".to_string(),
        ..Default::default()
    };
    card.skills = vec![AgentSkill::new(
        "weather-lookup".to_string(),
        "Weather lookup".to_string(),
        "Look up the weather".to_string(),
        vec!["forecast".to_string()],
    )];
    card
}

#[tokio::test]
async fn discovers_peer_by_skill_then_delegates() {
    // A real worker agent on an ephemeral socket.
    let endpoint = spawn_agent(CompletingEcho).await;

    // Register its card + endpoint in the registry (what phase-1 startup does).
    let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
    registry
        .register(weather_card(), endpoint.clone())
        .await
        .expect("register is infallible in-memory");

    // Resolve BY SKILL — no URL hard-coded anywhere in the caller.
    let mut matches = registry
        .find_by_skill("weather-lookup")
        .await
        .expect("lookup is infallible in-memory");
    assert_eq!(matches.len(), 1, "exactly one agent advertises the skill");
    let found = matches.remove(0);
    assert_eq!(found.endpoint, endpoint);

    // Delegate to the resolved endpoint over the wire.
    let transport: Arc<dyn Transport> = Arc::new(JsonRpcClient::new(found.endpoint));
    let source = A2aAgentToolSource::new(
        &found.card.name,
        "Weather specialist.".to_string(),
        transport,
    );
    assert_eq!(source.tool_name(), "ask_weather_agent");

    let call = ToolCall {
        id: "call-1".to_string(),
        name: source.tool_name().to_string(),
        arguments: serde_json::json!({ "message": "weather in Oslo?" }).to_string(),
    };
    let reply = source
        .invoke("orchestrator-task", &call)
        .await
        .expect("delegation should succeed");
    assert_eq!(reply, "Echo: weather in Oslo?");
}

#[tokio::test]
async fn find_by_skill_misses_return_empty() {
    let registry = InMemoryAgentRegistry::new();
    registry
        .register(weather_card(), "http://127.0.0.1:1".to_string())
        .await
        .unwrap();

    assert!(
        registry.find_by_skill("billing").await.unwrap().is_empty(),
        "an unadvertised skill resolves to no agents"
    );
}
