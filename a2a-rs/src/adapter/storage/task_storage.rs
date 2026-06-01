//! In-memory task storage implementation

// This module is already conditionally compiled with #[cfg(feature = "server")] in mod.rs

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex; // Changed from std::sync::Mutex

use crate::adapter::business::push_notification::{
    PushNotificationRegistry, PushNotificationSender,
};

#[cfg(feature = "http-client")]
use crate::adapter::business::push_notification::HttpPushNotificationSender;
#[cfg(not(feature = "http-client"))]
use crate::adapter::business::push_notification::NoopPushNotificationSender;
use crate::domain::{
    A2AError, ContextId, Message, Task, TaskPushNotificationConfig, TaskId, TaskState,
};
use crate::port::{
    AsyncNotificationManager, AsyncPushNotifier, AsyncTaskLifecycle, AsyncTaskQuery,
};

/// Simple in-memory task storage for testing and example purposes.
///
/// Persistence-only since the Phase 4 struct-split: streaming fan-out lives in
/// [`InMemoryStreamingHandler`](crate::adapter::InMemoryStreamingHandler) and
/// push-webhook delivery behind the [`AsyncPushNotifier`] port (this struct hands
/// out its registry via [`push_notifier`](Self::push_notifier)). The store still
/// owns push-config CRUD ([`AsyncNotificationManager`]) because that is config
/// *persistence*.
pub struct InMemoryTaskStorage {
    /// Tasks stored by ID
    pub(crate) tasks: Arc<Mutex<HashMap<String, Task>>>,
    /// Push notification registry (config store + delivery backend)
    pub(crate) push_notification_registry: Arc<PushNotificationRegistry>,
}

impl InMemoryTaskStorage {
    /// Create a new empty task storage
    pub fn new() -> Self {
        // Use the appropriate push notification sender based on available features
        #[cfg(feature = "http-client")]
        let push_sender = HttpPushNotificationSender::new();
        #[cfg(not(feature = "http-client"))]
        let push_sender = NoopPushNotificationSender;

        let push_registry = PushNotificationRegistry::new(push_sender);

        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            push_notification_registry: Arc::new(push_registry),
        }
    }

    /// Create a new task storage with a custom push notification sender
    pub fn with_push_sender(push_sender: impl PushNotificationSender + 'static) -> Self {
        let push_registry = PushNotificationRegistry::new(push_sender);

        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            push_notification_registry: Arc::new(push_registry),
        }
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

impl Default for InMemoryTaskStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncTaskLifecycle for InMemoryTaskStorage {
    async fn create(&self, id: &TaskId, context_id: &ContextId) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        let context_id = context_id.as_str();
        let mut tasks_guard = self.tasks.lock().await;

        if tasks_guard.contains_key(task_id) {
            return Err(A2AError::TaskNotFound(format!(
                "Task {} already exists",
                task_id
            )));
        }

        let task = Task::new(task_id.to_string(), context_id.to_string());
        tasks_guard.insert(task_id.to_string(), task.clone());

        Ok(task)
    }

    async fn update_status(
        &self,
        id: &TaskId,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        let mut tasks_guard = self.tasks.lock().await;

        let task = tasks_guard
            .get_mut(task_id)
            .ok_or_else(|| A2AError::TaskNotFound(task_id.to_string()))?;

        // Update the task status with the optional message
        task.update_status(state, message);

        // Persistence only: announcing the change to streaming subscribers is
        // the orchestration layer's job (see `TaskStatusBroadcast` and
        // `REFACTORING_PLAN.md` §4.0.2), not a side effect of the mutator.
        Ok(task.clone())
    }

    async fn exists(&self, id: &TaskId) -> Result<bool, A2AError> {
        let task_id = id.as_str();
        let tasks_guard = self.tasks.lock().await;
        Ok(tasks_guard.contains_key(task_id))
    }

    async fn get(&self, id: &TaskId, history_length: Option<u32>) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        // Get the task
        let task = {
            let tasks_guard = self.tasks.lock().await;

            let Some(task) = tasks_guard.get(task_id) else {
                return Err(A2AError::TaskNotFound(task_id.to_string()));
            };

            // Apply history length limitation if specified
            task.with_limited_history(history_length)
        }; // Lock is dropped here

        Ok(task)
    }

    async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        let mut tasks_guard = self.tasks.lock().await;

        let Some(task) = tasks_guard.get(task_id) else {
            return Err(A2AError::TaskNotFound(task_id.to_string()));
        };

        let mut updated_task = task.clone();

        // Only working tasks can be canceled
        if updated_task.status.state != TaskState::Working {
            return Err(A2AError::TaskNotCancelable(format!(
                "Task {} is in state {:?} and cannot be canceled",
                task_id, updated_task.status.state
            )));
        }

        // Create a cancellation message to add to history
        let cancel_message = Message {
            role: ::buffa::EnumValue::from(crate::domain::Role::Agent),
            parts: vec![crate::domain::Part::text(format!("Task {} canceled.", task_id))],
            message_id: uuid::Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            context_id: updated_task.context_id.clone(),
            ..Default::default()
        };

        // Update the status with the cancellation message to track in history
        updated_task.update_status(TaskState::Canceled, Some(cancel_message));
        tasks_guard.insert(task_id.to_string(), updated_task.clone());

        // Persistence only: the orchestration layer announces the cancellation
        // to streaming subscribers (see `REFACTORING_PLAN.md` §4.0.2).
        Ok(updated_task)
    }
}

#[async_trait]
impl AsyncTaskQuery for InMemoryTaskStorage {
    async fn list(
        &self,
        params: &crate::domain::ListTasksParams,
    ) -> Result<crate::domain::ListTasksResult, A2AError> {
        use crate::domain::ListTasksResult;

        let tasks_guard = self.tasks.lock().await;

        // Filter tasks based on parameters
        let mut filtered_tasks: Vec<_> = tasks_guard
            .values()
            .filter(|task| {
                // Filter by context_id if provided
                if let Some(ref context_id) = params.context_id {
                    if &task.context_id != context_id {
                        return false;
                    }
                }

                // Filter by status if provided
                if let Some(ref status) = params.status {
                    if &task.status.state != status {
                        return false;
                    }
                }

                // Filter by status_timestamp_after if provided
                if let Some(status_timestamp_after) = &params.status_timestamp_after {
                    if let Ok(after_dt) =
                        chrono::DateTime::parse_from_rfc3339(status_timestamp_after)
                    {
                        let after_utc = after_dt.with_timezone(&chrono::Utc);
                        if let Some(timestamp) = task.status.timestamp_utc() {
                            if timestamp <= after_utc {
                                return false;
                            }
                        }
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort by timestamp (most recent first)
        filtered_tasks.sort_by(|a, b| {
            let a_time = a
                .status
                .timestamp_utc()
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);
            let b_time = b
                .status
                .timestamp_utc()
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);
            b_time.cmp(&a_time)
        });

        let total_size = filtered_tasks.len() as i32;

        // Handle pagination
        let page_size = params.page_size.unwrap_or(50).clamp(1, 100) as usize;
        let page_start = if let Some(ref token) = params.page_token {
            // Parse page token as a number (simple implementation)
            token.parse::<usize>().unwrap_or(0)
        } else {
            0
        };

        let page_end = (page_start + page_size).min(filtered_tasks.len());
        let has_more = page_end < filtered_tasks.len();

        // Get the page of tasks
        let mut page_tasks: Vec<_> = filtered_tasks[page_start..page_end].to_vec();

        // Apply history length limit
        let history_length = params.history_length.unwrap_or(0);
        for task in &mut page_tasks {
            *task = task.with_limited_history(Some(history_length as u32));

            // Remove artifacts if not requested
            if !params.include_artifacts.unwrap_or(false) {
                task.artifacts.clear();
            }
        }

        // Generate next page token
        let next_page_token = if has_more {
            page_end.to_string()
        } else {
            String::new()
        };

        Ok(ListTasksResult {
            tasks: page_tasks,
            total_size,
            page_size: page_size as i32,
            next_page_token,
        })
    }
}

// AsyncNotificationManager implementation.
//
// In-memory storage keeps a single config per task in the push-notification
// registry, so the multi-config CRUD surface is expressed in those terms.
#[async_trait]
impl AsyncNotificationManager for InMemoryTaskStorage {
    async fn set_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        #[cfg(feature = "tracing")]
        tracing::info!(
            task_id = %config.task_id,
            url = %config.url,
            "🚀 Registering push notification config for task"
        );

        // Register with the push notification registry
        self.push_notification_registry
            .register(&config.task_id, config.clone())
            .await?;

        #[cfg(feature = "tracing")]
        tracing::info!(
            task_id = %config.task_id,
            "✅ Push notification config registered successfully"
        );

        Ok(config.clone())
    }

    async fn get_config(
        &self,
        params: &crate::domain::GetTaskPushNotificationConfigParams,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        match self.push_notification_registry.get_config(&params.id).await? {
            Some(config) => Ok(config),
            None => Err(A2AError::PushNotificationNotSupported),
        }
    }

    async fn list_configs(
        &self,
        params: &crate::domain::ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        // In-memory storage supports one config per task; return it as a
        // single-item vec (or empty if none registered).
        match self.push_notification_registry.get_config(&params.id).await? {
            Some(config) => Ok(vec![config]),
            None => Ok(vec![]),
        }
    }

    async fn delete_config(
        &self,
        params: &crate::domain::DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        // In-memory storage keeps a single config per task, so config_id is
        // not used for lookup. Idempotent per the v1.0.0 spec.
        self.push_notification_registry.unregister(&params.id).await?;
        Ok(())
    }
}

impl Clone for InMemoryTaskStorage {
    fn clone(&self) -> Self {
        Self {
            tasks: self.tasks.clone(),
            push_notification_registry: self.push_notification_registry.clone(),
        }
    }
}
