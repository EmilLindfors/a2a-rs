//! Control plane — compose the runtime, registry, and card-source ports into a
//! platform.
//!
//! [`ControlPlane`] is the service (hex rule 9a) that owns an
//! [`AgentRuntime`](crate::runtime::AgentRuntime), an
//! [`AgentRegistry`](crate::registry::AgentRegistry), and a
//! [`CardSource`](crate::registry::CardSource), and orchestrates the use-cases
//! that span them: deploying an agent both *runs* it (via the runtime) and
//! *publishes its card* (via the registry) so peers discover it, and
//! [`recover`](ControlPlane::recover) rebuilds both halves after a restart. It
//! is assembled at the edge with concrete adapters injected — today a
//! [`LocalProcessRuntime`](crate::runtime::LocalProcessRuntime) +
//! [`InMemoryAgentRegistry`](crate::registry::InMemoryAgentRegistry), a container
//! runtime / persistent registry later, with no change here.
//!
//! **Startup order matters:** call [`recover`](ControlPlane::recover) before
//! serving. Everything the control plane knows is process state, so a restart
//! that skips it answers `GET /agents` with `[]` while the agents are still up.
//!
//! [`control_plane_router`] exposes the service over HTTP, and
//! [`ControlPlaneClient`] is the other side of that contract — what `a2a
//! deploy/ps/logs/stop` drive, and what the Terraform provider will target. The
//! bodies both adapters agree on live in [`wire`].

mod auth;
mod client;
mod http;
mod wire;

pub use auth::ControlPlaneAuth;
pub use client::{ControlPlaneClient, ControlPlaneClientError};
pub use http::control_plane_router;
pub use wire::{AgentLogs, AgentStatus, ApiErrorBody, DeployRequest, LogsQuery};

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

use crate::core::AgentBuilder;
use crate::core::config::ConfigError;
use crate::registry::{AgentId, AgentRegistry, CardSource, RegistryError};
use crate::runtime::{
    AgentRuntime, AgentSpec, EnvAllowlist, Recovered, RuntimeError, RuntimeHealth,
};

/// A deployed agent's id, endpoint, and current health — the control-plane DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// A submitted config that has passed policy checks **and** parsed cleanly.
///
/// Only [`ControlPlane::prepare`] can construct one, and [`ControlPlane::deploy`]
/// accepts nothing else — so an unvetted config cannot reach the runtime by
/// forgetting a call. The caller gets the [`AgentId`] out in order to name the
/// file it materializes, which is why preparing and deploying are two steps.
pub struct PreparedDeploy {
    builder: AgentBuilder,
    id: AgentId,
}

impl PreparedDeploy {
    /// The agent's id, derived from its name. Stable across prepare → deploy.
    pub fn id(&self) -> &AgentId {
        &self.id
    }
}

/// Deliberately shows only the id. The inner config has had its `${VAR}` refs
/// **expanded**, so a derived `Debug` would print resolved secrets into any log
/// line that formats a `PreparedDeploy`.
impl std::fmt::Debug for PreparedDeploy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedDeploy")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Owns the runtime + registry + card-source ports and drives the deploy,
/// undeploy, and recovery use-cases across them.
#[derive(Clone)]
pub struct ControlPlane {
    runtime: Arc<dyn AgentRuntime>,
    registry: Arc<dyn AgentRegistry>,
    cards: Arc<dyn CardSource>,
    allowed_env: EnvAllowlist,
}

impl ControlPlane {
    /// Assemble a control plane over concrete adapters.
    ///
    /// `cards` reads a running agent's published card; it is required rather
    /// than optional because [`recover`](Self::recover) needs it to put adopted
    /// agents back into discovery, and a control plane that silently skipped
    /// that step would come back from a restart supervising agents nobody can
    /// find.
    ///
    /// The env allowlist is deny-all until set with
    /// [`with_allowed_env`](Self::with_allowed_env).
    pub fn new(
        runtime: Arc<dyn AgentRuntime>,
        registry: Arc<dyn AgentRegistry>,
        cards: Arc<dyn CardSource>,
    ) -> Self {
        Self {
            runtime,
            registry,
            cards,
            allowed_env: EnvAllowlist::deny_all(),
        }
    }

    /// Permit submitted configs to reference these host environment variables.
    ///
    /// Give the runtime adapter the *same* allowlist at the composition edge:
    /// this one vets what the API accepts, the runtime's decides what actually
    /// crosses into the agent. One policy value, two enforcement points — the
    /// runtime stays safe even when something other than this service drives it.
    pub fn with_allowed_env(mut self, allowed: EnvAllowlist) -> Self {
        self.allowed_env = allowed;
        self
    }

    /// Vet and parse a submitted config, yielding the token [`deploy`](Self::deploy)
    /// requires.
    ///
    /// The allowlist is checked against the **raw** text first. Parsing expands
    /// `${VAR}` against this process's environment and distinguishes a set from
    /// an unset variable in its error, so a config naming a forbidden secret must
    /// be rejected before it can be used to probe what secrets exist here.
    pub fn prepare(&self, raw_toml: &str) -> Result<PreparedDeploy, ControlPlaneError> {
        self.allowed_env.check(raw_toml)?;
        let builder = AgentBuilder::from_toml(raw_toml)?;
        let id = AgentId::from_name(&builder.config().agent.name);
        Ok(PreparedDeploy { builder, id })
    }

    /// Deploy a [`prepare`](Self::prepare)d agent: provision + start it in the
    /// runtime, then register its card so peers discover it. The runtime and
    /// registry ids coincide (both the slug of the agent name).
    ///
    /// The caller materializes the config on disk — at the path
    /// `prepared.id()` told it to use — and passes that `config_path` (the file
    /// the runtime reads). So the service orchestrates ports + pure config
    /// without itself touching the filesystem (hex rule 9a), and the TOML is
    /// parsed exactly once, during `prepare`.
    pub async fn deploy(
        &self,
        prepared: PreparedDeploy,
        config_path: PathBuf,
    ) -> Result<DeployedAgent, ControlPlaneError> {
        let PreparedDeploy { builder, id } = prepared;
        let config = builder.config();
        let spec = AgentSpec {
            id,
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

    /// Re-adopt the agents this platform was already running, and put the ones
    /// that answer back into discovery.
    ///
    /// Call once at startup, before serving. Both halves of the control plane's
    /// knowledge are process state — the runtime's instance table and the
    /// registry's cards — so without this a restarted control plane reports an
    /// empty fleet while the agents are still up: `GET /agents` says nothing is
    /// deployed, `DELETE` says `NotFound`, peers stop resolving each other by
    /// skill, and a Terraform `Read` concludes the agents were destroyed
    /// out-of-band and recreates them on top of the running ones.
    ///
    /// Registration is attempted only for agents the runtime reports
    /// [`Healthy`](RuntimeHealth::Healthy) — that state *means* the card probe
    /// succeeded, so anything else would be a fetch known in advance to fail.
    /// An agent that is adopted but not registered is still managed (it can be
    /// stopped, and it will be registered by a later recovery once it answers);
    /// it is simply not yet discoverable.
    ///
    /// Idempotent: adoption re-inserts by id and registration upserts by card
    /// name, so calling this twice changes nothing.
    pub async fn recover(&self) -> Result<Recovered<DeployedAgent>, ControlPlaneError> {
        let adopted: std::collections::HashSet<AgentId> = match self.runtime.recover().await? {
            Recovered::Ephemeral => return Ok(Recovered::Ephemeral),
            Recovered::Adopted(ids) => ids.into_iter().collect(),
        };

        let mut recovered = Vec::with_capacity(adopted.len());
        for status in self.runtime.list().await? {
            if !adopted.contains(&status.id) {
                continue;
            }
            if status.health == RuntimeHealth::Healthy {
                match self.cards.fetch(&status.endpoint).await {
                    Ok(card) => {
                        self.registry
                            .register(card, status.endpoint.clone())
                            .await?;
                    }
                    // Reachable a moment ago, not now. Adopt it anyway: managing
                    // it is what lets an operator stop it.
                    Err(e) => warn!("recovered '{}' but could not register it: {e}", status.id),
                }
            } else {
                warn!(
                    "recovered '{}' as {:?}; not registered for discovery until it answers",
                    status.id, status.health
                );
            }
            recovered.push(DeployedAgent {
                id: status.id.to_string(),
                endpoint: status.endpoint,
                health: status.health,
            });
        }
        Ok(Recovered::Adopted(recovered))
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

    /// An agent's captured output, oldest line first, limited to the last `tail`
    /// lines when given.
    ///
    /// Health says an agent is [`Unhealthy`](RuntimeHealth::Unhealthy); this is
    /// where the reason is. Whether it can be answered at all depends on the
    /// backend — see [`AgentRuntime::logs`].
    pub async fn logs(
        &self,
        id: &AgentId,
        tail: Option<usize>,
    ) -> Result<Vec<String>, ControlPlaneError> {
        Ok(self.runtime.logs(id, tail).await?)
    }

    /// List every deployed agent with its endpoint and health, ordered by id.
    ///
    /// Sorted because a runtime's own order is its map's, i.e. arbitrary and
    /// unstable between calls — which makes `a2a ps` shuffle between runs and
    /// makes the API's response undiffable for no reason.
    pub async fn list(&self) -> Result<Vec<DeployedAgent>, ControlPlaneError> {
        let mut agents: Vec<DeployedAgent> = self
            .runtime
            .list()
            .await?
            .into_iter()
            .map(|s| DeployedAgent {
                id: s.id.to_string(),
                endpoint: s.endpoint,
                health: s.health,
            })
            .collect();
        agents.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(agents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{InMemoryAgentRegistry, InMemoryCardSource};
    use crate::runtime::{InMemoryAgentRuntime, LocalProcessRuntime};
    use a2a_rs::domain::AgentCard;

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
        let cards = Arc::new(InMemoryCardSource::new());
        (
            ControlPlane::new(runtime, registry.clone(), cards),
            registry,
        )
    }

    /// A card an agent would serve at its endpoint.
    fn card(name: &str) -> AgentCard {
        AgentCard {
            name: name.to_string(),
            ..Default::default()
        }
    }

    /// Read a temp config back and push it through the real prepare → deploy
    /// pipeline, the way the HTTP adapter does.
    fn prepared(cp: &ControlPlane, config: &TempConfig) -> PreparedDeploy {
        let raw = std::fs::read_to_string(&config.path).unwrap();
        cp.prepare(&raw).expect("prepare")
    }

    #[tokio::test]
    async fn deploy_runs_and_registers_then_undeploy_tears_down() {
        let (cp, registry) = control_plane();
        let config = TempConfig::echo("Deploy Me", 8123);

        let deployed = cp
            .deploy(prepared(&cp, &config), config.path.clone())
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

        cp.deploy(prepared(&cp, &config), config.path.clone())
            .await
            .expect("deploy");

        let matches = registry.find_by_skill("echo-skill").await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].card.name, "Skilled Agent");
    }

    /// `prepare` is the policy gate, and it must reject *before* parsing: a
    /// config naming a secret this process holds must not be distinguishable
    /// from one naming a secret it doesn't.
    #[tokio::test]
    async fn prepare_rejects_env_refs_outside_the_allowlist() {
        let (cp, _registry) = control_plane();
        let raw = r#"
[agent]
name = "Leaky"
description = "${A2A_CP_TEST_SECRET}"

[handler]
type = "echo"

[server]
http_port = 8125
"#;
        let err = cp.prepare(raw).expect_err("deny-all must reject the ref");
        assert!(
            matches!(&err, ControlPlaneError::Runtime(RuntimeError::DisallowedEnv(v))
                if v.contains("A2A_CP_TEST_SECRET")),
            "got: {err}"
        );

        // Same config, same (unset) variable — permitted now, so it gets past the
        // policy gate and fails on expansion instead. Different error ⇒ the gate
        // is what rejected it above, not the missing value.
        let permissive = ControlPlane::new(
            Arc::new(InMemoryAgentRuntime::new()),
            Arc::new(InMemoryAgentRegistry::new()),
            Arc::new(InMemoryCardSource::new()),
        )
        .with_allowed_env(EnvAllowlist::new(["A2A_CP_TEST_SECRET"]));
        assert!(
            matches!(
                permissive.prepare(raw),
                Err(ControlPlaneError::Config(_)) | Ok(_)
            ),
            "an allow-listed var must clear the policy gate"
        );
    }

    /// The failure this exists to prevent: the process restarts, the runtime
    /// outlived it, but the registry did not — so a control plane that did not
    /// recover would report an empty fleet while the agents are still serving.
    #[tokio::test]
    async fn recover_readopts_running_agents_and_rebuilds_discovery() {
        let runtime: Arc<dyn AgentRuntime> = Arc::new(InMemoryAgentRuntime::new());
        let cards = Arc::new(InMemoryCardSource::new());
        let config = TempConfig::echo("Survivor", 8126);
        let endpoint = "http://127.0.0.1:8126";
        cards.insert(endpoint, card("Survivor")).await;

        let before = ControlPlane::new(
            runtime.clone(),
            Arc::new(InMemoryAgentRegistry::new()),
            cards.clone(),
        );
        before
            .deploy(prepared(&before, &config), config.path.clone())
            .await
            .expect("deploy");

        // The restart: same durable runtime, a brand-new (empty) registry.
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let after = ControlPlane::new(runtime, registry.clone(), cards);
        let id = AgentId::from_name("Survivor");
        assert!(
            after.list().await.unwrap().len() == 1,
            "the runtime half survives on its own"
        );
        assert!(
            registry.get(&id).await.unwrap().is_none(),
            "…but discovery does not, which is what recovery has to fix"
        );

        let recovered = after.recover().await.expect("recover");
        let Recovered::Adopted(agents) = recovered else {
            panic!("a durable runtime must report Adopted, not Ephemeral");
        };
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "survivor");
        assert_eq!(agents[0].endpoint, endpoint);
        assert_eq!(agents[0].health, RuntimeHealth::Healthy);

        assert!(
            registry.get(&id).await.unwrap().is_some(),
            "a recovered agent must be discoverable again"
        );

        // And it is genuinely managed again, not just listed.
        after.undeploy(&id).await.expect("undeploy after recovery");
        assert_eq!(after.status(&id).await.unwrap(), RuntimeHealth::Stopped);
    }

    /// Recovery runs at every startup, including the first one.
    #[tokio::test]
    async fn recover_is_idempotent_and_fine_with_nothing_to_adopt() {
        let (cp, registry) = control_plane();
        assert_eq!(cp.recover().await.unwrap(), Recovered::Adopted(vec![]));

        let config = TempConfig::echo("Twice", 8127);
        cp.deploy(prepared(&cp, &config), config.path.clone())
            .await
            .expect("deploy");

        // Two recoveries in a row leave exactly one of everything.
        cp.recover().await.expect("first recover");
        cp.recover().await.expect("second recover");
        assert_eq!(cp.list().await.unwrap().len(), 1);
        assert_eq!(registry.list().await.unwrap().len(), 1);
    }

    /// An adopted agent that will not hand over a card is still *managed* — the
    /// operator can stop it — it is only missing from discovery.
    #[tokio::test]
    async fn an_unreachable_agent_is_adopted_but_not_registered() {
        // Nothing inserted into the card source: every fetch fails.
        let cards = Arc::new(InMemoryCardSource::new());
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let cp = ControlPlane::new(
            Arc::new(InMemoryAgentRuntime::new()),
            registry.clone(),
            cards,
        );
        let config = TempConfig::echo("Mute", 8128);
        cp.deploy(prepared(&cp, &config), config.path.clone())
            .await
            .expect("deploy");

        let Recovered::Adopted(agents) = cp.recover().await.expect("recover") else {
            panic!("expected Adopted");
        };
        assert_eq!(agents.len(), 1, "adopted despite the failed card fetch");
        // `deploy` registered it from the config; recovery could not refresh it,
        // and must not have removed it either.
        assert!(
            registry
                .get(&AgentId::from("mute"))
                .await
                .unwrap()
                .is_some()
        );
        cp.undeploy(&AgentId::from("mute"))
            .await
            .expect("still manageable");
    }

    /// An ephemeral runtime must not be reported as an empty fleet: "nothing is
    /// running" and "I cannot tell what is running" are different answers.
    #[tokio::test]
    async fn an_ephemeral_runtime_reports_ephemeral_not_empty() {
        let cp = ControlPlane::new(
            Arc::new(LocalProcessRuntime::with_exe("a2a")),
            Arc::new(InMemoryAgentRegistry::new()),
            Arc::new(InMemoryCardSource::new()),
        );
        assert_eq!(cp.recover().await.unwrap(), Recovered::Ephemeral);
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
