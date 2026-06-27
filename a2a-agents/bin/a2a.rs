//! Generic A2A Agent runner
//!
//! This binary can run multiple A2A agents concurrently, configured via TOML files.
//! It supports different built-in implementations via the `agent.implementation` setting.

#[cfg(feature = "reimbursement-agent")]
use a2a_agents::agents::reimbursement::ReimbursementHandler;
use a2a_agents::core::AgentBuilder;
use a2a_agents::core::builder::AutoStorage;
use a2a_agents_common::llm::{LlmProvider, LlmSettings, provider_from_env, provider_from_settings};

use a2a_rs::{
    InMemoryStreamingHandler,
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use clap::Parser;

use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Generic A2A Agent Runner
#[derive(Parser, Debug)]
#[clap(
    name = "a2a",
    version,
    about = "Runs one or more A2A agents from declarative configurations"
)]
struct Args {
    /// Agent configuration file paths (TOML format)
    #[clap(short, long, required = true)]
    config: Vec<String>,
}

/// A simple echo handler used as a fallback or for testing
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    if args.config.is_empty() {
        error!("At least one configuration file must be specified");
        std::process::exit(1);
    }

    info!("🚀 Starting A2A Agents ({} config(s))", args.config.len());

    let mut handles: Vec<JoinHandle<anyhow::Result<()>>> = Vec::new();

    for config_path in &args.config {
        let config_path = config_path.clone();

        let handle = tokio::spawn(async move {
            info!("📄 Loading agent config from: {}", config_path);

            // Build the configuration first to inspect the implementation type
            let builder = match AgentBuilder::from_file(&config_path) {
                Ok(b) => b,
                Err(e) => {
                    error!("Failed to load config {}: {}", config_path, e);
                    return Err(anyhow::anyhow!("Config error: {}", e));
                }
            };

            let implementation = builder
                .config()
                .agent
                .implementation
                .as_deref()
                .unwrap_or("echo");
            info!("🛠️  Using implementation: {}", implementation);

            match implementation {
                #[cfg(feature = "reimbursement-agent")]
                "reimbursement" => {
                    let storage =
                        AutoStorage::from_config(&builder.config().server.storage).await?;

                    // Initialize LLM from config if provided, else from the env.
                    let llm_provider: Option<Arc<dyn LlmProvider>> =
                        match &builder.config().llm {
                            Some(llm_config) => {
                                info!(
                                    "Loading LLM configuration from TOML (provider: {})",
                                    llm_config.provider
                                );
                                let settings = LlmSettings {
                                    provider: llm_config.provider.clone(),
                                    api_key: llm_config.api_key.clone(),
                                    model: llm_config.model.clone(),
                                    base_url: llm_config.base_url.clone(),
                                    http_referer: llm_config.http_referer.clone(),
                                    x_title: llm_config.x_title.clone(),
                                };
                                Some(provider_from_settings(&settings).map_err(|e| {
                                    anyhow::anyhow!("invalid LLM configuration: {e}")
                                })?)
                            }
                            None => provider_from_env(),
                        };

                    let streaming = InMemoryStreamingHandler::new();
                    let push = storage.push_notifier();
                    let handler = ReimbursementHandler::with_llm(
                        storage.clone(),
                        streaming,
                        push,
                        llm_provider,
                    );

                    // We use build() instead of build_with_auto_storage() since we created storage manually
                    // Note: If mcp-client is enabled, we'd need to manually initialize it here,
                    // but for reimbursement agent we can just build it normally.
                    let runtime = builder
                        .with_handler(handler)
                        .with_storage(storage)
                        .build()?;

                    runtime.run().await?;
                }
                "echo" => {
                    let runtime = builder
                        .with_handler(EchoHandler)
                        .build_with_auto_storage()
                        .await?;

                    runtime.run().await?;
                }
                other => {
                    warn!(
                        "Unknown implementation '{}' in {}. Falling back to 'echo'.",
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
        });

        handles.push(handle);
    }

    // Wait for all servers to complete (or fail)
    for handle in handles {
        match handle.await {
            Ok(Err(e)) => {
                error!("Agent task failed: {}", e);
            }
            Err(e) => {
                error!("Agent task panicked or was cancelled: {}", e);
            }
            _ => {}
        }
    }

    Ok(())
}
