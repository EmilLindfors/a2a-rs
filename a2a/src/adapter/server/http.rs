//! HTTP server adapter for the A2A protocol

#![cfg(feature = "http-server")]

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::{
    application::json_rpc::{self, A2ARequest, JSONRPCResponse},
    domain::{A2AError, AgentCard},
    port::server::{AgentInfoProvider, AsyncA2ARequestProcessor},
};

/// HTTP server for the A2A protocol
pub struct HttpServer<P, A>
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    /// Request processor
    processor: Arc<P>,
    /// Agent info provider
    agent_info: Arc<A>,
    /// Server address
    address: String,
}

impl<P, A> HttpServer<P, A>
where
    P: AsyncA2ARequestProcessor + Clone + Send + Sync + 'static,
    A: AgentInfoProvider + Clone + Send + Sync + 'static,
{
    /// Create a new HTTP server with the given processor and agent info provider
    pub fn new(processor: P, agent_info: A, address: String) -> Self {
        Self {
            processor: Arc::new(processor),
            agent_info: Arc::new(agent_info),
            address,
        }
    }

    /// Start the HTTP server
    pub async fn start(&self) -> Result<(), A2AError> {
        let processor = self.processor.clone();
        let agent_info = self.agent_info.clone();

        let app = Router::new()
            .route("/", post(handle_request))
            // v0.3.0 standard well-known URI per RFC 8615
            .route("/.well-known/agent-card.json", get(handle_agent_card))
            // Backward compatibility routes
            .route("/agent-card", get(handle_agent_card))
            .route("/.well-known/agent.json", get(handle_agent_card))
            .with_state(ServerState {
                processor: processor.clone(),
                agent_info: agent_info.clone(),
            });

        let listener = tokio::net::TcpListener::bind(&self.address)
            .await
            .map_err(|e| A2AError::Internal(format!("Failed to bind to address: {}", e)))?;

        axum::serve(listener, app)
            .await
            .map_err(|e| A2AError::Internal(format!("Server error: {}", e)))?;

        Ok(())
    }
}

/// State for the HTTP server
#[derive(Clone)]
struct ServerState<P, A>
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    processor: Arc<P>,
    agent_info: Arc<A>,
}

/// Handle a request from a client
async fn handle_request<P, A>(
    State(state): State<ServerState<P, A>>,
    Json(request): Json<Value>,
) -> impl IntoResponse
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    // Convert the request to a string
    let request_str = match serde_json::to_string(&request) {
        Ok(str) => str,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": "Invalid JSON payload",
                        "data": e.to_string()
                    }
                })),
            )
                .into_response()
        }
    };

    // Process the request
    match state.processor.process_raw_request(&request_str).await {
        Ok(response) => {
            let response_value: Value = match serde_json::from_str(&response) {
                Ok(value) => value,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": {
                                "code": -32603,
                                "message": "Internal error",
                                "data": "Failed to parse response"
                            }
                        })),
                    )
                        .into_response()
                }
            };
            (StatusCode::OK, Json(response_value)).into_response()
        }
        Err(e) => {
            let error = e.to_jsonrpc_error();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": error
                })),
            )
                .into_response()
        }
    }
}

/// Handle a request for the agent card
async fn handle_agent_card<P, A>(State(state): State<ServerState<P, A>>) -> impl IntoResponse
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    match state.agent_info.get_agent_card().await {
        Ok(card) => (StatusCode::OK, Json(card)).into_response(),
        Err(e) => {
            let error = e.to_jsonrpc_error();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": error
                })),
            )
                .into_response()
        }
    }
}