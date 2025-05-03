//! HTTP server adapter for the A2A protocol

// This module is already conditionally compiled with #[cfg(feature = "http-server")] in mod.rs

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};

use crate::{
    adapter::server::{
        auth::{Authenticator, NoopAuthenticator, with_auth},
        error::HttpServerError,
    },
    domain::A2AError,
    port::server::{AgentInfoProvider, AsyncA2ARequestProcessor},
};

/// HTTP server for the A2A protocol
pub struct HttpServer<P, A, Auth = NoopAuthenticator>
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    Auth: Authenticator + Send + Sync + 'static,
{
    /// Request processor
    processor: Arc<P>,
    /// Agent info provider
    agent_info: Arc<A>,
    /// Server address
    address: String,
    /// Authenticator
    authenticator: Option<Arc<Auth>>,
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
            authenticator: None,
        }
    }
}

impl<P, A, Auth> HttpServer<P, A, Auth>
where
    P: AsyncA2ARequestProcessor + Clone + Send + Sync + 'static,
    A: AgentInfoProvider + Clone + Send + Sync + 'static,
    Auth: Authenticator + Clone + Send + Sync + 'static,
{
    /// Create a new HTTP server with authentication
    pub fn with_auth(processor: P, agent_info: A, address: String, authenticator: Auth) -> Self {
        Self {
            processor: Arc::new(processor),
            agent_info: Arc::new(agent_info),
            address,
            authenticator: Some(Arc::new(authenticator)),
        }
    }

    /// Start the HTTP server
    pub async fn start(&self) -> Result<(), A2AError> {
        let processor = self.processor.clone();
        let agent_info = self.agent_info.clone();

        let mut app = Router::new()
            .route("/", post(handle_request))
            .route("/agent-card", get(handle_agent_card))
            .route("/skills", get(handle_skills))
            .route("/skills/{id}", get(handle_skill_by_id))
            .with_state(ServerState {
                processor: processor.clone(),
                agent_info: agent_info.clone(),
            });

        // Apply authentication if provided
        if let Some(auth) = &self.authenticator {
            // Clone the authenticator for the middleware
            let auth_clone = auth.clone();

            // Create an auth router with the authenticator
            app = with_auth(app, (*auth_clone).clone());
        }

        let listener = tokio::net::TcpListener::bind(&self.address)
            .await
            .map_err(HttpServerError::Io)?;

        axum::serve(listener, app)
            .await
            .map_err(|e| HttpServerError::Server(format!("Server error: {}", e)))?;

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
                .into_response();
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
                        .into_response();
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

/// Handle a request for all agent skills
async fn handle_skills<P, A>(State(state): State<ServerState<P, A>>) -> impl IntoResponse
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    match state.agent_info.get_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
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

/// Handle a request for a specific agent skill by ID
async fn handle_skill_by_id<P, A>(
    State(state): State<ServerState<P, A>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    match state.agent_info.get_skill_by_id(&id).await {
        Ok(Some(skill)) => (StatusCode::OK, Json(skill)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": format!("Skill with ID '{}' not found", id)
            })),
        )
            .into_response(),
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
