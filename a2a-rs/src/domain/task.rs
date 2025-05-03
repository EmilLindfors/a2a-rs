use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{
    agent::PushNotificationConfig,
    message::{Artifact, Message},
};

/// States a task can be in
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Submitted,
    Working,
    InputRequired,
    Completed,
    Canceled,
    Failed,
    Unknown,
}

/// Status of a task including state, message, and timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    pub state: TaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

/// A task in the A2A protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sessionId")]
    pub session_id: Option<String>,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<Artifact>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<Message>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for identifying a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskIdParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for querying a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskQueryParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "historyLength")]
    pub history_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for sending a task
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
    pub id: String,
    #[serde(rename = "pushNotificationConfig")]
    pub push_notification_config: PushNotificationConfig,
}

/// Event for task status updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusUpdateEvent {
    pub id: String,
    pub status: TaskStatus,
    #[serde(rename = "final")]
    pub final_: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Event for task artifact updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskArtifactUpdateEvent {
    pub id: String,
    pub artifact: Artifact,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

impl Task {
    /// Create a new task with the given ID in the submitted state
    pub fn new(id: String) -> Self {
        Self {
            id,
            session_id: None,
            status: TaskStatus {
                state: TaskState::Submitted,
                message: None,
                timestamp: Some(Utc::now()),
            },
            artifacts: None,
            history: None,
            metadata: None,
        }
    }

    /// Create a new task with the given ID and session ID in the submitted state
    pub fn with_session(id: String, session_id: String) -> Self {
        Self {
            id,
            session_id: Some(session_id),
            status: TaskStatus {
                state: TaskState::Submitted,
                message: None,
                timestamp: Some(Utc::now()),
            },
            artifacts: None,
            history: None,
            metadata: None,
        }
    }

    /// Update the task status
    pub fn update_status(&mut self, state: TaskState, message: Option<Message>) {
        // Set the new status
        self.status = TaskStatus {
            state,
            message: message.clone(),
            timestamp: Some(Utc::now()),
        };

        // Add message to history if provided and state_transition_history is enabled
        if let Some(msg) = message {
            if let Some(history) = &mut self.history {
                history.push(msg);
            } else {
                self.history = Some(vec![msg]);
            }
        }
    }

    /// Get a copy of this task with history limited to the specified length
    ///
    /// This method follows the A2A spec for history truncation:
    /// - If no history_length is provided, returns the full history
    /// - If history_length is 0, removes history entirely
    /// - If history_length is less than the current history size,
    ///   keeps only the most recent messages (truncates from the beginning)
    pub fn with_limited_history(&self, history_length: Option<u32>) -> Self {
        // If no history limit specified or no history, return as is
        if history_length.is_none() || self.history.is_none() {
            return self.clone();
        }

        let limit = history_length.unwrap() as usize;
        let mut task_copy = self.clone();

        // Limit history if specified
        if let Some(history) = &mut task_copy.history {
            if limit == 0 {
                // If limit is 0, remove history entirely
                task_copy.history = None;
            } else if history.len() > limit {
                // If history is longer than limit, truncate it
                // Keep the most recent messages by removing from the beginning
                // For example, if history has 10 items and limit is 3, we skip 7 items (10-3)
                // and keep items 8, 9, and 10
                *history = history
                    .iter()
                    .skip(history.len() - limit)
                    .cloned()
                    .collect();
            }
            // Otherwise, if history.len() <= limit, we keep the full history
        }

        task_copy
    }

    /// Add an artifact to the task
    pub fn add_artifact(&mut self, artifact: Artifact) {
        if let Some(artifacts) = &mut self.artifacts {
            artifacts.push(artifact);
        } else {
            self.artifacts = Some(vec![artifact]);
        }
    }
}
