use a2a_rs::adapter::{
    BearerTokenAuthenticator, ConnectRpcAdapter, HttpPushNotificationSender, HttpServer,
    InMemoryTaskStorage, SimpleAgentInfo,
};
use a2a_rs::port::{AsyncNotificationManager, AsyncTaskLifecycle, AsyncTaskQuery};

// SQLx storage support (feature-gated)
#[cfg(feature = "sqlx")]
use a2a_rs::adapter::storage::SqlxTaskStorage;

use super::config::{AuthConfig, ServerConfig, StorageConfig};
use super::handler::ReimbursementHandler;

/// Modern A2A server setup using ReimbursementHandler
pub struct ReimbursementServer {
    config: ServerConfig,
}

impl ReimbursementServer {
    /// Create a new modern reimbursement server with default config
    pub fn new(host: String, port: u16) -> Self {
        let config = ServerConfig {
            host,
            http_port: port,
            ws_port: port + 1,
            storage: StorageConfig::default(),
            auth: AuthConfig::default(),
        };
        Self { config }
    }

    /// Create server from config
    pub fn from_config(config: ServerConfig) -> Self {
        Self { config }
    }

    /// Create in-memory storage
    fn create_in_memory_storage(&self) -> InMemoryTaskStorage {
        tracing::info!("Using in-memory storage with push notification support");
        let push_sender = HttpPushNotificationSender::new()
            .with_timeout(30)
            .with_max_retries(3);
        InMemoryTaskStorage::with_push_sender(push_sender)
    }

    #[cfg(feature = "sqlx")]
    /// Create SQLx storage (only available with sqlx feature)
    async fn create_sqlx_storage(
        &self,
        url: &str,
        _max_connections: u32,
        enable_logging: bool,
    ) -> Result<SqlxTaskStorage, Box<dyn std::error::Error>> {
        tracing::info!(
            "Using SQLx storage with URL: {} and push notification support",
            url
        );
        if enable_logging {
            tracing::info!("SQL query logging enabled");
        }

        // Include reimbursement-specific migrations
        let reimbursement_migrations = &[include_str!(
            "../../../migrations/001_create_reimbursements.sql"
        )];

        // SqlxTaskStorage uses HttpPushNotificationSender by default
        let storage = SqlxTaskStorage::with_migrations(url, reimbursement_migrations)
            .await
            .map_err(|e| format!("Failed to create SQLx storage: {}", e))?;
        Ok(storage)
    }

    /// Start the HTTP server
    pub async fn start_http(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.config.storage {
            StorageConfig::InMemory => {
                let storage = self.create_in_memory_storage();
                self.start(storage).await
            }
            #[cfg(feature = "sqlx")]
            StorageConfig::Sqlx {
                url,
                max_connections,
                enable_logging,
            } => {
                let storage = self
                    .create_sqlx_storage(url, *max_connections, *enable_logging)
                    .await?;
                self.start(storage).await
            }
            #[cfg(not(feature = "sqlx"))]
            StorageConfig::Sqlx { .. } => {
                Err("SQLx storage requested but 'sqlx' feature is not enabled.".into())
            }
        }
    }

    /// Start HTTP server
    pub async fn start<S>(&self, storage: S) -> Result<(), Box<dyn std::error::Error>>
    where
        S: AsyncTaskLifecycle
            + AsyncTaskQuery
            + AsyncNotificationManager
            + a2a_rs::port::AsyncStreamingHandler
            + Clone
            + Send
            + Sync
            + 'static,
    {
        // Create message handler with storage for history management
        let message_handler = ReimbursementHandler::new(storage.clone());
        self.start_with_handler(message_handler, storage).await
    }

    /// Start HTTP server with specific handler
    async fn start_with_handler<S, H>(
        &self,
        message_handler: H,
        storage: S,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        S: AsyncTaskLifecycle
            + AsyncTaskQuery
            + AsyncNotificationManager
            + a2a_rs::port::AsyncStreamingHandler
            + Clone
            + Send
            + Sync
            + 'static,
        H: a2a_rs::port::message_handler::AsyncMessageHandler + Clone + Send + Sync + 'static,
    {
        // Create agent info with reimbursement capabilities
        let agent_info = SimpleAgentInfo::new(
            "Reimbursement Agent".to_string(),
            format!("http://{}:{}", self.config.host, self.config.http_port),
        )
        .with_description("An intelligent agent that handles employee reimbursement requests, from form generation to approval processing.".to_string())
        .with_provider(
            "Example Organization".to_string(),
            "https://example.org".to_string(),
        )
        .with_documentation_url("https://example.org/docs/reimbursement-agent".to_string())
        .with_streaming()
        .with_push_notifications()
        .with_state_transition_history()
        .with_authenticated_extended_card()
        .add_comprehensive_skill(
            "process_reimbursement".to_string(),
            "Process Reimbursement".to_string(),
            Some("Helps with the reimbursement process for users given the amount and purpose of the reimbursement. Generates forms, validates submissions, and processes approvals.".to_string()),
            Some(vec![
                "reimbursement".to_string(),
                "expense".to_string(),
                "finance".to_string(),
                "forms".to_string(),
            ]),
            Some(vec![
                "Can you reimburse me $20 for my lunch with the clients?".to_string(),
                "I need to submit a reimbursement for $150 for office supplies".to_string(),
                "Process my travel expense of $500 for the conference".to_string(),
            ]),
            Some(vec!["text".to_string(), "data".to_string()]),
            Some(vec!["text".to_string(), "data".to_string()]),
        );

        // Create processor with separate handlers and agent info
        let processor = ConnectRpcAdapter::new(
            message_handler,
            storage.clone(), // storage implements AsyncTaskLifecycle + AsyncTaskQuery
            storage,         // storage also implements AsyncNotificationManager
            agent_info.clone(),
        );

        // Create HTTP server
        let bind_address = format!("{}:{}", self.config.host, self.config.http_port);

        println!(
            "🌐 Starting HTTP reimbursement server on {}:{}",
            self.config.host, self.config.http_port
        );
        println!(
            "📋 Agent card: http://{}:{}/agent-card",
            self.config.host, self.config.http_port
        );
        println!(
            "🛠️  Skills: http://{}:{}/skills",
            self.config.host, self.config.http_port
        );

        match &self.config.storage {
            StorageConfig::InMemory => println!("💾 Storage: In-memory (non-persistent)"),
            StorageConfig::Sqlx { url, .. } => println!("💾 Storage: SQLx ({})", url),
        }

        match &self.config.auth {
            AuthConfig::None => {
                println!("🔓 Authentication: None (public access)");

                // Create server without authentication
                let server = HttpServer::new(processor, agent_info, bind_address);
                server
                    .start()
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
            }
            AuthConfig::BearerToken { tokens, format } => {
                println!(
                    "🔐 Authentication: Bearer token ({} token(s){})",
                    tokens.len(),
                    format
                        .as_ref()
                        .map(|f| format!(", format: {}", f))
                        .unwrap_or_default()
                );

                let authenticator = BearerTokenAuthenticator::new(tokens.clone());
                let server =
                    HttpServer::with_auth(processor, agent_info, bind_address, authenticator);
                server
                    .start()
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
            }
            AuthConfig::ApiKey {
                keys,
                location,
                name,
            } => {
                println!(
                    "🔐 Authentication: API key ({} {}, {} key(s))",
                    location,
                    name,
                    keys.len()
                );
                println!("⚠️  API key authentication not yet supported, using no authentication");

                // Create server without authentication
                let server = HttpServer::new(processor, agent_info, bind_address);
                server
                    .start()
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
            }
        }
    }
}
