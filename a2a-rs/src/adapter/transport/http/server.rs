//! HTTP server adapter for the A2A protocol

// This module is already conditionally compiled with #[cfg(feature = "http-server")] in mod.rs

use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};

#[cfg(feature = "tracing")]
use tracing::{debug, error, info, instrument};

use crate::{
    adapter::{
        auth::{NoopAuthenticator, with_auth},
        error::HttpServerError,
    },
    domain::{
        A2AError,
        generated::{A2aService, A2aServiceExt},
    },
    port::Authenticator,
    services::server::AgentInfoProvider,
};

/// HTTP server for the A2A protocol
pub struct HttpServer<P, A, Auth = NoopAuthenticator>
where
    P: A2aService + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    Auth: Authenticator + Send + Sync + 'static,
{
    /// The `A2aService` implementation this server dispatches requests to
    /// (e.g. [`ConnectRpcAdapter`](crate::adapter::ConnectRpcAdapter)).
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
    P: A2aService + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
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
    P: A2aService + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
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

        // Register the ConnectRPC service
        let connect_router = processor.register(connectrpc::Router::new());

        let mut app = Router::new()
            // v1.0.0 well-known URI endpoint (RFC 8615)
            .route("/.well-known/agent-card.json", get(handle_agent_card))
            // Backward compatibility routes
            .route("/agent-card", get(handle_agent_card))
            .route("/skills", get(handle_skills))
            .route("/skills/{id}", get(handle_skill_by_id))
            .fallback_service(connect_router.into_axum_service())
            .with_state(ServerState {
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

struct ServerState<A>
where
    A: AgentInfoProvider + Send + Sync + 'static,
{
    agent_info: Arc<A>,
}

impl<A> Clone for ServerState<A>
where
    A: AgentInfoProvider + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            agent_info: self.agent_info.clone(),
        }
    }
}

/// Handle a request for the agent card
#[cfg_attr(feature = "tracing", instrument(skip(state)))]
async fn handle_agent_card<A>(State(state): State<ServerState<A>>) -> impl IntoResponse
where
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
            // Map A2AError to HTTP response
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Handle a request for all agent skills
async fn handle_skills<A>(State(state): State<ServerState<A>>) -> impl IntoResponse
where
    A: AgentInfoProvider + Send + Sync + 'static,
{
    match state.agent_info.get_skills().await {
        Ok(skills) => (StatusCode::OK, Json(skills)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": e.to_string()
            })),
        )
            .into_response(),
    }
}

/// Handle a request for a specific agent skill by ID
async fn handle_skill_by_id<A>(
    State(state): State<ServerState<A>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse
where
    A: AgentInfoProvider + Send + Sync + 'static,
{
    match state.agent_info.get_skill_by_id(&id).await {
        Ok(Some(skill)) => (StatusCode::OK, Json(skill)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("Skill with ID '{}' not found", id)
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": e.to_string()
            })),
        )
            .into_response(),
    }
}
