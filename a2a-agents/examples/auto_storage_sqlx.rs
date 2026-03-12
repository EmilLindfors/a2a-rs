//! Agent with automatic SQLx storage and migrations
//!
//! This example shows how to use the builder with SQLx storage.
//! The builder automatically:
//! 1. Creates SQLx storage from TOML config
//! 2. Runs your custom migrations
//! 3. Wires everything together
//!
//! All you provide is your handler and migrations!

use a2a_agents::core::{AgentBuilder, BuildError};
use a2a_rs::{
    domain::{A2AError, Message, Part, Role, Task, TaskState},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use uuid::Uuid;

/// Simple echo handler
#[derive(Clone)]
struct PersistentEchoHandler;

#[async_trait]
impl AsyncMessageHandler for PersistentEchoHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let text = message
            .parts
            .iter()
            .find_map(|part| match part {
                Part::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "No text provided".to_string());

        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("Echo (from SQLx): {}", text))])
            .message_id(Uuid::new_v4().to_string())
            .context_id(message.context_id.clone().unwrap_or_default())
            .build();

        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id(message.context_id.clone().unwrap_or_default())
            .status(a2a_rs::domain::TaskStatus {
                state: TaskState::Completed,
                message: Some(response.clone()),
                timestamp: Some(chrono::Utc::now()),
            })
            .history(vec![message.clone(), response])
            .build())
    }

    async fn validate_message(&self, message: &Message) -> Result<(), A2AError> {
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), BuildError> {
    // Load .env file if present
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    println!("🚀 Starting agent with automatic SQLx storage");
    println!("   Database URL from config: ${{DATABASE_URL}}");
    println!("   Migrations will be run automatically");
    println!();

    // Optional: define custom migrations for your agent
    // These will be run automatically before the agent starts
    let migrations = &[
        // Example migration - you can add agent-specific tables here
        r#"
        CREATE TABLE IF NOT EXISTS echo_stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            message_count INTEGER NOT NULL DEFAULT 0,
            last_echo TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    ];

    // The builder:
    // 1. Reads TOML config (including database URL from env)
    // 2. Creates SQLx connection pool
    // 3. Runs all migrations (framework + your custom ones)
    // 4. Wires storage with push notification support
    // 5. Starts the servers
    //
    // All from configuration!
    AgentBuilder::from_file("examples/auto_storage_sqlx.toml")?
        .with_handler(PersistentEchoHandler)
        .build_with_auto_storage_and_migrations(migrations)
        .await?
        .run()
        .await
        .map_err(|e| BuildError::RuntimeError(e.to_string()))?;

    Ok(())
}
