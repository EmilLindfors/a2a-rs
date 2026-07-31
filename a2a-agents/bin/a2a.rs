//! Generic A2A Agent runner
//!
//! This binary can run multiple A2A agents concurrently, configured via TOML files.
//! It selects a built-in handler via the typed `[handler]` block (falling back to
//! the legacy `agent.implementation` string).
//!
//! Subcommands:
//!
//! * `new` — scaffold a starter config from a template.
//! * `run` — run one or more agents from TOML config files.
//! * `up` — run every agent a fleet file names.
//! * `validate` — load and validate config files without starting servers.
//! * `doctor` — check this machine against what the configs need.
//! * `print-schema` — print the JSON Schema for `AgentConfig` to stdout.
//! * `control-plane` — serve the deploy/list/status/logs/undeploy HTTP API.
//! * `deploy` / `ps` / `logs` / `stop` — drive a running control plane.

#[cfg(feature = "reimbursement-agent")]
use a2a_agents::agents::reimbursement::ReimbursementHandler;
#[cfg(feature = "reimbursement-agent")]
use a2a_agents::core::builder::AutoStorage;
use a2a_agents::core::config::LlmConfig;
use a2a_agents::core::{
    AgentBuilder, AgentConfig, AgentTemplate, FleetConfig, Requirement, fleet_conflicts,
    fleet_header, member_block, member_path, requirements,
};
use a2a_agents::core::{HandlerType, LlmHandlerConfig};
use a2a_agents::utils::slugify;
use a2a_agents::{
    AgentId, AgentRegistry, AgentRuntime, ContainerHardening, ContainerRuntime, ControlPlane,
    ControlPlaneAuth, ControlPlaneClient, ControlPlaneClientError, EnvAllowlist, HttpCardSource,
    InMemoryAgentRegistry, ListFilter, LocalProcessRuntime, Recovered, control_plane_router,
};
use a2a_agents_common::llm::{LlmProvider, LlmSettings, provider_from_env, provider_from_settings};

#[cfg(feature = "mcp-server")]
use a2a_agents::core::config::RemoteAgentTarget;
#[cfg(feature = "mcp-server")]
use a2a_agents::handlers::tools::ToolSource;
#[cfg(feature = "mcp-server")]
use a2a_agents::{A2aAgentToolSource, LlmHandler, McpToolSource, UnusedInner};
#[cfg(feature = "mcp-server")]
use a2a_rs::domain::AgentCard;
use a2a_rs::{
    InMemoryStreamingHandler,
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[clap(
    name = "a2a",
    version,
    about = "Runs A2A agents from declarative configs"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run one or more A2A agents from declarative TOML configs.
    Run {
        #[clap(short, long, required = true)]
        config: Vec<String>,
    },
    /// Run every agent named by a fleet file.
    ///
    /// A fleet file lists agent configs (paths relative to itself), so a
    /// multi-agent setup is one reviewable, version-controlled artifact instead
    /// of a `--config` per agent on every invocation. The whole fleet is checked
    /// before anything binds — including the invariants that only exist between
    /// agents, like two of them claiming the same port or registry id.
    Up {
        /// Fleet file to run.
        #[clap(short, long, default_value = "fleet.toml")]
        file: String,
        /// Treat an unset `${VAR}` reference in any member as an error.
        #[clap(long)]
        strict_env: bool,
    },
    /// Load and validate config files without starting servers.
    ///
    /// Unset `${VAR}` references are reported but do not fail the check, so a
    /// config's shape is verifiable without the secrets it will run with. Pass
    /// `--strict-env` to require every referenced variable to resolve.
    Validate {
        #[clap(short, long, required_unless_present = "fleet")]
        config: Vec<String>,
        /// Validate every agent in a fleet file, plus the fleet's cross-agent
        /// invariants. Combinable with `--config`.
        #[clap(short, long)]
        fleet: Option<String>,
        /// Treat an unset `${VAR}` reference as an error (pre-deploy check).
        #[clap(long)]
        strict_env: bool,
    },
    /// Scaffold a new agent config from a template.
    ///
    /// Writes a commented, immediately-runnable TOML file and prints the next
    /// commands to run. Refuses to clobber an existing file without `--force`.
    New {
        /// Agent name, as it appears on the agent card. Also the default
        /// filename (slugified).
        name: String,
        /// Which starter config to generate.
        #[clap(short, long, default_value = "echo")]
        template: String,
        /// Where to write it. Defaults to `<slug-of-name>.toml` in the cwd.
        #[clap(short, long)]
        output: Option<String>,
        /// HTTP port to bind. Defaults per template (8080, or 8090 for
        /// `orchestrator` so it can run alongside its first peer).
        #[clap(short, long)]
        port: Option<u16>,
        /// Also add the new agent to this fleet file, creating it if absent.
        ///
        /// Repeat the flag across several `a2a new` runs to build a fleet up one
        /// agent at a time, then `a2a up -f <file>` to run them together.
        #[clap(long)]
        fleet: Option<String>,
        /// Overwrite the output file if it already exists.
        #[clap(long)]
        force: bool,
    },
    /// Pre-flight check: can this machine actually run these agents?
    ///
    /// `validate` answers "is the config well-formed"; this answers "is the
    /// world ready for it" — port free, MCP command installed, model key set,
    /// container engine present. Each of those otherwise surfaces at runtime as
    /// a different confusing symptom.
    Doctor {
        /// Agent config to check. Repeatable. With neither `--config` nor
        /// `--fleet`, only the environment is checked.
        #[clap(short, long)]
        config: Vec<String>,
        /// Check every agent a fleet file names.
        #[clap(short, long)]
        fleet: Option<String>,
    },
    /// Print the JSON Schema for `AgentConfig` to stdout.
    PrintSchema {
        /// Print the schema for a fleet file (`FleetConfig`) instead.
        #[clap(long)]
        fleet: bool,
    },
    /// Serve the control-plane HTTP API: deploy/list/status/undeploy agents,
    /// each run via the chosen `--runtime` (local processes or containers).
    ///
    /// Requires a bearer token (`--token` / `A2A_CONTROL_PLANE_TOKEN`) unless
    /// `--no-auth` is given: deploying is remote code execution.
    ///
    /// Secrets: keep them out of deployed TOMLs as `${VAR}` refs, set the
    /// variables in *this* process's environment, and permit each one with
    /// `--allow-env VAR`. Unlisted variables are rejected at deploy. The
    /// container runtime passes allowed vars through by name (values never touch
    /// disk or argv); local children inherit the environment wholesale, which is
    /// why `--runtime local` is for dev loops only.
    ControlPlane {
        /// Address to bind the control-plane HTTP API to.
        #[clap(long, default_value = DEFAULT_CONTROL_PLANE_BIND)]
        bind: String,
        /// Directory where deployed agent configs are written (and read by the
        /// runtime — `a2a run` children, or container mounts).
        #[clap(long, default_value = "./agents")]
        config_dir: String,
        /// Which runtime backend runs the agents.
        #[clap(long, value_enum, default_value = "local")]
        runtime: RuntimeKind,
        /// Container engine binary (only used with `--runtime container`).
        #[clap(long, default_value = "docker")]
        engine: String,
        /// Base image (only used with `--runtime container`).
        #[clap(long, default_value = "a2a-agents:latest")]
        image: String,
        /// Bearer token callers must present. Prefer the
        /// `A2A_CONTROL_PLANE_TOKEN` env var — an argv token is visible to `ps`.
        #[clap(long)]
        token: Option<String>,
        /// Run the API with no authentication. Explicit opt-out for trusted dev
        /// loops; without it, a missing token is a startup error.
        #[clap(long, conflicts_with = "token")]
        no_auth: bool,
        /// Permit deployed configs to reference this env var via `${VAR}`.
        /// Repeatable. Deny-by-default: unlisted vars are rejected at deploy.
        #[clap(long = "allow-env", value_name = "VAR")]
        allow_env: Vec<String>,
        /// Where each agent's captured output is written, one file per agent
        /// (`--runtime local` only; the container engine keeps its own).
        /// Defaults to `<config-dir>/logs`. This is what `a2a logs` reads, and
        /// it also stops every agent's output interleaving into this terminal.
        #[clap(long)]
        log_dir: Option<String>,
        /// Memory ceiling per agent, in the engine's notation (`512m`, `2g`).
        /// Unset means no limit (`--runtime container` only).
        #[clap(long)]
        memory: Option<String>,
        /// CPU allowance per agent, in the engine's notation (`0.5`, `2`).
        /// Unset means no limit (`--runtime container` only).
        #[clap(long)]
        cpus: Option<String>,
        /// Create containers with no hardening — no dropped capabilities, no
        /// read-only root, no process cap. For an agent that genuinely needs
        /// what the defaults remove; not something to reach for casually
        /// (`--runtime container` only).
        #[clap(long)]
        no_hardening: bool,
    },
    /// Deploy agents to a running control plane.
    ///
    /// The config is sent **as written** — `${VAR}` references are resolved by
    /// the control plane against its own environment and `--allow-env`
    /// allowlist, so the machine deploying does not need the secrets the agent
    /// runs with.
    Deploy {
        /// Agent config to deploy. Repeatable.
        #[clap(short, long, required_unless_present = "fleet")]
        config: Vec<String>,
        /// Deploy every agent a fleet file names. Combinable with `--config`.
        #[clap(short, long)]
        fleet: Option<String>,
        #[command(flatten)]
        target: ControlPlaneTarget,
    },
    /// List the agents a control plane is running, with their health.
    Ps {
        /// Also show agents that have been stopped. Their entries are kept so
        /// `a2a logs` can still explain why they went — they are simply not
        /// part of what is running.
        #[clap(short, long)]
        all: bool,
        #[command(flatten)]
        target: ControlPlaneTarget,
    },
    /// Print a deployed agent's captured output.
    ///
    /// This is the question health cannot answer: `unhealthy` says the card
    /// probe is failing, not why.
    Logs {
        /// Agent id, as shown by `a2a ps`.
        id: String,
        /// Print only the last N lines.
        #[clap(short, long)]
        tail: Option<usize>,
        #[command(flatten)]
        target: ControlPlaneTarget,
    },
    /// Stop deployed agents and remove them from discovery.
    Stop {
        /// Agent ids, as shown by `a2a ps`.
        #[clap(required = true)]
        id: Vec<String>,
        #[command(flatten)]
        target: ControlPlaneTarget,
    },
}

/// Which control plane a client subcommand talks to.
///
/// Flattened into every command that drives one, so `--url`/`--token` mean the
/// same thing everywhere and are resolved in exactly one place.
#[derive(Args, Debug)]
struct ControlPlaneTarget {
    /// Control-plane API base URL. Defaults to `$A2A_CONTROL_PLANE_URL`, then
    /// to where `a2a control-plane` binds by default.
    #[clap(long)]
    url: Option<String>,
    /// Bearer token the control plane requires. Defaults to
    /// `$A2A_CONTROL_PLANE_TOKEN` — prefer that, since an argv token is visible
    /// to `ps`.
    #[clap(long)]
    token: Option<String>,
}

impl ControlPlaneTarget {
    /// Resolve flags, then environment, then the default bind address.
    fn client(self) -> ControlPlaneClient {
        let url = self
            .url
            .or_else(|| std::env::var(URL_ENV).ok())
            .unwrap_or_else(|| DEFAULT_CONTROL_PLANE_URL.to_string());
        ControlPlaneClient::new(url)
            .with_optional_token(self.token.or_else(|| std::env::var(TOKEN_ENV).ok()))
    }
}

/// Which [`AgentRuntime`] backend the control plane runs agents on.
#[derive(Clone, Debug, ValueEnum)]
enum RuntimeKind {
    /// Supervise agents as child `a2a run` processes ([`LocalProcessRuntime`]).
    Local,
    /// Run each agent in a container ([`ContainerRuntime`]).
    Container,
}

#[derive(Clone)]
struct EchoHandler;

#[async_trait]
impl AsyncMessageHandler for EchoHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let text = message
            .parts
            .iter()
            .find_map(|p| p.get_text())
            .unwrap_or("<empty>")
            .to_string();
        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("echo: {text}"))])
            .message_id(Uuid::new_v4().to_string())
            .build();
        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id(message.context_id.clone())
            .status(TaskStatus::new(
                TaskState::Completed,
                Some(response.clone()),
            ))
            .history(vec![message.clone(), response])
            .build())
    }
}

fn resolve_llm(llm_config: &Option<LlmConfig>) -> Option<Arc<dyn LlmProvider>> {
    match llm_config {
        Some(cfg) => {
            info!(
                "Loading LLM configuration from TOML (provider: {})",
                cfg.provider
            );
            let settings = LlmSettings {
                provider: cfg.provider.clone(),
                api_key: cfg.api_key.clone(),
                model: cfg.model.clone(),
                base_url: cfg.base_url.clone(),
                http_referer: cfg.http_referer.clone(),
                x_title: cfg.x_title.clone(),
            };
            match provider_from_settings(&settings) {
                Ok(p) => Some(p),
                Err(e) => {
                    error!("invalid LLM configuration: {e}; falling back to env");
                    provider_from_env()
                }
            }
        }
        None => provider_from_env(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    // `from_default_env()` alone enables *nothing* when RUST_LOG is unset, so
    // `a2a run` used to start a server and print absolutely nothing — no
    // confirmation, no URL, no errors. Default to our own crates at info and
    // everything else at warn; RUST_LOG still overrides entirely.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("warn,a2a=info,a2a_agents=info,a2a_rs=info")
            }),
        )
        // Logs on stderr, reports on stdout. `fmt()` defaults to stdout, which
        // made `a2a validate > report.txt` capture whatever happened to be
        // logged alongside the report the command exists to produce.
        .with_writer(std::io::stderr)
        // Colour only when someone is looking at a terminal. A supervised agent
        // writes into a captured log file (`a2a logs`), and escape codes baked
        // into a file are noise in everything that later reads it.
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::PrintSchema { fleet } => {
            print_schema(fleet);
        }
        Command::Validate {
            config,
            fleet,
            strict_env,
        } => {
            let mut ok = config.is_empty() || validate_configs(&config, strict_env);
            if let Some(fleet_path) = &fleet {
                ok &= check_fleet(fleet_path, strict_env)?.is_some();
            }
            if !ok {
                std::process::exit(1);
            }
        }
        Command::New {
            name,
            template,
            output,
            port,
            fleet,
            force,
        } => {
            scaffold_agent(
                &name,
                &template,
                output.as_deref(),
                port,
                fleet.as_deref(),
                force,
            )?;
        }
        Command::Run { config } => {
            run_agents(config).await?;
        }
        Command::Doctor { config, fleet } => {
            if !run_doctor(&config, fleet.as_deref())? {
                std::process::exit(1);
            }
        }
        Command::Up { file, strict_env } => {
            // Check the whole fleet first: a port or id clash is a silent-wrong
            // failure once the agents are running, and finding it after the fact
            // costs more than the second of validation it takes to prevent.
            let Some(paths) = check_fleet(&file, strict_env)? else {
                std::process::exit(1);
            };
            run_agents(paths).await?;
        }
        Command::ControlPlane {
            bind,
            config_dir,
            runtime,
            engine,
            image,
            token,
            no_auth,
            allow_env,
            log_dir,
            memory,
            cpus,
            no_hardening,
        } => {
            run_control_plane(ControlPlaneArgs {
                bind,
                config_dir,
                runtime_kind: runtime,
                engine,
                image,
                token,
                no_auth,
                allow_env,
                log_dir,
                memory,
                cpus,
                no_hardening,
            })
            .await?;
        }
        Command::Deploy {
            config,
            fleet,
            target,
        } => {
            if !deploy_agents(&config, fleet.as_deref(), target.client()).await? {
                std::process::exit(1);
            }
        }
        Command::Ps { all, target } => {
            let filter = if all {
                ListFilter::All
            } else {
                ListFilter::Live
            };
            list_agents(target.client(), filter).await?;
        }
        Command::Logs { id, tail, target } => {
            print_logs(&id, tail, target.client()).await?;
        }
        Command::Stop { id, target } => {
            if !stop_agents(&id, target.client()).await? {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

/// Add the context a CLI user needs when the control plane simply is not there.
///
/// Everything else the client reports is already actionable — a rejected token,
/// an unknown agent, the control plane's own diagnosis of a bad config — but
/// "connection refused" reads as a bug until you are told what was supposed to
/// be listening.
fn explain(error: ControlPlaneClientError) -> anyhow::Error {
    match &error {
        ControlPlaneClientError::Unreachable { .. } => anyhow::anyhow!(
            "{error}\n\nStart one with `a2a control-plane`, or point at another \
             with --url / {URL_ENV}."
        ),
        _ => anyhow::Error::new(error),
    }
}

/// Send configs to a control plane, reporting each on **stdout**, and answer
/// whether every one of them deployed.
///
/// Checked before anything is sent: each config's *shape* (leniently — the
/// `${VAR}`s are the control plane's to resolve, and this machine may hold none
/// of them), and the invariants that only exist *between* agents. A port or
/// registry-id clash is silent-wrong once deployed, and finding it halfway
/// through a fleet leaves the operator to unpick a partial rollout.
///
/// Past those gates each agent is deployed independently and failures are
/// reported rather than aborting: the remaining agents are no more likely to
/// fail than the first, and stopping early would just make the partial state
/// less predictable.
async fn deploy_agents(
    config_paths: &[String],
    fleet: Option<&str>,
    client: ControlPlaneClient,
) -> anyhow::Result<bool> {
    let mut paths = config_paths.to_vec();
    if let Some(fleet_path) = fleet {
        paths.extend(fleet_member_paths(fleet_path)?.1);
    }

    // Read raw and keep it raw: what goes on the wire is the file as written.
    let mut agents: Vec<(String, String, AgentConfig)> = Vec::with_capacity(paths.len());
    for path in &paths {
        let raw = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
        let (config, _unset) = AgentConfig::check_toml(&raw)
            .map_err(|e| anyhow::anyhow!("{path} is not a valid agent config: {e}"))?;
        agents.push((path.clone(), raw, config));
    }

    let conflicts = fleet_conflicts(
        agents
            .iter()
            .map(|(path, _, config)| (path.as_str(), config)),
    );
    if !conflicts.is_empty() {
        for conflict in &conflicts {
            println!("conflict {conflict}");
        }
        anyhow::bail!("nothing was deployed: {} conflict(s)", conflicts.len());
    }

    println!(
        "deploying {} agent(s) to {}",
        agents.len(),
        client.base_url()
    );
    let mut all_ok = true;
    for (path, raw, _) in &agents {
        match client.deploy(raw).await {
            Ok(deployed) => println!(
                "ok      {:<24}{:<14}{}",
                deployed.id, deployed.health, deployed.endpoint
            ),
            Err(e) => {
                all_ok = false;
                println!("failed  {path}");
                for line in explain(e).to_string().lines() {
                    println!("        {line}");
                }
            }
        }
    }
    Ok(all_ok)
}

/// Print the deployed fleet as a table on **stdout**.
async fn list_agents(client: ControlPlaneClient, filter: ListFilter) -> anyhow::Result<()> {
    let agents = client.list(filter).await.map_err(explain)?;
    if agents.is_empty() {
        println!("no agents running at {}", client.base_url());
        if filter == ListFilter::Live {
            println!("stopped ones are hidden; see them with `a2a ps --all`");
        }
        println!("deploy one with `a2a deploy --config <file>`");
        return Ok(());
    }
    println!("{:<24}{:<14}ENDPOINT", "ID", "HEALTH");
    for agent in &agents {
        println!("{:<24}{:<14}{}", agent.id, agent.health, agent.endpoint);
    }
    Ok(())
}

/// Print one agent's captured output, verbatim.
///
/// No prefixes or timestamps of our own: these lines are the agent's, and
/// anything added here would fight the formatting it already chose.
async fn print_logs(
    id: &str,
    tail: Option<usize>,
    client: ControlPlaneClient,
) -> anyhow::Result<()> {
    let logs = client
        .logs(&AgentId::from(id), tail)
        .await
        .map_err(explain)?;
    if logs.lines.is_empty() {
        // The control plane distinguishes "cannot serve logs" (an error, with a
        // reason) from this, so an empty answer really does mean silence.
        println!("'{}' has not logged anything yet", logs.id);
        return Ok(());
    }
    for line in &logs.lines {
        println!("{line}");
    }
    Ok(())
}

/// Stop each named agent, reporting per id, and answer whether all of them
/// stopped.
async fn stop_agents(ids: &[String], client: ControlPlaneClient) -> anyhow::Result<bool> {
    let mut all_ok = true;
    for id in ids {
        let id = AgentId::from(id.as_str());
        match client.undeploy(&id).await {
            Ok(()) => println!("stopped {id}"),
            Err(e) => {
                all_ok = false;
                println!("failed  {id}");
                for line in explain(e).to_string().lines() {
                    println!("        {line}");
                }
            }
        }
    }
    Ok(all_ok)
}

/// Announce what is starting and where to reach it, on **stdout**.
///
/// Printed before the servers bind: `start_http` never returns, so anything
/// emitted after would be unreachable. That means these URLs are an intent, not
/// a confirmation — a bind failure is reported separately by the agent task.
///
/// Deliberately not `tracing`: this is the one thing a person needs off the
/// screen (the endpoint, and a command to poke it), not a timestamped event.
fn print_run_banner(config_paths: &[String]) {
    // Re-read leniently: an unreadable config is reported by the agent task, and
    // a banner is not worth failing a run over.
    let agents: Vec<(String, String)> = config_paths
        .iter()
        .filter_map(|path| {
            let (config, _) = AgentConfig::check_file(path).ok()?;
            Some((config.agent.name.clone(), config.agent_url()))
        })
        .collect();

    if agents.is_empty() {
        return;
    }

    println!();
    for (name, url) in &agents {
        println!("  {name}");
        println!("    {url}");
        println!("    card: {url}/.well-known/agent-card.json");
    }
    println!();
    if let Some((_, url)) = agents.first() {
        println!("  try:  a2acli send --url {url} 'hello'");
        println!("        curl {url}/.well-known/agent-card.json");
    }
    println!();
    println!("  ctrl-c to stop. RUST_LOG=debug for more detail.");
    println!();
}

/// Write a starter config and tell the user what to do with it.
///
/// The generated file is validated before being written — a scaffold that
/// produces something `a2a validate` rejects would be worse than no scaffold at
/// all, and it means a template that drifts from the schema fails loudly here
/// rather than in the user's editor.
fn scaffold_agent(
    name: &str,
    template: &str,
    output: Option<&str>,
    port: Option<u16>,
    fleet: Option<&str>,
    force: bool,
) -> anyhow::Result<()> {
    let template: AgentTemplate = template.parse().map_err(anyhow::Error::msg)?;
    let port = port.unwrap_or_else(|| template.default_port());
    let rendered = template.render(name, port);

    // Parse leniently: the point is to catch a broken *template*, and the check
    // should not depend on the user's environment.
    AgentConfig::check_toml(&rendered)
        .map_err(|e| anyhow::anyhow!("template '{template}' produced an invalid config: {e}"))?;

    let path = PathBuf::from(match output {
        Some(path) => path.to_string(),
        None => format!("{}.toml", slugify(name, '-')),
    });
    if path.exists() && !force {
        anyhow::bail!(
            "{} already exists; pass --force to overwrite or --output to write elsewhere",
            path.display()
        );
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &rendered)?;

    let display = path.display();
    println!("created {display}  ({template} template, port {port})");

    // A fleet turns the follow-up commands into the fleet's own: `a2a up` is
    // what runs the set, and running only the agent just scaffolded would leave
    // its peers down.
    match fleet {
        Some(fleet) => {
            let added = add_to_fleet(fleet, &path)?;
            println!("{} {fleet}", if added { "added to" } else { "already in" });
            println!();
            println!("next:");
            println!("    a2a validate --fleet {fleet}");
            println!("    a2a up -f {fleet}");
        }
        None => {
            println!();
            println!("next:");
            println!("    a2a validate --config {display}");
            println!("    a2a run --config {display}");
        }
    }

    if template.needs_llm_key() {
        println!();
        println!(
            "this template calls a model — set one of OPENAI_API_KEY, GEMINI_API_KEY, \
             or OPENROUTER_API_KEY."
        );
        println!("without a key it still runs, answering with a deterministic fallback.");
    }
    Ok(())
}

/// Add `config_path` to the fleet file at `fleet_path`, creating the file if it
/// is not there. Answers whether it was a new member.
///
/// Appends text rather than re-serializing a parsed [`FleetConfig`]: a fleet file
/// is hand-written and carries comments and ordering that a round-trip through
/// `toml::to_string` would silently discard.
fn add_to_fleet(fleet_path: &str, config_path: &Path) -> anyhow::Result<bool> {
    let fleet_path = Path::new(fleet_path);
    let cwd = std::env::current_dir()?;
    let member = member_path(fleet_path, config_path, &cwd);

    // An existing fleet is parsed first, so a malformed file is reported as such
    // instead of being appended to and left more broken than it was found.
    let existing = match fleet_path.exists() {
        true => {
            let fleet = FleetConfig::from_file(fleet_path)
                .map_err(|e| anyhow::anyhow!("{}: {e}", fleet_path.display()))?;
            if fleet.agents.iter().any(|m| m.config == member) {
                return Ok(false);
            }
            std::fs::read_to_string(fleet_path)?
        }
        false => {
            if let Some(parent) = fleet_path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let name = fleet_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Fleet".to_string());
            fleet_header(&name)
        }
    };

    std::fs::write(fleet_path, format!("{existing}{}", member_block(&member)))?;
    Ok(true)
}

/// Check each config and report on **stdout**, returning whether all passed.
///
/// Results go to stdout rather than through `tracing`: this is a CLI command
/// whose output a human reads and a script greps, so it should not carry
/// timestamps, levels, and module paths. Long-running commands (`run`,
/// `control-plane`) still log — they emit events over time, which is what
/// `tracing` is for.
fn validate_configs(paths: &[String], strict_env: bool) -> bool {
    paths.iter().fold(true, |all_ok, path| {
        all_ok & check_config(path, strict_env).0
    })
}

/// Check one config, print its report block, and hand back the parsed config.
///
/// The config comes back (when it parsed at all) so callers that need to reason
/// *across* files — the fleet check below — do not re-read and re-parse what was
/// just reported on. `false` with a `Some` means the shape is fine but the check
/// failed anyway, which today means `--strict-env` found an unset reference.
fn check_config(path: &str, strict_env: bool) -> (bool, Option<AgentConfig>) {
    match AgentConfig::check_file(path) {
        Ok((config, unset)) => {
            let failed_strict = strict_env && !unset.is_empty();

            println!(
                "{} {path}",
                if failed_strict { "invalid" } else { "ok     " }
            );
            println!(
                "        agent {:?}, handler {}, port {}, {} skill(s)",
                config.agent.name,
                config.handler_type(),
                config.server.http_port,
                config.skills.len()
            );
            if !unset.is_empty() {
                let how = if strict_env {
                    "unset"
                } else {
                    "unset, not checked"
                };
                println!("        env ({how}): {}", unset.join(", "));
            }
            // Valid, and invisible: skill-based peer resolution has nothing to
            // match on, so an orchestrator never finds this agent and reports
            // only that no specialist fit. A warning, not a failure — an agent
            // reached by explicit `agent_id` is legitimate.
            if config.skills.is_empty() {
                println!(
                    "        warning: no [[skills]] — peers resolving by skill cannot \
                     discover this agent"
                );
            }
            (!failed_strict, Some(config))
        }
        Err(e) => {
            println!("invalid {path}");
            // Config errors are multi-line (TOML spans); indent the whole
            // block so it reads as belonging to the file above it.
            for line in e.to_string().lines() {
                println!("        {line}");
            }
            (false, None)
        }
    }
}

/// Check every agent in a fleet file plus the invariants that hold *between*
/// them, returning the member config paths when the fleet is sound.
///
/// `Ok(None)` means problems were found and already reported in full, so the
/// caller only has to choose the exit code. An `Err` is reserved for the fleet
/// file itself being unreadable, malformed, or pointing at a config that is not
/// there — that is one line of context, not a report.
fn check_fleet(fleet_path: &str, strict_env: bool) -> anyhow::Result<Option<Vec<String>>> {
    let (fleet, paths) = fleet_member_paths(fleet_path)?;

    match &fleet.name {
        Some(name) => println!("fleet {name:?} — {} agent(s)", paths.len()),
        None => println!("fleet {fleet_path} — {} agent(s)", paths.len()),
    }

    let mut all_ok = true;
    let mut loaded: Vec<(String, AgentConfig)> = Vec::with_capacity(paths.len());
    for path in &paths {
        let (ok, config) = check_config(path, strict_env);
        all_ok &= ok;
        if let Some(config) = config {
            loaded.push((path.clone(), config));
        }
    }

    let conflicts = fleet_conflicts(loaded.iter().map(|(path, config)| (path.as_str(), config)));
    for conflict in &conflicts {
        println!("conflict {conflict}");
    }

    Ok((all_ok && conflicts.is_empty()).then_some(paths))
}

/// Read a fleet file and resolve its members to existing config paths.
///
/// A member that is not on disk is an error about the *fleet file*, not about an
/// agent, so it is reported here before any per-agent output starts.
fn fleet_member_paths(fleet_path: &str) -> anyhow::Result<(FleetConfig, Vec<String>)> {
    // `a2a up` defaults to ./fleet.toml, so "no such file" is the most likely
    // first encounter with fleets. Answer the question it raises.
    if !Path::new(fleet_path).exists() {
        anyhow::bail!(
            "no fleet file at {fleet_path}. A fleet lists the agent configs to run together:\n\
             \n    [[agents]]\n    config = \"weather.toml\"\n\
             \n    [[agents]]\n    config = \"orchestrator.toml\"\n\
             \nScaffold the members with `a2a new <name>`, then run `a2a up -f {fleet_path}`."
        );
    }

    let fleet =
        FleetConfig::from_file(fleet_path).map_err(|e| anyhow::anyhow!("{fleet_path}: {e}"))?;

    let mut paths = Vec::with_capacity(fleet.agents.len());
    for (member, path) in fleet
        .agents
        .iter()
        .zip(fleet.config_paths(Path::new(fleet_path)))
    {
        if !path.exists() {
            anyhow::bail!(
                "{fleet_path}: agent config '{}' not found (looked in {})",
                member.config,
                path.display()
            );
        }
        paths.push(path.display().to_string());
    }

    Ok((fleet, paths))
}

/// How much a `doctor` finding matters. `Problem` is the only level that fails
/// the command: a warning is something that will work, differently than you may
/// have meant (an `llm` agent with no key still answers, from a fallback).
#[derive(Clone, Copy)]
enum Level {
    Ok,
    Warn,
    Problem,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Problem => "problem",
        }
    }
}

/// One line of a `doctor` report.
struct Finding {
    level: Level,
    message: String,
}

impl Finding {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            level: Level::Ok,
            message: message.into(),
        }
    }
    fn warn(message: impl Into<String>) -> Self {
        Self {
            level: Level::Warn,
            message: message.into(),
        }
    }
    fn problem(message: impl Into<String>) -> Self {
        Self {
            level: Level::Problem,
            message: message.into(),
        }
    }
}

/// Environment variables that give an agent a model, in the order
/// [`provider_from_env`] prefers them.
///
/// Listed here rather than calling that function because constructing a provider
/// *logs*, and this is CLI output. The verdict it produces is the same: each of
/// its branches is gated on one of these being set.
const LLM_KEY_VARS: [&str; 6] = [
    "OPENROUTER_API_KEY",
    "GEMINI_API_KEY",
    "OPENAI_API_KEY",
    "AI_API_KEY",
    "OPENAI_API_BASE_URL",
    "AI_API_BASE_URL",
];

/// The first configured LLM variable, if any.
fn llm_env_var() -> Option<&'static str> {
    LLM_KEY_VARS.into_iter().find(|var| {
        std::env::var(var)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

/// Resolve `command` the way spawning it will: absolute/relative paths as given,
/// bare names against `PATH` (honouring `PATHEXT` on Windows, which is how `npx`
/// resolves to `npx.cmd`).
///
/// Approximates "runnable" as "is a file" — checking the executable bit as well
/// would be more precise on unix, and would still not prove it runs.
fn find_on_path(command: &str) -> Option<PathBuf> {
    let candidate = Path::new(command);
    if candidate.is_absolute() || candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }

    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };

    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let direct = dir.join(command);
        if direct.is_file() {
            return Some(direct);
        }
        extensions.iter().find_map(|ext| {
            let with_ext = dir.join(format!("{command}{ext}"));
            with_ext.is_file().then_some(with_ext)
        })
    })
}

/// Can this machine bind `host:port` right now?
fn probe_bind(host: &str, port: u16) -> Result<(), std::io::Error> {
    // Dropped immediately: this asks whether the address is available, and the
    // agent binds it for real moments later.
    std::net::TcpListener::bind((host, port)).map(drop)
}

/// What the host offers, independent of any config.
///
/// `bare` distinguishes `a2a doctor` with no configs — where this report is the
/// whole output — from a run that also checks configs. A capability that is
/// *absent* is only a warning in the bare case: with configs in hand, whether
/// its absence matters is a question about them, and the per-config
/// requirements answer it precisely ([`Requirement::LlmProviderFromEnv`] fires
/// for `llm` handlers and not for `echo`). Warning unconditionally made an echo
/// agent on a keyless machine — CI, or any laptop that never exported a model
/// key — report a warning about a provider it will never call, which is the
/// false positive that teaches people to ignore the tool.
fn environment_findings(bare: bool) -> Vec<Finding> {
    let mut findings = Vec::new();

    match llm_env_var() {
        Some(var) => findings.push(Finding::ok(format!("model provider: {var} is set"))),
        None if bare => findings.push(Finding::warn(format!(
            "no model key in the environment ({}) — `llm` handlers fall back to a \
             deterministic reply",
            LLM_KEY_VARS.join(", ")
        ))),
        None => {}
    }

    match ["docker", "podman"]
        .into_iter()
        .find_map(|engine| find_on_path(engine).map(|path| (engine, path)))
    {
        Some((engine, path)) => findings.push(Finding::ok(format!(
            "container engine: {engine} ({})",
            path.display()
        ))),
        None if bare => findings.push(Finding::warn(
            "no container engine on PATH (docker, podman) — `a2a control-plane \
             --runtime container` needs one",
        )),
        None => {}
    }

    findings
}

/// What one config needs, checked against this machine.
///
/// The parsed config comes back so the caller can also reason across configs —
/// two of them wanting the same port is a reason the run will not work, and it
/// is invisible from inside either one.
fn config_findings(path: &str) -> (Vec<Finding>, Option<AgentConfig>) {
    let (config, unset) = match AgentConfig::check_file(path) {
        Ok(checked) => checked,
        // Nothing else can be said about a config that does not load, and
        // `validate` is the command that explains why in full.
        Err(e) => {
            return (
                vec![Finding::problem(format!(
                    "config does not load: {e} (see `a2a validate --config {path}`)"
                ))],
                None,
            );
        }
    };

    let mut findings = vec![Finding::ok(format!(
        "config is valid — {:?}, handler {}",
        config.agent.name,
        config.handler_type()
    ))];

    if !unset.is_empty() {
        findings.push(Finding::problem(format!(
            "unset environment variables: {} — `a2a run` fails until they resolve",
            unset.join(", ")
        )));
    }

    for requirement in requirements(&config) {
        findings.push(match requirement {
            Requirement::HttpBind { host, port } | Requirement::McpHttpBind { host, port } => {
                match probe_bind(&host, port) {
                    Ok(()) => Finding::ok(format!("{host}:{port} is free")),
                    Err(e) => Finding::problem(format!("cannot bind {host}:{port}: {e}")),
                }
            }
            Requirement::McpCommand { server, command } => match find_on_path(&command) {
                Some(found) => Finding::ok(format!(
                    "MCP server {server:?}: `{command}` found ({})",
                    found.display()
                )),
                None => Finding::problem(format!(
                    "MCP server {server:?}: `{command}` is not on PATH — the agent will \
                     start without its tools"
                )),
            },
            Requirement::LlmProviderFromEnv => match llm_env_var() {
                Some(var) => Finding::ok(format!("llm handler will use {var}")),
                None => Finding::warn(
                    "llm handler with no key — it will answer with a deterministic \
                     fallback that lists its tools",
                ),
            },
            Requirement::UnknownHandler { name } => Finding::problem(format!(
                "handler {name:?} is not built into this binary — the runner falls back \
                 to echo, so the agent will not do what this config says"
            )),
        });
    }

    (findings, Some(config))
}

/// Report what this machine offers against what these configs need, returning
/// whether anything is outright broken.
///
/// Stdout, like `validate`: a person reads this and a script greps it.
fn run_doctor(config_paths: &[String], fleet: Option<&str>) -> anyhow::Result<bool> {
    let mut paths = config_paths.to_vec();
    if let Some(fleet_path) = fleet {
        paths.extend(fleet_member_paths(fleet_path)?.1);
    }

    let mut problems = 0usize;
    let mut warnings = 0usize;
    let mut report = |heading: String, findings: Vec<Finding>| {
        println!("{heading}");
        for finding in findings {
            match finding.level {
                Level::Problem => problems += 1,
                Level::Warn => warnings += 1,
                Level::Ok => {}
            }
            println!("  {:<8}{}", finding.level.tag(), finding.message);
        }
        println!();
    };

    report(
        "environment".to_string(),
        environment_findings(paths.is_empty()),
    );

    let mut loaded: Vec<(String, AgentConfig)> = Vec::with_capacity(paths.len());
    for path in &paths {
        let (findings, config) = config_findings(path);
        report(path.clone(), findings);
        if let Some(config) = config {
            loaded.push((path.clone(), config));
        }
    }

    // Whatever set of configs was named — a fleet, or several `--config`s — has
    // to be runnable *together*, which neither the machine nor any one config
    // can answer.
    if loaded.len() > 1 {
        let conflicts =
            fleet_conflicts(loaded.iter().map(|(path, config)| (path.as_str(), config)));
        let findings = if conflicts.is_empty() {
            vec![Finding::ok("these agents can run together")]
        } else {
            conflicts
                .iter()
                .map(|conflict| Finding::problem(conflict.to_string()))
                .collect()
        };
        report("together".to_string(), findings);
    }

    match (problems, warnings) {
        (0, 0) => println!("all clear"),
        (p, w) => println!("{p} problem(s), {w} warning(s)"),
    }
    Ok(problems == 0)
}

/// Everything `a2a control-plane` needs, kept as one struct so the composition
/// root below reads as configuration rather than a nine-argument call.
struct ControlPlaneArgs {
    bind: String,
    config_dir: String,
    runtime_kind: RuntimeKind,
    engine: String,
    image: String,
    token: Option<String>,
    no_auth: bool,
    allow_env: Vec<String>,
    log_dir: Option<String>,
    memory: Option<String>,
    cpus: Option<String>,
    no_hardening: bool,
}

/// Environment variable holding the control-plane bearer token, so it need not
/// appear in argv (where `ps` would show it). Read by both halves: the server
/// requires it, the client presents it.
const TOKEN_ENV: &str = "A2A_CONTROL_PLANE_TOKEN";

/// Environment variable naming the control plane the client subcommands drive.
const URL_ENV: &str = "A2A_CONTROL_PLANE_URL";

/// Where `a2a control-plane` binds unless told otherwise.
const DEFAULT_CONTROL_PLANE_BIND: &str = "127.0.0.1:9090";

/// Where the client subcommands look unless told otherwise — the same address,
/// as a URL, so `a2a control-plane` in one terminal and `a2a ps` in another need
/// no configuration to find each other.
const DEFAULT_CONTROL_PLANE_URL: &str = "http://127.0.0.1:9090";

/// Serve the control-plane HTTP API over the chosen runtime + in-memory
/// registry. Deploying an agent provisions+starts it and registers its card; the
/// API is the surface the Terraform provider targets.
///
/// This is the composition edge: it resolves the auth mode and the secrets
/// allowlist, then injects them into the adapters.
async fn run_control_plane(args: ControlPlaneArgs) -> anyhow::Result<()> {
    let auth = resolve_auth(args.token, args.no_auth)?;

    let allowed_env = EnvAllowlist::new(args.allow_env.iter().cloned());
    if allowed_env.is_empty() {
        info!(
            "no --allow-env vars: configs referencing ${{VAR}} will be rejected \
             (this is the secure default)"
        );
    } else {
        info!(
            "deployed configs may reference these env vars: {}",
            args.allow_env.join(", ")
        );
    }

    let config_dir = PathBuf::from(&args.config_dir);
    let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::default());
    let runtime: Arc<dyn AgentRuntime> = match args.runtime_kind {
        RuntimeKind::Local => {
            // Flags that only the container backend can honour. Saying nothing
            // would let someone believe their agents are memory-capped or
            // deliberately unhardened when neither is true — the kind of
            // silently-ignored setting this tool rejects everywhere else.
            let ignored: Vec<&str> = [
                args.memory.is_some().then_some("--memory"),
                args.cpus.is_some().then_some("--cpus"),
                args.no_hardening.then_some("--no-hardening"),
            ]
            .into_iter()
            .flatten()
            .collect();
            if !ignored.is_empty() {
                warn!(
                    "ignoring {} — container-only; `--runtime local` supervises plain child \
                     processes and enforces nothing",
                    ignored.join(", ")
                );
            }

            // Capture by default. Without it the children inherit this terminal
            // and `a2a logs` has nothing to serve — which would make the
            // subcommand a dead end on the runtime people try first.
            let log_dir = args
                .log_dir
                .map(PathBuf::from)
                .unwrap_or_else(|| config_dir.join("logs"));
            info!("agent logs: {} (a2a logs <id>)", log_dir.display());
            Arc::new(
                LocalProcessRuntime::new()
                    .with_allowed_env(allowed_env)
                    .with_log_dir(log_dir),
            )
        }
        RuntimeKind::Container => {
            let hardening = resolve_hardening(args.no_hardening, args.memory, args.cpus);
            Arc::new(
                ContainerRuntime::with_engine(args.engine)
                    .with_image(args.image)
                    .with_allowed_env(allowed_env)
                    .with_hardening(hardening),
            )
        }
    };
    let cp = Arc::new(ControlPlane::new(
        runtime,
        registry,
        Arc::new(HttpCardSource::new()),
    ));

    // Adopt whatever survived the last run *before* accepting requests, so the
    // first `GET /agents` tells the truth. A failure here is fatal on purpose:
    // if the backend cannot be queried, every deploy would fail too, and a
    // control plane that starts anyway would report an empty fleet.
    match cp.recover().await? {
        Recovered::Adopted(agents) if agents.is_empty() => {
            info!("no previously deployed agents found");
        }
        Recovered::Adopted(agents) => {
            info!(
                "recovered {} agent(s) from the previous run: {}",
                agents.len(),
                agents
                    .iter()
                    .map(|a| a.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        Recovered::Ephemeral => warn!(
            "this runtime cannot survive a restart: any agents deployed before now are no \
             longer managed, and this control plane starts with an empty fleet. Use \
             `--runtime container` for a control plane that can be bounced."
        ),
    }

    let router = control_plane_router(cp, config_dir, auth);

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    // Report the address actually bound, not the one requested: with `:0` (or a
    // hostname that resolves) those differ, and the printed URL has to be one a
    // caller can use.
    match listener.local_addr() {
        Ok(bound) => info!("control-plane API listening on http://{bound}"),
        Err(_) => info!("control-plane API listening on http://{}", args.bind),
    }
    axum::serve(listener, router).await?;
    Ok(())
}

/// Decide what the container engine is asked to enforce on each agent.
///
/// Hardening is on unless explicitly waived — that is the point of it — and
/// waiving it warns, for the same reason `--no-auth` does: it is a choice worth
/// seeing in a log six months later.
fn resolve_hardening(
    no_hardening: bool,
    memory: Option<String>,
    cpus: Option<String>,
) -> ContainerHardening {
    if no_hardening {
        warn!(
            "--no-hardening: agent containers keep all capabilities, a writable root \
             filesystem, and no process limit"
        );
        // Resource ceilings are still honoured — they are a limit, not a
        // restriction, and someone who waived the security flags may well still
        // want their agents bounded.
        return ContainerHardening {
            memory,
            cpus,
            ..ContainerHardening::none()
        };
    }

    let hardening = ContainerHardening {
        memory,
        cpus,
        ..ContainerHardening::default()
    };
    match (&hardening.memory, &hardening.cpus) {
        (None, None) => info!(
            "agent containers: capabilities dropped, no-new-privileges, read-only root \
             where storage allows; no memory/CPU ceiling (--memory, --cpus)"
        ),
        _ => info!(
            "agent containers: hardened, memory {:?}, cpus {:?}",
            hardening.memory.as_deref().unwrap_or("unlimited"),
            hardening.cpus.as_deref().unwrap_or("unlimited")
        ),
    }
    hardening
}

/// Decide how the API authenticates, refusing to start unauthenticated unless
/// that was asked for explicitly.
///
/// The control plane starts processes and containers and hands them the
/// operator's allowlisted secrets, so "no token configured" must be a hard error
/// rather than a silent downgrade to an open endpoint.
fn resolve_auth(token: Option<String>, no_auth: bool) -> anyhow::Result<ControlPlaneAuth> {
    let token = token.or_else(|| std::env::var(TOKEN_ENV).ok());
    match (token, no_auth) {
        // clap rejects `--token` alongside `--no-auth`, so reaching here means
        // the token came from the environment. Ambiguous intent about an
        // authentication setting is a hard error, not a silent winner.
        (Some(_), true) => anyhow::bail!(
            "--no-auth was passed but {TOKEN_ENV} is set; unset it to run unauthenticated, \
             or drop --no-auth to require the token"
        ),
        (Some(token), false) if token.trim().is_empty() => {
            anyhow::bail!("control-plane token is empty; provide a real secret or pass --no-auth")
        }
        (Some(token), false) => Ok(ControlPlaneAuth::bearer(token)),
        (None, true) => {
            warn!(
                "control-plane API is running UNAUTHENTICATED (--no-auth): anyone who can reach \
                 the bind address can start containers and read the secrets you allow-list. \
                 Dev loops only."
            );
            Ok(ControlPlaneAuth::Disabled)
        }
        (None, false) => anyhow::bail!(
            "control-plane requires a bearer token: pass --token <secret>, set {TOKEN_ENV}, \
             or opt out explicitly with --no-auth"
        ),
    }
}

#[cfg(feature = "schema")]
fn print_schema(fleet: bool) {
    use schemars::schema_for;
    let schema = if fleet {
        schema_for!(FleetConfig)
    } else {
        schema_for!(AgentConfig)
    };
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}

#[cfg(not(feature = "schema"))]
fn print_schema(_fleet: bool) {
    error!("the `schema` feature is required for print-schema; rebuild with --features schema");
    std::process::exit(1);
}

async fn run_agents(config_paths: Vec<String>) -> anyhow::Result<()> {
    if config_paths.is_empty() {
        error!("At least one configuration file must be specified");
        std::process::exit(1);
    }
    info!("Starting A2A Agents ({} config(s))", config_paths.len());

    // Phase 1: register every agent's card in a shared registry *before* any
    // handler is built, so a config that references a peer by skill/agent-id
    // resolves race-free at startup. `InMemoryAgentRegistry` is the dev/default
    // adapter; a persistent or control-plane-backed one is a drop-in behind the
    // same `AgentRegistry` port.
    let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::default());
    for config_path in &config_paths {
        match AgentBuilder::from_file(config_path) {
            Ok(builder) => match builder.agent_card().await {
                Ok(card) => {
                    let endpoint = builder.config().agent_url();
                    let name = builder.config().agent.name.clone();
                    match registry.register(card, endpoint).await {
                        Ok(id) => info!("registered agent '{}' as '{}'", name, id),
                        Err(e) => warn!("could not register agent '{}': {}", name, e),
                    }
                }
                Err(e) => warn!("could not build agent card for {}: {}", config_path, e),
            },
            Err(e) => warn!(
                "skipping invalid config {} during registration: {}",
                config_path, e
            ),
        }
    }

    print_run_banner(&config_paths);

    // Phase 2: build and run each agent; LLM handlers resolve registry refs.
    let mut agents: JoinSet<Result<(), String>> = JoinSet::new();
    for config_path in &config_paths {
        let config_path = config_path.clone();
        let registry = registry.clone();
        agents.spawn(async move {
            run_one_agent(&config_path, registry)
                .await
                .map_err(|e| format!("{config_path}: {e}"))
        });
    }

    // An agent stopping is news — it had just been announced by the banner as
    // running, and the most common cause (a port already bound) leaves the
    // survivors looking healthy. Reported on stdout in *completion* order, so a
    // fleet member that dies on startup says so at once rather than when the
    // last agent finally exits.
    let total = config_paths.len();
    let mut failures = 0usize;
    while let Some(joined) = agents.join_next().await {
        let reason = match joined {
            Ok(Ok(())) => continue,
            Ok(Err(reason)) => reason,
            Err(e) => format!("agent task panicked or was cancelled: {e}"),
        };
        failures += 1;
        println!("failed  {reason}");
    }

    // The survivors are left running deliberately: a fleet is more useful
    // degraded than absent, and the operator can see what is missing. But the
    // exit code has to reflect it — a supervisor (systemd, a container, this
    // tool's own `LocalProcessRuntime`) reads nothing else.
    if failures > 0 {
        anyhow::bail!("{failures} of {total} agent(s) stopped early");
    }
    Ok(())
}

async fn run_one_agent(config_path: &str, registry: Arc<dyn AgentRegistry>) -> anyhow::Result<()> {
    // Only the (mcp-server-gated) LLM handler consumes the registry.
    #[cfg(not(feature = "mcp-server"))]
    let _ = &registry;
    info!("Loading agent config from: {}", config_path);
    // Reported by the caller, which counts it and sets the exit code; logging it
    // here as well would print the same failure twice.
    let builder =
        AgentBuilder::from_file(config_path).map_err(|e| anyhow::anyhow!("config error: {e}"))?;
    let handler_type = builder.config().handler_type();
    info!("Using handler: {}", handler_type);
    match handler_type {
        #[cfg(feature = "reimbursement-agent")]
        HandlerType::Reimbursement => {
            let storage = AutoStorage::from_config(&builder.config().server.storage).await?;
            let llm_provider = resolve_llm(&builder.config().llm);
            let streaming = InMemoryStreamingHandler::new();
            let push = storage.push_notifier();
            let handler =
                ReimbursementHandler::with_llm(storage.clone(), streaming, push, llm_provider);
            let runtime = builder
                .with_handler(handler)
                .with_storage(storage)
                .build()?;
            runtime.run().await?;
        }
        #[cfg(feature = "mcp-server")]
        HandlerType::Llm => {
            run_llm_agent(builder, registry).await?;
        }
        HandlerType::Echo => {
            let runtime = builder
                .with_handler(EchoHandler)
                .build_with_auto_storage()
                .await?;
            runtime.run().await?;
        }
        // `Custom(_)` plus any variant whose handler feature is disabled in this
        // build fall through to the echo default with a warning.
        other => {
            warn!(
                "Unsupported handler '{}' in {}. Falling back to echo.",
                other, config_path
            );
            let runtime = builder
                .with_handler(EchoHandler)
                .build_with_auto_storage()
                .await?;
            runtime.run().await?;
        }
    }
    Ok(())
}

/// Build the tool description shown to the model from a remote agent's card.
/// Prefers the card's `description`, falling back to a generic delegation hint.
#[cfg(feature = "mcp-server")]
fn description_from_card(name: &str, card: &AgentCard) -> String {
    let card_desc = serde_json::to_value(card)
        .ok()
        .and_then(|v| {
            v.get("description")
                .and_then(|d| d.as_str())
                .map(str::to_string)
        })
        .filter(|s| !s.is_empty());
    match card_desc {
        Some(d) => format!("Delegate to the '{name}' A2A agent: {d}"),
        None => format!("Delegate a request to the '{name}' A2A agent."),
    }
}

#[cfg(feature = "mcp-server")]
async fn run_llm_agent(
    builder: AgentBuilder,
    registry: Arc<dyn AgentRegistry>,
) -> anyhow::Result<()> {
    use a2a_mcp::McpToA2ABridge;
    use a2a_rs::InMemoryTaskStorage;
    use rmcp::ServiceExt;
    use rmcp::model::{ClientCapabilities, ClientInfo, Implementation, ProtocolVersion};
    use rmcp::transport::TokioChildProcess;

    let llm_cfg: LlmHandlerConfig = builder.config().handler.llm.clone().unwrap_or_default();
    let llm_provider = resolve_llm(&builder.config().llm);

    // Assemble the tool sources the LLM loop can call: one per connected MCP
    // server (each spawned as a child process) plus one per configured remote
    // A2A agent (reached over the wire as a delegation tool).
    let mut sources: Vec<Arc<dyn ToolSource>> = Vec::new();

    if builder.config().features.mcp_client.enabled {
        for srv in &builder.config().features.mcp_client.servers {
            let mut cmd = tokio::process::Command::new(&srv.command);
            cmd.args(&srv.args);
            for (k, v) in &srv.env {
                cmd.env(k, v);
            }
            if let Some(cwd) = &srv.cwd {
                cmd.current_dir(cwd);
            }
            match TokioChildProcess::builder(cmd).spawn() {
                Ok((transport, _stderr)) => {
                    let implementation =
                        Implementation::new(format!("a2a-agent-{}", srv.name), "0.1.0");
                    let client_info =
                        ClientInfo::new(ClientCapabilities::default(), implementation)
                            .with_protocol_version(ProtocolVersion::V_2024_11_05);
                    match client_info.serve(transport).await {
                        Ok(svc) => {
                            match McpToA2ABridge::new(svc.peer().clone(), UnusedInner).await {
                                Ok(b) => {
                                    sources.push(Arc::new(McpToolSource::new(Arc::new(b))));
                                    info!("connected MCP tool server '{}'", srv.name);
                                }
                                Err(e) => warn!("MCP bridge init failed for {}: {}", srv.name, e),
                            }
                        }
                        Err(e) => warn!("MCP connect failed for {}: {}", srv.name, e),
                    }
                }
                Err(e) => warn!("failed to spawn MCP server {}: {}", srv.name, e),
            }
        }
    }

    for agent in &llm_cfg.agents {
        let target = match agent.target() {
            Ok(t) => t,
            Err(e) => {
                warn!("skipping remote agent '{}': {}", agent.name, e);
                continue;
            }
        };

        // Resolve the reference to a dialable endpoint, carrying the discovered
        // card when resolved via the registry so the tool description needs no
        // second card fetch.
        let resolved: Option<(String, Option<AgentCard>)> = match target {
            RemoteAgentTarget::Url(url) => Some((url.to_string(), None)),
            RemoteAgentTarget::AgentId(id) => match registry.get(&AgentId::from(id)).await {
                Ok(Some(found)) => Some((found.endpoint, Some(found.card))),
                Ok(None) => {
                    warn!(
                        "remote agent '{}': no agent with id '{}' in registry",
                        agent.name, id
                    );
                    None
                }
                Err(e) => {
                    warn!(
                        "remote agent '{}': registry lookup failed: {}",
                        agent.name, e
                    );
                    None
                }
            },
            RemoteAgentTarget::Skill(skill) => match registry.find_by_skill(skill).await {
                Ok(mut matches) if !matches.is_empty() => {
                    if matches.len() > 1 {
                        warn!(
                            "remote agent '{}': {} agents advertise skill '{}'; using the first",
                            agent.name,
                            matches.len(),
                            skill
                        );
                    }
                    let found = matches.remove(0);
                    Some((found.endpoint, Some(found.card)))
                }
                Ok(_) => {
                    warn!(
                        "remote agent '{}': no agent advertises skill '{}'",
                        agent.name, skill
                    );
                    None
                }
                Err(e) => {
                    warn!(
                        "remote agent '{}': registry lookup failed: {}",
                        agent.name, e
                    );
                    None
                }
            },
        };

        let Some((endpoint, resolved_card)) = resolved else {
            continue;
        };

        match a2a_rs::auto_connect(&endpoint).await {
            Ok(transport) => {
                let description = match &agent.description {
                    Some(d) => d.clone(),
                    None => {
                        // Prefer the card resolved from the registry; else fetch
                        // it from the endpoint; else a generic hint.
                        let card = match resolved_card {
                            Some(c) => Some(c),
                            None => a2a_rs::fetch_agent_card(&endpoint).await.ok(),
                        };
                        match card {
                            Some(c) => description_from_card(&agent.name, &c),
                            None => format!(
                                "Delegate a request to the '{}' A2A agent at {}.",
                                agent.name, endpoint
                            ),
                        }
                    }
                };
                let source =
                    A2aAgentToolSource::new(&agent.name, description, Arc::from(transport));
                info!(
                    "exposing remote agent '{}' ({}) as tool '{}'",
                    agent.name,
                    endpoint,
                    source.tool_name()
                );
                sources.push(Arc::new(source));
            }
            Err(e) => warn!(
                "could not connect to remote agent {} at {}: {}",
                agent.name, endpoint, e
            ),
        }
    }

    let storage = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();
    let push: Arc<dyn a2a_rs::port::AsyncPushNotifier> = storage.push_notifier();
    let handler = LlmHandler::new(
        llm_cfg.system_prompt,
        llm_cfg.max_tool_rounds,
        storage.clone(),
        streaming.clone(),
        push,
        sources,
        llm_provider,
    );
    let runtime = builder
        .with_handler(handler)
        .with_storage(storage)
        .with_streaming(streaming)
        .build()?;
    runtime.run().await?;
    Ok(())
}
