use bon::Builder;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(feature = "tracing")]
use tracing::instrument;

use super::{
    agent::PushNotificationConfig,
    message::{Artifact, Message},
};

#[cfg(feature = "tracing")]
use crate::measure_duration;

/// States a task can be in during its lifecycle.
///
/// Tasks progress through various states from submission to completion:
/// - `Submitted`: Task has been received and is queued for processing
/// - `Working`: Task is currently being processed
/// - `InputRequired`: Task needs additional input from the user
/// - `Completed`: Task has finished successfully
/// - `Canceled`: Task was canceled before completion
/// - `Failed`: Task encountered an error and could not complete
/// - `Rejected`: Task was rejected (invalid, unauthorized, etc.)
/// - `AuthRequired`: Task requires authentication to proceed
/// - `Unknown`: Task state could not be determined
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Submitted,
    Working,
    InputRequired,
    Completed,
    Canceled,
    Failed,
    Rejected,
    AuthRequired,
    Unknown,
}

/// Status of a task including state, optional message, and timestamp.
///
/// Represents a point-in-time status of a task, including its current state,
/// an optional status message providing additional context, and the timestamp
/// when this status was recorded.
///
/// # Example
/// ```rust
/// use a2a_rs::{TaskStatus, TaskState};
/// use chrono::Utc;
/// 
/// let status = TaskStatus {
///     state: TaskState::Working,
///     message: None,
///     timestamp: Some(Utc::now()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    pub state: TaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self {
            state: TaskState::Submitted,
            message: None,
            timestamp: Some(Utc::now()),
        }
    }
}

/// A task in the A2A protocol with status, history, and artifacts.
///
/// Tasks represent units of work that agents process. Each task has:
/// - A unique ID and context ID for tracking
/// - Current status including state and optional message
/// - Optional artifacts produced during processing
/// - Optional message history for the conversation
/// - Optional metadata for additional context
///
/// # Example
/// ```rust
/// use a2a_rs::{Task, TaskStatus, TaskState};
/// 
/// let task = Task::builder()
///     .id("task-123".to_string())
///     .context_id("ctx-456".to_string())
///     .status(TaskStatus {
///         state: TaskState::Working,
///         message: None,
///         timestamp: None,
///     })
///     .build();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct Task {
    pub id: String,
    #[serde(rename = "contextId")]
    pub context_id: String,
    #[builder(default = TaskStatus::default())]
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<Artifact>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<Message>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    #[builder(default = "task".to_string())]
    pub kind: String, // Always "task"
}

/// Parameters for identifying a task by ID.
///
/// Simple structure containing a task ID and optional metadata
/// for task identification in API requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskIdParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for querying a task with optional history constraints.
///
/// Allows querying a task by ID with optional limits on the amount
/// of history to return and additional metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskQueryParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "historyLength")]
    pub history_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Configuration options for sending messages including output modes and notifications.
///
/// Specifies how a message should be processed and delivered:
/// - `accepted_output_modes`: Output formats the client can handle
/// - `history_length`: Limit on conversation history to include
/// - `push_notification_config`: Settings for push notifications
/// - `blocking`: Whether the request should wait for completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSendConfiguration {
    #[serde(rename = "acceptedOutputModes")]
    pub accepted_output_modes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "historyLength")]
    pub history_length: Option<u32>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "pushNotificationConfig"
    )]
    pub push_notification_config: Option<PushNotificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking: Option<bool>,
}

/// Parameters for sending a message with optional configuration.
///
/// Contains the message to send along with optional configuration
/// that controls how the message is processed and delivered.
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
    pub push_notification: Option<PushNotificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "historyLength")]
    pub history_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Configuration for task push notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPushNotificationConfig {
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(rename = "pushNotificationConfig")]
    pub push_notification_config: PushNotificationConfig,
}

impl Task {
    /// Create a new task with the given ID in the submitted state
    pub fn new(id: String, context_id: String) -> Self {
        Self {
            id,
            context_id,
            status: TaskStatus {
                state: TaskState::Submitted,
                message: None,
                timestamp: Some(Utc::now()),
            },
            artifacts: None,
            history: None,
            metadata: None,
            kind: "task".to_string(),
        }
    }

    /// Create a new task with the given ID and context ID in the submitted state
    pub fn with_context(id: String, context_id: String) -> Self {
        Self::new(id, context_id)
    }

    /// Update the task status
    #[cfg_attr(feature = "tracing", instrument(skip(self, message), fields(
        task.id = %self.id,
        task.old_state = ?self.status.state,
        task.new_state = ?state,
        task.has_message = message.is_some()
    )))]
    pub fn update_status(&mut self, state: TaskState, message: Option<Message>) {
        #[cfg(feature = "tracing")]
        tracing::info!("Updating task status");

        // Set the new status
        self.status = TaskStatus {
            state: state.clone(),
            message: message.clone(),
            timestamp: Some(Utc::now()),
        };

        // Add message to history if provided and state_transition_history is enabled
        if let Some(msg) = message {
            if let Some(history) = &mut self.history {
                #[cfg(feature = "tracing")]
                tracing::debug!(
                    "Adding message to existing history (size: {})",
                    history.len()
                );
                history.push(msg);
            } else {
                #[cfg(feature = "tracing")]
                tracing::debug!("Creating new history with message");
                self.history = Some(vec![msg]);
            }
        }

        #[cfg(feature = "tracing")]
        tracing::info!("Task status updated successfully");
    }

    /// Get a copy of this task with history limited to the specified length
    ///
    /// This method follows the A2A spec for history truncation:
    /// - If no history_length is provided, returns the full history
    /// - If history_length is 0, removes history entirely
    /// - If history_length is less than the current history size,
    ///   keeps only the most recent messages (truncates from the beginning)
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        task.id = %self.id,
        history.current_size = self.history.as_ref().map(|h| h.len()).unwrap_or(0),
        history.requested_limit = ?history_length
    )))]
    pub fn with_limited_history(&self, history_length: Option<u32>) -> Self {
        // If no history limit specified or no history, return as is
        if history_length.is_none() || self.history.is_none() {
            #[cfg(feature = "tracing")]
            tracing::debug!("No history truncation needed");
            return self.clone();
        }

        #[cfg(feature = "tracing")]
        let _span = tracing::Span::current();

        let limit = history_length.unwrap() as usize;

        #[cfg(feature = "tracing")]
        let mut task_copy = measure_duration!(_span, "operation.duration_ms", { self.clone() });

        #[cfg(not(feature = "tracing"))]
        let mut task_copy = self.clone();

        // Limit history if specified
        if let Some(history) = &mut task_copy.history {
            let original_size = history.len();

            if limit == 0 {
                // If limit is 0, remove history entirely
                #[cfg(feature = "tracing")]
                tracing::debug!("Removing all history (limit = 0)");
                task_copy.history = None;
            } else if history.len() > limit {
                // If history is longer than limit, truncate it
                // Keep the most recent messages by removing from the beginning
                // For example, if history has 10 items and limit is 3, we skip 7 items (10-3)
                // and keep items 8, 9, and 10
                let items_to_skip = history.len() - limit;
                #[cfg(feature = "tracing")]
                tracing::debug!(
                    "Truncating history from {} to {} items (removing {} oldest)",
                    original_size,
                    limit,
                    items_to_skip
                );

                *history = history.iter().skip(items_to_skip).cloned().collect();
            } else {
                #[cfg(feature = "tracing")]
                tracing::debug!("History size ({}) within limit ({})", original_size, limit);
            }
            // Otherwise, if history.len() <= limit, we keep the full history
        }

        task_copy
    }

    /// Add an artifact to the task
    #[cfg_attr(feature = "tracing", instrument(skip(self, artifact), fields(
        task.id = %self.id,
        artifact.id = %artifact.artifact_id,
        artifacts.count = self.artifacts.as_ref().map(|a| a.len()).unwrap_or(0)
    )))]
    pub fn add_artifact(&mut self, artifact: Artifact) {
        if let Some(artifacts) = &mut self.artifacts {
            #[cfg(feature = "tracing")]
            tracing::debug!("Adding artifact to existing list");
            artifacts.push(artifact);
        } else {
            #[cfg(feature = "tracing")]
            tracing::debug!("Creating new artifacts list with artifact");
            self.artifacts = Some(vec![artifact]);
        }
    }

    /// Validate a task (useful after building with builder)
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        task.id = %self.id,
        task.kind = %self.kind,
        task.state = ?self.status.state,
        history.size = self.history.as_ref().map(|h| h.len()).unwrap_or(0)
    )))]
    pub fn validate(&self) -> Result<(), crate::domain::A2AError> {
        #[cfg(feature = "tracing")]
        tracing::debug!("Validating task");

        // Validate that kind is "task"
        if self.kind != "task" {
            #[cfg(feature = "tracing")]
            tracing::error!("Invalid task kind: {}", self.kind);
            return Err(crate::domain::A2AError::InvalidParams(
                "Task kind must be 'task'".to_string(),
            ));
        }

        // Validate message IDs are unique if history exists
        if let Some(ref hist) = &self.history {
            #[cfg(feature = "tracing")]
            tracing::trace!("Checking for duplicate message IDs in history");

            let mut message_ids = std::collections::HashSet::new();
            for message in hist {
                if !message_ids.insert(&message.message_id) {
                    #[cfg(feature = "tracing")]
                    tracing::error!("Duplicate message ID found: {}", message.message_id);
                    return Err(crate::domain::A2AError::InvalidParams(format!(
                        "Duplicate message ID in history: {}",
                        message.message_id
                    )));
                }
            }
        }

        // Validate all messages in history
        if let Some(ref hist) = &self.history {
            #[cfg(feature = "tracing")]
            tracing::trace!("Validating {} messages in history", hist.len());

            for (index, message) in hist.iter().enumerate() {
                #[cfg(feature = "tracing")]
                tracing::trace!("Validating message {} in history", index);
                message.validate()?;
            }
        }

        // Validate status message if present
        if let Some(ref msg) = &self.status.message {
            #[cfg(feature = "tracing")]
            tracing::trace!("Validating status message");
            msg.validate()?;
        }

        #[cfg(feature = "tracing")]
        tracing::debug!("Task validation successful");
        Ok(())
    }
}
