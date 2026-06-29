//! Example runner for the TOML-only LLM agent.
//!
//! This is a thin wrapper around the generic `a2a` binary's `run` subcommand
//! for the `llm_agent.toml` config. It exists so the example is discoverable
//! via `cargo run --example llm_agent`; the real work is done by the
//! config-driven `LlmHandler` registered in `bin/a2a.rs`.
//!
//! Run with:
//!
//! ```bash
//! cargo run -p a2a-agents --example llm_agent --features mcp-server
//! ```

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use a2a_agents::LlmHandler;
    use a2a_agents::core::AgentBuilder;
    use a2a_rs::InMemoryStreamingHandler;
    use a2a_rs::InMemoryTaskStorage;
    use std::sync::Arc;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path =
        std::env::var("LLM_AGENT_CONFIG").unwrap_or_else(|_| "examples/llm_agent.toml".to_string());
    println!("Loading agent config from: {config_path}");
    println!("Set OPENAI_API_KEY / GEMINI_API_KEY / OPENROUTER_API_KEY for LLM answers.");

    let builder = AgentBuilder::from_file(&config_path)?;
    let llm_cfg = builder.config().handler.llm.clone().unwrap_or_default();
    let llm = a2a_agents_common::llm::provider_from_env();

    let storage = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();
    let push: Arc<dyn a2a_rs::port::AsyncPushNotifier> = storage.push_notifier();
    // No MCP servers or remote agents wired in this minimal example; the handler
    // answers directly from the LLM. See `orchestrator_agent.toml` for delegation.
    let handler = LlmHandler::new(
        llm_cfg.system_prompt,
        llm_cfg.max_tool_rounds,
        storage.clone(),
        streaming.clone(),
        push,
        Vec::new(),
        llm,
    );
    println!("LLM agent listening on http://127.0.0.1:8080");
    println!("Agent card: http://127.0.0.1:8080/.well-known/agent-card.json");
    builder
        .with_handler(handler)
        .with_storage(storage)
        .with_streaming(streaming)
        .build()?
        .run()
        .await?;
    Ok(())
}
