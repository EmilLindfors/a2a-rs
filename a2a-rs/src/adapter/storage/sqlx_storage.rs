//! SQLx-based task storage implementation
//!
//! This module provides a persistent storage solution using SQLx, supporting
//! SQLite, PostgreSQL, and MySQL databases.

#[cfg(feature = "sqlx-storage")]
use async_trait::async_trait;
#[cfg(feature = "sqlx-storage")]
use serde_json;
#[cfg(feature = "sqlx-storage")]
use sqlx::{Row, SqlitePool};

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
    A2AError, ContextId, Message, Task, TaskPushNotificationConfig, TaskId, TaskState, TaskStatus,
    VersionedTask,
};
#[cfg(feature = "sqlx-storage")]
use crate::port::{
    AsyncNotificationManager, AsyncPushNotifier, AsyncTaskLifecycle, AsyncTaskQuery,
    AsyncTaskVersioning,
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
    /// Database pool
    pool: SqlitePool,
    /// Push notification registry (config store + delivery backend)
    push_notification_registry: Arc<PushNotificationRegistry>,
}

#[cfg(feature = "sqlx-storage")]
use super::database_config::DatabaseType;

#[cfg(feature = "sqlx-storage")]
impl SqlxTaskStorage {
    /// Validate that the database URL is a supported SQLite URL.
    ///
    /// Returns an error if the URL points to a different database type
    /// or if the required feature is not enabled.
    fn validate_url(database_url: &str) -> Result<(), A2AError> {
        match DatabaseType::from_url(database_url) {
            Some(DatabaseType::Sqlite) => Ok(()),
            Some(db_type) => Err(A2AError::DatabaseError(format!(
                "{db_type} database detected from URL '{database_url}', but SqlxTaskStorage \
                 currently only supports SQLite. For {db_type} support, see the project roadmap."
            ))),
            None => Err(A2AError::DatabaseError(format!(
                "Unrecognized database URL scheme in '{database_url}'. \
                 Expected a URL starting with sqlite:, e.g. 'sqlite::memory:' or 'sqlite:data.db'"
            ))),
        }
    }

    /// Create a new SQLx task storage with the given database URL.
    ///
    /// Currently only SQLite URLs are supported (e.g. `sqlite::memory:`, `sqlite:data.db`).
    /// Passing a PostgreSQL or MySQL URL will return an error.
    pub async fn new(database_url: &str) -> Result<Self, A2AError> {
        Self::validate_url(database_url)?;

        let pool = SqlitePool::connect(database_url).await.map_err(|e| {
            A2AError::DatabaseError(format!("Failed to connect to database: {}", e))
        })?;

        // Run base migrations
        Self::run_base_migrations(&pool).await?;

        // Use the appropriate push notification sender based on available features
        #[cfg(feature = "http-client")]
        let push_sender = HttpPushNotificationSender::new();
        #[cfg(not(feature = "http-client"))]
        let push_sender = NoopPushNotificationSender::default();

        let push_registry = PushNotificationRegistry::new(push_sender);

        Ok(Self {
            pool,
            push_notification_registry: Arc::new(push_registry),
        })
    }

    /// Create a new SQLx task storage with a custom push notification sender.
    ///
    /// Currently only SQLite URLs are supported.
    pub async fn with_push_sender(
        database_url: &str,
        push_sender: impl PushNotificationSender + 'static,
    ) -> Result<Self, A2AError> {
        Self::validate_url(database_url)?;

        let pool = SqlitePool::connect(database_url).await.map_err(|e| {
            A2AError::DatabaseError(format!("Failed to connect to database: {}", e))
        })?;

        // Run migrations
        Self::run_base_migrations(&pool).await?;

        let push_registry = PushNotificationRegistry::new(push_sender);

        Ok(Self {
            pool,
            push_notification_registry: Arc::new(push_registry),
        })
    }

    /// Create a new SQLx task storage with additional migrations.
    ///
    /// Currently only SQLite URLs are supported.
    pub async fn with_migrations(
        database_url: &str,
        additional_migrations: &[&str],
    ) -> Result<Self, A2AError> {
        Self::validate_url(database_url)?;

        let pool = SqlitePool::connect(database_url).await.map_err(|e| {
            A2AError::DatabaseError(format!("Failed to connect to database: {}", e))
        })?;

        // Run base migrations
        Self::run_base_migrations(&pool).await?;

        // Run additional migrations
        Self::run_additional_migrations(&pool, additional_migrations).await?;

        // Use the appropriate push notification sender based on available features
        #[cfg(feature = "http-client")]
        let push_sender = HttpPushNotificationSender::new();
        #[cfg(not(feature = "http-client"))]
        let push_sender = NoopPushNotificationSender::default();

        let push_registry = PushNotificationRegistry::new(push_sender);

        Ok(Self {
            pool,
            push_notification_registry: Arc::new(push_registry),
        })
    }

    /// Run base A2A framework migrations (SQLite dialect).
    async fn run_base_migrations(pool: &SqlitePool) -> Result<(), A2AError> {
        sqlx::query(include_str!("../../../migrations/001_initial_schema.sql"))
            .execute(pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Migration 001 failed: {}", e)))?;

        sqlx::query(include_str!(
            "../../../migrations/002_v030_push_configs.sql"
        ))
        .execute(pool)
        .await
        .map_err(|e| A2AError::DatabaseError(format!("Migration 002 failed: {}", e)))?;

        // Migration 003 is an `ALTER TABLE ADD COLUMN`, which SQLite cannot
        // express idempotently. Since base migrations re-run on every `new()`,
        // tolerate the "duplicate column name" error on an already-migrated DB.
        if let Err(e) = sqlx::query(include_str!("../../../migrations/003_task_version.sql"))
            .execute(pool)
            .await
        {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                return Err(A2AError::DatabaseError(format!("Migration 003 failed: {msg}")));
            }
        }

        Ok(())
    }

    /// Run additional migrations provided by the application
    async fn run_additional_migrations(
        pool: &SqlitePool,
        migrations: &[&str],
    ) -> Result<(), A2AError> {
        for (i, migration_sql) in migrations.iter().enumerate() {
            sqlx::query(migration_sql)
                .execute(pool)
                .await
                .map_err(|e| {
                    A2AError::DatabaseError(format!("Additional migration {} failed: {}", i + 1, e))
                })?;
        }
        Ok(())
    }

    /// Convert database row to Task
    fn row_to_task(row: &sqlx::sqlite::SqliteRow) -> Result<Task, A2AError> {
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
        let query_str = if let Some(limit) = limit {
            format!(
                "SELECT timestamp, status_state, message FROM task_history WHERE task_id = ? ORDER BY timestamp DESC LIMIT {}",
                limit
            )
        } else {
            "SELECT timestamp, status_state, message FROM task_history WHERE task_id = ? ORDER BY timestamp DESC".to_string()
        };

        let query = sqlx::query(&query_str);

        let rows = query
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

        sqlx::query("INSERT INTO task_history (task_id, status_state, message) VALUES (?, ?, ?)")
            .bind(task_id)
            .bind(state_str)
            .bind(message_json)
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to add task history: {}", e)))?;

        Ok(())
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
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncTaskLifecycle for SqlxTaskStorage {
    async fn create(&self, id: &TaskId, context_id: &ContextId) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        let context_id = context_id.as_str();
        // Check if task already exists
        let existing = sqlx::query("SELECT id FROM tasks WHERE id = ?")
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
        sqlx::query("INSERT INTO tasks (id, context_id, status_state, status_message, metadata, artifacts) VALUES (?, ?, ?, ?, ?, ?)")
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
        let result = sqlx::query("UPDATE tasks SET status_state = ?, version = version + 1 WHERE id = ?")
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
        let row = sqlx::query("SELECT id FROM tasks WHERE id = ?")
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
        let row = sqlx::query("SELECT * FROM tasks WHERE id = ?")
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

        // Only working tasks can be canceled
        if task.status.state != TaskState::Working {
            return Err(A2AError::TaskNotCancelable(format!(
                "Task {} is in state {:?} and cannot be canceled",
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
        sqlx::query("UPDATE tasks SET status_state = ?, version = version + 1 WHERE id = ?")
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
        let row = sqlx::query("SELECT version FROM tasks WHERE id = ?")
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

        // Conditional update: SQLite applies it atomically, so the row count
        // tells us whether the version matched without a separate lock.
        let result = sqlx::query(
            "UPDATE tasks SET status_state = ?, version = version + 1 WHERE id = ? AND version = ?",
        )
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

        // Filter by status_timestamp_after
        let timestamp_str = if let Some(status_timestamp_after) = &params.status_timestamp_after {
            // Parse ISO 8601 string
            let timestamp =
                chrono::DateTime::parse_from_rfc3339(status_timestamp_after).map_err(|e| {
                    A2AError::DatabaseError(format!(
                        "Invalid timestamp value: {} ({})",
                        status_timestamp_after, e
                    ))
                })?;
            where_conditions.push("updated_at >= ?".to_string());
            Some(
                timestamp
                    .with_timezone(&chrono::Utc)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
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
        let count_query = format!("SELECT COUNT(*) as count FROM tasks{}", where_clause);
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

        let total_size: i32 = count_row
            .try_get("count")
            .map_err(|e| A2AError::DatabaseError(format!("Failed to get count: {}", e)))?;

        // Handle pagination
        let page_size = params.page_size.unwrap_or(50).clamp(1, 100);
        let offset = if let Some(ref token) = params.page_token {
            token.parse::<i32>().unwrap_or(0)
        } else {
            0
        };

        // Build main query with LIMIT and OFFSET
        let main_query = format!(
            "SELECT * FROM tasks{} ORDER BY updated_at DESC LIMIT ? OFFSET ?",
            where_clause
        );

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
        let row = match params.push_notification_config_id.as_ref() {
            Some(config_id) => sqlx::query(
                "SELECT id, task_id, url, token, authentication FROM push_notification_configs WHERE task_id = ? AND id = ?"
            )
            .bind(&params.id)
            .bind(config_id),
            None => sqlx::query(
                "SELECT id, task_id, url, token, authentication FROM push_notification_configs WHERE task_id = ? ORDER BY id LIMIT 1"
            )
            .bind(&params.id),
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
        let rows = sqlx::query(
            "SELECT id, task_id, url, token, authentication FROM push_notification_configs WHERE task_id = ?"
        )
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
        let query = if params.push_notification_config_id.is_empty() {
            sqlx::query("DELETE FROM push_notification_configs WHERE task_id = ?").bind(&params.id)
        } else {
            sqlx::query("DELETE FROM push_notification_configs WHERE task_id = ? AND id = ?")
                .bind(&params.id)
                .bind(&params.push_notification_config_id)
        };
        let _result = query.execute(&self.pool).await.map_err(|e| {
            A2AError::DatabaseError(format!("Failed to delete push config: {}", e))
        })?;

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
        sqlx::query(
            "INSERT OR REPLACE INTO push_notification_configs (id, task_id, url, token, authentication) VALUES (?, ?, ?, ?, ?)",
        )
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
            push_notification_registry: self.push_notification_registry.clone(),
        }
    }
}
