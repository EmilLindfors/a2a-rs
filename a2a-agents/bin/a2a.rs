//! Generic A2A Agent runner
//!
//! This binary can run multiple A2A agents concurrently, configured via TOML files.
//! It supports different built-in implementations via the `agent.implementation` setting.

#[cfg(feature = "reimbursement-agent")]
use a2a_agents::agents::reimbursement::ReimbursementHandler;
use a2a_agents::core::AgentBuilder;
use a2a_agents::core::builder::AutoStorage;
use a2a_agents_common::llm::LlmProvider;
use a2a_agents_common::llm::gemini::{GeminiConfig, GeminiProvider};
use a2a_agents_common::llm::openai::{OpenAiConfig, OpenAiProvider};

use a2a_rs::{
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

                    // Initialize LLM from config if provided
                    let mut llm_provider: Option<Arc<dyn LlmProvider>> = None;
                    if let Some(llm_config) = &builder.config().llm {
                        info!(
                            "Loading LLM configuration from TOML (provider: {})",
                            llm_config.provider
                        );
                        match llm_config.provider.as_str() {
                            "openai" => {
                                let openai_config = OpenAiConfig {
                                    base_url: llm_config
                                        .base_url
                                        .clone()
                                        .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                                    model: llm_config
                                        .model
                                        .clone()
                                        .unwrap_or_else(|| "gpt-4o-mini".to_string()),
                                    api_key: llm_config.api_key.clone(),
                                };
                                llm_provider = Some(Arc::new(OpenAiProvider::new(openai_config)));
                            }
                            "gemini" => {
                                let gemini_config = GeminiConfig {
                                    base_url: llm_config.base_url.clone().unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta/models".to_string()),
                                    api_key: llm_config.api_key.clone().unwrap_or_default(),
                                    model: llm_config.model.clone().unwrap_or_else(|| "gemini-1.5-pro".to_string()),
                                };
                                llm_provider = Some(Arc::new(GeminiProvider::new(gemini_config)));
                            }
                            other => {
                                warn!("Unsupported LLM provider in config: {}", other);
                            }
                        }
                    } else {
                        // Fallback to environment variables if no config is provided
                        if let Ok(gemini) = GeminiProvider::from_env() {
                            info!("Gemini client initialized successfully from environment");
                            llm_provider = Some(Arc::new(gemini));
                        } else if let Ok(openai) = OpenAiProvider::from_env() {
                            info!("OpenAI client initialized successfully from environment");
                            llm_provider = Some(Arc::new(openai));
                        } else {
                            warn!(
                                "Failed to initialize any AI client. Conversational features will be disabled."
                            );
                        }
                    }

                    let handler = ReimbursementHandler::with_llm(storage.clone(), llm_provider);

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
