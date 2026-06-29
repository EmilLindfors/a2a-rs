//! [`LocalProcessRuntime`] — supervise agents as child `a2a run` processes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::warn;

use super::{AgentRuntime, AgentSpec, RuntimeError, RuntimeHealth, RuntimeStatus};
use crate::registry::AgentId;

/// One supervised agent: its spec and where it is in its lifecycle.
struct Supervised {
    spec: AgentSpec,
    state: ProcState,
}

/// Lifecycle state of a supervised agent process.
enum ProcState {
    /// Provisioned but never started.
    Provisioned,
    /// A live child process serving the agent. Boxed to keep the enum small —
    /// `Child` dwarfs the unit variants.
    Running(Box<Child>),
    /// The process has been stopped (or exited on its own).
    Stopped,
}

/// An [`AgentRuntime`] that runs each agent as a child `a2a run --config <path>`
/// OS process.
///
/// A first-class adapter (hex rule 6 — not test-only): it gives real process
/// boundaries on a dev box without Docker, and naturally contains an agent's
/// `mcp_client` arbitrary-`command` child exec inside that agent's own process
/// tree. The supervised binary defaults to the current executable
/// ([`new`](Self::new)) so the supervisor runs copies of itself; point it
/// elsewhere with [`with_exe`](Self::with_exe).
///
/// Cheap to `clone` (shares one map). Children are spawned with `kill_on_drop`,
/// so dropping the runtime tears down everything it started.
#[derive(Clone)]
pub struct LocalProcessRuntime {
    exe: PathBuf,
    agents: Arc<Mutex<HashMap<AgentId, Supervised>>>,
}

impl LocalProcessRuntime {
    /// Supervise copies of the current executable (the `a2a` binary). Falls back
    /// to `"a2a"` on `PATH` if the current exe path can't be resolved.
    pub fn new() -> Self {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("a2a"));
        Self::with_exe(exe)
    }

    /// Supervise a specific `a2a` binary (e.g. `env!("CARGO_BIN_EXE_a2a")` in
    /// tests, or a pinned install path).
    pub fn with_exe(exe: impl Into<PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for LocalProcessRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentRuntime for LocalProcessRuntime {
    async fn provision(&self, spec: AgentSpec) -> Result<AgentId, RuntimeError> {
        let id = spec.id.clone();
        self.agents.lock().await.insert(
            id.clone(),
            Supervised {
                spec,
                state: ProcState::Provisioned,
            },
        );
        Ok(id)
    }

    async fn start(&self, id: &AgentId) -> Result<(), RuntimeError> {
        let mut guard = self.agents.lock().await;
        let entry = guard.get_mut(id).ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
        if matches!(entry.state, ProcState::Running(_)) {
            return Err(RuntimeError::AlreadyRunning(id.clone()));
        }

        let child = Command::new(&self.exe)
            .arg("run")
            .arg("--config")
            .arg(&entry.spec.config_path)
            .kill_on_drop(true)
            .spawn()
            .map_err(|source| RuntimeError::Spawn {
                id: id.clone(),
                source,
            })?;

        entry.state = ProcState::Running(Box::new(child));
        Ok(())
    }

    async fn stop(&self, id: &AgentId) -> Result<(), RuntimeError> {
        let mut guard = self.agents.lock().await;
        let entry = guard.get_mut(id).ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
        if let ProcState::Running(child) = &mut entry.state {
            if let Err(e) = child.kill().await {
                warn!("error killing agent '{}': {}", id, e);
            }
        }
        entry.state = ProcState::Stopped;
        Ok(())
    }

    async fn health(&self, id: &AgentId) -> Result<RuntimeHealth, RuntimeError> {
        // Resolve process state under the lock, then probe the card *outside* it
        // so a slow network probe never serializes other lifecycle ops.
        let endpoint = {
            let mut guard = self.agents.lock().await;
            let entry = guard.get_mut(id).ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
            match &mut entry.state {
                ProcState::Provisioned => return Ok(RuntimeHealth::Provisioned),
                ProcState::Stopped => return Ok(RuntimeHealth::Stopped),
                ProcState::Running(child) => match child.try_wait() {
                    // The process exited on its own — record and report it.
                    Ok(Some(_)) => {
                        entry.state = ProcState::Stopped;
                        return Ok(RuntimeHealth::Stopped);
                    }
                    // Still running (or status unknown) — probe the card below.
                    Ok(None) | Err(_) => entry.spec.endpoint.clone(),
                },
            }
        };

        match a2a_rs::fetch_agent_card(&endpoint).await {
            Ok(_) => Ok(RuntimeHealth::Healthy),
            Err(_) => Ok(RuntimeHealth::Unhealthy),
        }
    }

    async fn list(&self) -> Result<Vec<RuntimeStatus>, RuntimeError> {
        // Snapshot ids under the lock, then resolve health per-id (which re-locks
        // briefly and probes outside the lock) so we never hold it across awaits.
        let ids: Vec<AgentId> = self.agents.lock().await.keys().cloned().collect();
        let mut statuses = Vec::with_capacity(ids.len());
        for id in ids {
            let health = self.health(&id).await?;
            let endpoint = match self.agents.lock().await.get(&id) {
                Some(entry) => entry.spec.endpoint.clone(),
                None => continue, // deprovisioned between snapshot and probe
            };
            statuses.push(RuntimeStatus {
                id,
                health,
                endpoint,
            });
        }
        Ok(statuses)
    }
}
