//! A wire-compatible JSON-RPC 2.0 + HTTP+JSON (REST) A2A server.
//!
//! Unlike [`http_client_server`], which speaks ConnectRPC, this example mounts
//! the [`JsonRpcAdapter`] so off-the-shelf A2A clients (the official `a2acli`,
//! the Go/C#/Python SDKs) can talk to it. Composition happens here at the edge:
//! `jsonrpc_router(adapter).merge(rest_router(adapter))` plus the well-known
//! agent-card route, all on one `axum::serve`.
//!
//! Run it:
//! ```sh
//! cargo run -p a2a-rs --example jsonrpc_server --features jsonrpc-server
//! ```
//! Then exercise it with curl (JSON-RPC):
//! ```sh
//! curl -s localhost:8137/ -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage",
//!   "params":{"message":{"messageId":"m1","role":"ROLE_USER",
//!   "parts":[{"text":"hello"}],"taskId":"t1"}}}'
//! curl -s localhost:8137/.well-known/agent-card.json
//! ```
//! …or REST:
//! ```sh
//! curl -s localhost:8137/message:send -d '{"message":{"messageId":"m1",
//!   "role":"ROLE_USER","parts":[{"text":"hi"}],"taskId":"t1"}}'
//! curl -s localhost:8137/tasks/t1
//! ```
//! …or the official CLI (clone at ./a2aproject/a2a-rs/a2acli):
//! ```sh
//! cargo run --bin a2acli -- --base-url http://localhost:8137 card
//! cargo run --bin a2acli -- --base-url http://localhost:8137 send "hello"
//! ```

use std::sync::Arc;

use axum::{Json, Router, extract::State, response::IntoResponse, routing::get};

use a2a_rs::adapter::{
    InMemoryTaskStorage, JsonRpcAdapter, SimpleAgentInfo, jsonrpc_router, rest_router,
};
use a2a_rs::services::server::AgentInfoProvider;

mod common;
use common::SimpleAgentHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = "127.0.0.1:8137";

    // 1. Inner application: an in-memory handler behind the JSON-RPC adapter.
    let handler = SimpleAgentHandler::with_storage(InMemoryTaskStorage::new());
    let adapter_card =
        SimpleAgentInfo::new("jsonrpc-agent".to_string(), format!("http://{address}"));
    let adapter = Arc::new(JsonRpcAdapter::with_handler(handler, adapter_card));

    // 2. Agent card served for client transport negotiation. The primary
    //    interface (from `new`) already advertises JSON-RPC at `base`; we add the
    //    REST binding so an official client reading `supportedInterfaces` can
    //    negotiate to either endpoint this server mounts.
    let base = format!("http://{address}");
    let card_info = Arc::new(
        SimpleAgentInfo::new("Example JSON-RPC A2A Agent".to_string(), base.clone())
            .with_description("Wire-compatible JSON-RPC 2.0 + HTTP+JSON A2A server".to_string())
            .with_preferred_transport("JSONRPC".to_string())
            .add_interface(base, "HTTP+JSON".to_string())
            .add_skill("echo".to_string(), "Echo".to_string(), Some("Echoes input".to_string())),
    );

    // 3. Composition at the edge: both transports + the agent card on one server.
    //    Each sub-router carries its own state, so they merge as `Router<()>`.
    let card_router = Router::new()
        .route("/.well-known/agent-card.json", get(agent_card))
        .with_state(card_info);
    let app: Router = jsonrpc_router(adapter.clone())
        .merge(rest_router(adapter))
        .merge(card_router);

    println!("🚀 JSON-RPC + REST A2A server on http://{address}");
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn agent_card(State(info): State<Arc<SimpleAgentInfo>>) -> impl IntoResponse {
    match info.get_agent_card().await {
        Ok(card) => Json(card).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
