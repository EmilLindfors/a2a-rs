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
use a2a_agents::core::LlmHandlerConfig;
use a2a_agents_common::llm::{LlmProvider, LlmSettings, provider_from_env, provider_from_settings};

#[cfg(feature = "mcp-server")]
use a2a_agents::core::config::RemoteAgentConfig;
#[cfg(feature = "mcp-server")]
use a2a_agents::handlers::tools::ToolSource;
#[cfg(feature = "mcp-server")]
use a2a_agents::{A2aAgentToolSource, LlmHandler, McpToolSource, UnusedInner};
use a2a_rs::{
    InMemoryStreamingHandler,
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use clap::{Parser, Subcommand};
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
    }
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
    let mut handles: Vec<JoinHandle<anyhow::Result<()>>> = Vec::new();
    for config_path in &config_paths {
        let config_path = config_path.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = run_one_agent(&config_path).await {
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

async fn run_one_agent(config_path: &str) -> anyhow::Result<()> {
    info!("Loading agent config from: {}", config_path);
    let builder = match AgentBuilder::from_file(config_path) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to load config {}: {}", config_path, e);
            return Err(anyhow::anyhow!("Config error: {}", e));
        }
    };
    let handler_type = builder.config().handler_type().to_string();
    info!("Using handler: {}", handler_type);
    match handler_type.as_str() {
        #[cfg(feature = "reimbursement-agent")]
        "reimbursement" => {
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
        "llm" => {
            run_llm_agent(builder).await?;
        }
        "echo" => {
            let runtime = builder
                .with_handler(EchoHandler)
                .build_with_auto_storage()
                .await?;
            runtime.run().await?;
        }
        other => {
            warn!("Unknown handler {} in {}. Falling back to echo.", other, config_path);
            let runtime = builder
                .with_handler(EchoHandler)
                .build_with_auto_storage()
                .await?;
            runtime.run().await?;
        }
    }
    Ok(())
}

/// Build the tool description shown to the model for a remote agent. Prefers an
/// explicit config override, then the remote agent card's description, then a
/// generic fallback so a missing/unreachable card never blocks startup.
#[cfg(feature = "mcp-server")]
async fn remote_agent_description(agent: &RemoteAgentConfig) -> String {
    if let Some(desc) = &agent.description {
        return desc.clone();
    }
    match a2a_rs::fetch_agent_card(&agent.url).await {
        Ok(card) => {
            let card_desc = serde_json::to_value(&card)
                .ok()
                .and_then(|v| v.get("description").and_then(|d| d.as_str()).map(str::to_string))
                .filter(|s| !s.is_empty());
            match card_desc {
                Some(d) => format!("Delegate to the '{}' A2A agent: {d}", agent.name),
                None => format!("Delegate a request to the '{}' A2A agent.", agent.name),
            }
        }
        Err(_) => format!(
            "Delegate a request to the '{}' A2A agent at {}.",
            agent.name, agent.url
        ),
    }
}

#[cfg(feature = "mcp-server")]
async fn run_llm_agent(builder: AgentBuilder) -> anyhow::Result<()> {
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
        match a2a_rs::auto_connect(&agent.url).await {
            Ok(transport) => {
                let description = remote_agent_description(agent).await;
                let source =
                    A2aAgentToolSource::new(&agent.name, description, Arc::from(transport));
                info!(
                    "exposing remote agent '{}' ({}) as tool '{}'",
                    agent.name,
                    agent.url,
                    source.tool_name()
                );
                sources.push(Arc::new(source));
            }
            Err(e) => warn!(
                "could not connect to remote agent {} at {}: {}",
                agent.name, agent.url, e
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
