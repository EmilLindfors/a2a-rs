//! [`LocalProcessRuntime`] — supervise agents as child `a2a run` processes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::warn;

use super::{
    AgentRuntime, AgentSpec, EnvAllowlist, Recovered, RuntimeError, RuntimeHealth, RuntimeStatus,
    tail_lines,
};
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
///
/// # Logs
///
/// By default a child **inherits** this process's stdout/stderr, so its output
/// lands in the supervisor's terminal and [`logs`](AgentRuntime::logs) has
/// nothing to read — it reports [`RuntimeError::Unsupported`] rather than
/// pretending the agent was silent. Give it a
/// [`log_dir`](Self::with_log_dir) to capture each agent's output into its own
/// file instead; that is what makes `a2a logs <id>` work over this backend, and
/// it is also what stops a multi-agent control plane from interleaving every
/// agent's output into one unlabelled stream.
///
/// # Secrets — dev-only isolation
///
/// [`provision`](AgentRuntime::provision) enforces the same [`EnvAllowlist`] as
/// [`ContainerRuntime`](super::ContainerRuntime), so a deployed config cannot
/// template an un-permitted secret into itself (and thence into its agent card).
///
/// But a child process **inherits this process's entire environment**, and there
/// is no allowlist that changes that: an agent whose config declares an
/// `mcp_client` with an arbitrary `command` can read every variable the control
/// plane holds. That is inherent to process inheritance and is why this adapter
/// is dev/test-only — deploy untrusted or third-party configs on
/// [`ContainerRuntime`](super::ContainerRuntime), where only the allowlisted
/// variables cross the boundary.
#[derive(Clone)]
pub struct LocalProcessRuntime {
    exe: PathBuf,
    /// Which host env vars a deployed config may reference. Deny-all by default.
    allowed_env: EnvAllowlist,
    /// Where each agent's captured output goes. `None` ⇒ children inherit this
    /// process's streams and there is nothing to serve back.
    log_dir: Option<PathBuf>,
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
            allowed_env: EnvAllowlist::deny_all(),
            log_dir: None,
            agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Capture each agent's stdout and stderr into `<dir>/<id>.log`, making
    /// [`logs`](AgentRuntime::logs) answerable over this backend.
    ///
    /// Appends rather than truncates, so restarting an agent keeps the output
    /// that explains why it needed restarting.
    pub fn with_log_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.log_dir = Some(dir.into());
        self
    }

    /// Permit deployed configs to reference these host environment variables.
    /// Anything not listed is rejected at [`provision`](AgentRuntime::provision).
    ///
    /// Note the caveat in the type docs: this bounds what a *config* may name,
    /// not what a child process can read.
    pub fn with_allowed_env(mut self, allowed: EnvAllowlist) -> Self {
        self.allowed_env = allowed;
        self
    }
}

impl Default for LocalProcessRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Where an agent's captured output lives under a log directory.
fn log_path(dir: &Path, id: &AgentId) -> PathBuf {
    // `AgentId` is a slug, so it is already a safe filename component.
    dir.join(format!("{id}.log"))
}

/// Open an agent's log file as a pair of child stdio handles (one per stream).
///
/// Both handles are the *same* appended file, so the child's stdout and stderr
/// interleave the way they would on a terminal — which matters because an agent
/// prints its banner on stdout and everything `tracing` emits on stderr, and
/// reading only one of those is how you end up looking at an empty log for a
/// crashing agent.
async fn log_stdio(dir: &Path, id: &AgentId) -> Result<(Stdio, Stdio), std::io::Error> {
    tokio::fs::create_dir_all(dir).await?;
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(dir, id))
        .await?
        // Back to a std handle: `Stdio` is what `Command` takes, and the child
        // owns the descriptor from here on.
        .into_std()
        .await;
    let dup = file.try_clone()?;
    Ok((Stdio::from(file), Stdio::from(dup)))
}

#[async_trait]
impl AgentRuntime for LocalProcessRuntime {
    async fn provision(&self, spec: AgentSpec) -> Result<AgentId, RuntimeError> {
        // Vet the raw config before it is ever expanded — see `EnvAllowlist::check`.
        let content = tokio::fs::read_to_string(&spec.config_path)
            .await
            .map_err(|e| RuntimeError::Config(e.to_string()))?;
        self.allowed_env.check(&content)?;

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

    /// Always [`Recovered::Ephemeral`]: nothing here survives the supervisor.
    ///
    /// Children are spawned with `kill_on_drop`, so an orderly exit takes them
    /// with it, and the only record that they existed was the in-memory table
    /// that died with the process. Even after a `SIGKILL` leaves orphans behind,
    /// there is nothing durable tying a stray `a2a run` to an [`AgentId`] — so
    /// the honest answer is "I cannot tell you", not an empty list. This is the
    /// concrete reason the container runtime is the supported control-plane
    /// backend and this one is for dev loops.
    async fn recover(&self) -> Result<Recovered<AgentId>, RuntimeError> {
        Ok(Recovered::Ephemeral)
    }

    async fn start(&self, id: &AgentId) -> Result<(), RuntimeError> {
        // The lock is held across the log-file open below. That serializes
        // concurrent starts, which is the point: the not-already-running check
        // and the spawn have to be one step, or two callers both pass it.
        let mut guard = self.agents.lock().await;
        let entry = guard
            .get_mut(id)
            .ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
        if matches!(entry.state, ProcState::Running(_)) {
            return Err(RuntimeError::AlreadyRunning(id.clone()));
        }

        let mut command = Command::new(&self.exe);
        command
            .arg("run")
            .arg("--config")
            .arg(&entry.spec.config_path)
            .kill_on_drop(true);
        // Without a log dir the child inherits our streams — the dev-loop
        // default, where the operator is watching the terminal anyway.
        if let Some(dir) = &self.log_dir {
            let (out, err) = log_stdio(dir, id)
                .await
                .map_err(|source| RuntimeError::Spawn {
                    id: id.clone(),
                    source,
                })?;
            command.stdout(out).stderr(err);
        }

        let child = command.spawn().map_err(|source| RuntimeError::Spawn {
            id: id.clone(),
            source,
        })?;

        entry.state = ProcState::Running(Box::new(child));
        Ok(())
    }

    async fn stop(&self, id: &AgentId) -> Result<(), RuntimeError> {
        let mut guard = self.agents.lock().await;
        let entry = guard
            .get_mut(id)
            .ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
        if let ProcState::Running(child) = &mut entry.state
            && let Err(e) = child.kill().await
        {
            warn!("error killing agent '{}': {}", id, e);
        }
        entry.state = ProcState::Stopped;
        Ok(())
    }

    async fn health(&self, id: &AgentId) -> Result<RuntimeHealth, RuntimeError> {
        // Resolve process state under the lock, then probe the card *outside* it
        // so a slow network probe never serializes other lifecycle ops.
        let endpoint = {
            let mut guard = self.agents.lock().await;
            let entry = guard
                .get_mut(id)
                .ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
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

    /// Read back what the child wrote to its captured log file.
    ///
    /// Only answerable when this runtime was given a
    /// [`log_dir`](Self::with_log_dir); otherwise the output went to the
    /// supervisor's own streams and is not ours to serve.
    async fn logs(&self, id: &AgentId, tail: Option<usize>) -> Result<Vec<String>, RuntimeError> {
        if !self.agents.lock().await.contains_key(id) {
            return Err(RuntimeError::NotFound(id.clone()));
        }
        let Some(dir) = &self.log_dir else {
            return Err(RuntimeError::Unsupported {
                operation: "logs",
                reason: "this local runtime does not capture agent output — its children \
                         inherit the supervisor's stdout/stderr. Start the control plane \
                         with --log-dir to capture it per agent."
                    .to_string(),
            });
        };
        match tokio::fs::read_to_string(log_path(dir, id)).await {
            Ok(text) => Ok(tail_lines(&text, tail)),
            // Provisioned but never started, so no file exists yet. That is an
            // empty log, not a missing agent — `NotFound` above already ruled
            // the latter out.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(RuntimeError::Backend(format!(
                "could not read the log for '{id}': {e}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_path_is_the_slug_under_the_log_dir() {
        assert_eq!(
            log_path(
                Path::new("/var/log/a2a"),
                &AgentId::from_name("Weather Agent")
            ),
            Path::new("/var/log/a2a/weather-agent.log")
        );
    }

    /// Without a log dir the runtime must say it cannot serve logs, not report
    /// the agent as silent — an operator debugging a crash needs to know which.
    #[tokio::test]
    async fn logs_without_a_log_dir_are_unsupported_not_empty() {
        let runtime = LocalProcessRuntime::with_exe("a2a");
        let spec = AgentSpec {
            id: AgentId::from_name("Quiet"),
            config_path: PathBuf::from("quiet.toml"),
            endpoint: "http://127.0.0.1:8080".to_string(),
        };
        // Provision directly: the file-reading `provision` would need a config
        // on disk, and this is about the log seam.
        runtime.agents.lock().await.insert(
            spec.id.clone(),
            Supervised {
                spec,
                state: ProcState::Provisioned,
            },
        );

        let err = runtime
            .logs(&AgentId::from("quiet"), None)
            .await
            .expect_err("a runtime with no log dir cannot serve logs");
        assert!(
            matches!(&err, RuntimeError::Unsupported { operation, .. } if *operation == "logs"),
            "got: {err}"
        );
        // The message has to point somewhere.
        assert!(err.to_string().contains("--log-dir"), "{err}");
    }

    #[tokio::test]
    async fn logs_for_an_unprovisioned_agent_are_not_found() {
        let runtime = LocalProcessRuntime::with_exe("a2a").with_log_dir(std::env::temp_dir());
        assert!(matches!(
            runtime.logs(&AgentId::from("ghost"), None).await,
            Err(RuntimeError::NotFound(_))
        ));
    }
}
