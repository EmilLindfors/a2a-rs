//! Generic A2A Agent runner
//!
//! This binary can run multiple A2A agents concurrently, configured via TOML files.
//! It selects a built-in handler via the typed `[handler]` block (falling back to
//! the legacy `agent.implementation` string).
//!
//! Subcommands:
//!
//! * `run` — run one or more agents from TOML config files.
//! * `validate` — load and validate config files without starting servers.
//! * `print-schema` — print the JSON Schema for `AgentConfig` to stdout.

#[cfg(feature = "reimbursement-agent")]
use a2a_agents::agents::reimbursement::ReimbursementHandler;
use a2a_agents::core::AgentBuilder;
use a2a_agents::core::builder::AutoStorage;
use a2a_agents::core::config::LlmConfig;
use a2a_agents::core::{HandlerType, LlmHandlerConfig};
use a2a_agents::{
    AgentRegistry, AgentRuntime, ContainerRuntime, ControlPlane, InMemoryAgentRegistry,
    LocalProcessRuntime, control_plane_router,
};
use a2a_agents_common::llm::{LlmProvider, LlmSettings, provider_from_env, provider_from_settings};

#[cfg(feature = "mcp-server")]
use a2a_agents::core::config::RemoteAgentTarget;
#[cfg(feature = "mcp-server")]
use a2a_agents::handlers::tools::ToolSource;
#[cfg(feature = "mcp-server")]
use a2a_agents::{A2aAgentToolSource, AgentId, LlmHandler, McpToolSource, UnusedInner};
#[cfg(feature = "mcp-server")]
use a2a_rs::domain::AgentCard;
use a2a_rs::{
    InMemoryStreamingHandler,
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[clap(name = "a2a", version, about = "Runs A2A agents from declarative configs")]
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
    /// Load and validate config files without starting servers.
    Validate {
        #[clap(short, long, required = true)]
        config: Vec<String>,
    },
    /// Print the JSON Schema for `AgentConfig` to stdout.
    PrintSchema,
    /// Serve the control-plane HTTP API: deploy/list/status/undeploy agents,
    /// each run via the chosen `--runtime` (local processes or containers).
    ControlPlane {
        /// Address to bind the control-plane HTTP API to.
        #[clap(long, default_value = "127.0.0.1:9090")]
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
    },
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
            .status(TaskStatus::new(TaskState::Completed, Some(response.clone())))
            .history(vec![message.clone(), response])
            .build())
    }
}

fn resolve_llm(llm_config: &Option<LlmConfig>) -> Option<Arc<dyn LlmProvider>> {
    match llm_config {
        Some(cfg) => {
            info!("Loading LLM configuration from TOML (provider: {})", cfg.provider);
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
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::PrintSchema => {
            print_schema();
        }
        Command::Validate { config } => {
            for path in &config {
                match AgentBuilder::from_file(path) {
                    Ok(builder) => {
                        info!("OK: {} (handler: {})", path, builder.config().handler_type());
                    }
                    Err(e) => {
                        error!("INVALID: {}: {}", path, e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Command::Run { config } => {
            run_agents(config).await?;
        }
        Command::ControlPlane {
            bind,
            config_dir,
            runtime,
            engine,
            image,
        } => {
            run_control_plane(bind, config_dir, runtime, engine, image).await?;
        }
    }
    Ok(())
}

/// Serve the control-plane HTTP API over a `LocalProcessRuntime` + in-memory
/// registry. Deploying an agent provisions+starts it as a child process and
/// registers its card; the API is the surface the Terraform provider targets.
async fn run_control_plane(
    bind: String,
    config_dir: String,
    runtime_kind: RuntimeKind,
    engine: String,
    image: String,
) -> anyhow::Result<()> {
    let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::default());
    let runtime: Arc<dyn AgentRuntime> = match runtime_kind {
        RuntimeKind::Local => Arc::new(LocalProcessRuntime::new()),
        RuntimeKind::Container => {
            Arc::new(ContainerRuntime::with_engine(engine).with_image(image))
        }
    };
    let cp = Arc::new(ControlPlane::new(runtime, registry));
    let router = control_plane_router(cp, PathBuf::from(config_dir));

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!("control-plane API listening on http://{}", bind);
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(feature = "schema")]
fn print_schema() {
    use a2a_agents::core::AgentConfig;
    use schemars::schema_for;
    let schema = schema_for!(AgentConfig);
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}

#[cfg(not(feature = "schema"))]
fn print_schema() {
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

    // Phase 2: build and run each agent; LLM handlers resolve registry refs.
    let mut handles: Vec<JoinHandle<anyhow::Result<()>>> = Vec::new();
    for config_path in &config_paths {
        let config_path = config_path.clone();
        let registry = registry.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = run_one_agent(&config_path, registry).await {
                error!("Agent task failed for {}: {}", config_path, e);
            }
            Ok(())
        });
        handles.push(handle);
    }
    for handle in handles {
        if let Err(e) = handle.await {
            error!("Agent task panicked or was cancelled: {}", e);
        }
    }
    Ok(())
}

async fn run_one_agent(config_path: &str, registry: Arc<dyn AgentRegistry>) -> anyhow::Result<()> {
    // Only the (mcp-server-gated) LLM handler consumes the registry.
    #[cfg(not(feature = "mcp-server"))]
    let _ = &registry;
    info!("Loading agent config from: {}", config_path);
    let builder = match AgentBuilder::from_file(config_path) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to load config {}: {}", config_path, e);
            return Err(anyhow::anyhow!("Config error: {}", e));
        }
    };
    let handler_type = builder.config().handler_type();
    info!("Using handler: {}", handler_type);
    match handler_type {
        #[cfg(feature = "reimbursement-agent")]
        HandlerType::Reimbursement => {
            let storage = AutoStorage::from_config(&builder.config().server.storage).await?;
            let llm_provider = resolve_llm(&builder.config().llm);
            let streaming = InMemoryStreamingHandler::new();
            let push = storage.push_notifier();
            let handler = ReimbursementHandler::with_llm(
                storage.clone(),
                streaming,
                push,
                llm_provider,
            );
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
        .and_then(|v| v.get("description").and_then(|d| d.as_str()).map(str::to_string))
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

    let llm_cfg: LlmHandlerConfig = builder
        .config()
        .handler
        .llm
        .clone()
        .unwrap_or_default();
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
                    warn!("remote agent '{}': registry lookup failed: {}", agent.name, e);
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
                    warn!("remote agent '{}': registry lookup failed: {}", agent.name, e);
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
