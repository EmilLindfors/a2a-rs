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
    /// Extra routes served outside the authenticator, if any.
    open_router: Option<Router>,
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
            open_router: None,
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
            open_router: None,
        }
    }

    /// Serve `router`'s routes on the same listener, *outside* the
    /// authenticator.
    ///
    /// For endpoints that carry their own authentication — a webhook receiver
    /// validating a per-task token, a health probe — where requiring the
    /// agent's credentials would mean handing them to whoever calls back. The
    /// A2A routes and the agent card stay behind the authenticator exactly as
    /// before; only the routes given here bypass it.
    ///
    /// The counterpart of the composable [`jsonrpc_router`] /
    /// [`rest_router`](crate::adapter::rest_router) surface for a transport
    /// whose app is assembled inside this type.
    ///
    /// `router` must not define a fallback: the ConnectRPC service is this
    /// server's fallback, and [`Router::merge`] panics on a second one.
    ///
    /// [`jsonrpc_router`]: crate::adapter::jsonrpc_router
    pub fn with_open_router(mut self, router: Router) -> Self {
        self.open_router = Some(router);
        self
    }

    /// Start the HTTP server on the configured address.
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        server.address = %self.address,
        server.has_auth = self.authenticator.is_some()
    )))]
    pub async fn start(&self) -> Result<(), A2AError> {
        let listener = tokio::net::TcpListener::bind(&self.address)
            .await
            .map_err(HttpServerError::Io)?;
        self.serve_on(listener).await
    }

    /// Serve on a listener the caller has already bound.
    ///
    /// [`start`](Self::start) binds the address itself, which leaves a caller
    /// that asked for port 0 no way to learn which port it got, and any caller
    /// no way to know the socket is accepting yet. Binding first answers both:
    /// `listener.local_addr()` reports the real address, and the kernel queues
    /// connections from the moment of the bind, so a client may connect before
    /// this future is ever polled. That is what a test needs to skip the
    /// "sleep and hope" step, and what lets a supervisor hand out port 0 and
    /// report back the port the agent actually listens on.
    #[cfg_attr(feature = "tracing", instrument(skip(self, listener), fields(
        server.has_auth = self.authenticator.is_some()
    )))]
    pub async fn serve_on(&self, listener: tokio::net::TcpListener) -> Result<(), A2AError> {
        #[cfg(feature = "tracing")]
        info!(
            "HTTP server listening on {}",
            listener
                .local_addr()
                .map(|addr| addr.to_string())
                .unwrap_or_else(|_| self.address.clone())
        );

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

        // Merged after the auth middleware is applied, which is what keeps
        // these routes outside it: an axum layer wraps the routes present when
        // it is added, and these are added afterwards on purpose.
        if let Some(open) = &self.open_router {
            app = app.merge(open.clone());
        }

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

/// Force the card's primary interface to advertise the binding this server
/// actually mounts.
///
/// [`HttpServer::start`] serves exactly one protocol — ConnectRPC, registered as
/// the fallback service — but an agent card built via `SimpleAgentInfo::new`
/// defaults its primary interface to `JSONRPC` (the spec default). Left alone,
/// every `HttpServer` publishes a card that lies about its own transport, and
/// card-driven clients negotiate to a JSON-RPC endpoint that was never mounted.
/// Rather than make each caller remember `with_preferred_transport`, the server
/// states the truth about itself.
///
/// Secondary interfaces are untouched, so a deployment fronted by a proxy that
/// *does* offer other bindings still advertises them via
/// `SimpleAgentInfo::add_interface`. A card with no interfaces at all carries no
/// dialable URL either, so there is nothing truthful to stamp — it is left as-is.
fn stamp_served_binding(card: &mut crate::domain::AgentCard) {
    if let Some(primary) = card.supported_interfaces.first_mut() {
        primary.protocol_binding = crate::domain::PROTOCOL_BINDING_CONNECTRPC.to_string();
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
        Ok(mut card) => {
            #[cfg(feature = "tracing")]
            debug!("Agent card retrieved successfully");
            stamp_served_binding(&mut card);
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
