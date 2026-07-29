//! HTTP API adapter for the [`ControlPlane`] service (axum 0.7).
//!
//! `POST /agents` deploys an agent from rendered TOML, `GET /agents` lists them,
//! `GET /agents/:id` reports health, `GET /agents/:id/logs` replays its output,
//! `DELETE /agents/:id` undeploys. This is the surface `a2a deploy/ps/logs/stop`
//! drives through [`ControlPlaneClient`](super::ControlPlaneClient), and the one
//! the Terraform provider will target (Create/Read/Delete).

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::auth::ControlPlaneAuth;
use super::wire::{AgentLogs, AgentStatus, ApiErrorBody, DeployRequest, ListQuery, LogsQuery};
use super::{ControlPlane, ControlPlaneError, DeployedAgent, ListFilter};
use crate::registry::AgentId;
use crate::runtime::RuntimeError;

/// Shared handler state: the service plus where rendered configs are written.
#[derive(Clone)]
struct AppState {
    cp: Arc<ControlPlane>,
    config_dir: PathBuf,
}

/// Build the control-plane HTTP router over `cp`, writing deployed configs into
/// `config_dir` (the path the spawned `a2a run` child reads).
///
/// `auth` guards every route. It is a required parameter rather than a builder
/// step so an unauthenticated control plane cannot happen by omission — the
/// caller has to write [`ControlPlaneAuth::Disabled`] to get one.
pub fn control_plane_router(
    cp: Arc<ControlPlane>,
    config_dir: PathBuf,
    auth: ControlPlaneAuth,
) -> Router {
    let state = AppState { cp, config_dir };
    let router = Router::new()
        .route("/agents", post(deploy).get(list))
        .route("/agents/:id", get(status).delete(undeploy))
        .route("/agents/:id/logs", get(logs))
        .with_state(state);
    auth.apply(router)
}

async fn deploy(
    State(state): State<AppState>,
    Json(req): Json<DeployRequest>,
) -> Result<(StatusCode, Json<DeployedAgent>), ApiError> {
    // Vet + parse once. `prepare` checks the raw text against the env allowlist
    // *before* parsing expands it, so a config naming a forbidden secret never
    // reaches expansion — and the returned token is the only thing `deploy`
    // accepts, so the check cannot be skipped here.
    let prepared = state.cp.prepare(&req.config_toml)?;

    tokio::fs::create_dir_all(&state.config_dir).await?;
    let path = state.config_dir.join(format!("{}.toml", prepared.id()));
    tokio::fs::write(&path, &req.config_toml).await?;

    let deployed = state.cp.deploy(prepared, path).await?;
    Ok((StatusCode::CREATED, Json(deployed)))
}

async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<DeployedAgent>>, ApiError> {
    let filter = if query.all {
        ListFilter::All
    } else {
        ListFilter::Live
    };
    Ok(Json(state.cp.list(filter).await?))
}

async fn status(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> Result<Json<AgentStatus>, ApiError> {
    let id = AgentId::from(id);
    let health = state.cp.status(&id).await?;
    Ok(Json(AgentStatus {
        id: id.to_string(),
        health,
    }))
}

async fn logs(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
    Query(query): Query<LogsQuery>,
) -> Result<Json<AgentLogs>, ApiError> {
    let id = AgentId::from(id);
    let lines = state.cp.logs(&id, query.tail).await?;
    Ok(Json(AgentLogs {
        id: id.to_string(),
        lines,
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
            ApiError::Domain(
                ControlPlaneError::Runtime(RuntimeError::AlreadyRunning(_))
                | ControlPlaneError::PortInUse { .. },
            ) => StatusCode::CONFLICT,
            // Everything the *caller* got wrong, including a config naming
            // secrets the operator has not permitted. `DisallowedEnv` carries a
            // fully actionable message ("permit them with `--allow-env`"), so
            // falling through to 500 would read — to an operator and to any
            // alerting — as the server breaking.
            ApiError::Domain(
                ControlPlaneError::Config(_)
                | ControlPlaneError::Card(_)
                | ControlPlaneError::Runtime(
                    RuntimeError::DisallowedEnv(_) | RuntimeError::Config(_),
                ),
            ) => StatusCode::BAD_REQUEST,
            // The backend genuinely cannot answer (logs from a runtime that
            // does not capture them). Not the caller's mistake and not a
            // failure — 501 says "this deployment can't", which is what the
            // operator has to act on.
            ApiError::Domain(ControlPlaneError::Runtime(RuntimeError::Unsupported { .. })) => {
                StatusCode::NOT_IMPLEMENTED
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let error = match &self {
            ApiError::Domain(e) => e.to_string(),
            ApiError::Io(e) => e.to_string(),
        };
        (status, Json(ApiErrorBody { error })).into_response()
    }
}
