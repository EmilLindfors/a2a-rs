//! Task management port definitions

#[cfg(feature = "server")]
use async_trait::async_trait;

use crate::{
    Message,
    domain::{
        A2AError, ContextId, ListTasksParams, ListTasksResult, Task, TaskId, TaskIdParams,
        TaskQueryParams, TaskState,
    },
};

/// Async task lifecycle management: the core CRUD capability over individual tasks.
///
/// A handler implements this trait if it can create, read, mutate, and cancel
/// tasks. Listing/querying across tasks is a separate capability — see
/// [`AsyncTaskQuery`]. Convenience wrappers that validate request parameters
/// live on [`AsyncTaskLifecycleExt`], which is blanket-implemented for every
/// `AsyncTaskLifecycle`.
#[cfg(feature = "server")]
#[async_trait]
pub trait AsyncTaskLifecycle: Send + Sync {
    /// Create a new task in the given context.
    async fn create(&self, id: &TaskId, context_id: &ContextId) -> Result<Task, A2AError>;

    /// Get a task by ID with optional history length limit.
    async fn get(&self, id: &TaskId, history_length: Option<u32>) -> Result<Task, A2AError>;

    /// Update task status, optionally appending a message to history.
    async fn update_status(
        &self,
        id: &TaskId,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2AError>;

    /// Cancel a task.
    async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError>;

    /// Check whether a task exists.
    async fn exists(&self, id: &TaskId) -> Result<bool, A2AError>;
}

/// Async task querying: listing tasks with filtering and pagination.
///
/// Kept distinct from [`AsyncTaskLifecycle`] so a handler that only stores and
/// mutates individual tasks is not forced to implement cross-task search.
#[cfg(feature = "server")]
#[async_trait]
pub trait AsyncTaskQuery: Send + Sync {
    /// List tasks with filtering and pagination (A2A v1.0.0 `tasks/list`).
    async fn list(&self, params: &ListTasksParams) -> Result<ListTasksResult, A2AError>;
}

/// Validation conveniences over [`AsyncTaskLifecycle`].
///
/// Blanket-implemented for every `AsyncTaskLifecycle`, so implementors get these
/// for free and only ever stub the core primitives. Constructing a [`TaskId`]
/// from request parameters performs the empty-string validation, so these
/// wrappers parse the wire parameters once at the boundary.
#[cfg(feature = "server")]
#[async_trait]
pub trait AsyncTaskLifecycleExt: AsyncTaskLifecycle {
    /// Validate query parameters, then fetch the task.
    async fn get_validated(&self, params: &TaskQueryParams) -> Result<Task, A2AError> {
        let id: TaskId = params.id.parse()?;
        if let Some(history_length) = params.history_length {
            if history_length > 1000 {
                return Err(A2AError::ValidationError {
                    field: "history_length".to_string(),
                    message: "History length cannot exceed 1000".to_string(),
                });
            }
        }
        self.get(&id, params.history_length).await
    }

    /// Validate ID parameters, then cancel the task.
    async fn cancel_validated(&self, params: &TaskIdParams) -> Result<Task, A2AError> {
        let id: TaskId = params.id.parse()?;
        self.cancel(&id).await
    }
}

#[cfg(feature = "server")]
impl<T: AsyncTaskLifecycle + ?Sized> AsyncTaskLifecycleExt for T {}
