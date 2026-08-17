//! Agent builder for declarative agent construction
//!
//! Provides a fluent API for building agents from configuration files
//! or programmatically with minimal boilerplate.

use crate::core::config::{AgentConfig, ConfigError, StorageConfig};
use crate::core::server::AgentServer;
use a2a_rs::domain::{
    A2AError, ContextId, ContextState, Conversation, Digest, StateKey, Task, TaskId,
    TaskPushNotificationConfig, TaskState,
};
use a2a_rs::port::{
    AsyncContextStateStore, AsyncConversationStore, AsyncMessageHandler, AsyncNotificationManager,
    AsyncPushNotifier, AsyncStreamingHandler, AsyncTaskLifecycle, AsyncTaskQuery,
};
use a2a_rs::{HttpPushNotificationSender, InMemoryStreamingHandler, InMemoryTaskStorage};
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

/// Both memory ports, so what an agent remembers goes wherever
/// `[server.storage]` says.
///
/// Without these the LLM handler could only be given a concrete
/// `InMemoryTaskStorage`, which is what it was given — so an agent configured
/// for `type = "sqlx"` served its tasks from the database and kept its
/// conversation in the process, and forgot it on the restart the control plane
/// performs on purpose.
#[async_trait]
impl AsyncConversationStore for AutoStorage {
    async fn load(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Conversation, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.load(context_id, caller, limit).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.load(context_id, caller, limit).await,
        }
    }

    async fn compact(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        digest: Digest,
    ) -> Result<(), A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.compact(context_id, caller, digest).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.compact(context_id, caller, digest).await,
        }
    }
}

#[async_trait]
impl AsyncContextStateStore for AutoStorage {
    async fn load_state(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
    ) -> Result<ContextState, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.load_state(context_id, caller).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.load_state(context_id, caller).await,
        }
    }

    async fn remember(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        key: &StateKey,
        value: &str,
    ) -> Result<(), A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.remember(context_id, caller, key, value).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.remember(context_id, caller, key, value).await,
        }
    }

    async fn forget(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        key: &StateKey,
    ) -> Result<bool, A2AError> {
        match self {
            AutoStorage::InMemory(s) => s.forget(context_id, caller, key).await,
            #[cfg(feature = "sqlx")]
            AutoStorage::Sqlx(s) => s.forget(context_id, caller, key).await,
        }
    }
}

impl AutoStorage {
    /// Create auto storage from server configuration
    pub async fn from_config(config: &StorageConfig) -> Result<Self, BuildError> {
        Self::from_config_with_migrations(config, &[]).await
    }

    /// Create auto storage from server configuration, with agent-specific
    /// migrations run after the framework's own.
    ///
    /// In-memory storage has no schema, so migrations are dropped with a
    /// warning rather than failing the build — the agent still runs.
    pub async fn from_config_with_migrations(
        config: &StorageConfig,
        migrations: &[&str],
    ) -> Result<Self, BuildError> {
        match config {
            StorageConfig::InMemory => {
                if !migrations.is_empty() {
                    tracing::warn!(
                        "Migrations provided but using in-memory storage - migrations ignored"
                    );
                }
                let push_sender = HttpPushNotificationSender::new()
                    .with_timeout(30)
                    .with_max_retries(3);
                Ok(AutoStorage::InMemory(
                    InMemoryTaskStorage::with_push_sender(push_sender),
                ))
            }
            #[cfg(feature = "sqlx")]
            StorageConfig::Sqlx {
                url,
                max_connections,
                enable_logging,
            } => {
                let storage = SqlxTaskStorage::builder(url)
                    .max_connections(*max_connections)
                    .log_statements(*enable_logging)
                    .migrations(migrations)
                    .connect()
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
    pub async fn agent_card(&self) -> Result<a2a_rs::domain::AgentCard, a2a_rs::domain::A2AError> {
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
    /// Build the storage `[server.storage]` names and serve `handler` over it.
    ///
    /// For a handler that holds no ports of its own — the echo quick start, and
    /// anything else that answers from the message alone. A handler that takes
    /// storage, streaming or push wants [`build_wired`](AgentBuilder::build_wired)
    /// instead: it hands the handler the same instances the transport gets,
    /// which this method cannot do because the handler was already built by the
    /// time it is called.
    pub async fn build_with_auto_storage(self) -> Result<AgentServer<H, AutoStorage>, BuildError> {
        let handler = self.handler.ok_or(BuildError::MissingHandler)?;
        let streaming = self.streaming;

        let storage = AutoStorage::from_config(&self.config.server.storage).await?;

        let mut runtime = AgentServer::new(self.config, Arc::new(handler), Arc::new(storage));
        if let Some(streaming) = streaming {
            runtime = runtime.with_streaming(streaming);
        }
        Ok(runtime)
    }

    /// Create storage from configuration with custom migrations
    /// This is useful when you need to run agent-specific database migrations
    pub async fn build_with_auto_storage_and_migrations(
        self,
        migrations: &[&str],
    ) -> Result<AgentServer<H, AutoStorage>, BuildError> {
        let handler = self.handler.ok_or(BuildError::MissingHandler)?;
        let streaming = self.streaming;

        let storage =
            AutoStorage::from_config_with_migrations(&self.config.server.storage, migrations)
                .await?;

        let mut runtime = AgentServer::new(self.config, Arc::new(handler), Arc::new(storage));
        if let Some(streaming) = streaming {
            runtime = runtime.with_streaming(streaming);
        }
        Ok(runtime)
    }
}

/// The collaborators a handler is built from, assembled once from the config.
///
/// Handed to [`AgentBuilder::build_wired`]'s closure so a handler picks what it
/// needs without also deciding where any of it comes from.
pub struct AgentPorts {
    /// Task persistence, from `[server.storage]`. Also the conversation and the
    /// state bag for a handler that reads them back — the same rows, which is
    /// what stops "the transcript" and "the task history" being two records
    /// that can disagree.
    pub storage: AutoStorage,
    /// The streaming backend. The handler broadcasts to it and the transport
    /// subscribes through it; clones share one subscriber registry, so it has
    /// to be this instance on both sides or an SSE client sees nothing.
    pub streaming: InMemoryStreamingHandler,
    /// Webhook delivery, taken from the storage's own registry so a config
    /// registered over `tasks/pushNotificationConfig/set` is the one called.
    pub push: Arc<dyn AsyncPushNotifier>,
}

impl AgentBuilder<(), ()> {
    /// Assemble the ports from the config, build the handler out of them, and
    /// wire every one of them into the server.
    ///
    /// The one path from a config to a running agent. Assembling them per
    /// handler is how the LLM handler came to ignore `[server.storage]` and keep
    /// its conversation in the process, and how the reimbursement handler came
    /// to broadcast into a streaming backend the transport never subscribed to.
    /// Neither shows up in a test of the handler, the config or the store — only
    /// in the wire between them, and nothing tests wires.
    ///
    /// A streaming backend set with [`with_streaming`](AgentBuilder::with_streaming)
    /// is replaced: the handler and the transport have to hold the same
    /// instance, and only this method knows which one the handler got.
    pub async fn build_wired<H>(
        self,
        make_handler: impl FnOnce(&AgentPorts) -> H,
    ) -> Result<AgentServer<H, AutoStorage>, BuildError>
    where
        H: AsyncMessageHandler + Clone + Send + Sync + 'static,
    {
        let storage = AutoStorage::from_config(&self.config.server.storage).await?;
        let push = storage.push_notifier();
        let ports = AgentPorts {
            storage,
            streaming: InMemoryStreamingHandler::new(),
            push,
        };
        let handler = make_handler(&ports);

        let AgentPorts {
            storage, streaming, ..
        } = ports;
        Ok(
            AgentServer::new(self.config, Arc::new(handler), Arc::new(storage))
                .with_streaming(Arc::new(streaming)),
        )
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
