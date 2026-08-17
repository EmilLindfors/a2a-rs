//! SQLx-based task storage implementation.
//!
//! Persists tasks, history, push configs and conversations to SQLite or
//! PostgreSQL. The backend is chosen from the URL scheme at runtime, over sqlx's
//! `Any` driver, so both are served by one set of queries; the differences live
//! in [`Dialect`](super::dialect::Dialect).

#[cfg(feature = "sqlx-storage")]
use async_trait::async_trait;
#[cfg(feature = "sqlx-storage")]
use serde_json;
#[cfg(feature = "sqlx-storage")]
use sqlx::{
    AnyPool, ConnectOptions, Row,
    any::{AnyConnectOptions, AnyPoolOptions},
};
#[cfg(feature = "sqlx-storage")]
use std::{str::FromStr, time::Duration};

#[cfg(feature = "sqlx-storage")]
use crate::adapter::business::push_notification::{
    PushNotificationRegistry, PushNotificationSender,
};

#[cfg(feature = "sqlx-storage")]
#[cfg(feature = "http-client")]
use crate::adapter::business::push_notification::HttpPushNotificationSender;
#[cfg(feature = "sqlx-storage")]
#[cfg(not(feature = "http-client"))]
use crate::adapter::business::push_notification::NoopPushNotificationSender;

#[cfg(feature = "sqlx-storage")]
use crate::domain::{
    A2AError, ContextId, ContextState, Conversation, Digest, Message, Seq, SequencedMessage,
    StateKey, StateScope, Task, TaskId, TaskPushNotificationConfig, TaskState, TaskStateExt,
    TaskStatus, VersionedTask,
};
#[cfg(feature = "sqlx-storage")]
use crate::port::{
    AsyncContextStateStore, AsyncConversationStore, AsyncNotificationManager, AsyncPushNotifier,
    AsyncTaskLifecycle, AsyncTaskQuery, AsyncTaskVersioning, context_state::scope_key,
};

#[cfg(feature = "sqlx-storage")]
use std::sync::Arc;

#[cfg(feature = "sqlx-storage")]
/// SQLx-based task storage for persistent storage.
///
/// Persistence-only: streaming fan-out lives in
/// [`InMemoryStreamingHandler`](crate::adapter::InMemoryStreamingHandler) and
/// push-webhook delivery behind the [`AsyncPushNotifier`] port (handed out via
/// [`push_notifier`](Self::push_notifier)). The store still owns push-config
/// CRUD ([`AsyncNotificationManager`]) — that is config persistence.
pub struct SqlxTaskStorage {
    /// Database pool, over the driver the URL scheme selected.
    pool: AnyPool,
    /// Which SQL the queries are rendered in. Fixed at connect time from the
    /// same URL the pool was opened with.
    dialect: Dialect,
    /// Push notification registry (config store + delivery backend)
    push_notification_registry: Arc<PushNotificationRegistry>,
}

#[cfg(feature = "sqlx-storage")]
use super::database_config::DatabaseType;
#[cfg(feature = "sqlx-storage")]
use super::dialect::Dialect;

/// The task columns this store reads, spelled out rather than `SELECT *`.
///
/// Both tables carry timestamps the `Any` driver cannot decode — it handles
/// text, integers, floats, booleans and bytes — and a row is converted whole, so
/// selecting a column nobody reads fails the query on PostgreSQL.
#[cfg(feature = "sqlx-storage")]
const TASK_COLUMNS: &str = "id, context_id, status_state, status_message, metadata, artifacts";

/// How the pool is opened. Applied to both the main pool and the
/// one-connection migration pool, which connect to the same database.
#[cfg(feature = "sqlx-storage")]
struct PoolSettings {
    max_connections: u32,
    acquire_timeout: Duration,
    log_statements: bool,
}

#[cfg(feature = "sqlx-storage")]
impl Default for PoolSettings {
    /// sqlx's own pool defaults, so an unconfigured store is sized as it was
    /// before there was anything to configure. Statement logging is the one
    /// departure: sqlx logs every statement at `DEBUG` by default, and this
    /// crate makes that a choice (see [`SqlxStorageBuilder::log_statements`]).
    fn default() -> Self {
        Self {
            max_connections: 10,
            acquire_timeout: Duration::from_secs(30),
            log_statements: false,
        }
    }
}

#[cfg(feature = "sqlx-storage")]
impl PoolSettings {
    fn connect_options(&self, url: &str) -> Result<AnyConnectOptions, A2AError> {
        let options = AnyConnectOptions::from_str(url)
            .map_err(|e| A2AError::DatabaseError(format!("Invalid database URL '{url}': {e}")))?;

        Ok(if self.log_statements {
            options
        } else {
            options.disable_statement_logging()
        })
    }
}

/// Builds a [`SqlxTaskStorage`]: see [`SqlxTaskStorage::builder`].
#[cfg(feature = "sqlx-storage")]
pub struct SqlxStorageBuilder {
    url: String,
    pool: PoolSettings,
    push_sender: Option<Arc<dyn PushNotificationSender>>,
    additional_migrations: Vec<String>,
}

#[cfg(feature = "sqlx-storage")]
impl SqlxStorageBuilder {
    /// Take the URL and the pool settings from a [`DatabaseConfig`].
    pub fn from_config(config: &super::database_config::DatabaseConfig) -> Self {
        SqlxTaskStorage::builder(&config.url)
            .max_connections(config.max_connections)
            .acquire_timeout(Duration::from_secs(config.timeout_seconds))
            .log_statements(config.enable_logging)
    }

    /// Cap the connection pool. Defaults to 10, sqlx's own default.
    ///
    /// On PostgreSQL this is a share of a server-wide limit, so a fleet of
    /// agents against one server is the case worth setting it for.
    pub fn max_connections(mut self, max: u32) -> Self {
        self.pool.max_connections = max;
        self
    }

    /// How long a query waits for a free connection before failing. Defaults
    /// to 30 seconds, sqlx's own default.
    pub fn acquire_timeout(mut self, timeout: Duration) -> Self {
        self.pool.acquire_timeout = timeout;
        self
    }

    /// Log every statement the store executes, at `DEBUG` through the `log`
    /// crate (which `tracing-subscriber` bridges into tracing). Off by default.
    pub fn log_statements(mut self, log: bool) -> Self {
        self.pool.log_statements = log;
        self
    }

    /// Deliver push notifications through this sender rather than the default
    /// (HTTP with the `http-client` feature, a no-op without it).
    pub fn push_sender(mut self, sender: impl PushNotificationSender + 'static) -> Self {
        self.push_sender = Some(Arc::new(sender));
        self
    }

    /// Run these statements after the framework's own migrations.
    ///
    /// The caller's own SQL, run verbatim, so it has to be written in the
    /// dialect the URL selects.
    pub fn migrations<S: AsRef<str>>(mut self, migrations: impl IntoIterator<Item = S>) -> Self {
        self.additional_migrations
            .extend(migrations.into_iter().map(|s| s.as_ref().to_string()));
        self
    }

    /// Open the pool, migrate, and hand back the store.
    pub async fn connect(self) -> Result<SqlxTaskStorage, A2AError> {
        if self.pool.max_connections == 0 {
            return Err(A2AError::DatabaseError(
                "max_connections must be greater than 0; a pool that hands out no connections \
                 fails every query"
                    .to_string(),
            ));
        }

        let (pool, dialect) = SqlxTaskStorage::connect(&self.url, &self.pool).await?;
        SqlxTaskStorage::run_additional_migrations(&pool, &self.additional_migrations).await?;

        let push_registry = match self.push_sender {
            Some(sender) => PushNotificationRegistry::from_shared(sender),
            None => {
                #[cfg(feature = "http-client")]
                let sender = HttpPushNotificationSender::new();
                #[cfg(not(feature = "http-client"))]
                let sender = NoopPushNotificationSender::default();
                PushNotificationRegistry::new(sender)
            }
        };

        Ok(SqlxTaskStorage {
            pool,
            dialect,
            push_notification_registry: Arc::new(push_registry),
        })
    }
}

/// What the `contexts` row says about who may read a conversation.
///
/// The absence of a row is a third answer and is spelled `Option<ContextClaim>`
/// rather than a variant here: it is the one case the caller has to *act* on by
/// writing, and folding it in would let a call site treat "nothing holds this
/// yet" as a decision that had been made.
#[cfg(feature = "sqlx-storage")]
enum ContextClaim {
    /// A row with no owner — an agent running without an authenticator. Open to
    /// anyone.
    Open,
    /// Claimed by this principal on the first write, and never reassigned.
    Owner(String),
}

#[cfg(feature = "sqlx-storage")]
impl ContextClaim {
    fn verdict(&self, context_id: &str, caller: Option<&str>) -> Result<(), A2AError> {
        match self {
            Self::Open => Ok(()),
            Self::Owner(owner) if Some(owner.as_str()) == caller => Ok(()),
            Self::Owner(_) => Err(A2AError::ContextAccessDenied {
                context_id: context_id.to_string(),
            }),
        }
    }
}

#[cfg(feature = "sqlx-storage")]
impl SqlxTaskStorage {
    /// Resolve the URL to a dialect, or say why it cannot be.
    ///
    /// Three ways this fails, and they need different answers: an unrecognized
    /// scheme, a recognized one with no adapter behind it (MySQL), and a
    /// recognized one whose driver was not compiled in.
    fn dialect_for(database_url: &str) -> Result<Dialect, A2AError> {
        let Some(database_type) = DatabaseType::from_url(database_url) else {
            return Err(A2AError::DatabaseError(format!(
                "Unrecognized database URL scheme in '{database_url}'. Expected sqlite: or \
                 postgres:, e.g. 'sqlite::memory:' or 'postgres://user:pass@localhost/a2a'"
            )));
        };

        let Some(dialect) = Dialect::of(database_type) else {
            return Err(A2AError::DatabaseError(format!(
                "{database_type} is not supported by SqlxTaskStorage. It stores tasks in SQLite \
                 or PostgreSQL; there is no {database_type} schema."
            )));
        };

        if !database_type.is_feature_enabled() {
            return Err(A2AError::DatabaseError(format!(
                "{database_type} detected from URL '{database_url}', but the '{}' feature is not \
                 enabled. Add `features = [\"{}\"]` to your a2a-rs dependency.",
                database_type.feature_name(),
                database_type.feature_name(),
            )));
        }

        Ok(dialect)
    }

    /// Give an in-memory SQLite URL a name the whole pool can share.
    ///
    /// `sqlite::memory:` is an *anonymous* database, and sqlx names one by
    /// inventing `sqlx-in-memory-{n}` while parsing the URL. A typed
    /// `SqlitePool` parses once and every connection lands in the same one; the
    /// `Any` driver parses per connection, so a pool of ten would be ten empty
    /// databases and the second query would not see the first one's table.
    /// Pinning one name here keeps `sqlite::memory:` meaning what it means
    /// everywhere else — one database, private to this store.
    fn pooled_url(database_url: &str) -> std::borrow::Cow<'_, str> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

        let anonymous_memory = (database_url.contains(":memory:")
            || database_url.contains("mode=memory"))
            && !database_url.contains("cache=shared");
        if !anonymous_memory {
            return std::borrow::Cow::Borrowed(database_url);
        }

        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::borrow::Cow::Owned(format!(
            "sqlite:file:a2a-in-memory-{n}?mode=memory&cache=shared"
        ))
    }

    /// Open the pool and bring the schema up to date.
    async fn connect(
        database_url: &str,
        settings: &PoolSettings,
    ) -> Result<(AnyPool, Dialect), A2AError> {
        let dialect = Self::dialect_for(database_url)?;

        // The `Any` driver dispatches on the URL scheme at runtime, and it
        // panics if no driver was registered. Guarded by a `Once` inside sqlx,
        // so every constructor can call it.
        sqlx::any::install_default_drivers();

        let url = match dialect {
            Dialect::Sqlite => Self::pooled_url(database_url),
            Dialect::Postgres => std::borrow::Cow::Borrowed(database_url),
        };
        let pool = AnyPoolOptions::new()
            .max_connections(settings.max_connections)
            .acquire_timeout(settings.acquire_timeout)
            .connect_with(settings.connect_options(&url)?)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to connect to database: {e}")))?;

        // The migrations get a pool of their own, capped at one connection, and
        // that cap is what makes the lock possible: an advisory lock belongs to
        // a session, and a one-connection pool *is* a session — reachable
        // through `&pool`, which is the only way to execute anything here (see
        // `run_base_migrations`). Opened after the main pool so an in-memory
        // SQLite database, which lives only as long as something is connected to
        // it, is already held open by the pool that will keep using it.
        let migrations = AnyPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(settings.acquire_timeout)
            .connect_with(settings.connect_options(&url)?)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to open the migration connection: {e}"))
            })?;
        let migrated = Self::run_base_migrations(migrations.clone(), dialect).await;
        migrations.close().await;
        migrated?;

        Ok((pool, dialect))
    }

    /// Create a new SQLx task storage with the given database URL.
    ///
    /// The scheme picks the backend: `sqlite::memory:`, `sqlite:data.db`, or
    /// `postgres://user:pass@host/db`. Each needs its cargo feature (`sqlite`,
    /// `postgres`) compiled in, and the error says so when it is missing.
    ///
    /// Pool sizing, statement logging, a custom push sender and agent-specific
    /// migrations go through [`builder`](Self::builder).
    pub async fn new(database_url: &str) -> Result<Self, A2AError> {
        Self::builder(database_url).connect().await
    }

    /// Configure a store before opening it.
    ///
    /// ```no_run
    /// # use a2a_rs::adapter::storage::SqlxTaskStorage;
    /// # async fn f() -> Result<(), a2a_rs::domain::A2AError> {
    /// let storage = SqlxTaskStorage::builder("postgres://user:pass@localhost/a2a")
    ///     .max_connections(20)
    ///     .log_statements(true)
    ///     .connect()
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub fn builder(database_url: impl Into<String>) -> SqlxStorageBuilder {
        SqlxStorageBuilder {
            url: database_url.into(),
            pool: PoolSettings::default(),
            push_sender: None,
            additional_migrations: Vec::new(),
        }
    }

    /// Run base A2A framework migrations, in the dialect the URL selected.
    ///
    /// These re-run on every construction, so every file has to be idempotent —
    /// which is what the legacy-table probe and `tolerates_existing_column` are
    /// for. `raw_sql` rather than `query`: a migration file holds several
    /// statements, and PostgreSQL only accepts those unprepared.
    ///
    /// `pool` is the one-connection migration pool, so on PostgreSQL an
    /// advisory lock taken through it holds for everything that follows: a
    /// fleet starting together is the normal case for a shared database, and
    /// concurrent `CREATE TABLE IF NOT EXISTS` on related tables does not
    /// no-op — it deadlocks, or fails on the catalog's unique index.
    ///
    /// Everything runs through the pool and never on a borrowed connection.
    /// sqlx implements `Executor` for `&'c mut AnyConnection` at a single
    /// lifetime, so a future holding such a borrow cannot be proved `Send` by a
    /// caller that spawns — which `a2a up` does for every agent — and the whole
    /// construction path would stop compiling for anyone who spawns it.
    ///
    /// Owned `AnyPool` here and in every helper below, cloned per call — it is
    /// an `Arc` inside. A borrowed parameter would make each of these futures
    /// generic over that lifetime, which is the same shape callers cannot prove.
    async fn run_base_migrations(pool: AnyPool, dialect: Dialect) -> Result<(), A2AError> {
        if let Some(lock) = dialect.migration_lock() {
            sqlx::raw_sql(lock).execute(&pool).await.map_err(|e| {
                A2AError::DatabaseError(format!("Failed to take the migration lock: {e}"))
            })?;
        }
        // No explicit unlock: the caller closes this pool, which ends the
        // session, which releases the lock. An unlock statement would be one
        // more thing to get wrong on the error path.

        let [initial, push_configs, rest @ ..] = dialect.migrations();

        Self::run_migration(pool.clone(), initial).await?;
        // Between 001 and 002: 001 may have just created the v0.2 table, and 002
        // creates the one that replaces it.
        Self::drop_legacy_push_configs(pool.clone(), dialect).await?;
        Self::run_migration(pool.clone(), push_configs).await?;
        for migration in rest {
            Self::run_migration(pool.clone(), migration).await?;
        }

        // The 004 backfill. Guarded by `context_id IS NULL` rather than run only
        // when the column was just added: that is idempotent on both backends,
        // and on an already-migrated database it matches nothing.
        sqlx::raw_sql(
            "UPDATE task_history SET context_id = \
             (SELECT context_id FROM tasks WHERE tasks.id = task_history.task_id) \
             WHERE context_id IS NULL",
        )
        .execute(&pool)
        .await
        .map_err(|e| A2AError::DatabaseError(format!("Migration 004 backfill failed: {e}")))?;

        Self::drop_dead_context_state_column(pool.clone(), dialect).await;

        Ok(())
    }

    /// Drop `contexts.state`, which 005 created and nothing ever wrote.
    ///
    /// The state bag went to its own table in 006, so the column is dead on a
    /// database old enough to have it. Best effort on purpose: an unused column
    /// costs nothing, and `ALTER TABLE … DROP COLUMN` has enough conditions
    /// attached on SQLite that failing it must not stop an agent from starting.
    async fn drop_dead_context_state_column(pool: AnyPool, dialect: Dialect) {
        let probe = sqlx::query(dialect.dead_context_state_column_probe())
            .fetch_optional(&pool)
            .await;
        if !matches!(probe, Ok(Some(_))) {
            return;
        }

        if let Err(e) = sqlx::raw_sql("ALTER TABLE contexts DROP COLUMN state")
            .execute(&pool)
            .await
        {
            #[cfg(feature = "tracing")]
            tracing::debug!("left the unused contexts.state column in place: {e}");
            #[cfg(not(feature = "tracing"))]
            let _ = e;
        }
    }

    /// Run one migration file, once more if another process was running the
    /// same one.
    ///
    /// A shared database is the reason to run PostgreSQL at all, so several
    /// agents starting together is the normal case — and `CREATE TABLE IF NOT
    /// EXISTS` checks and creates in two steps, so the loser of that race sees
    /// the object appear in between and fails on the catalog's unique index
    /// rather than no-opping. By the retry the winner has finished and every
    /// statement in the file finds what it wanted already there.
    async fn run_migration(
        pool: AnyPool,
        migration: super::dialect::Migration,
    ) -> Result<(), A2AError> {
        let mut attempt = sqlx::raw_sql(migration.sql).execute(&pool).await;
        if attempt
            .as_ref()
            .err()
            .is_some_and(super::dialect::is_concurrent_ddl_conflict)
        {
            attempt = sqlx::raw_sql(migration.sql).execute(&pool).await;
        }

        match attempt {
            Ok(_) => Ok(()),
            // An `ALTER TABLE ADD COLUMN` this dialect cannot write
            // idempotently, run a second time. The column being there is the
            // outcome the migration wanted.
            Err(e)
                if migration.tolerates_existing_column
                    && e.to_string().contains("duplicate column name") =>
            {
                Ok(())
            }
            Err(e) => Err(A2AError::DatabaseError(format!(
                "Migration {} failed: {e}",
                migration.name
            ))),
        }
    }

    /// Drop the v0.2 push-config table, and only when it is still the v0.2 one.
    ///
    /// Migration 002 replaces that table, and it used to do the drop itself —
    /// but base migrations re-run on every startup, so every restart destroyed
    /// the push configs the agent had stored. The probe makes the drop happen
    /// once, on the database that actually needs it.
    async fn drop_legacy_push_configs(pool: AnyPool, dialect: Dialect) -> Result<(), A2AError> {
        let legacy = sqlx::query(dialect.legacy_push_config_probe())
            .fetch_optional(&pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to inspect push config table: {e}"))
            })?;

        if legacy.is_some() {
            sqlx::raw_sql("DROP TABLE IF EXISTS push_notification_configs")
                .execute(&pool)
                .await
                .map_err(|e| {
                    A2AError::DatabaseError(format!(
                        "Failed to drop the pre-v0.3 push config table: {e}"
                    ))
                })?;
        }
        Ok(())
    }

    /// Run additional migrations provided by the application
    async fn run_additional_migrations(
        pool: &AnyPool,
        migrations: &[String],
    ) -> Result<(), A2AError> {
        for (i, migration_sql) in migrations.iter().enumerate() {
            sqlx::raw_sql(migration_sql)
                .execute(pool)
                .await
                .map_err(|e| {
                    A2AError::DatabaseError(format!("Additional migration {} failed: {}", i + 1, e))
                })?;
        }
        Ok(())
    }

    /// Render a query for this store's backend.
    ///
    /// Every query in this file goes through here, which is where `?` becomes
    /// `$1..$n` on PostgreSQL.
    fn sql<'a>(&self, sql: &'a str) -> std::borrow::Cow<'a, str> {
        self.dialect.bind_params(sql)
    }

    /// Convert database row to Task
    fn row_to_task(row: &sqlx::any::AnyRow) -> Result<Task, A2AError> {
        let task_id: String = row
            .try_get("id")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get task_id: {}", e)))?;
        let context_id: String = row
            .try_get("context_id")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get context_id: {}", e)))?;
        let status_state: String = row
            .try_get("status_state")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get status_state: {}", e)))?;
        let status_message_json: Option<String> = row
            .try_get("status_message")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get status_message: {}", e)))?;
        let metadata_json: Option<String> = row
            .try_get("metadata")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get metadata: {}", e)))?;
        let artifacts_json: Option<String> = row
            .try_get("artifacts")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get artifacts: {}", e)))?;

        // Parse task state
        let state = match status_state.as_str() {
            "submitted" => TaskState::Submitted,
            "working" => TaskState::Working,
            "input-required" => TaskState::InputRequired,
            "completed" => TaskState::Completed,
            "canceled" => TaskState::Canceled,
            "failed" => TaskState::Failed,
            "rejected" => TaskState::Rejected,
            "auth-required" => TaskState::AuthRequired,
            "unknown" => TaskState::Unknown,
            _ => TaskState::Unknown,
        };

        // Parse status message
        let status_message = if let Some(msg_str) = status_message_json {
            Some(serde_json::from_str(&msg_str).map_err(|e| {
                A2AError::DatabaseError(format!("Failed to parse status message: {}", e))
            })?)
        } else {
            None
        };

        // Parse metadata
        let metadata =
            if let Some(meta_str) = metadata_json {
                Some(serde_json::from_str(&meta_str).map_err(|e| {
                    A2AError::DatabaseError(format!("Failed to parse metadata: {}", e))
                })?)
            } else {
                None
            };

        // Parse artifacts
        let artifacts = if let Some(artifacts_str) = artifacts_json {
            Some(serde_json::from_str(&artifacts_str).map_err(|e| {
                A2AError::DatabaseError(format!("Failed to parse artifacts: {}", e))
            })?)
        } else {
            None
        };

        let now = chrono::Utc::now();
        let task_status = TaskStatus {
            state: ::buffa::EnumValue::from(state),
            message: status_message.into(),
            timestamp: ::buffa::MessageField::some(::buffa_types::google::protobuf::Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
                ..Default::default()
            }),
            ..Default::default()
        };

        let task = Task {
            id: task_id.clone(),
            context_id,
            status: ::buffa::MessageField::some(task_status),
            history: Vec::new(),
            metadata: metadata.into(),
            artifacts: artifacts.unwrap_or_default(),
            ..Default::default()
        };

        Ok(task)
    }

    /// Load task history from database
    async fn load_task_history(
        &self,
        task_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<Message>, A2AError> {
        // Ordered by `id`, not `timestamp`: the timestamp default is
        // `datetime('now')`, which SQLite resolves to the second, so rows written
        // in the same second have no defined relative order. `id` is the
        // autoincrement insertion order and is what the conversation log means by
        // sequence.
        //
        // `message IS NOT NULL` is inside the query rather than a filter on the
        // rows, so `limit` counts messages. Filtering afterwards made
        // `history_length = 5` return fewer than five whenever a status
        // transition carried no message.
        let query_str = if let Some(limit) = limit {
            format!(
                "SELECT id, status_state, message FROM task_history \
                 WHERE task_id = ? AND message IS NOT NULL ORDER BY id DESC LIMIT {}",
                limit
            )
        } else {
            "SELECT id, status_state, message FROM task_history \
             WHERE task_id = ? AND message IS NOT NULL ORDER BY id DESC"
                .to_string()
        };

        let query_str = self.sql(&query_str);
        let rows = sqlx::query(&query_str)
            .bind(task_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to load task history: {}", e)))?;

        let mut history = Vec::new();
        for row in rows {
            let message_json: Option<String> = row.try_get("message").map_err(|e| {
                A2AError::DatabaseError(format!("Failed to get message from history: {}", e))
            })?;

            if let Some(msg_str) = message_json {
                let message: Message = serde_json::from_str(&msg_str).map_err(|e| {
                    A2AError::DatabaseError(format!("Failed to parse message from history: {}", e))
                })?;
                history.push(message);
            }
        }

        // Reverse to get chronological order
        history.reverse();
        Ok(history)
    }

    /// Add entry to task history
    async fn add_to_history(
        &self,
        task_id: &str,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<(), A2AError> {
        let state_str = match state {
            TaskState::Submitted => "submitted",
            TaskState::Working => "working",
            TaskState::InputRequired => "input-required",
            TaskState::Completed => "completed",
            TaskState::Canceled => "canceled",
            TaskState::Failed => "failed",
            TaskState::Rejected => "rejected",
            TaskState::AuthRequired => "auth-required",
            TaskState::Unknown => "unknown",
        };

        let message_json = if let Some(msg) = message {
            Some(serde_json::to_string(&msg).map_err(|e| {
                A2AError::DatabaseError(format!("Failed to serialize message: {}", e))
            })?)
        } else {
            None
        };

        // `context_id` is denormalized from `tasks` at insert rather than joined
        // at read: a task's context never changes, and rebuilding a conversation
        // for the model is the hottest read there is.
        let sql = self.sql(
            "INSERT INTO task_history (task_id, context_id, status_state, message) \
             VALUES (?, (SELECT context_id FROM tasks WHERE id = ?), ?, ?)",
        );
        sqlx::query(&sql)
            .bind(task_id)
            .bind(task_id)
            .bind(state_str)
            .bind(message_json)
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to add task history: {}", e)))?;

        Ok(())
    }

    /// Claim `context_id` for `caller` if nobody holds it, then refuse a caller
    /// that is not the holder.
    ///
    /// Ownership is first-write and never changes afterwards, so an existing
    /// row is the whole answer and only its absence needs a write. That is why
    /// the read comes first: every turn after the one that opened a context
    /// settles this in a single statement, on a path a handler takes twice a
    /// turn (the conversation and the state bag).
    ///
    /// An unowned context — one claimed with no principal, which is what an
    /// agent running without an authenticator produces — stays readable by
    /// anyone.
    async fn claim_or_check_context(
        &self,
        context_id: &str,
        caller: Option<&str>,
    ) -> Result<(), A2AError> {
        if let Some(claim) = self.read_claim(context_id).await? {
            return claim.verdict(context_id, caller);
        }

        // Nothing holds it. The insert ignores a conflict, so a caller arriving
        // second cannot take a context over.
        sqlx::query(self.dialect.insert_context_if_absent())
            .bind(context_id)
            .bind(caller)
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to register context: {}", e)))?;

        // Read back rather than assuming the insert was ours: two callers can
        // open one context in the same instant, and the loser has to be refused.
        // `rows_affected` would settle it without this statement, and would rest
        // an access decision on how each driver counts an ignored insert.
        match self.read_claim(context_id).await? {
            Some(claim) => claim.verdict(context_id, caller),
            None => Ok(()),
        }
    }

    /// Read the claim on a context, or `None` if it has no row yet.
    async fn read_claim(&self, context_id: &str) -> Result<Option<ContextClaim>, A2AError> {
        let sql = self.sql("SELECT owner FROM contexts WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(context_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to read context owner: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };
        let owner: Option<String> = row
            .try_get("owner")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get context owner: {}", e)))?;

        Ok(Some(match owner {
            Some(owner) => ContextClaim::Owner(owner),
            None => ContextClaim::Open,
        }))
    }

    /// Hand out this store's push-notification registry as an
    /// [`AsyncPushNotifier`].
    ///
    /// The returned notifier shares the same config registry the store writes to
    /// via [`AsyncNotificationManager::set_config`], so a config registered on
    /// the store is immediately visible to the notifier at the composition edge.
    pub fn push_notifier(&self) -> Arc<dyn AsyncPushNotifier> {
        self.push_notification_registry.clone()
    }

    /// The connection cap the pool was opened with — what
    /// [`SqlxStorageBuilder::max_connections`] asked for, read back off the
    /// pool. On PostgreSQL this is the store's share of a server-wide limit.
    pub fn max_connections(&self) -> u32 {
        self.pool.options().get_max_connections()
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncTaskLifecycle for SqlxTaskStorage {
    async fn create(&self, id: &TaskId, context_id: &ContextId) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        let context_id = context_id.as_str();
        // Check if task already exists
        let exists_sql = self.sql("SELECT id FROM tasks WHERE id = ?");
        let existing = sqlx::query(&exists_sql)
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to check existing task: {}", e))
            })?;

        if existing.is_some() {
            return Err(A2AError::TaskNotFound(format!(
                "Task {} already exists",
                task_id
            )));
        }

        // Create new task
        let task = Task::new(task_id.to_string(), context_id.to_string());

        // Convert metadata and artifacts to JSON strings
        let metadata_json = task
            .metadata
            .as_option()
            .map(|m| serde_json::to_string(m).unwrap_or_default());
        let artifacts_json = serde_json::to_string(&task.artifacts).unwrap_or_default();
        let status_message_str = task
            .status
            .as_option()
            .and_then(|s| s.message.as_option())
            .map(|m| serde_json::to_string(m).unwrap_or_default());

        // Insert into database
        let insert_sql = self.sql(
            "INSERT INTO tasks (id, context_id, status_state, status_message, metadata, artifacts) \
             VALUES (?, ?, ?, ?, ?, ?)",
        );
        sqlx::query(&insert_sql)
            .bind(&task.id)
            .bind(&task.context_id)
            .bind("submitted")
            .bind(status_message_str)
            .bind(metadata_json)
            .bind(artifacts_json)
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to create task: {}", e)))?;

        // Add initial history entry
        self.add_to_history(task_id, TaskState::Submitted, None)
            .await?;

        Ok(task)
    }

    async fn update_status(
        &self,
        id: &TaskId,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        // Convert state to string
        let state_str = match state {
            TaskState::Submitted => "submitted",
            TaskState::Working => "working",
            TaskState::InputRequired => "input-required",
            TaskState::Completed => "completed",
            TaskState::Canceled => "canceled",
            TaskState::Failed => "failed",
            TaskState::Rejected => "rejected",
            TaskState::AuthRequired => "auth-required",
            TaskState::Unknown => "unknown",
        };

        // Update task in database (bump the optimistic-concurrency version)
        let sql = self.sql("UPDATE tasks SET status_state = ?, version = version + 1 WHERE id = ?");
        let result = sqlx::query(&sql)
            .bind(state_str)
            .bind(task_id)
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to update task status: {}", e)))?;

        if result.rows_affected() == 0 {
            return Err(A2AError::TaskNotFound(task_id.to_string()));
        }

        // Add to history
        self.add_to_history(task_id, state, message).await?;

        // Persistence only: announcing the change to streaming subscribers is
        // the orchestration layer's job (see `TaskStatusBroadcast`), not a side
        // effect of the mutator.
        self.get(id, None).await
    }

    async fn exists(&self, id: &TaskId) -> Result<bool, A2AError> {
        let task_id = id.as_str();
        let sql = self.sql("SELECT id FROM tasks WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to check task existence: {}", e))
            })?;

        Ok(row.is_some())
    }

    async fn get(&self, id: &TaskId, history_length: Option<u32>) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        // Get task from database
        let query_str = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?");
        let sql = self.sql(&query_str);
        let row = sqlx::query(&sql)
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get task: {}", e)))?;

        let Some(row) = row else {
            return Err(A2AError::TaskNotFound(task_id.to_string()));
        };

        let mut task = Self::row_to_task(&row)?;

        // Load history
        if history_length.is_some() || history_length.is_none() {
            let history = self.load_task_history(task_id, history_length).await?;
            task.history = history;
        }

        Ok(task)
    }

    async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        // Get current task
        let task = self.get(id, None).await?;

        // Anything that has not finished can be canceled — a queued
        // (`Submitted`) task most of all, and an `InputRequired` one, where
        // cancelling is how a client says "never mind". See
        // `TaskState::is_cancelable`.
        if !task.status.state.is_cancelable() {
            return Err(A2AError::TaskNotCancelable(format!(
                "Task {} has already finished in state {:?} and cannot be canceled",
                task_id, task.status.state
            )));
        }

        // Create a cancellation message
        let mut cancel_message = Message::agent_text(
            format!("Task {} canceled.", task_id),
            uuid::Uuid::new_v4().to_string(),
        );
        cancel_message.task_id = task_id.to_string();
        cancel_message.context_id = task.context_id.clone();

        // Update task status (bump the optimistic-concurrency version)
        let sql = self.sql("UPDATE tasks SET status_state = ?, version = version + 1 WHERE id = ?");
        sqlx::query(&sql)
            .bind("canceled")
            .bind(task_id)
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to cancel task: {}", e)))?;

        // Add to history with cancellation message
        self.add_to_history(task_id, TaskState::Canceled, Some(cancel_message))
            .await?;

        // Persistence only: the orchestration layer announces the cancellation
        // to streaming subscribers (see `TaskStatusBroadcast`).
        self.get(id, None).await
    }
}

#[cfg(feature = "sqlx-storage")]
impl SqlxTaskStorage {
    /// Read the current stored version of a task, or `None` if it doesn't exist.
    async fn current_version(&self, task_id: &str) -> Result<Option<u64>, A2AError> {
        let sql = self.sql("SELECT version FROM tasks WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to read task version: {}", e)))?;
        match row {
            Some(row) => {
                let v: i64 = row.try_get("version").map_err(|e| {
                    A2AError::DatabaseError(format!("Failed to get version column: {}", e))
                })?;
                Ok(Some(v as u64))
            }
            None => Ok(None),
        }
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncTaskVersioning for SqlxTaskStorage {
    async fn version(&self, id: &TaskId) -> Result<u64, A2AError> {
        self.current_version(id.as_str())
            .await?
            .ok_or_else(|| A2AError::TaskNotFound(id.as_str().to_string()))
    }

    async fn get_versioned(
        &self,
        id: &TaskId,
        history_length: Option<u32>,
    ) -> Result<VersionedTask, A2AError> {
        let task = self.get(id, history_length).await?;
        let version = self.version(id).await?;
        Ok(VersionedTask::new(task, version))
    }

    async fn update_status_checked(
        &self,
        id: &TaskId,
        expected: u64,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<VersionedTask, A2AError> {
        let task_id = id.as_str();
        let state_str = match state {
            TaskState::Submitted => "submitted",
            TaskState::Working => "working",
            TaskState::InputRequired => "input-required",
            TaskState::Completed => "completed",
            TaskState::Canceled => "canceled",
            TaskState::Failed => "failed",
            TaskState::Rejected => "rejected",
            TaskState::AuthRequired => "auth-required",
            TaskState::Unknown => "unknown",
        };

        // Conditional update: both backends apply it atomically, so the row
        // count tells us whether the version matched without a separate lock.
        let sql = self.sql(
            "UPDATE tasks SET status_state = ?, version = version + 1 WHERE id = ? AND version = ?",
        );
        let result = sqlx::query(&sql)
            .bind(state_str)
            .bind(task_id)
            .bind(expected as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to update task status: {}", e)))?;

        if result.rows_affected() == 0 {
            // No row matched: either the task is gone or the version moved on.
            return match self.current_version(task_id).await? {
                Some(actual) => Err(A2AError::VersionConflict {
                    id: task_id.to_string(),
                    expected,
                    actual,
                }),
                None => Err(A2AError::TaskNotFound(task_id.to_string())),
            };
        }

        self.add_to_history(task_id, state, message).await?;
        let task = self.get(id, None).await?;
        Ok(VersionedTask::new(task, expected + 1))
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncTaskQuery for SqlxTaskStorage {
    async fn list(
        &self,
        params: &crate::domain::ListTasksParams,
    ) -> Result<crate::domain::ListTasksResult, A2AError> {
        use crate::domain::ListTasksResult;

        // Build WHERE clause conditions
        let mut where_conditions = Vec::new();

        // Filter by context_id
        if params.context_id.is_some() {
            where_conditions.push("context_id = ?".to_string());
        }

        // Filter by status
        if params.status.is_some() {
            where_conditions.push("status_state = ?".to_string());
        }

        // Filter by status_timestamp_after. Both the predicate and the value
        // are the dialect's, since one backend keeps its timestamps as text and
        // the other as a timestamp the parameter has to be cast to.
        let timestamp_str = if let Some(status_timestamp_after) = &params.status_timestamp_after {
            // Parse ISO 8601 string
            let timestamp =
                chrono::DateTime::parse_from_rfc3339(status_timestamp_after).map_err(|e| {
                    A2AError::DatabaseError(format!(
                        "Invalid timestamp value: {} ({})",
                        status_timestamp_after, e
                    ))
                })?;
            where_conditions.push(self.dialect.updated_since_predicate().to_string());
            Some(
                self.dialect
                    .format_timestamp(timestamp.with_timezone(&chrono::Utc)),
            )
        } else {
            None
        };

        // Build WHERE clause
        let where_clause = if where_conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", where_conditions.join(" AND "))
        };

        // First, get total count with same filters
        let count_sql = format!("SELECT COUNT(*) as count FROM tasks{}", where_clause);
        let count_query = self.sql(&count_sql);
        let mut count_q = sqlx::query(&count_query);

        // Bind parameters for count query
        if let Some(ref context_id) = params.context_id {
            count_q = count_q.bind(context_id);
        }
        if let Some(ref status) = params.status {
            let state_str = match *status {
                crate::domain::TaskState::Submitted => "submitted",
                crate::domain::TaskState::Working => "working",
                crate::domain::TaskState::InputRequired => "input-required",
                crate::domain::TaskState::Completed => "completed",
                crate::domain::TaskState::Canceled => "canceled",
                crate::domain::TaskState::Failed => "failed",
                crate::domain::TaskState::Rejected => "rejected",
                crate::domain::TaskState::AuthRequired => "auth-required",
                crate::domain::TaskState::Unknown => "unknown",
            };
            count_q = count_q.bind(state_str);
        }
        if let Some(ref ts) = timestamp_str {
            count_q = count_q.bind(ts);
        }

        let count_row = count_q
            .fetch_one(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to count tasks: {}", e)))?;

        // Read as 64-bit: `COUNT(*)` is a bigint on PostgreSQL and an integer
        // wide enough to be one on SQLite, and the driver will not narrow it.
        let total_size: i32 = count_row
            .try_get::<i64, _>("count")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get count: {}", e)))?
            .try_into()
            .unwrap_or(i32::MAX);

        // Handle pagination
        let page_size = params.page_size.unwrap_or(50).clamp(1, 100);
        let offset = if let Some(ref token) = params.page_token {
            token.parse::<i32>().unwrap_or(0)
        } else {
            0
        };

        // Build main query with LIMIT and OFFSET
        let main_sql = format!(
            "SELECT {TASK_COLUMNS} FROM tasks{} ORDER BY updated_at DESC LIMIT ? OFFSET ?",
            where_clause
        );
        let main_query = self.sql(&main_sql);

        let mut main_q = sqlx::query(&main_query);

        // Bind parameters for main query
        if let Some(ref context_id) = params.context_id {
            main_q = main_q.bind(context_id);
        }
        if let Some(ref status) = params.status {
            let state_str = match *status {
                crate::domain::TaskState::Submitted => "submitted",
                crate::domain::TaskState::Working => "working",
                crate::domain::TaskState::InputRequired => "input-required",
                crate::domain::TaskState::Completed => "completed",
                crate::domain::TaskState::Canceled => "canceled",
                crate::domain::TaskState::Failed => "failed",
                crate::domain::TaskState::Rejected => "rejected",
                crate::domain::TaskState::AuthRequired => "auth-required",
                crate::domain::TaskState::Unknown => "unknown",
            };
            main_q = main_q.bind(state_str);
        }
        if let Some(ref ts) = timestamp_str {
            main_q = main_q.bind(ts);
        }

        // Bind LIMIT and OFFSET
        main_q = main_q.bind(page_size).bind(offset);

        let rows = main_q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to list tasks: {}", e)))?;

        // Convert rows to tasks
        let mut tasks: Vec<Task> = rows
            .iter()
            .filter_map(|row| Self::row_to_task(row).ok())
            .collect();

        // Load history for each task if requested
        let history_length = params.history_length.unwrap_or(0);
        for task in &mut tasks {
            if history_length > 0 {
                let history = self
                    .load_task_history(&task.id, Some(history_length as u32))
                    .await?;
                task.history = history;
            } else {
                task.history.clear();
            }

            // Remove artifacts if not requested
            if !params.include_artifacts.unwrap_or(false) {
                task.artifacts.clear();
            }
        }

        // Generate next page token
        let has_more = offset + page_size < total_size;
        let next_page_token = if has_more {
            (offset + page_size).to_string()
        } else {
            String::new()
        };

        Ok(ListTasksResult {
            tasks,
            total_size,
            page_size,
            next_page_token,
        })
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncNotificationManager for SqlxTaskStorage {
    async fn get_config(
        &self,
        params: &crate::domain::GetTaskPushNotificationConfigParams,
    ) -> Result<crate::domain::TaskPushNotificationConfig, A2AError> {
        // When a specific config id is supplied, filter by it; otherwise fall
        // back to the task's config (single-config-per-task convenience, matching
        // the in-memory adapter and the v1.0.0 single-config helpers).
        // Note: push_notification_config_id filtering requires migration 002 to be applied.
        let by_id = self.sql(
            "SELECT id, task_id, url, token, authentication FROM push_notification_configs \
             WHERE task_id = ? AND id = ?",
        );
        let by_task = self.sql(
            "SELECT id, task_id, url, token, authentication FROM push_notification_configs \
             WHERE task_id = ? ORDER BY id LIMIT 1",
        );
        let row = match params.push_notification_config_id.as_ref() {
            Some(config_id) => sqlx::query(&by_id).bind(&params.id).bind(config_id),
            None => sqlx::query(&by_task).bind(&params.id),
        }
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| A2AError::DatabaseError(format!("Failed to get push config: {}", e)))?;

        if let Some(row) = row {
            let id: String = row
                .try_get("id")
                .map_err(|e| A2AError::DatabaseError(format!("Failed to get config id: {}", e)))?;
            let url: String = row
                .try_get("url")
                .map_err(|e| A2AError::DatabaseError(format!("Failed to get url: {}", e)))?;
            let token: Option<String> = row.try_get("token").ok();
            let auth_json: Option<String> = row.try_get("authentication").ok();

            let auth_info = if let Some(auth_str) = auth_json {
                serde_json::from_str(&auth_str).ok()
            } else {
                None
            };

            Ok(crate::domain::TaskPushNotificationConfig {
                task_id: params.id.clone(),
                id,
                url,
                token: token.unwrap_or_default(),
                authentication: auth_info.into(),
                tenant: "".to_string(),
                ..Default::default()
            })
        } else {
            Err(A2AError::TaskNotFound(format!(
                "Push notification config not found for task {}{}",
                params.id,
                params
                    .push_notification_config_id
                    .as_ref()
                    .map(|id| format!(" with id {}", id))
                    .unwrap_or_default()
            )))
        }
    }

    async fn list_configs(
        &self,
        params: &crate::domain::ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<crate::domain::TaskPushNotificationConfig>, A2AError> {
        // Query all configs for the task
        let sql = self.sql(
            "SELECT id, task_id, url, token, authentication FROM push_notification_configs \
             WHERE task_id = ?",
        );
        let rows = sqlx::query(&sql)
            .bind(&params.id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to list push configs: {}", e)))?;

        let configs: Vec<crate::domain::TaskPushNotificationConfig> = rows
            .iter()
            .filter_map(|row| {
                let id: String = row.try_get("id").ok()?;
                let url: String = row.try_get("url").ok()?;
                let token: Option<String> = row.try_get("token").ok().flatten();
                let auth_json: Option<String> = row.try_get("authentication").ok().flatten();

                let auth_info = if let Some(auth_str) = auth_json {
                    serde_json::from_str(&auth_str).ok()
                } else {
                    None
                };

                Some(crate::domain::TaskPushNotificationConfig {
                    task_id: params.id.clone(),
                    id,
                    url,
                    token: token.unwrap_or_default(),
                    authentication: auth_info.into(),
                    tenant: "".to_string(),
                    ..Default::default()
                })
            })
            .collect();

        Ok(configs)
    }

    async fn delete_config(
        &self,
        params: &crate::domain::DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        // Delete the specific config when an id is supplied; otherwise delete all
        // configs for the task (single-config-per-task convenience, matching the
        // in-memory adapter).
        let all_for_task = self.sql("DELETE FROM push_notification_configs WHERE task_id = ?");
        let one = self.sql("DELETE FROM push_notification_configs WHERE task_id = ? AND id = ?");
        let query = if params.push_notification_config_id.is_empty() {
            sqlx::query(&all_for_task).bind(&params.id)
        } else {
            sqlx::query(&one)
                .bind(&params.id)
                .bind(&params.push_notification_config_id)
        };
        let _result = query
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to delete push config: {}", e)))?;

        // Idempotent - don't error if already deleted (v1.0.0 spec behavior)
        Ok(())
    }

    async fn set_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Generate ID if not provided
        let config_id = if config.id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            config.id.clone()
        };

        // Serialize authentication if present
        let auth_json = config
            .authentication
            .as_option()
            .map(|auth| serde_json::to_string(auth).unwrap_or_default());

        // Store in database (using new schema with id, token, authentication)
        sqlx::query(self.dialect.upsert_push_config())
            .bind(&config_id)
            .bind(&config.task_id)
            .bind(&config.url)
            .bind(&config.token)
            .bind(auth_json)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to set push notification config: {}", e))
            })?;

        // Register with the push notification registry
        self.push_notification_registry
            .register(&config.task_id, config.clone())
            .await?;

        // Return config with ID set
        let mut result_config = config.clone();
        result_config.id = config_id;
        Ok(result_config)
    }
}

#[cfg(feature = "sqlx-storage")]
impl Clone for SqlxTaskStorage {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            dialect: self.dialect,
            push_notification_registry: self.push_notification_registry.clone(),
        }
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncConversationStore for SqlxTaskStorage {
    async fn load(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Conversation, A2AError> {
        let context_id = context_id.as_str();
        // Claims on read: a handler loads history at the top of every turn, so
        // the first turn is what establishes who owns the conversation.
        // Claiming only on compaction would leave a context readable by anyone
        // until it first grew long enough to summarize.
        self.claim_or_check_context(context_id, caller).await?;

        // Highest watermark rather than newest row: two turns of one
        // conversation can compact concurrently and land out of order, and the
        // digest covering more is the one to read from.
        let digest_sql = self.sql(
            "SELECT covers_through_seq, summary, replaced_messages, model \
             FROM context_digests WHERE context_id = ? \
             ORDER BY covers_through_seq DESC LIMIT 1",
        );
        let digest_row = sqlx::query(&digest_sql)
            .bind(context_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to load context digest: {}", e))
            })?;

        let digest = match digest_row {
            Some(row) => {
                let covers_through: i64 = row.try_get("covers_through_seq").map_err(|e| {
                    A2AError::DatabaseError(format!("Failed to get digest watermark: {}", e))
                })?;
                let summary: String = row.try_get("summary").map_err(|e| {
                    A2AError::DatabaseError(format!("Failed to get digest summary: {}", e))
                })?;
                let replaced_messages: i64 = row.try_get("replaced_messages").map_err(|e| {
                    A2AError::DatabaseError(format!("Failed to get digest message count: {}", e))
                })?;
                let model: String = row.try_get("model").map_err(|e| {
                    A2AError::DatabaseError(format!("Failed to get digest model: {}", e))
                })?;
                Some(Digest {
                    covers_through: Seq::new(covers_through.max(0) as u64),
                    summary,
                    replaced_messages: replaced_messages.max(0) as u32,
                    model,
                })
            }
            None => None,
        };

        let watermark = digest
            .as_ref()
            .map(|digest| digest.covers_through.get())
            .unwrap_or(0) as i64;

        // Ordered by `id`, which is the sequence number. Limiting keeps the
        // newest — the older end is what a summary stands in for, so dropping
        // the recent half would leave the model the least relevant part.
        // Hence DESC plus a reverse, rather than an offset the caller cannot
        // compute without first counting the rows.
        let query = match limit {
            Some(limit) => format!(
                "SELECT id, message FROM task_history \
                 WHERE context_id = ? AND id > ? AND message IS NOT NULL \
                 ORDER BY id DESC LIMIT {}",
                limit
            ),
            None => "SELECT id, message FROM task_history \
                     WHERE context_id = ? AND id > ? AND message IS NOT NULL \
                     ORDER BY id DESC"
                .to_string(),
        };
        let query = self.sql(&query);

        let rows = sqlx::query(&query)
            .bind(context_id)
            .bind(watermark)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to load conversation: {}", e)))?;

        let mut tail = Vec::with_capacity(rows.len());
        for row in rows {
            let seq: i64 = row.try_get("id").map_err(|e| {
                A2AError::DatabaseError(format!("Failed to get history sequence: {}", e))
            })?;
            let message_json: String = row.try_get("message").map_err(|e| {
                A2AError::DatabaseError(format!("Failed to get history message: {}", e))
            })?;
            let message: Message = serde_json::from_str(&message_json).map_err(|e| {
                A2AError::DatabaseError(format!("Failed to parse history message: {}", e))
            })?;
            tail.push(SequencedMessage {
                seq: Seq::new(seq.max(0) as u64),
                message,
            });
        }
        tail.reverse();

        Ok(Conversation { digest, tail })
    }

    async fn compact(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        digest: Digest,
    ) -> Result<(), A2AError> {
        let context_id = context_id.as_str();
        self.claim_or_check_context(context_id, caller).await?;

        let sql = self.sql(
            "INSERT INTO context_digests \
             (context_id, covers_through_seq, summary, replaced_messages, model) \
             VALUES (?, ?, ?, ?, ?)",
        );
        sqlx::query(&sql)
            .bind(context_id)
            .bind(digest.covers_through.get() as i64)
            .bind(&digest.summary)
            .bind(digest.replaced_messages as i64)
            .bind(&digest.model)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to append context digest: {}", e))
            })?;

        Ok(())
    }
}

/// How a stored scope is spelled in the `scope` column.
///
/// This adapter's encoding, not the domain's: [`StateScope`] carries the key
/// prefix a model writes, which is not the same string.
#[cfg(feature = "sqlx-storage")]
fn scope_column(scope: StateScope) -> Option<&'static str> {
    match scope {
        StateScope::User => Some("user"),
        StateScope::Context => Some("context"),
        // Never stored. That is the whole content of the scope.
        StateScope::Temp => None,
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncContextStateStore for SqlxTaskStorage {
    async fn load_state(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
    ) -> Result<ContextState, A2AError> {
        let context_id = context_id.as_str();
        // The same claim-then-check the conversation gets: a context id that
        // reads back what was remembered in it is a capability, and this store
        // is reached on the same turn as `load`.
        self.claim_or_check_context(context_id, caller).await?;

        // Both scopes in one round trip. With no principal the second parameter
        // binds NULL, and `scope_key = NULL` matches nothing — which is the
        // right answer, since a `user:` key cannot have been written without
        // one.
        let sql = self.sql(
            "SELECT scope, name, value FROM context_state \
             WHERE (scope = 'context' AND scope_key = ?) \
                OR (scope = 'user' AND scope_key = ?)",
        );
        let rows = sqlx::query(&sql)
            .bind(context_id)
            .bind(caller)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to load context state: {}", e)))?;

        let mut state = ContextState::new();
        for row in rows {
            let scope: String = row.try_get("scope").map_err(|e| {
                A2AError::DatabaseError(format!("Failed to get state scope: {}", e))
            })?;
            let name: String = row
                .try_get("name")
                .map_err(|e| A2AError::DatabaseError(format!("Failed to get state key: {}", e)))?;
            let value: String = row.try_get("value").map_err(|e| {
                A2AError::DatabaseError(format!("Failed to get state value: {}", e))
            })?;

            let scope = match scope.as_str() {
                "user" => StateScope::User,
                "context" => StateScope::Context,
                // A scope this build does not know. Skipped rather than guessed
                // at: filing it under the wrong scope would report a lifetime
                // the row does not have.
                _other => {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("ignoring state row with unknown scope '{_other}'");
                    continue;
                }
            };
            match StateKey::scoped(scope, &name) {
                Ok(key) => state.insert(key, value),
                Err(_e) => {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("ignoring unusable state key '{name}': {_e}");
                }
            }
        }
        Ok(state)
    }

    async fn remember(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        key: &StateKey,
        value: &str,
    ) -> Result<(), A2AError> {
        let context_id = context_id.as_str();
        self.claim_or_check_context(context_id, caller).await?;

        let (Some(scope_key), Some(scope)) = (
            scope_key(key.scope(), context_id, caller, key)?,
            scope_column(key.scope()),
        ) else {
            return Ok(());
        };

        sqlx::query(self.dialect.upsert_context_state())
            .bind(scope)
            .bind(scope_key)
            .bind(key.name())
            .bind(value)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to write context state: {}", e))
            })?;
        Ok(())
    }

    async fn forget(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        key: &StateKey,
    ) -> Result<bool, A2AError> {
        let context_id = context_id.as_str();
        self.claim_or_check_context(context_id, caller).await?;

        let (Some(scope_key), Some(scope)) = (
            scope_key(key.scope(), context_id, caller, key)?,
            scope_column(key.scope()),
        ) else {
            return Ok(false);
        };

        let sql =
            self.sql("DELETE FROM context_state WHERE scope = ? AND scope_key = ? AND name = ?");
        let deleted = sqlx::query(&sql)
            .bind(scope)
            .bind(scope_key)
            .bind(key.name())
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to drop context state: {}", e)))?;
        Ok(deleted.rows_affected() > 0)
    }
}

#[cfg(all(test, feature = "sqlx-storage"))]
mod tests {
    use super::*;

    /// Migration 006 drops `contexts.state`, which 005 created and nothing ever
    /// wrote. A database made by an older build still has it, and this is the
    /// path that clears it — on SQLite, where `ALTER TABLE … DROP COLUMN` has
    /// the most conditions attached and the drop is deliberately best effort.
    ///
    /// Inside the adapter rather than in `tests/`, because putting the column
    /// back needs the pool.
    #[tokio::test]
    async fn the_dead_state_column_is_dropped_on_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}?mode=rwc", dir.path().join("a2a.db").display());

        let storage = SqlxTaskStorage::new(&url).await.unwrap();
        sqlx::raw_sql("ALTER TABLE contexts ADD COLUMN state TEXT NOT NULL DEFAULT '{}'")
            .execute(&storage.pool)
            .await
            .expect("put the pre-006 column back");
        assert!(has_state_column(&storage).await);
        drop(storage);

        let restarted = SqlxTaskStorage::new(&url).await.unwrap();
        assert!(
            !has_state_column(&restarted).await,
            "the unused column should be gone after the migration runs"
        );
    }

    async fn has_state_column(storage: &SqlxTaskStorage) -> bool {
        sqlx::query(storage.dialect.dead_context_state_column_probe())
            .fetch_optional(&storage.pool)
            .await
            .unwrap()
            .is_some()
    }
}
