//! Task management port definitions

#[cfg(feature = "server")]
use async_trait::async_trait;

use crate::domain::{A2AError, Task, TaskIdParams, TaskQueryParams, TaskState};

/// A trait for managing task lifecycle and operations
pub trait TaskManager {
    /// Create a new task
    fn create_task(&self, task_id: &str, context_id: &str) -> Result<Task, A2AError>;

    /// Get a task by ID with optional history
    fn get_task(&self, task_id: &str, history_length: Option<u32>) -> Result<Task, A2AError>;

    /// Update task status
    fn update_task_status(&self, task_id: &str, state: TaskState) -> Result<Task, A2AError>;

    /// Cancel a task
    fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError>;

    /// Check if a task exists
    fn task_exists(&self, task_id: &str) -> Result<bool, A2AError>;

    /// List tasks with optional filtering
    fn list_tasks(
        &self,
        _context_id: Option<&str>,
        _limit: Option<u32>,
    ) -> Result<Vec<Task>, A2AError> {
        // Default implementation - can be overridden
        // Basic implementation that doesn't support filtering
        Err(A2AError::UnsupportedOperation(
            "Task listing not implemented".to_string(),
        ))
    }

    /// Get task metadata
    fn get_task_metadata(
        &self,
        task_id: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>, A2AError> {
        let task = self.get_task(task_id, None)?;
        Ok(task.metadata.unwrap_or_default())
    }

    /// Validate task parameters
    fn validate_task_params(&self, params: &TaskQueryParams) -> Result<(), A2AError> {
        if params.id.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".to_string(),
                message: "Task ID cannot be empty".to_string(),
            });
        }

        if let Some(history_length) = params.history_length {
            if history_length > 1000 {
                return Err(A2AError::ValidationError {
                    field: "history_length".to_string(),
                    message: "History length cannot exceed 1000".to_string(),
                });
            }
        }

        Ok(())
    }
}

#[cfg(feature = "server")]
#[async_trait]
/// An async trait for managing task lifecycle and operations
pub trait AsyncTaskManager: Send + Sync {
    /// Create a new task
    async fn create_task<'a>(
        &self,
        task_id: &'a str,
        context_id: &'a str,
    ) -> Result<Task, A2AError>;

    /// Get a task by ID with optional history
    async fn get_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError>;

    /// Update task status
    async fn update_task_status<'a>(
        &self,
        task_id: &'a str,
        state: TaskState,
    ) -> Result<Task, A2AError>;

    /// Cancel a task
    async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError>;

    /// Check if a task exists
    async fn task_exists<'a>(&self, task_id: &'a str) -> Result<bool, A2AError>;

    /// List tasks with optional filtering
    async fn list_tasks<'a>(
        &self,
        _context_id: Option<&'a str>,
        _limit: Option<u32>,
    ) -> Result<Vec<Task>, A2AError> {
        // Default implementation - can be overridden
        // Basic implementation that doesn't support filtering
        Err(A2AError::UnsupportedOperation(
            "Task listing not implemented".to_string(),
        ))
    }

    /// Get task metadata
    async fn get_task_metadata<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<serde_json::Map<String, serde_json::Value>, A2AError> {
        let task = self.get_task(task_id, None).await?;
        Ok(task.metadata.unwrap_or_default())
    }

    /// Validate task parameters
    async fn validate_task_params<'a>(&self, params: &'a TaskQueryParams) -> Result<(), A2AError> {
        if params.id.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".to_string(),
                message: "Task ID cannot be empty".to_string(),
            });
        }

        if let Some(history_length) = params.history_length {
            if history_length > 1000 {
                return Err(A2AError::ValidationError {
                    field: "history_length".to_string(),
                    message: "History length cannot exceed 1000".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Get task with validation
    async fn get_task_validated<'a>(&self, params: &'a TaskQueryParams) -> Result<Task, A2AError> {
        self.validate_task_params(params).await?;
        self.get_task(&params.id, params.history_length).await
    }

    /// Cancel task with validation
    async fn cancel_task_validated<'a>(&self, params: &'a TaskIdParams) -> Result<Task, A2AError> {
        if params.id.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".to_string(),
                message: "Task ID cannot be empty".to_string(),
            });
        }

        self.cancel_task(&params.id).await
    }
}
