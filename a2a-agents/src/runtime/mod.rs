//! Agent runtime — a place to *run* agents as managed, isolatable units.
//!
//! The [`AgentRuntime`] port is a **platform capability**: it provisions, starts,
//! stops, and health-checks agent *instances*, independently of how each agent
//! serves requests once running ([`AgentServer`](crate::AgentServer) is that
//! per-agent leaf). This is the keystone of Pillar 3 — a future control-plane
//! and the Terraform provider drive a real backend through this port instead of
//! a single in-process fan-out.
//!
//! Per the hexagonal rules this lives in the platform layer (never in the pure
//! `a2a-rs` protocol crate): a capability port plus a first-class adapter.
//! [`LocalProcessRuntime`] supervises agents as child `a2a run` OS processes
//! (dev/test, no Docker); a `ContainerRuntime` (Docker/Podman) and the
//! control-plane service are later drop-ins behind the same port — which is why
//! every method returns [`Result`] even though the local adapter rarely fails.
//!
//! Identity is shared with the [`registry`](crate::registry): a runtime instance
//! and its registry entry use the same [`AgentId`], so the two compose at the
//! control-plane edge.

mod container;
mod local;
mod memory;

pub use container::ContainerRuntime;
pub use local::LocalProcessRuntime;
pub use memory::InMemoryAgentRuntime;

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::AgentConfig;
use crate::registry::AgentId;

/// What to run and how to reach it — the unit of deployment a runtime manages.
///
/// Today an agent *is* a TOML config, so a spec pairs that config path with the
/// derived [`AgentId`] and the endpoint the agent will serve on (used for health
/// probing). Build one with [`from_config_path`](Self::from_config_path), or
/// construct the fields directly.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    /// Stable id, derived from the agent's name (slug). Shared with the registry.
    pub id: AgentId,
    /// Path to the agent's TOML config, passed to `a2a run --config <path>`.
    pub config_path: PathBuf,
    /// Dialable base URL the agent serves on (from [`AgentConfig::agent_url`]),
    /// probed to decide [`RuntimeHealth::Healthy`] vs [`RuntimeHealth::Unhealthy`].
    pub endpoint: String,
}

impl AgentSpec {
    /// Derive a spec from a config file, reusing [`AgentConfig`] to read the
    /// agent's name (→ [`AgentId`]) and bound endpoint. Invalid configs surface
    /// as [`RuntimeError::Config`].
    pub fn from_config_path(path: impl Into<PathBuf>) -> Result<Self, RuntimeError> {
        let config_path = path.into();
        let config =
            AgentConfig::from_file(&config_path).map_err(|e| RuntimeError::Config(e.to_string()))?;
        Ok(Self {
            id: AgentId::from_name(&config.agent.name),
            endpoint: config.agent_url(),
            config_path,
        })
    }
}

/// Liveness of a managed agent instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeHealth {
    /// Known to the runtime but not started.
    Provisioned,
    /// Process running **and** its agent card answered a probe.
    Healthy,
    /// Process running but the agent-card probe is failing (starting up or stuck).
    Unhealthy,
    /// The process has exited (or was stopped).
    Stopped,
}

/// A managed agent's id, current [`RuntimeHealth`], and endpoint.
#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    /// The agent's id.
    pub id: AgentId,
    /// Liveness at the time [`list`](AgentRuntime::list) was called.
    pub health: RuntimeHealth,
    /// The endpoint the instance serves on.
    pub endpoint: String,
}

/// Errors a runtime operation can return.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// No agent with this id has been provisioned.
    #[error("no agent provisioned with id '{0}'")]
    NotFound(AgentId),

    /// `start` was called on an agent that is already running.
    #[error("agent '{0}' is already running")]
    AlreadyRunning(AgentId),

    /// The agent process could not be spawned.
    #[error("failed to spawn agent '{id}': {source}")]
    Spawn {
        /// The agent that failed to start.
        id: AgentId,
        /// The underlying spawn error.
        #[source]
        source: std::io::Error,
    },

    /// The agent's config could not be loaded while building its spec.
    #[error("invalid agent config: {0}")]
    Config(String),

    /// The runtime backend (container engine, etc.) reported a failure — a
    /// non-zero `docker`/`podman` exit, or the engine binary being unavailable.
    #[error("runtime backend error: {0}")]
    Backend(String),
}

/// The capability the platform needs to *run* agents: provision, start, stop,
/// health-check, and list managed instances. One trait per capability (hex rule
/// 2); implemented by [`LocalProcessRuntime`] today, a container or
/// control-plane adapter later.
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Register an agent to be run, without starting it. Returns its [`AgentId`].
    async fn provision(&self, spec: AgentSpec) -> Result<AgentId, RuntimeError>;

    /// Start a provisioned agent. [`RuntimeError::AlreadyRunning`] if it is
    /// already live, [`RuntimeError::NotFound`] if it was never provisioned.
    async fn start(&self, id: &AgentId) -> Result<(), RuntimeError>;

    /// Stop a running agent. Idempotent: stopping an already-stopped agent is
    /// `Ok`. [`RuntimeError::NotFound`] if it was never provisioned.
    async fn stop(&self, id: &AgentId) -> Result<(), RuntimeError>;

    /// Report an agent's current [`RuntimeHealth`].
    async fn health(&self, id: &AgentId) -> Result<RuntimeHealth, RuntimeError>;

    /// List every provisioned agent with its current status.
    async fn list(&self) -> Result<Vec<RuntimeStatus>, RuntimeError>;
}
