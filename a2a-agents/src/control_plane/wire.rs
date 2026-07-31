//! The control-plane API's request and response bodies.
//!
//! Kept in one place because there are now **two** adapters on this contract —
//! [`control_plane_router`](super::control_plane_router) serving it and
//! [`ControlPlaneClient`](super::ControlPlaneClient) calling it — and a wire
//! format defined twice is a wire format that drifts. Both sides derive their
//! serialization from these types, so a field renamed here breaks the compile
//! rather than the deployment.
//!
//! [`DeployedAgent`](super::DeployedAgent) is not here: it is the *service's*
//! own return type, which the API passes straight through. Only the envelopes
//! that exist because the transport is HTTP live in this module.

use serde::{Deserialize, Serialize};

use crate::runtime::RuntimeHealth;

/// `POST /agents` body.
///
/// The agent's config as **raw, unexpanded** TOML: `${VAR}` references are
/// resolved by the control plane against its own environment, subject to its
/// allowlist, so a caller neither needs nor learns the secrets an agent runs
/// with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployRequest {
    /// The agent config to deploy.
    pub config_toml: String,
}

/// `GET /agents/:id` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    /// The agent's id.
    pub id: String,
    /// Its current runtime health.
    pub health: RuntimeHealth,
}

/// `GET /agents/:id/logs` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogs {
    /// The agent's id.
    pub id: String,
    /// Captured output, oldest line first.
    pub lines: Vec<String>,
}

/// `GET /agents/:id/logs` query parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogsQuery {
    /// Return only the last `tail` lines. Absent means the whole log.
    pub tail: Option<usize>,
}

/// `GET /agents` query parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListQuery {
    /// Include stopped agents. Defaults to false, so a listing shows what is
    /// actually running — the same choice `docker ps` makes.
    #[serde(default)]
    pub all: bool,
}

/// The body every failing route returns.
///
/// A single `error` string on purpose: the client turns it back into the text of
/// its own error, so an operator running `a2a deploy` reads the control plane's
/// diagnosis (which line of TOML, which env var) rather than a status code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorBody {
    /// What went wrong, as the control plane described it.
    pub error: String,
}
