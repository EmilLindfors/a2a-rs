//! Control plane — compose the runtime and registry ports into a platform.
//!
//! [`ControlPlane`] is the service (hex rule 9a) that owns an
//! [`AgentRuntime`](crate::runtime::AgentRuntime) and an
//! [`AgentRegistry`](crate::registry::AgentRegistry) and orchestrates the
//! deploy/undeploy use-cases across them: deploying an agent both *runs* it (via
//! the runtime) and *publishes its card* (via the registry) so peers discover it.
//! It is assembled at the edge with concrete adapters injected — today a
//! [`LocalProcessRuntime`](crate::runtime::LocalProcessRuntime) +
//! [`InMemoryAgentRegistry`](crate::registry::InMemoryAgentRegistry), a container
//! runtime / persistent registry later, with no change here.
//!
//! [`control_plane_router`] exposes the service over HTTP — the surface the
//! Terraform provider targets.

mod http;

pub use http::control_plane_router;

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::AgentBuilder;
use crate::core::config::ConfigError;
use crate::registry::{AgentId, AgentRegistry, RegistryError};
use crate::runtime::{AgentRuntime, AgentSpec, RuntimeError, RuntimeHealth};

/// A deployed agent's id, endpoint, and current health — the control-plane DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployedAgent {
    /// The agent's id (slug of its name), shared by runtime and registry.
    pub id: String,
    /// The endpoint the agent serves on.
    pub endpoint: String,
    /// Its current runtime health.
    pub health: RuntimeHealth,
}

/// Errors a control-plane operation can return.
#[derive(Debug, Error)]
pub enum ControlPlaneError {
    /// A runtime operation failed (spawn, not-found, already-running, …).
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    /// A registry operation failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),

    /// The agent's config could not be loaded or parsed.
    #[error("invalid agent config: {0}")]
    Config(#[from] ConfigError),

    /// The agent card could not be built from the config.
    #[error("could not build agent card: {0}")]
    Card(String),
}

/// Owns the runtime + registry ports and drives deploy/undeploy across them.
#[derive(Clone)]
pub struct ControlPlane {
    runtime: Arc<dyn AgentRuntime>,
    registry: Arc<dyn AgentRegistry>,
}

impl ControlPlane {
    /// Assemble a control plane over concrete runtime + registry adapters.
    pub fn new(runtime: Arc<dyn AgentRuntime>, registry: Arc<dyn AgentRegistry>) -> Self {
        Self { runtime, registry }
    }

    /// Deploy an already-parsed agent: provision + start it in the runtime, then
    /// register its card so peers discover it. The runtime and registry ids
    /// coincide (both the slug of the agent name).
    ///
    /// The caller materializes the config on disk and passes the resulting
    /// `config_path` (the file the runtime's child process reads) together with
    /// the `builder` parsed from it — so the service orchestrates ports + pure
    /// config without itself touching the filesystem (hex rule 9a), and the TOML
    /// is parsed exactly once.
    pub async fn deploy(
        &self,
        builder: &AgentBuilder,
        config_path: PathBuf,
    ) -> Result<DeployedAgent, ControlPlaneError> {
        let config = builder.config();
        let spec = AgentSpec {
            id: AgentId::from_name(&config.agent.name),
            config_path,
            endpoint: config.agent_url(),
        };
        let endpoint = spec.endpoint.clone();

        // Build the card before mutating any state, so a bad card fails the
        // deploy without leaving a half-started agent behind.
        let card = builder
            .agent_card()
            .await
            .map_err(|e| ControlPlaneError::Card(e.to_string()))?;

        let id = self.runtime.provision(spec).await?;
        self.runtime.start(&id).await?;
        self.registry.register(card, endpoint.clone()).await?;

        let health = self.runtime.health(&id).await?;
        Ok(DeployedAgent {
            id: id.to_string(),
            endpoint,
            health,
        })
    }

    /// Stop an agent in the runtime and deregister it. Idempotent on the
    /// registry side (a missing entry is not an error).
    pub async fn undeploy(&self, id: &AgentId) -> Result<(), ControlPlaneError> {
        self.runtime.stop(id).await?;
        match self.registry.deregister(id).await {
            Ok(()) | Err(RegistryError::NotFound(_)) => Ok(()),
        }
    }

    /// Report an agent's current runtime health.
    pub async fn status(&self, id: &AgentId) -> Result<RuntimeHealth, ControlPlaneError> {
        Ok(self.runtime.health(id).await?)
    }

    /// List every deployed agent with its endpoint and health.
    pub async fn list(&self) -> Result<Vec<DeployedAgent>, ControlPlaneError> {
        Ok(self
            .runtime
            .list()
            .await?
            .into_iter()
            .map(|s| DeployedAgent {
                id: s.id.to_string(),
                endpoint: s.endpoint,
                health: s.health,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::InMemoryAgentRegistry;
    use crate::runtime::InMemoryAgentRuntime;

    /// A temp echo-agent config the control plane can deploy. Removed on drop.
    struct TempConfig {
        path: std::path::PathBuf,
    }

    impl TempConfig {
        fn echo(name: &str, port: u16) -> Self {
            let path = std::env::temp_dir()
                .join(format!("cp_test_{}_{port}.toml", AgentId::from_name(name)));
            let toml = format!(
                r#"
[agent]
name = "{name}"

[handler]
type = "echo"

[server]
host = "127.0.0.1"
http_port = {port}

[[skills]]
id = "echo-skill"
name = "Echo"
"#
            );
            std::fs::write(&path, toml).unwrap();
            Self { path }
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn control_plane() -> (ControlPlane, Arc<dyn AgentRegistry>) {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let runtime: Arc<dyn AgentRuntime> = Arc::new(InMemoryAgentRuntime::new());
        (ControlPlane::new(runtime, registry.clone()), registry)
    }

    #[tokio::test]
    async fn deploy_runs_and_registers_then_undeploy_tears_down() {
        let (cp, registry) = control_plane();
        let config = TempConfig::echo("Deploy Me", 8123);

        let builder = AgentBuilder::from_file(&config.path).unwrap();
        let deployed = cp
            .deploy(&builder, config.path.clone())
            .await
            .expect("deploy");
        assert_eq!(deployed.id, "deploy-me");
        assert_eq!(deployed.endpoint, "http://127.0.0.1:8123");
        assert_eq!(deployed.health, RuntimeHealth::Healthy);

        // It is both running (status) and discoverable (registry).
        let id = AgentId::from_name("Deploy Me");
        assert_eq!(cp.status(&id).await.unwrap(), RuntimeHealth::Healthy);
        assert!(registry.get(&id).await.unwrap().is_some());
        assert_eq!(cp.list().await.unwrap().len(), 1);

        // Undeploy stops it and removes it from discovery.
        cp.undeploy(&id).await.expect("undeploy");
        assert_eq!(cp.status(&id).await.unwrap(), RuntimeHealth::Stopped);
        assert!(registry.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn deploy_is_discoverable_by_skill() {
        let (cp, registry) = control_plane();
        let config = TempConfig::echo("Skilled Agent", 8124);

        let builder = AgentBuilder::from_file(&config.path).unwrap();
        cp.deploy(&builder, config.path.clone())
            .await
            .expect("deploy");

        let matches = registry.find_by_skill("echo-skill").await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].card.name, "Skilled Agent");
    }

    #[tokio::test]
    async fn undeploy_unknown_agent_errors_not_found() {
        let (cp, _registry) = control_plane();
        let err = cp
            .undeploy(&AgentId::from("ghost"))
            .await
            .expect_err("undeploy of an unprovisioned agent should fail");
        assert!(matches!(
            err,
            ControlPlaneError::Runtime(RuntimeError::NotFound(_))
        ));
    }
}
