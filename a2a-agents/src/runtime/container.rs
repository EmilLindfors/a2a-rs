//! [`ContainerRuntime`] — run each agent in a Docker/Podman container.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::warn;

use super::{
    AgentRuntime, AgentSpec, EnvAllowlist, Recovered, RuntimeError, RuntimeHealth, RuntimeStatus,
    tail_lines,
};
use crate::core::AgentConfig;
use crate::registry::AgentId;

/// Default base image (one image, config injected per agent). Override with
/// [`ContainerRuntime::with_image`].
const DEFAULT_IMAGE: &str = "a2a-agents:latest";

/// Where the agent's TOML is mounted inside the container.
const CONTAINER_CONFIG_PATH: &str = "/etc/agent.toml";

/// Label carrying the [`AgentId`]. `<engine> ps --filter label=a2a-agent` is
/// this adapter's durable store: it is what makes the engine, not this process's
/// memory, the source of truth about which agents exist.
const LABEL_AGENT: &str = "a2a-agent";

/// Label carrying the published host port, so recovery reconstructs the whole
/// in-memory map from the engine rather than half of it. Parsing it back out of
/// `{{.Ports}}` would mean parsing `0.0.0.0:8080->8080/tcp, :::8080->8080/tcp`;
/// stamping it is exact.
const LABEL_PORT: &str = "a2a-port";

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
/// **Secrets:** keep them out of the TOML as `${VAR}` refs. Each referenced
/// variable — provided it is on the [`EnvAllowlist`] — is injected into the
/// container as a value-less `-e VAR` pass-through, resolved from *this*
/// process's environment by the engine CLI. So the bind-mounted TOML and the
/// `docker create` argv never carry secret values, and the in-container
/// `a2a run` expands the refs at startup. The deploying process (e.g.
/// `a2a control-plane`) must therefore hold the secrets in its own env;
/// provisioning fails fast if a ref has no value and no `${VAR:-default}`.
///
/// The allowlist is **deny-by-default** ([`with_allowed_env`](Self::with_allowed_env)):
/// without it, any accepted config could name any variable this process holds
/// and have it expanded into the agent's card for anyone to read back.
///
/// **Platform:** the config is bind-mounted (`-v host:container`), so host config
/// paths must be expressible as a Docker volume source — works on Linux/macOS;
/// Windows host paths need conversion (out of scope here).
#[derive(Clone)]
pub struct ContainerRuntime {
    engine: String,
    image: String,
    /// Which host env vars a deployed config may reference. Deny-all by default.
    allowed_env: EnvAllowlist,
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
            allowed_env: EnvAllowlist::deny_all(),
            agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Override the base image (default [`DEFAULT_IMAGE`]).
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = image.into();
        self
    }

    /// Permit deployed configs to reference these host environment variables.
    /// Anything not listed is rejected at [`provision`](AgentRuntime::provision).
    pub fn with_allowed_env(mut self, allowed: EnvAllowlist) -> Self {
        self.allowed_env = allowed;
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
/// the config as a read-only mount, bind all interfaces, pass through the env
/// vars the config references, and run `a2a run --config /etc/agent.toml`.
///
/// `env_refs` are passed as value-less `-e VAR` flags: the engine CLI reads
/// each value from its own environment (this process's), so secret values never
/// appear in the argv, and a variable unset here is simply not set in the
/// container (letting an in-config `${VAR:-default}` apply).
fn create_args(
    image: &str,
    id: &AgentId,
    port: u16,
    config_path: &Path,
    env_refs: &[String],
) -> Vec<String> {
    let mut args = vec![
        "create".to_string(),
        "--name".to_string(),
        container_name(id),
        "-p".to_string(),
        format!("{port}:{port}"),
        "-e".to_string(),
        "HOST=0.0.0.0".to_string(),
    ];
    for var in env_refs {
        args.push("-e".to_string());
        args.push(var.clone());
    }
    args.extend([
        "-v".to_string(),
        format!("{}:{CONTAINER_CONFIG_PATH}:ro", config_path.display()),
        "--label".to_string(),
        format!("{LABEL_AGENT}={id}"),
        "--label".to_string(),
        format!("{LABEL_PORT}={port}"),
        image.to_string(),
        "run".to_string(),
        "--config".to_string(),
        CONTAINER_CONFIG_PATH.to_string(),
    ]);
    args
}

/// A `--format` template printing one label's value (the Go-template braces are
/// doubled because this is a `format!` string).
fn label_template(label: &str) -> String {
    format!("{{{{.Label \"{label}\"}}}}")
}

/// Read back what [`create_args`] stamped: one `<id>\t<port>` line per
/// container.
///
/// Lines missing either label (a container labelled by an older version, or by
/// something else entirely) are skipped with a warning rather than failing the
/// whole recovery — one unrecognizable container must not cost the operator the
/// rest of the fleet.
fn parse_labelled_containers(output: &str) -> Vec<(AgentId, u16)> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| match line.split_once('\t') {
            Some((id, port)) if !id.is_empty() => match port.trim().parse::<u16>() {
                Ok(port) => Some((AgentId::from(id.trim()), port)),
                Err(_) => {
                    warn!("skipping container '{id}': unreadable {LABEL_PORT} label '{port}'");
                    None
                }
            },
            _ => {
                warn!("skipping container: unexpected `ps` line '{line}'");
                None
            }
        })
        .collect()
}

/// Build the `docker logs` argv for an agent's container.
///
/// `--timestamps` because the reason anyone reads these is to line an agent's
/// output up against something else that happened.
fn logs_args(id: &AgentId, tail: Option<usize>) -> Vec<String> {
    let mut args = vec!["logs".to_string(), "--timestamps".to_string()];
    if let Some(n) = tail {
        args.push("--tail".to_string());
        args.push(n.to_string());
    }
    args.push(container_name(id));
    args
}

/// Run the engine with `args`, returning **both** output streams concatenated.
///
/// Distinct from [`run_engine`] because `docker logs` replays the container's
/// stdout on stdout and its stderr on stderr — and an agent's `tracing` output
/// goes to stderr, so reading stdout alone would show an operator an empty log
/// for an agent that is printing the very error they are looking for.
///
/// The two streams are captured separately by the OS pipe, so interleaving is
/// lost; stdout is emitted first. Ordering *within* each stream is preserved,
/// which is what the timestamps make usable.
async fn run_engine_combined(engine: &str, args: &[String]) -> Result<String, RuntimeError> {
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
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(combined)
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
        let content = tokio::fs::read_to_string(&spec.config_path)
            .await
            .map_err(|e| RuntimeError::Config(e.to_string()))?;
        // Vet the *raw* config against the allowlist before parsing: parsing
        // expands `${VAR}` against this process's env and reports set vs. unset
        // differently, so a rejected config must never reach it — otherwise it
        // could probe which secrets the control plane holds. The vars that
        // survive are what gets passed through into the container (where the
        // in-container `a2a run` re-expands them), keeping secrets out of the
        // bind-mounted TOML.
        let env_refs = self.allowed_env.check(&content)?;
        // The published port is the agent's configured HTTP port. Parsing here
        // also expands `${VAR}` refs against *this* process's env, so a missing
        // secret fails provisioning instead of crash-looping the container.
        let config =
            AgentConfig::from_toml(&content).map_err(|e| RuntimeError::Config(e.to_string()))?;
        let port = config.server.http_port;

        // Clear a stale container of the same name so re-provision is idempotent.
        let _ = run_engine(
            &self.engine,
            &["rm".to_string(), "-f".to_string(), container_name(&id)],
        )
        .await;

        run_engine(
            &self.engine,
            &create_args(&self.image, &id, port, &spec.config_path, &env_refs),
        )
        .await?;
        self.agents.lock().await.insert(id.clone(), port);
        Ok(id)
    }

    /// Rebuild the id → port map from the engine, which kept running while this
    /// process was not.
    ///
    /// `ps -a` on purpose: a stopped-but-existing container is still an agent
    /// this runtime manages, and adopting it is what makes `stop`/`undeploy`
    /// work on it after a restart instead of returning `NotFound`.
    async fn recover(&self) -> Result<Recovered<AgentId>, RuntimeError> {
        let output = run_engine(
            &self.engine,
            &[
                "ps".to_string(),
                "-a".to_string(),
                "--filter".to_string(),
                format!("label={LABEL_AGENT}"),
                "--format".to_string(),
                format!(
                    "{}\t{}",
                    label_template(LABEL_AGENT),
                    label_template(LABEL_PORT)
                ),
            ],
        )
        .await?;

        let found = parse_labelled_containers(&output);
        let mut agents = self.agents.lock().await;
        let mut adopted = Vec::with_capacity(found.len());
        for (id, port) in found {
            agents.insert(id.clone(), port);
            adopted.push(id);
        }
        // Deterministic for reporting; the engine's order is its own business.
        adopted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(Recovered::Adopted(adopted))
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
            let port = *guard
                .get(id)
                .ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
            (container_name(id), port)
        };
        match self.inspect_status(&name).await.as_deref() {
            Some("created") => Ok(RuntimeHealth::Provisioned),
            Some("running") => {
                match a2a_rs::fetch_agent_card(&format!("http://127.0.0.1:{port}")).await {
                    Ok(_) => Ok(RuntimeHealth::Healthy),
                    Err(_) => Ok(RuntimeHealth::Unhealthy),
                }
            }
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

    /// Replay the container's output via the engine, which retains it for the
    /// container's whole life — including after it exits, which is exactly when
    /// the log matters most.
    async fn logs(&self, id: &AgentId, tail: Option<usize>) -> Result<Vec<String>, RuntimeError> {
        if !self.agents.lock().await.contains_key(id) {
            return Err(RuntimeError::NotFound(id.clone()));
        }
        let output = run_engine_combined(&self.engine, &logs_args(id, tail)).await?;
        // The engine already applied `--tail`; trimming again is only to drop a
        // trailing blank line.
        Ok(tail_lines(&output, None))
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
        let args = create_args(
            "a2a-agents:latest",
            &id,
            8080,
            Path::new("/cfg/echo.toml"),
            &[],
        );
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
                "--label",
                "a2a-port=8080",
                "a2a-agents:latest",
                "run",
                "--config",
                "/etc/agent.toml",
            ]
        );
    }

    /// Recovery reads back exactly what provisioning wrote, so the two must be
    /// checked against each other — a label renamed on one side only would lose
    /// the whole fleet on the next restart.
    #[test]
    fn recovery_reads_back_what_create_stamped() {
        let id = AgentId::from_name("Weather Agent");
        let args = create_args(
            "a2a-agents:latest",
            &id,
            9100,
            Path::new("/cfg/w.toml"),
            &[],
        );

        // Pull the labels out of the argv the way the engine would store them.
        let labels: Vec<&String> = args
            .iter()
            .enumerate()
            .filter(|(i, _)| i > &0 && args[i - 1] == "--label")
            .map(|(_, value)| value)
            .collect();
        let agent_label = labels
            .iter()
            .find_map(|l| l.strip_prefix(&format!("{LABEL_AGENT}=")))
            .expect("agent label");
        let port_label = labels
            .iter()
            .find_map(|l| l.strip_prefix(&format!("{LABEL_PORT}=")))
            .expect("port label");

        assert_eq!(
            parse_labelled_containers(&format!("{agent_label}\t{port_label}")),
            [(id, 9100)]
        );
    }

    #[test]
    fn parsing_skips_unusable_lines_but_keeps_the_rest() {
        let output = "\
weather-agent\t8080
billing-agent\tnot-a-port
\t9000
orphan-container
billing-agent\t8081
";
        // The two well-formed lines survive; a bad port, a missing id, and a
        // container labelled by something else are dropped rather than aborting.
        assert_eq!(
            parse_labelled_containers(output),
            [
                (AgentId::from("weather-agent"), 8080),
                (AgentId::from("billing-agent"), 8081),
            ]
        );
    }

    #[test]
    fn label_template_is_a_go_template_for_one_label() {
        assert_eq!(label_template("a2a-agent"), r#"{{.Label "a2a-agent"}}"#);
    }

    #[test]
    fn logs_args_target_the_agents_container() {
        let id = AgentId::from_name("Weather Agent");
        assert_eq!(
            logs_args(&id, None),
            ["logs", "--timestamps", "a2a-agent-weather-agent"]
        );
        assert_eq!(
            logs_args(&id, Some(50)),
            [
                "logs",
                "--timestamps",
                "--tail",
                "50",
                "a2a-agent-weather-agent"
            ]
        );
    }

    #[test]
    fn create_args_pass_env_refs_through_by_name_only() {
        let id = AgentId::from_name("LLM Agent");
        let args = create_args(
            "a2a-agents:latest",
            &id,
            8080,
            Path::new("/cfg/llm.toml"),
            &["API_TOKEN".to_string(), "OPENROUTER_API_KEY".to_string()],
        );
        // Value-less `-e VAR` flags, right after the adapter-owned HOST, so
        // secret values never appear in the argv.
        let host_pos = args.iter().position(|a| a == "HOST=0.0.0.0").unwrap();
        assert_eq!(
            &args[host_pos + 1..host_pos + 5],
            ["-e", "API_TOKEN", "-e", "OPENROUTER_API_KEY"]
        );
        assert!(!args.iter().any(|a| a.contains('=') && a.contains("TOKEN")));
    }
}
