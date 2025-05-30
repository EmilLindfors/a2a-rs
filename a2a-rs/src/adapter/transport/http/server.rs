//! HTTP server adapter for the A2A protocol

// This module is already conditionally compiled with #[cfg(feature = "http-server")] in mod.rs

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

#[cfg(feature = "tracing")]
use tracing::{debug, error, info, instrument};

use crate::{
    adapter::{
        auth::{with_auth, Authenticator, NoopAuthenticator},
        error::HttpServerError,
    },
    domain::A2AError,
    services::server::{AgentInfoProvider, AsyncA2ARequestProcessor},
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
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        server.address = %self.address,
        server.has_auth = self.authenticator.is_some()
    )))]
    pub async fn start(&self) -> Result<(), A2AError> {
        #[cfg(feature = "tracing")]
        info!("Starting HTTP server");

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

        #[cfg(feature = "tracing")]
        info!("HTTP server listening on {}", self.address);

        axum::serve(listener, app).await.map_err(|e| {
            #[cfg(feature = "tracing")]
            error!("Server error: {}", e);
            HttpServerError::Server(format!("Server error: {}", e))
        })?;

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
#[cfg_attr(feature = "tracing", instrument(skip(state), fields(
    request.id = %request.get("id").and_then(|v| v.as_str()).unwrap_or("unknown"),
    request.method = %request.get("method").and_then(|v| v.as_str()).unwrap_or("unknown")
)))]
async fn handle_request<P, A>(
    State(state): State<ServerState<P, A>>,
    Json(request): Json<Value>,
) -> impl IntoResponse
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    #[cfg(feature = "tracing")]
    debug!("Processing JSON-RPC request");

    #[cfg(feature = "tracing")]
    let start_time = std::time::Instant::now();

    // Convert the request to a string
    let request_str = match serde_json::to_string(&request) {
        Ok(str) => str,
        Err(e) => {
            #[cfg(feature = "tracing")]
            error!("Invalid JSON payload: {}", e);
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
            #[cfg(feature = "tracing")]
            debug!("Request processed successfully");
            let response_value: Value = match serde_json::from_str(&response) {
                Ok(value) => value,
                Err(e) => {
                    #[cfg(feature = "tracing")]
                    error!("Failed to parse response: {}", e);
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

            #[cfg(feature = "tracing")]
            {
                let duration = start_time.elapsed();
                tracing::Span::current().record("request.duration_ms", duration.as_millis() as u64);
                info!(
                    duration_ms = duration.as_millis() as u64,
                    "Request completed successfully"
                );
            }

            (StatusCode::OK, Json(response_value)).into_response()
        }
        Err(e) => {
            #[cfg(feature = "tracing")]
            error!("Request processing failed: {}", e);
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
#[cfg_attr(feature = "tracing", instrument(skip(state)))]
async fn handle_agent_card<P, A>(State(state): State<ServerState<P, A>>) -> impl IntoResponse
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    #[cfg(feature = "tracing")]
    debug!("Fetching agent card");
    match state.agent_info.get_agent_card().await {
        Ok(card) => {
            #[cfg(feature = "tracing")]
            debug!("Agent card retrieved successfully");
            (StatusCode::OK, Json(card)).into_response()
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
