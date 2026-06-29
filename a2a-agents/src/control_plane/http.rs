//! HTTP API adapter for the [`ControlPlane`] service (axum 0.7).
//!
//! `POST /agents` deploys an agent from rendered TOML, `GET /agents` lists them,
//! `GET /agents/:id` reports health, `DELETE /agents/:id` undeploys. This is the
//! surface the Terraform provider drives (Create/Read/Delete).

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::{ControlPlane, ControlPlaneError, DeployedAgent};
use crate::core::AgentBuilder;
use crate::registry::AgentId;
use crate::runtime::{RuntimeError, RuntimeHealth};

/// Shared handler state: the service plus where rendered configs are written.
#[derive(Clone)]
struct AppState {
    cp: Arc<ControlPlane>,
    config_dir: PathBuf,
}

/// Build the control-plane HTTP router over `cp`, writing deployed configs into
/// `config_dir` (the path the spawned `a2a run` child reads).
pub fn control_plane_router(cp: Arc<ControlPlane>, config_dir: PathBuf) -> Router {
    let state = AppState { cp, config_dir };
    Router::new()
        .route("/agents", post(deploy).get(list))
        .route("/agents/:id", get(status).delete(undeploy))
        .with_state(state)
}

/// `POST /agents` body: the rendered agent config TOML.
#[derive(Deserialize)]
struct DeployRequest {
    config_toml: String,
}

/// `GET /agents/:id` response.
#[derive(Serialize)]
struct StatusResponse {
    id: String,
    health: RuntimeHealth,
}

async fn deploy(
    State(state): State<AppState>,
    Json(req): Json<DeployRequest>,
) -> Result<(StatusCode, Json<DeployedAgent>), ApiError> {
    // Parse once: validates the TOML and yields the name for the filename; the
    // same builder is handed to the service so it never re-reads the file.
    let builder = AgentBuilder::from_toml(&req.config_toml)?;
    let id = AgentId::from_name(&builder.config().agent.name);

    tokio::fs::create_dir_all(&state.config_dir).await?;
    let path = state.config_dir.join(format!("{id}.toml"));
    tokio::fs::write(&path, &req.config_toml).await?;

    let deployed = state.cp.deploy(&builder, path).await?;
    Ok((StatusCode::CREATED, Json(deployed)))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<DeployedAgent>>, ApiError> {
    Ok(Json(state.cp.list().await?))
}

async fn status(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<StatusResponse>, ApiError> {
    let id = AgentId::from(id);
    let health = state.cp.status(&id).await?;
    Ok(Json(StatusResponse {
        id: id.to_string(),
        health,
    }))
}

async fn undeploy(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<StatusCode, ApiError> {
    state.cp.undeploy(&AgentId::from(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Adapter-level error: the service error plus the adapter's own I/O (writing the
/// config file). Kept here so [`ControlPlaneError`] stays free of transport/IO.
enum ApiError {
    Domain(ControlPlaneError),
    Io(std::io::Error),
}

impl From<ControlPlaneError> for ApiError {
    fn from(e: ControlPlaneError) -> Self {
        ApiError::Domain(e)
    }
}

impl From<crate::core::config::ConfigError> for ApiError {
    fn from(e: crate::core::config::ConfigError) -> Self {
        ApiError::Domain(ControlPlaneError::Config(e))
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::Io(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::Domain(ControlPlaneError::Runtime(RuntimeError::NotFound(_))) => {
                StatusCode::NOT_FOUND
            }
            ApiError::Domain(ControlPlaneError::Runtime(RuntimeError::AlreadyRunning(_))) => {
                StatusCode::CONFLICT
            }
            ApiError::Domain(ControlPlaneError::Config(_) | ControlPlaneError::Card(_)) => {
                StatusCode::BAD_REQUEST
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = match &self {
            ApiError::Domain(e) => e.to_string(),
            ApiError::Io(e) => e.to_string(),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
