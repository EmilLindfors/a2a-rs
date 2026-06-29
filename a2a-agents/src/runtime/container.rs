//! [`ContainerRuntime`] — run each agent in a Docker/Podman container.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::sync::Mutex;

use super::{AgentRuntime, AgentSpec, RuntimeError, RuntimeHealth, RuntimeStatus};
use crate::core::AgentConfig;
use crate::registry::AgentId;

/// Default base image (one image, config injected per agent). Override with
/// [`ContainerRuntime::with_image`].
const DEFAULT_IMAGE: &str = "a2a-agents:latest";

/// Where the agent's TOML is mounted inside the container.
const CONTAINER_CONFIG_PATH: &str = "/etc/agent.toml";

/// An [`AgentRuntime`] that runs each agent in its own container via a
/// `docker`/`podman` CLI (shelled out through [`tokio::process`], so no Docker
/// API dependency — the engine binary is the only requirement).
///
/// One container per agent, named `a2a-agent-<id>`. The engine is the source of
/// truth for liveness (`inspect`); the in-memory map only remembers each agent's
/// published port so health can probe its card.
///
/// **Binding:** the in-container agent must bind `0.0.0.0` to be reachable
/// through the published port. The base image sets `HOST=0.0.0.0` and this
/// adapter passes `-e HOST=0.0.0.0`; since the config's `default_host` reads
/// `HOST`, a config that **omits** `host` binds all interfaces. A config that
/// hard-codes `host = "127.0.0.1"` will not be reachable.
///
/// **Platform:** the config is bind-mounted (`-v host:container`), so host config
/// paths must be expressible as a Docker volume source — works on Linux/macOS;
/// Windows host paths need conversion (out of scope here).
#[derive(Clone)]
pub struct ContainerRuntime {
    engine: String,
    image: String,
    /// id -> published host port (presence == provisioned).
    agents: Arc<Mutex<HashMap<AgentId, u16>>>,
}

impl ContainerRuntime {
    /// Use `docker` with the default image.
    pub fn new() -> Self {
        Self::with_engine("docker")
    }

    /// Use a specific engine binary (`"docker"` or `"podman"`).
    pub fn with_engine(engine: impl Into<String>) -> Self {
        Self {
            engine: engine.into(),
            image: DEFAULT_IMAGE.to_string(),
            agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Override the base image (default [`DEFAULT_IMAGE`]).
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = image.into();
        self
    }

    /// `inspect -f {{.State.Status}}` → the container's status, or `None` when no
    /// such container exists.
    async fn inspect_status(&self, name: &str) -> Option<String> {
        let args = [
            "inspect".to_string(),
            "-f".to_string(),
            "{{.State.Status}}".to_string(),
            name.to_string(),
        ];
        run_engine(&self.engine, &args).await.ok()
    }
}

impl Default for ContainerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// The container name for an agent: `a2a-agent-<id>`.
fn container_name(id: &AgentId) -> String {
    format!("a2a-agent-{id}")
}

/// Build the `docker create` argv that runs an agent: publish its port, inject
/// the config as a read-only mount, bind all interfaces, and run
/// `a2a run --config /etc/agent.toml`.
fn create_args(image: &str, id: &AgentId, port: u16, config_path: &Path) -> Vec<String> {
    vec![
        "create".to_string(),
        "--name".to_string(),
        container_name(id),
        "-p".to_string(),
        format!("{port}:{port}"),
        "-e".to_string(),
        "HOST=0.0.0.0".to_string(),
        "-v".to_string(),
        format!("{}:{CONTAINER_CONFIG_PATH}:ro", config_path.display()),
        "--label".to_string(),
        format!("a2a-agent={id}"),
        image.to_string(),
        "run".to_string(),
        "--config".to_string(),
        CONTAINER_CONFIG_PATH.to_string(),
    ]
}

/// Run the engine with `args`, returning trimmed stdout. A spawn failure or a
/// non-zero exit (with stderr) becomes [`RuntimeError::Backend`].
async fn run_engine(engine: &str, args: &[String]) -> Result<String, RuntimeError> {
    let output = Command::new(engine)
        .args(args)
        .output()
        .await
        .map_err(|e| RuntimeError::Backend(format!("could not run `{engine}`: {e}")))?;
    if !output.status.success() {
        let verb = args.first().map(String::as_str).unwrap_or("");
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RuntimeError::Backend(format!(
            "`{engine} {verb}` failed: {}",
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[async_trait]
impl AgentRuntime for ContainerRuntime {
    async fn provision(&self, spec: AgentSpec) -> Result<AgentId, RuntimeError> {
        let id = spec.id.clone();
        // The published port is the agent's configured HTTP port.
        let config = AgentConfig::from_file(&spec.config_path)
            .map_err(|e| RuntimeError::Config(e.to_string()))?;
        let port = config.server.http_port;

        // Clear a stale container of the same name so re-provision is idempotent.
        let _ = run_engine(&self.engine, &["rm".to_string(), "-f".to_string(), container_name(&id)])
            .await;

        run_engine(&self.engine, &create_args(&self.image, &id, port, &spec.config_path)).await?;
        self.agents.lock().await.insert(id.clone(), port);
        Ok(id)
    }

    async fn start(&self, id: &AgentId) -> Result<(), RuntimeError> {
        let name = {
            let guard = self.agents.lock().await;
            if !guard.contains_key(id) {
                return Err(RuntimeError::NotFound(id.clone()));
            }
            container_name(id)
        };
        if self.inspect_status(&name).await.as_deref() == Some("running") {
            return Err(RuntimeError::AlreadyRunning(id.clone()));
        }
        run_engine(&self.engine, &["start".to_string(), name]).await?;
        Ok(())
    }

    async fn stop(&self, id: &AgentId) -> Result<(), RuntimeError> {
        let name = {
            let guard = self.agents.lock().await;
            if !guard.contains_key(id) {
                return Err(RuntimeError::NotFound(id.clone()));
            }
            container_name(id)
        };
        run_engine(&self.engine, &["stop".to_string(), name]).await?;
        Ok(())
    }

    async fn health(&self, id: &AgentId) -> Result<RuntimeHealth, RuntimeError> {
        let (name, port) = {
            let guard = self.agents.lock().await;
            let port = *guard.get(id).ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
            (container_name(id), port)
        };
        match self.inspect_status(&name).await.as_deref() {
            Some("created") => Ok(RuntimeHealth::Provisioned),
            Some("running") => match a2a_rs::fetch_agent_card(&format!("http://127.0.0.1:{port}")).await
            {
                Ok(_) => Ok(RuntimeHealth::Healthy),
                Err(_) => Ok(RuntimeHealth::Unhealthy),
            },
            // exited / dead / paused / removed-out-of-band
            _ => Ok(RuntimeHealth::Stopped),
        }
    }

    async fn list(&self) -> Result<Vec<RuntimeStatus>, RuntimeError> {
        let ids: Vec<(AgentId, u16)> = {
            let guard = self.agents.lock().await;
            guard.iter().map(|(id, port)| (id.clone(), *port)).collect()
        };
        let mut statuses = Vec::with_capacity(ids.len());
        for (id, port) in ids {
            let health = self.health(&id).await?;
            statuses.push(RuntimeStatus {
                id,
                health,
                endpoint: format!("http://127.0.0.1:{port}"),
            });
        }
        Ok(statuses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_name_is_prefixed_slug() {
        assert_eq!(
            container_name(&AgentId::from_name("Weather Agent")),
            "a2a-agent-weather-agent"
        );
    }

    #[test]
    fn create_args_build_expected_argv() {
        let id = AgentId::from_name("Echo Agent");
        let args = create_args("a2a-agents:latest", &id, 8080, Path::new("/cfg/echo.toml"));
        assert_eq!(
            args,
            vec![
                "create",
                "--name",
                "a2a-agent-echo-agent",
                "-p",
                "8080:8080",
                "-e",
                "HOST=0.0.0.0",
                "-v",
                "/cfg/echo.toml:/etc/agent.toml:ro",
                "--label",
                "a2a-agent=echo-agent",
                "a2a-agents:latest",
                "run",
                "--config",
                "/etc/agent.toml",
            ]
        );
    }
}
