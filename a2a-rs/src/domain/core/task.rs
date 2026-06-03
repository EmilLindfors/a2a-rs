use crate::domain::error::A2AError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(feature = "tracing")]
use tracing::instrument;

#[cfg(feature = "tracing")]
use crate::measure_duration;

use super::message::{Artifact, Message};

// Re-export generated types
pub use crate::domain::generated::{Task, TaskPushNotificationConfig, TaskState, TaskStatus};

#[allow(non_upper_case_globals)]
impl TaskState {
    pub const Submitted: Self = Self::TASK_STATE_SUBMITTED;
    pub const Working: Self = Self::TASK_STATE_WORKING;
    pub const InputRequired: Self = Self::TASK_STATE_INPUT_REQUIRED;
    pub const Completed: Self = Self::TASK_STATE_COMPLETED;
    pub const Canceled: Self = Self::TASK_STATE_CANCELED;
    pub const Failed: Self = Self::TASK_STATE_FAILED;
    pub const Rejected: Self = Self::TASK_STATE_REJECTED;
    pub const AuthRequired: Self = Self::TASK_STATE_AUTH_REQUIRED;
    pub const Unknown: Self = Self::TASK_STATE_UNSPECIFIED;

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::TASK_STATE_COMPLETED
                | Self::TASK_STATE_FAILED
                | Self::TASK_STATE_CANCELED
                | Self::TASK_STATE_REJECTED
        )
    }
}

pub trait TaskStateExt {
    fn is_terminal(&self) -> bool;
}

impl TaskStateExt for ::buffa::EnumValue<TaskState> {
    fn is_terminal(&self) -> bool {
        match self {
            ::buffa::EnumValue::Known(state) => state.is_terminal(),
            _ => false,
        }
    }
}

impl TaskStatus {
    pub fn new(state: TaskState, message: Option<Message>) -> Self {
        let timestamp = chrono::Utc::now();
        let seconds = timestamp.timestamp();
        let nanos = timestamp.timestamp_subsec_nanos() as i32;

        Self {
            state: ::buffa::EnumValue::from(state),
            message: message.into(),
            timestamp: ::buffa::MessageField::some(::buffa_types::google::protobuf::Timestamp {
                seconds,
                nanos,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    pub fn timestamp_utc(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.timestamp.as_option().and_then(|t| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(t.seconds, t.nanos as u32)
        })
    }
}

/// Parameters for identifying a task by ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskIdParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for querying a task with optional history constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskQueryParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "historyLength")]
    pub history_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Configuration options for sending messages including output modes and notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSendConfiguration {
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "acceptedOutputModes"
    )]
    pub accepted_output_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "historyLength")]
    pub history_length: Option<u32>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "pushNotificationConfig"
    )]
    pub push_notification_config: Option<TaskPushNotificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking: Option<bool>,
}

/// Parameters for sending a message with optional configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSendParams {
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<MessageSendConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for sending a task (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSendParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sessionId")]
    pub session_id: Option<String>,
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none", rename = "pushNotification")]
    pub push_notification: Option<TaskPushNotificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "historyLength")]
    pub history_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for listing tasks with filtering and pagination.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListTasksParams {
    #[serde(skip_serializing_if = "Option::is_none", rename = "contextId")]
    pub context_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskState>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "pageSize")]
    pub page_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "pageToken")]
    pub page_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "historyLength")]
    pub history_length: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "includeArtifacts")]
    pub include_artifacts: Option<bool>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "statusTimestampAfter"
    )]
    pub status_timestamp_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Result object for tasks/list method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTasksResult {
    pub tasks: Vec<Task>,
    #[serde(rename = "totalSize")]
    pub total_size: i32,
    #[serde(rename = "pageSize")]
    pub page_size: i32,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: String,
}

/// Parameters for getting a specific push notification config.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GetTaskPushNotificationConfigParams {
    pub id: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "pushNotificationConfigId"
    )]
    pub push_notification_config_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for listing all push notification configs for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTaskPushNotificationConfigsParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for deleting a push notification config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteTaskPushNotificationConfigParams {
    pub id: String,
    #[serde(rename = "pushNotificationConfigId")]
    pub push_notification_config_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

pub struct TaskBuilder {
    id: String,
    context_id: String,
    status: Option<TaskStatus>,
    artifacts: Vec<Artifact>,
    history: Vec<Message>,
    metadata: Option<::buffa_types::google::protobuf::Struct>,
}

impl TaskBuilder {
    pub fn new() -> Self {
        Self {
            id: String::new(),
            context_id: String::new(),
            status: None,
            artifacts: Vec::new(),
            history: Vec::new(),
            metadata: None,
        }
    }

    pub fn id(mut self, id: String) -> Self {
        self.id = id;
        self
    }

    pub fn context_id(mut self, context_id: String) -> Self {
        self.context_id = context_id;
        self
    }

    pub fn status(mut self, status: TaskStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn artifacts(mut self, artifacts: Vec<Artifact>) -> Self {
        self.artifacts = artifacts;
        self
    }

    pub fn history(mut self, history: Vec<Message>) -> Self {
        self.history = history;
        self
    }

    pub fn metadata(mut self, metadata: ::buffa_types::google::protobuf::Struct) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn build(self) -> Task {
        Task {
            id: self.id,
            context_id: self.context_id,
            status: self
                .status
                .unwrap_or_else(|| TaskStatus::new(TaskState::TASK_STATE_SUBMITTED, None))
                .into(),
            artifacts: self.artifacts,
            history: self.history,
            metadata: self.metadata.into(),
            ..Default::default()
        }
    }
}

impl Default for TaskBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Task {
    pub fn builder() -> TaskBuilder {
        TaskBuilder::new()
    }

    /// Create a new task with the given ID in the submitted state
    pub fn new(id: String, context_id: String) -> Self {
        Self {
            id,
            context_id,
            status: ::buffa::MessageField::some(TaskStatus::new(
                TaskState::TASK_STATE_SUBMITTED,
                None,
            )),
            artifacts: Vec::new(),
            history: Vec::new(),
            metadata: ::buffa::MessageField::none(),
            ..Default::default()
        }
    }

    /// Create a new task with the given ID and context ID in the submitted state
    pub fn with_context(id: String, context_id: String) -> Self {
        Self::new(id, context_id)
    }

    /// Update the task status
    #[cfg_attr(feature = "tracing", instrument(skip(self, message), fields(
        task.id = %self.id,
        task.old_state = ?self.status.as_option().map(|s| &s.state),
        task.new_state = ?state,
        task.has_message = message.is_some()
    )))]
    pub fn update_status(&mut self, state: TaskState, message: Option<Message>) {
        #[cfg(feature = "tracing")]
        tracing::info!("Updating task status");

        self.status = ::buffa::MessageField::some(TaskStatus::new(state, message.clone()));

        if let Some(msg) = message {
            self.history.push(msg);
        }

        #[cfg(feature = "tracing")]
        tracing::info!("Task status updated successfully");
    }

    /// Get a copy of this task with history limited to the specified length
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        task.id = %self.id,
        history.current_size = self.history.len(),
        history.requested_limit = ?history_length
    )))]
    pub fn with_limited_history(&self, history_length: Option<u32>) -> Self {
        if history_length.is_none() {
            #[cfg(feature = "tracing")]
            tracing::debug!("No history truncation needed");
            return self.clone();
        }

        #[cfg(feature = "tracing")]
        let _span = tracing::Span::current();

        let limit: usize = history_length.unwrap().try_into().unwrap_or(usize::MAX);

        #[cfg(feature = "tracing")]
        let mut task_copy = measure_duration!(_span, "operation.duration_ms", { self.clone() });

        #[cfg(not(feature = "tracing"))]
        let mut task_copy = self.clone();

        if limit == 0 {
            #[cfg(feature = "tracing")]
            tracing::debug!("Removing all history (limit = 0)");
            task_copy.history.clear();
        } else if task_copy.history.len() > limit {
            let items_to_skip = task_copy.history.len() - limit;
            #[cfg(feature = "tracing")]
            tracing::debug!(
                "Truncating history from {} to {} items (removing {} oldest)",
                self.history.len(),
                limit,
                items_to_skip
            );
            task_copy.history = task_copy
                .history
                .iter()
                .skip(items_to_skip)
                .cloned()
                .collect();
        }

        task_copy
    }

    /// Add an artifact to the task
    #[cfg_attr(feature = "tracing", instrument(skip(self, artifact), fields(
        task.id = %self.id,
        artifact.id = %artifact.artifact_id,
        artifacts.count = self.artifacts.len()
    )))]
    pub fn add_artifact(&mut self, artifact: Artifact) {
        self.artifacts.push(artifact);
    }

    /// Validate a task (useful after building with builder)
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        task.id = %self.id,
        task.state = ?self.status.as_option().map(|s| &s.state),
        history.size = self.history.len()
    )))]
    pub fn validate(&self) -> Result<(), A2AError> {
        #[cfg(feature = "tracing")]
        tracing::debug!("Validating task");

        let mut message_ids = std::collections::HashSet::new();
        for (_index, message) in self.history.iter().enumerate() {
            #[cfg(feature = "tracing")]
            tracing::trace!("Validating message {} in history", _index);

            if !message_ids.insert(&message.message_id) {
                #[cfg(feature = "tracing")]
                tracing::error!("Duplicate message ID found: {}", message.message_id);
                return Err(A2AError::InvalidParams(format!(
                    "Duplicate message ID in history: {}",
                    message.message_id
                )));
            }
            message.validate()?;
        }

        if let Some(status) = self.status.as_option() {
            if let Some(msg) = status.message.as_option() {
                #[cfg(feature = "tracing")]
                tracing::trace!("Validating status message");
                msg.validate()?;
            }
        }

        #[cfg(feature = "tracing")]
        tracing::debug!("Task validation successful");
        Ok(())
    }
}

/// A task paired with its storage version — the optimistic-concurrency token.
///
/// The version is a monotonic counter the storage adapter bumps on every
/// successful mutation of the task. A caller reads a task and its version, then
/// passes that version back on a conditional update
/// ([`AsyncTaskVersioning::update_status_checked`](crate::port::AsyncTaskVersioning::update_status_checked));
/// if another writer advanced the task in between, the update fails with
/// [`A2AError::VersionConflict`](crate::domain::A2AError::VersionConflict) instead
/// of silently clobbering it.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionedTask {
    /// The task at this version.
    pub task: Task,
    /// The storage version this snapshot was read or written at.
    pub version: u64,
}

impl VersionedTask {
    /// Pair a task with a version.
    pub fn new(task: Task, version: u64) -> Self {
        Self { task, version }
    }
}
