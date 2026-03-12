//! Agent builder for declarative agent construction
//!
//! Provides a fluent API for building agents from configuration files
//! or programmatically with minimal boilerplate.

#[cfg(feature = "mcp-client")]
use crate::core::McpClientManager;
use crate::core::config::{AgentConfig, ConfigError, StorageConfig};
use crate::core::runtime::AgentRuntime;
use a2a_rs::domain::{
    A2AError, Task, TaskArtifactUpdateEvent, TaskPushNotificationConfig, TaskState,
    TaskStatusUpdateEvent,
};
use a2a_rs::port::{
    AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskManager,
    StreamingSubscriber, UpdateEvent,
};
use a2a_rs::{HttpPushNotificationSender, InMemoryTaskStorage};
use async_trait::async_trait;
use futures::Stream;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
#[cfg(feature = "mcp-client")]
use tracing::info;

#[cfg(feature = "sqlx")]
use a2a_rs::adapter::storage::SqlxTaskStorage;

/// Storage wrapper that can hold either in-memory or SQLx storage
/// This allows us to return different storage types from the builder
#[derive(Clone)]
pub enum AutoStorage {
    InMemory(InMemoryTaskStorage),
    #[cfg(feature = "sqlx")]
    Sqlx(SqlxTaskStorage),
}

#[async_trait]
impl AsyncTaskManager for AutoStorage {
    async fn create_task(&self, task_id: &str, context_id: &str) -> Result<Task, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.create_task(task_id, context_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.create_task(task_id, context_id).await,
        }
    }

    async fn get_task(&self, task_id: &str, history_length: Option<u32>) -> Result<Task, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.get_task(task_id, history_length).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.get_task(task_id, history_length).await,
        }
    }

    async fn update_task_status(
        &self,
        task_id: &str,
        state: TaskState,
        message: Option<a2a_rs::domain::Message>,
    ) -> Result<Task, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.update_task_status(task_id, state, message).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.update_task_status(task_id, state, message).await,
        }
    }

    async fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.cancel_task(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.cancel_task(task_id).await,
        }
    }

    async fn task_exists(&self, task_id: &str) -> Result<bool, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.task_exists(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.task_exists(task_id).await,
        }
    }
}

#[async_trait]
impl AsyncNotificationManager for AutoStorage {
    async fn set_task_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.set_task_notification(config).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.set_task_notification(config).await,
        }
    }

    async fn get_task_notification(
        &self,
        task_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.get_task_notification(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.get_task_notification(task_id).await,
        }
    }

    async fn remove_task_notification(&self, task_id: &str) -> Result<(), A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.remove_task_notification(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.remove_task_notification(task_id).await,
        }
    }
}

#[async_trait]
impl AsyncStreamingHandler for AutoStorage {
    async fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn StreamingSubscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.add_status_subscriber(task_id, subscriber).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.add_status_subscriber(task_id, subscriber).await,
        }
    }

    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn StreamingSubscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.add_artifact_subscriber(task_id, subscriber).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.add_artifact_subscriber(task_id, subscriber).await,
        }
    }

    async fn remove_subscription(&self, subscription_id: &str) -> Result<(), A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.remove_subscription(subscription_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.remove_subscription(subscription_id).await,
        }
    }

    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.remove_task_subscribers(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.remove_task_subscribers(task_id).await,
        }
    }

    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.get_subscriber_count(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.get_subscriber_count(task_id).await,
        }
    }

    async fn broadcast_status_update(
        &self,
        task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.broadcast_status_update(task_id, update).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.broadcast_status_update(task_id, update).await,
        }
    }

    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.broadcast_artifact_update(task_id, update).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.broadcast_artifact_update(task_id, update).await,
        }
    }

    async fn status_update_stream(
        &self,
        task_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>, A2AError>
    {
        match self {
            AutoStorage::InMemory(s) => s.status_update_stream(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.status_update_stream(task_id).await,
        }
    }

    async fn artifact_update_stream(
        &self,
        task_id: &str,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        match self {
            AutoStorage::InMemory(s) => s.artifact_update_stream(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.artifact_update_stream(task_id).await,
        }
    }

    async fn combined_update_stream(
        &self,
        task_id: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<UpdateEvent, A2AError>> + Send>>, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.combined_update_stream(task_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.combined_update_stream(task_id).await,
        }
    }
}

/// Builder for creating A2A agents with declarative configuration
pub struct AgentBuilder<H = (), S = ()> {
    config: AgentConfig,
    handler: Option<H>,
    storage: Option<S>,
}

impl AgentBuilder<(), ()> {
    /// Create a new builder from a TOML configuration file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let config = AgentConfig::from_file(path)?;
        Ok(Self {
            config,
            handler: None,
            storage: None,
        })
    }

    /// Create a new builder from a TOML string
    pub fn from_toml(toml: &str) -> Result<Self, ConfigError> {
        let config = AgentConfig::from_toml(toml)?;
        Ok(Self {
            config,
            handler: None,
            storage: None,
        })
    }

    /// Create a new builder with programmatic configuration
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            handler: None,
            storage: None,
        }
    }
}

impl<H, S> AgentBuilder<H, S> {
    /// Set the message handler for this agent
    pub fn with_handler<NewH>(self, handler: NewH) -> AgentBuilder<NewH, S>
    where
        NewH: AsyncMessageHandler + Clone + Send + Sync + 'static,
    {
        AgentBuilder {
            config: self.config,
            handler: Some(handler),
            storage: self.storage,
        }
    }

    /// Set custom storage for this agent
    pub fn with_storage<NewS>(self, storage: NewS) -> AgentBuilder<H, NewS>
    where
        NewS: AsyncTaskManager + AsyncNotificationManager + Clone + Send + Sync + 'static,
    {
        AgentBuilder {
            config: self.config,
            handler: self.handler,
            storage: Some(storage),
        }
    }

    /// Access the configuration
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Modify the configuration
    pub fn with_config<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut AgentConfig),
    {
        f(&mut self.config);
        self
    }
}

impl<H, S> AgentBuilder<H, S>
where
    H: AsyncMessageHandler + Clone + Send + Sync + 'static,
    S: AsyncTaskManager + AsyncNotificationManager + Clone + Send + Sync + 'static,
{
    /// Build the agent runtime
    pub fn build(self) -> Result<AgentRuntime<H, S>, BuildError> {
        let handler = self.handler.ok_or(BuildError::MissingHandler)?;
        let storage = self.storage.ok_or(BuildError::MissingStorage)?;

        Ok(AgentRuntime::new(
            self.config,
            Arc::new(handler),
            Arc::new(storage),
        ))
    }
}

impl<H> AgentBuilder<H, ()>
where
    H: AsyncMessageHandler + Clone + Send + Sync + 'static,
{
    /// Create storage from the configuration
    /// This is a convenience method that automatically creates the appropriate storage
    /// based on what's configured in the TOML file
    pub async fn build_with_auto_storage(self) -> Result<AgentRuntime<H, AutoStorage>, BuildError> {
        let handler = self.handler.ok_or(BuildError::MissingHandler)?;

        let storage = match &self.config.server.storage {
            StorageConfig::InMemory => {
                let push_sender = HttpPushNotificationSender::new()
                    .with_timeout(30)
                    .with_max_retries(3);
                let storage = InMemoryTaskStorage::with_push_sender(push_sender);
                AutoStorage::InMemory(storage)
            }
            #[cfg(feature = "sqlx")]
            StorageConfig::Sqlx {
                url,
                enable_logging,
                ..
            } => {
                if *enable_logging {
                    tracing::info!("SQL query logging enabled");
                }

                let storage = SqlxTaskStorage::new(url).await.map_err(|e| {
                    BuildError::StorageError(format!("Failed to create SQLx storage: {}", e))
                })?;

                AutoStorage::Sqlx(storage)
            }
            #[cfg(not(feature = "sqlx"))]
            StorageConfig::Sqlx { .. } => {
                return Err(BuildError::StorageError(
                    "SQLx storage requested but 'sqlx' feature is not enabled".to_string(),
                ));
            }
        };

        // Initialize MCP client if configured
        #[cfg(feature = "mcp-client")]
        if self.config.features.mcp_client.enabled {
            info!("Initializing MCP client...");
            let mcp_client = McpClientManager::new();

            // Initialize connections to configured servers
            if let Err(e) = mcp_client
                .initialize(&self.config.features.mcp_client)
                .await
            {
                return Err(BuildError::RuntimeError(format!(
                    "Failed to initialize MCP client: {}",
                    e
                )));
            }

            return Ok(AgentRuntime::with_mcp_client(
                self.config,
                Arc::new(handler),
                Arc::new(storage),
                mcp_client,
            ));
        }

        Ok(AgentRuntime::new(
            self.config,
            Arc::new(handler),
            Arc::new(storage),
        ))
    }

    /// Create storage from configuration with custom migrations
    /// This is useful when you need to run agent-specific database migrations
    #[cfg(feature = "sqlx")]
    pub async fn build_with_auto_storage_and_migrations(
        self,
        migrations: &'static [&'static str],
    ) -> Result<AgentRuntime<H, AutoStorage>, BuildError> {
        let handler = self.handler.ok_or(BuildError::MissingHandler)?;

        let storage = match &self.config.server.storage {
            StorageConfig::InMemory => {
                tracing::warn!(
                    "Migrations provided but using in-memory storage - migrations ignored"
                );
                let push_sender = HttpPushNotificationSender::new()
                    .with_timeout(30)
                    .with_max_retries(3);
                let storage = InMemoryTaskStorage::with_push_sender(push_sender);
                AutoStorage::InMemory(storage)
            }
            StorageConfig::Sqlx {
                url,
                enable_logging,
                ..
            } => {
                if *enable_logging {
                    tracing::info!("SQL query logging enabled");
                }

                let storage = SqlxTaskStorage::with_migrations(url, migrations)
                    .await
                    .map_err(|e| {
                        BuildError::StorageError(format!("Failed to create SQLx storage: {}", e))
                    })?;

                AutoStorage::Sqlx(storage)
            }
        };

        // Initialize MCP client if configured
        #[cfg(feature = "mcp-client")]
        if self.config.features.mcp_client.enabled {
            info!("Initializing MCP client...");
            let mcp_client = McpClientManager::new();

            // Initialize connections to configured servers
            if let Err(e) = mcp_client
                .initialize(&self.config.features.mcp_client)
                .await
            {
                return Err(BuildError::RuntimeError(format!(
                    "Failed to initialize MCP client: {}",
                    e
                )));
            }

            return Ok(AgentRuntime::with_mcp_client(
                self.config,
                Arc::new(handler),
                Arc::new(storage),
                mcp_client,
            ));
        }

        Ok(AgentRuntime::new(
            self.config,
            Arc::new(handler),
            Arc::new(storage),
        ))
    }
}

/// Errors that can occur during agent building
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("Handler must be set before building")]
    MissingHandler,

    #[error("Storage must be set before building")]
    MissingStorage,

    #[error("Configuration error: {0}")]
    ConfigError(#[from] ConfigError),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Runtime error: {0}")]
    RuntimeError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_from_toml() {
        let toml = r#"
            [agent]
            name = "Test Agent"

            [server]
            http_port = 9000
        "#;

        let builder = AgentBuilder::from_toml(toml).unwrap();
        assert_eq!(builder.config().agent.name, "Test Agent");
        assert_eq!(builder.config().server.http_port, 9000);
    }

    #[test]
    fn test_builder_config_modification() {
        let toml = r#"
            [agent]
            name = "Test Agent"
        "#;

        let builder = AgentBuilder::from_toml(toml)
            .unwrap()
            .with_config(|config| {
                config.server.http_port = 7000;
            });

        assert_eq!(builder.config().server.http_port, 7000);
    }
}
