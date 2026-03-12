//! SQLx-based task storage implementation
//!
//! This module provides a persistent storage solution using SQLx, supporting
//! SQLite, PostgreSQL, and MySQL databases.

#[cfg(feature = "sqlx-storage")]
use std::collections::HashMap;

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
    A2AError, Artifact, Message, Task, TaskArtifactUpdateEvent, TaskPushNotificationConfig,
    TaskState, TaskStatus, TaskStatusUpdateEvent,
};
#[cfg(feature = "sqlx-storage")]
use crate::port::{
    AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskManager,
    streaming_handler::Subscriber,
};

#[cfg(feature = "sqlx-storage")]
use std::sync::Arc;
#[cfg(feature = "sqlx-storage")]
use tokio::sync::Mutex;

#[cfg(feature = "sqlx-storage")]
type StatusSubscribers = Vec<Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>>;
#[cfg(feature = "sqlx-storage")]
type ArtifactSubscribers = Vec<Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>>;

#[cfg(feature = "sqlx-storage")]
/// Structure to hold subscribers for a task
pub(crate) struct TaskSubscribers {
    status: StatusSubscribers,
    artifacts: ArtifactSubscribers,
}

#[cfg(feature = "sqlx-storage")]
impl TaskSubscribers {
    fn new() -> Self {
        Self {
            status: Vec::new(),
            artifacts: Vec::new(),
        }
    }
}

#[cfg(feature = "sqlx-storage")]
/// SQLx-based task storage for persistent storage
pub struct SqlxTaskStorage {
    /// Database pool
    pool: SqlitePool,
    /// Subscribers for task updates (in-memory for now)
    subscribers: Arc<Mutex<HashMap<String, TaskSubscribers>>>,
    /// Push notification registry
    push_notification_registry: Arc<PushNotificationRegistry>,
}

#[cfg(feature = "sqlx-storage")]
impl SqlxTaskStorage {
    /// Create a new SQLx task storage with the given database URL
    pub async fn new(database_url: &str) -> Result<Self, A2AError> {
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
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            push_notification_registry: Arc::new(push_registry),
        })
    }

    /// Create a new SQLx task storage with a custom push notification sender
    pub async fn with_push_sender(
        database_url: &str,
        push_sender: impl PushNotificationSender + 'static,
    ) -> Result<Self, A2AError> {
        let pool = SqlitePool::connect(database_url).await.map_err(|e| {
            A2AError::DatabaseError(format!("Failed to connect to database: {}", e))
        })?;

        // Run migrations
        Self::run_base_migrations(&pool).await?;

        let push_registry = PushNotificationRegistry::new(push_sender);

        Ok(Self {
            pool,
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            push_notification_registry: Arc::new(push_registry),
        })
    }

    /// Create a new SQLx task storage with additional migrations
    pub async fn with_migrations(
        database_url: &str,
        additional_migrations: &[&str],
    ) -> Result<Self, A2AError> {
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
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            push_notification_registry: Arc::new(push_registry),
        })
    }

    /// Run base A2A framework migrations
    async fn run_base_migrations(pool: &SqlitePool) -> Result<(), A2AError> {
        // Currently only SQLite migrations are supported.
        // For PostgreSQL/MySQL support, use DatabaseConfig::database_type() to select migrations.
        sqlx::query(include_str!("../../../migrations/001_initial_schema.sql"))
            .execute(pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Migration 001 failed: {}", e)))?;

        // Run v0.3.0 migration for enhanced push notification configs
        sqlx::query(include_str!(
            "../../../migrations/002_v030_push_configs.sql"
        ))
        .execute(pool)
        .await
        .map_err(|e| A2AError::DatabaseError(format!("Migration 002 failed: {}", e)))?;

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

        let task_status = TaskStatus {
            state,
            message: status_message,
            timestamp: Some(chrono::Utc::now()), // For now, use current time
        };

        let task = Task {
            id: task_id.clone(),
            context_id,
            status: task_status,
            history: None, // Will be set separately if needed
            metadata,
            artifacts,
            kind: "task".to_string(),
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

    /// Look up the context_id for a task from the database
    async fn get_task_context_id(&self, task_id: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT context_id FROM tasks WHERE id = ?")
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "default".to_string())
    }

    /// Send a status update to all subscribers for a task
    pub(crate) async fn broadcast_status_update(
        &self,
        task_id: &str,
        status: TaskStatus,
        final_: bool,
    ) -> Result<(), A2AError> {
        let context_id = self.get_task_context_id(task_id).await;

        // Create the update event
        let event = TaskStatusUpdateEvent {
            task_id: task_id.to_string(),
            context_id,
            kind: "status-update".to_string(),
            status,
            final_,
            metadata: None,
        };

        // Get all subscribers for this task and notify them
        {
            let subscribers_guard = self.subscribers.lock().await;

            if let Some(task_subscribers) = subscribers_guard.get(task_id) {
                // Clone the subscribers so we don't hold the lock during notification
                for subscriber in task_subscribers.status.iter() {
                    if let Err(e) = subscriber.on_update(event.clone()).await {
                        eprintln!("Failed to notify subscriber: {}", e);
                    }
                }
            }
        }; // Lock is dropped here

        // Send push notification if configured
        if let Err(e) = self
            .push_notification_registry
            .send_status_update(task_id, &event)
            .await
        {
            eprintln!("Failed to send push notification: {}", e);
        }

        Ok(())
    }

    /// Send an artifact update to all subscribers for a task
    pub(crate) async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        artifact: Artifact,
        _index: Option<u32>,
        _final: bool,
    ) -> Result<(), A2AError> {
        let context_id = self.get_task_context_id(task_id).await;

        // Create the update event
        let event = TaskArtifactUpdateEvent {
            task_id: task_id.to_string(),
            context_id,
            kind: "artifact-update".to_string(),
            artifact,
            append: None,
            last_chunk: None,
            metadata: None,
        };

        // Get all subscribers for this task
        {
            let subscribers_guard = self.subscribers.lock().await;

            if let Some(task_subscribers) = subscribers_guard.get(task_id) {
                // Clone the subscribers so we don't hold the lock during notification
                for subscriber in task_subscribers.artifacts.iter() {
                    if let Err(e) = subscriber.on_update(event.clone()).await {
                        eprintln!("Failed to notify subscriber: {}", e);
                    }
                }
            }
        }; // Lock is dropped here

        // Send push notification if configured
        if let Err(e) = self
            .push_notification_registry
            .send_artifact_update(task_id, &event)
            .await
        {
            eprintln!("Failed to send push notification: {}", e);
        }

        Ok(())
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncTaskManager for SqlxTaskStorage {
    async fn create_task(
        &self,
        task_id: &str,
        context_id: &str,
    ) -> Result<Task, A2AError> {
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
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default());
        let artifacts_json = task
            .artifacts
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default());
        let status_message_str = task
            .status
            .message
            .as_ref()
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

    async fn update_task_status(
        &self,
        task_id: &str,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2AError> {
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

        // Update task in database
        let result = sqlx::query("UPDATE tasks SET status_state = ? WHERE id = ?")
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

        // Get updated task
        let task = self.get_task(task_id, None).await?;

        // Clone status before broadcasting to avoid double clone
        let status = task.status.clone();

        // Broadcast status update
        self.broadcast_status_update(task_id, status, false)
            .await?;

        Ok(task)
    }

    async fn task_exists(&self, task_id: &str) -> Result<bool, A2AError> {
        let row = sqlx::query("SELECT id FROM tasks WHERE id = ?")
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to check task existence: {}", e))
            })?;

        Ok(row.is_some())
    }

    async fn get_task(
        &self,
        task_id: &str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
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
            task.history = if history.is_empty() {
                None
            } else {
                Some(history)
            };
        }

        Ok(task)
    }

    async fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError> {
        // Get current task
        let task = self.get_task(task_id, None).await?;

        // Only working tasks can be canceled
        if task.status.state != TaskState::Working {
            return Err(A2AError::TaskNotCancelable(format!(
                "Task {} is in state {:?} and cannot be canceled",
                task_id, task.status.state
            )));
        }

        // Create a cancellation message
        let cancel_message = Message {
            role: crate::domain::Role::Agent,
            parts: vec![crate::domain::Part::Text {
                text: format!("Task {} canceled.", task_id),
                metadata: None,
            }],
            metadata: None,
            reference_task_ids: None,
            message_id: uuid::Uuid::new_v4().to_string(),
            task_id: Some(task_id.to_string()),
            context_id: Some(task.context_id.clone()),
            extensions: None,
            kind: "message".to_string(),
        };

        // Update task status
        sqlx::query("UPDATE tasks SET status_state = ? WHERE id = ?")
            .bind("canceled")
            .bind(task_id)
            .execute(&self.pool)
            .await
            .map_err(|e| A2AError::DatabaseError(format!("Failed to cancel task: {}", e)))?;

        // Add to history with cancellation message
        self.add_to_history(task_id, TaskState::Canceled, Some(cancel_message))
            .await?;

        // Get updated task
        let updated_task = self.get_task(task_id, None).await?;

        // Clone status before broadcasting to avoid double clone
        let status = updated_task.status.clone();

        // Broadcast status update (with final flag set to true)
        self.broadcast_status_update(task_id, status, true)
            .await?;

        Ok(updated_task)
    }

    // ===== v0.3.0 Methods =====

    async fn list_tasks_v3(
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

        // Filter by lastUpdatedAfter
        let timestamp_str = if let Some(last_updated_after) = params.last_updated_after {
            // Convert milliseconds to SQLite timestamp
            let timestamp = chrono::DateTime::from_timestamp_millis(last_updated_after)
                .ok_or_else(|| {
                    A2AError::DatabaseError(format!(
                        "Invalid timestamp value: {}",
                        last_updated_after
                    ))
                })?;
            where_conditions.push("updated_at > ?".to_string());
            Some(timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
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
            let state_str = match status {
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
            let state_str = match status {
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
                task.history = if history.is_empty() {
                    None
                } else {
                    Some(history)
                };
            } else {
                task.history = None;
            }

            // Remove artifacts if not requested
            if !params.include_artifacts.unwrap_or(false) {
                task.artifacts = None;
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

    async fn get_push_notification_config(
        &self,
        params: &crate::domain::GetTaskPushNotificationConfigParams,
    ) -> Result<crate::domain::TaskPushNotificationConfig, A2AError> {
        // Query the database for the specific config
        // Note: push_notification_config_id filtering requires migration 002 to be applied
        let config_id = params.push_notification_config_id.as_ref().ok_or_else(|| {
            A2AError::TaskNotFound("push_notification_config_id is required".to_string())
        })?;

        let row = sqlx::query(
            "SELECT id, task_id, url, token, authentication FROM push_notification_configs WHERE task_id = ? AND id = ?"
        )
        .bind(&params.id)
        .bind(config_id)
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

            let authentication = if let Some(auth_str) = auth_json {
                serde_json::from_str(&auth_str).ok()
            } else {
                None
            };

            Ok(crate::domain::TaskPushNotificationConfig {
                task_id: params.id.clone(),
                push_notification_config: crate::domain::PushNotificationConfig {
                    id: Some(id),
                    url,
                    token,
                    authentication,
                },
            })
        } else {
            Err(A2AError::TaskNotFound(format!(
                "Push notification config not found for task {} with id {}",
                params.id, config_id
            )))
        }
    }

    async fn list_push_notification_configs(
        &self,
        params: &crate::domain::ListTaskPushNotificationConfigParams,
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

                let authentication = if let Some(auth_str) = auth_json {
                    serde_json::from_str(&auth_str).ok()
                } else {
                    None
                };

                Some(crate::domain::TaskPushNotificationConfig {
                    task_id: params.id.clone(),
                    push_notification_config: crate::domain::PushNotificationConfig {
                        id: Some(id),
                        url,
                        token,
                        authentication,
                    },
                })
            })
            .collect();

        Ok(configs)
    }

    async fn delete_push_notification_config(
        &self,
        params: &crate::domain::DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        // Delete the specific config
        let _result =
            sqlx::query("DELETE FROM push_notification_configs WHERE task_id = ? AND id = ?")
                .bind(&params.id)
                .bind(&params.push_notification_config_id)
                .execute(&self.pool)
                .await
                .map_err(|e| {
                    A2AError::DatabaseError(format!("Failed to delete push config: {}", e))
                })?;

        // Idempotent - don't error if already deleted (v0.3.0 spec behavior)
        Ok(())
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncNotificationManager for SqlxTaskStorage {
    async fn set_task_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Generate ID if not provided
        let config_id = config
            .push_notification_config
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Serialize authentication if present
        let auth_json = config
            .push_notification_config
            .authentication
            .as_ref()
            .map(|auth| serde_json::to_string(auth).unwrap_or_default());

        // Store in database (using new schema with id, token, authentication)
        sqlx::query(
            "INSERT OR REPLACE INTO push_notification_configs (id, task_id, url, token, authentication) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&config_id)
        .bind(&config.task_id)
        .bind(&config.push_notification_config.url)
        .bind(&config.push_notification_config.token)
        .bind(auth_json)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            A2AError::DatabaseError(format!("Failed to set push notification config: {}", e))
        })?;

        // Register with the push notification registry
        self.push_notification_registry
            .register(&config.task_id, config.push_notification_config.clone())
            .await?;

        // Return config with ID set
        let mut result_config = config.clone();
        result_config.push_notification_config.id = Some(config_id);
        Ok(result_config)
    }

    async fn get_task_notification(
        &self,
        task_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Get from database (get first config for backwards compatibility)
        let row =
            sqlx::query("SELECT id, url, token, authentication FROM push_notification_configs WHERE task_id = ? LIMIT 1")
                .bind(task_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| {
                    A2AError::DatabaseError(format!(
                        "Failed to get push notification config: {}",
                        e
                    ))
                })?;

        if let Some(row) = row {
            let id: String = row
                .try_get("id")
                .map_err(|e| A2AError::DatabaseError(format!("Failed to get id: {}", e)))?;
            let url: String = row
                .try_get("url")
                .map_err(|e| A2AError::DatabaseError(format!("Failed to get url: {}", e)))?;
            let token: Option<String> = row.try_get("token").ok();
            let auth_json: Option<String> = row.try_get("authentication").ok();

            let authentication = if let Some(auth_str) = auth_json {
                serde_json::from_str(&auth_str).ok()
            } else {
                None
            };

            Ok(TaskPushNotificationConfig {
                task_id: task_id.to_string(),
                push_notification_config: crate::domain::PushNotificationConfig {
                    id: Some(id),
                    url,
                    token,
                    authentication,
                },
            })
        } else {
            Err(A2AError::TaskNotFound(format!(
                "No push notification config found for task {}",
                task_id
            )))
        }
    }

    async fn remove_task_notification(&self, task_id: &str) -> Result<(), A2AError> {
        // Remove from database
        sqlx::query("DELETE FROM push_notification_configs WHERE task_id = ?")
            .bind(task_id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                A2AError::DatabaseError(format!("Failed to remove push notification config: {}", e))
            })?;

        // Unregister from registry
        self.push_notification_registry.unregister(task_id).await?;
        Ok(())
    }
}

#[cfg(feature = "sqlx-storage")]
#[async_trait]
impl AsyncStreamingHandler for SqlxTaskStorage {
    async fn add_status_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        // Add the subscriber
        {
            let mut subscribers_guard = self.subscribers.lock().await;

            let task_subscribers = subscribers_guard
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);

            task_subscribers.status.push(subscriber);
        } // Lock is dropped here

        // Try to get the current status to send as an initial update
        // But don't fail if the task doesn't exist yet - the subscriber will get updates when it's created
        if let Ok(task) = self.get_task(task_id, None).await {
            let _ = self
                .broadcast_status_update(task_id, task.status, false)
                .await;
        }

        Ok(format!("status-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn add_artifact_subscriber(
        &self,
        task_id: &str,
        subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        // Add the subscriber
        {
            let mut subscribers_guard = self.subscribers.lock().await;

            let task_subscribers = subscribers_guard
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);

            task_subscribers.artifacts.push(subscriber);
        } // Lock is dropped here

        // If there are existing artifacts, broadcast them
        // But don't fail if the task doesn't exist yet - the subscriber will get updates when it's created
        if let Ok(task) = self.get_task(task_id, None).await {
            if let Some(artifacts) = task.artifacts {
                for artifact in artifacts {
                    let _ = self
                        .broadcast_artifact_update(task_id, artifact, None, false)
                        .await;
                }
            }
        }

        Ok(format!("artifact-{}-{}", task_id, uuid::Uuid::new_v4()))
    }

    async fn remove_subscription(&self, _subscription_id: &str) -> Result<(), A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Subscription removal by ID requires storage layer refactoring".to_string(),
        ))
    }

    async fn remove_task_subscribers(&self, task_id: &str) -> Result<(), A2AError> {
        // Remove all subscribers
        {
            let mut subscribers_guard = self.subscribers.lock().await;
            subscribers_guard.remove(task_id);
        } // Lock is dropped here

        Ok(())
    }

    async fn get_subscriber_count(&self, task_id: &str) -> Result<usize, A2AError> {
        let subscribers_guard = self.subscribers.lock().await;

        if let Some(task_subscribers) = subscribers_guard.get(task_id) {
            Ok(task_subscribers.status.len() + task_subscribers.artifacts.len())
        } else {
            Ok(0)
        }
    }

    async fn broadcast_status_update(
        &self,
        task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        self.broadcast_status_update(task_id, update.status, update.final_)
            .await
    }

    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        self.broadcast_artifact_update(
            task_id,
            update.artifact,
            None,
            update.last_chunk.unwrap_or(false),
        )
        .await
    }

    async fn status_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>,
        >,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Status update stream requires storage layer refactoring".to_string(),
        ))
    }

    async fn artifact_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>,
        >,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Artifact update stream requires storage layer refactoring".to_string(),
        ))
    }

    async fn combined_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures::Stream<
                        Item = Result<crate::port::streaming_handler::UpdateEvent, A2AError>,
                    > + Send,
            >,
        >,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Combined update stream requires storage layer refactoring".to_string(),
        ))
    }
}

#[cfg(feature = "sqlx-storage")]
impl Clone for SqlxTaskStorage {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            subscribers: self.subscribers.clone(),
            push_notification_registry: self.push_notification_registry.clone(),
        }
    }
}
