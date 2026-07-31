//! Agent runtime — a place to *run* agents as managed, isolatable units.
//!
//! The [`AgentRuntime`] port is a **platform capability**: it provisions, starts,
//! stops, health-checks, and serves back the output of agent *instances*,
//! independently of how each agent serves requests once running
//! ([`AgentServer`](crate::AgentServer) is that per-agent leaf). This is the keystone of Pillar 3 — a future control-plane
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
//!
//! A runtime also answers whether it outlives the supervisor
//! ([`AgentRuntime::recover`] → [`Recovered`]). That is what separates a backend
//! a control plane can be restarted over — containers, whose engine keeps the
//! record — from one whose state dies with the process.

mod container;
mod local;
mod memory;

pub use container::{ContainerHardening, ContainerRuntime};
pub use local::LocalProcessRuntime;
pub use memory::InMemoryAgentRuntime;

use std::collections::BTreeSet;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::{AgentConfig, referenced_env_vars};
use crate::registry::AgentId;

/// The environment variables a deployed agent config is permitted to reference
/// via `${VAR}` — least privilege over the *deploying process's* secrets.
///
/// Without this, any config the control plane accepts can name any variable the
/// control-plane process holds (`description = "${AWS_SECRET_ACCESS_KEY}"`) and
/// have it expanded into the agent's card, where anyone who can fetch the card
/// reads it back. The allowlist is the operator's explicit statement of which
/// secrets the platform is willing to hand to agents.
///
/// **Deny-by-default:** [`deny_all`](Self::deny_all) is the [`Default`], so a
/// runtime built without an explicit allowlist rejects every config that
/// references anything. Populate it at the composition edge (the `--allow-env`
/// flag on `a2a control-plane`).
#[derive(Debug, Clone, Default)]
pub struct EnvAllowlist {
    allowed: BTreeSet<String>,
}

impl EnvAllowlist {
    /// Allow exactly these variable names.
    pub fn new(vars: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allowed: vars.into_iter().map(Into::into).collect(),
        }
    }

    /// Allow nothing — any `${VAR}` reference in a deployed config is rejected.
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Whether any variable is permitted.
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    /// Check a **raw, unexpanded** config against the allowlist, returning the
    /// variables it legitimately references (in sorted order) for a runtime to
    /// inject.
    ///
    /// Call this *before* parsing the config. Expansion resolves `${VAR}`
    /// against the host environment and errors differently for a set vs. unset
    /// variable, so parsing first would let a rejected config probe which
    /// secrets the control-plane process holds.
    ///
    /// `HOST` is excluded from the result and exempt from the check: it is
    /// adapter-owned (the container runtime sets `HOST=0.0.0.0` itself).
    pub fn check(&self, raw_config: &str) -> Result<Vec<String>, RuntimeError> {
        let referenced: Vec<String> = referenced_env_vars(raw_config)
            .into_iter()
            .filter(|var| var != "HOST")
            .collect();

        let disallowed: Vec<&str> = referenced
            .iter()
            .filter(|var| !self.allowed.contains(*var))
            .map(String::as_str)
            .collect();

        if disallowed.is_empty() {
            Ok(referenced)
        } else {
            Err(RuntimeError::DisallowedEnv(disallowed.join(", ")))
        }
    }
}

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
        let config = AgentConfig::from_file(&config_path)
            .map_err(|e| RuntimeError::Config(e.to_string()))?;
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

/// Matches the serialized form, so what an operator reads in `a2a ps` is what
/// they would find in the API's JSON.
impl std::fmt::Display for RuntimeHealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `pad`, not `write_str`: this is printed as a column in `a2a ps`, and
        // `write_str` silently ignores the `{:<14}` that keeps it one.
        f.pad(match self {
            RuntimeHealth::Provisioned => "provisioned",
            RuntimeHealth::Healthy => "healthy",
            RuntimeHealth::Unhealthy => "unhealthy",
            RuntimeHealth::Stopped => "stopped",
        })
    }
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

/// The outcome of asking a backend what it is already running
/// ([`AgentRuntime::recover`], [`ControlPlane::recover`](crate::ControlPlane::recover)).
///
/// The two cases are kept apart because "nothing to adopt" and "this backend
/// cannot tell you" lead to opposite conclusions, and conflating them is exactly
/// how a restarted control plane comes to report an empty fleet while the agents
/// are still up. `T` is whatever the layer hands back — [`AgentId`]s from the
/// port, richer status from the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovered<T> {
    /// The backend outlives this process and these are the instances it holds
    /// (possibly none — that answer is trustworthy).
    Adopted(Vec<T>),
    /// The backend cannot outlive this process, so whatever it was managing is
    /// gone and nothing could be adopted. Not an error, and not "none": an
    /// operator seeing this knows the fleet is unmanaged, not empty.
    Ephemeral,
}

impl<T> Recovered<T> {
    /// The adopted instances, or an empty slice for an ephemeral backend.
    ///
    /// For callers that only need to iterate. Anything that *reports* to a human
    /// should match instead, so the two cases stay distinguishable.
    pub fn adopted(&self) -> &[T] {
        match self {
            Recovered::Adopted(items) => items,
            Recovered::Ephemeral => &[],
        }
    }
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

    /// The config references environment variables the operator has not
    /// permitted deployed agents to read. See [`EnvAllowlist`].
    #[error(
        "agent config references environment variables that are not allowed: {0} \
         (permit them with `--allow-env`)"
    )]
    DisallowedEnv(String),

    /// This backend cannot serve the operation at all — as distinct from trying
    /// and failing ([`Backend`](Self::Backend)). Carries what would have to
    /// change, because "not supported" on its own leaves the operator with
    /// nowhere to go.
    #[error("{operation} is not available on this runtime: {reason}")]
    Unsupported {
        /// The operation that was asked for, e.g. `"logs"`.
        operation: &'static str,
        /// Why this backend cannot serve it, and what would make it able to.
        reason: String,
    },
}

/// The capability the platform needs to *run* agents: provision, start, stop,
/// health-check, and list managed instances. One trait per capability (hex rule
/// 2); implemented by [`LocalProcessRuntime`] today, a container or
/// control-plane adapter later.
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Register an agent to be run, without starting it. Returns its [`AgentId`].
    async fn provision(&self, spec: AgentSpec) -> Result<AgentId, RuntimeError>;

    /// Adopt the instances this backend is already running, so a restarted
    /// supervisor manages the fleet it left behind instead of a blank slate.
    ///
    /// Every adapter must answer, and the answer must be honest — there is
    /// deliberately no default. A backend whose state dies with this process
    /// returns [`Recovered::Ephemeral`] rather than an empty list, because a
    /// caller cannot otherwise tell "nothing was running" from "I have no idea
    /// what is running", and only the first of those is safe to report as an
    /// empty fleet.
    ///
    /// Call it once at startup, before serving. Adoption is expected to be
    /// idempotent: re-adopting an instance already in the map is not an error.
    async fn recover(&self) -> Result<Recovered<AgentId>, RuntimeError>;

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

    /// The agent's captured output, oldest line first, limited to the last
    /// `tail` lines when given.
    ///
    /// This is the one question an operator asks that health cannot answer:
    /// [`Unhealthy`](RuntimeHealth::Unhealthy) says the card probe is failing,
    /// not *why*, and the why is in the agent's own log. Supervising something
    /// without being able to read what it printed makes every failure a matter
    /// of guessing.
    ///
    /// Not every backend can capture output, so an adapter that cannot must say
    /// so with [`RuntimeError::Unsupported`] rather than return an empty list —
    /// "nothing was logged" and "I do not keep logs" send an operator to very
    /// different places. Same reasoning as [`Recovered::Ephemeral`], and the
    /// reason there is deliberately no default implementation.
    async fn logs(&self, id: &AgentId, tail: Option<usize>) -> Result<Vec<String>, RuntimeError>;
}

/// Keep the last `tail` lines of `text`, oldest first.
///
/// Shared by the adapters that get a blob back (a file, an engine's `logs`
/// output) rather than a line stream. `None` keeps everything.
pub(crate) fn tail_lines(text: &str, tail: Option<usize>) -> Vec<String> {
    let lines = text.lines();
    match tail {
        // `skip` rather than collect-then-truncate: the whole point is not to
        // hand the caller more than it asked for.
        Some(n) => {
            let total = text.lines().count();
            lines
                .skip(total.saturating_sub(n))
                .map(String::from)
                .collect()
        }
        None => lines.map(String::from).collect(),
    }
}

#[cfg(test)]
mod tail_tests {
    use super::tail_lines;

    const LOG: &str = "first\nsecond\nthird\n";

    #[test]
    fn no_limit_keeps_every_line_oldest_first() {
        assert_eq!(tail_lines(LOG, None), ["first", "second", "third"]);
    }

    #[test]
    fn a_limit_keeps_the_newest_lines() {
        assert_eq!(tail_lines(LOG, Some(2)), ["second", "third"]);
        assert_eq!(tail_lines(LOG, Some(1)), ["third"]);
    }

    /// Asking for more than there is returns what there is — not an error, and
    /// not padding.
    #[test]
    fn a_limit_larger_than_the_log_is_the_whole_log() {
        assert_eq!(tail_lines(LOG, Some(99)), ["first", "second", "third"]);
    }

    #[test]
    fn empty_input_and_zero_tail_are_both_empty() {
        assert!(tail_lines("", None).is_empty());
        assert!(tail_lines(LOG, Some(0)).is_empty());
    }
}

#[cfg(test)]
mod allowlist_tests {
    use super::*;

    const CONFIG: &str = r#"
        [agent]
        name = "Leaky"
        description = "${AWS_SECRET_ACCESS_KEY}"

        [handler.llm]
        api_key = "${OPENROUTER_API_KEY}"
    "#;

    #[test]
    fn deny_all_rejects_every_reference() {
        let err = EnvAllowlist::deny_all().check(CONFIG).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("AWS_SECRET_ACCESS_KEY"), "{msg}");
        assert!(msg.contains("OPENROUTER_API_KEY"), "{msg}");
    }

    #[test]
    fn partial_allowlist_still_rejects_and_names_only_the_offender() {
        let err = EnvAllowlist::new(["OPENROUTER_API_KEY"])
            .check(CONFIG)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("AWS_SECRET_ACCESS_KEY"), "{msg}");
        assert!(
            !msg.contains("OPENROUTER_API_KEY"),
            "an allowed var must not be reported as disallowed: {msg}"
        );
    }

    #[test]
    fn fully_allowed_config_yields_the_vars_to_inject() {
        let allowed =
            EnvAllowlist::new(["AWS_SECRET_ACCESS_KEY", "OPENROUTER_API_KEY", "UNUSED_VAR"]);
        // Only what the config actually references — not the whole allowlist.
        assert_eq!(
            allowed.check(CONFIG).unwrap(),
            ["AWS_SECRET_ACCESS_KEY", "OPENROUTER_API_KEY"]
        );
    }

    #[test]
    fn host_is_adapter_owned_and_neither_checked_nor_injected() {
        // `HOST` passes deny-all *and* stays out of the injection list.
        assert!(
            EnvAllowlist::deny_all()
                .check(r#"host = "${HOST}""#)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn config_with_no_references_passes_deny_all() {
        assert!(
            EnvAllowlist::deny_all()
                .check(r#"name = "plain""#)
                .unwrap()
                .is_empty()
        );
    }
}
