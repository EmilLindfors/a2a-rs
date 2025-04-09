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
            metadata: None,
        }
    }

    /// Update the task status
    pub fn update_status(&mut self, state: TaskState, message: Option<Message>) {
        self.status = TaskStatus {
            state,
            message,
            timestamp: Some(Utc::now()),
        };
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