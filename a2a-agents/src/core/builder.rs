//! Agent builder for declarative agent construction
//!
//! Provides a fluent API for building agents from configuration files
//! or programmatically with minimal boilerplate.

use crate::core::config::{AgentConfig, ConfigError, StorageConfig};
use crate::core::server::AgentServer;
use a2a_rs::domain::{A2AError, ContextId, Task, TaskId, TaskPushNotificationConfig, TaskState};
use a2a_rs::port::{
    AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskLifecycle,
    AsyncTaskQuery,
};
use a2a_rs::{HttpPushNotificationSender, InMemoryTaskStorage};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

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
impl AsyncTaskLifecycle for AutoStorage {
    async fn create(&self, id: &TaskId, context_id: &ContextId) -> Result<Task, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.create(id, context_id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.create(id, context_id).await,
        }
    }

    async fn get(&self, id: &TaskId, history_length: Option<u32>) -> Result<Task, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.get(id, history_length).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.get(id, history_length).await,
        }
    }

    async fn update_status(
        &self,
        id: &TaskId,
        state: TaskState,
        message: Option<a2a_rs::domain::Message>,
    ) -> Result<Task, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.update_status(id, state, message).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.update_status(id, state, message).await,
        }
    }

    async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.cancel(id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.cancel(id).await,
        }
    }

    async fn exists(&self, id: &TaskId) -> Result<bool, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.exists(id).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.exists(id).await,
        }
    }
}

#[async_trait]
impl AsyncTaskQuery for AutoStorage {
    async fn list(
        &self,
        params: &a2a_rs::domain::ListTasksParams,
    ) -> Result<a2a_rs::domain::ListTasksResult, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.list(params).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.list(params).await,
        }
    }
}

#[async_trait]
impl AsyncNotificationManager for AutoStorage {
    async fn set_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.set_config(config).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.set_config(config).await,
        }
    }

    async fn get_config(
        &self,
        params: &a2a_rs::domain::GetTaskPushNotificationConfigParams,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.get_config(params).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.get_config(params).await,
        }
    }

    async fn list_configs(
        &self,
        params: &a2a_rs::domain::ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.list_configs(params).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.list_configs(params).await,
        }
    }

    async fn delete_config(
        &self,
        params: &a2a_rs::domain::DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.delete_config(params).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.delete_config(params).await,
        }
    }
}

impl AutoStorage {
    /// Create auto storage from server configuration
    pub async fn from_config(config: &StorageConfig) -> Result<Self, BuildError> {
        match config {
            StorageConfig::InMemory => {
                let push_sender = a2a_rs::adapter::HttpPushNotificationSender::new()
                    .with_timeout(30)
                    .with_max_retries(3);
                let storage = a2a_rs::InMemoryTaskStorage::with_push_sender(push_sender);
                Ok(AutoStorage::InMemory(storage))
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

                let storage = a2a_rs::adapter::storage::SqlxTaskStorage::new(url)
                    .await
                    .map_err(|e| {
                        BuildError::StorageError(format!("Failed to create SQLx storage: {}", e))
                    })?;

                Ok(AutoStorage::Sqlx(storage))
            }
            #[cfg(not(feature = "sqlx"))]
            StorageConfig::Sqlx { .. } => Err(BuildError::StorageError(
                "SQLx storage requested but 'sqlx' feature is not enabled".to_string(),
            )),
        }
    }

    /// Hand out the inner store's push notifier (shares its config registry).
    pub fn push_notifier(&self) -> Arc<dyn a2a_rs::port::AsyncPushNotifier> {
        match self {
            AutoStorage::InMemory(s) => s.push_notifier(),
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.push_notifier(),
        }
    }
}

/// Builder for creating A2A agents with declarative configuration
pub struct AgentBuilder<H = (), S = ()> {
    config: AgentConfig,
    handler: Option<H>,
    storage: Option<S>,
    streaming: Option<Arc<dyn AsyncStreamingHandler>>,
}

impl AgentBuilder<(), ()> {
    /// Create a new builder from a TOML configuration file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let config = AgentConfig::from_file(path)?;
        Ok(Self {
            config,
            handler: None,
            storage: None,
            streaming: None,
        })
    }

    /// Create a new builder from a TOML string
    pub fn from_toml(toml: &str) -> Result<Self, ConfigError> {
        let config = AgentConfig::from_toml(toml)?;
        Ok(Self {
            config,
            handler: None,
            storage: None,
            streaming: None,
        })
    }

    /// Create a new builder with programmatic configuration
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            handler: None,
            storage: None,
            streaming: None,
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
            streaming: self.streaming,
        }
    }

    /// Set custom storage for this agent
    pub fn with_storage<NewS>(self, storage: NewS) -> AgentBuilder<H, NewS>
    where
        NewS: AsyncTaskLifecycle
            + AsyncTaskQuery
            + AsyncNotificationManager
            + Clone
            + Send
            + Sync
            + 'static,
    {
        AgentBuilder {
            config: self.config,
            handler: self.handler,
            storage: Some(storage),
            streaming: self.streaming,
        }
    }

    /// Attach a shared streaming backend for real-time updates.
    ///
    /// Pass the *same* [`AsyncStreamingHandler`] instance your handler
    /// broadcasts to (clones of an `InMemoryStreamingHandler` share their
    /// subscriber registry). The built [`AgentServer`] injects it into the
    /// transport so `tasks/subscribe` SSE streams observe those broadcasts —
    /// without it, the transport defaults to a no-op and updates never reach
    /// clients.
    pub fn with_streaming(mut self, streaming: impl AsyncStreamingHandler + 'static) -> Self {
        self.streaming = Some(Arc::new(streaming));
        self
    }

    /// Access the configuration
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Build this agent's [`AgentCard`](a2a_rs::domain::AgentCard) from its
    /// configuration, without starting a server. Used to self-register the
    /// agent with an [`AgentRegistry`](crate::registry::AgentRegistry) before it
    /// runs, so peers can discover it by skill.
    pub async fn agent_card(
        &self,
    ) -> Result<a2a_rs::domain::AgentCard, a2a_rs::domain::A2AError> {
        use a2a_rs::services::AgentInfoProvider;
        crate::core::server::agent_info_from_config(&self.config, self.config.agent_url())
            .get_agent_card()
            .await
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
    S: AsyncTaskLifecycle
        + AsyncTaskQuery
        + AsyncNotificationManager
        + Clone
        + Send
        + Sync
        + 'static,
{
    /// Build the agent runtime
    pub fn build(self) -> Result<AgentServer<H, S>, BuildError> {
        let handler = self.handler.ok_or(BuildError::MissingHandler)?;
        let storage = self.storage.ok_or(BuildError::MissingStorage)?;

        let mut runtime = AgentServer::new(self.config, Arc::new(handler), Arc::new(storage));
        if let Some(streaming) = self.streaming {
            runtime = runtime.with_streaming(streaming);
        }
        Ok(runtime)
    }
}

impl<H> AgentBuilder<H, ()>
where
    H: AsyncMessageHandler + Clone + Send + Sync + 'static,
{
    /// Create storage from the configuration
    /// This is a convenience method that automatically creates the appropriate storage
    /// based on what's configured in the TOML file
    pub async fn build_with_auto_storage(self) -> Result<AgentServer<H, AutoStorage>, BuildError> {
        let handler = self.handler.ok_or(BuildError::MissingHandler)?;
        let streaming = self.streaming;

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

        let mut runtime = AgentServer::new(self.config, Arc::new(handler), Arc::new(storage));
        if let Some(streaming) = streaming {
            runtime = runtime.with_streaming(streaming);
        }
        Ok(runtime)
    }

    /// Create storage from configuration with custom migrations
    /// This is useful when you need to run agent-specific database migrations
    #[cfg(feature = "sqlx")]
    pub async fn build_with_auto_storage_and_migrations(
        self,
        migrations: &'static [&'static str],
    ) -> Result<AgentServer<H, AutoStorage>, BuildError> {
        let handler = self.handler.ok_or(BuildError::MissingHandler)?;
        let streaming = self.streaming;

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

        let mut runtime = AgentServer::new(self.config, Arc::new(handler), Arc::new(storage));
        if let Some(streaming) = streaming {
            runtime = runtime.with_streaming(streaming);
        }
        Ok(runtime)
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
