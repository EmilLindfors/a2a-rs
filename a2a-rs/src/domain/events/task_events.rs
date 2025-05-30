use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::core::{message::Artifact, task::TaskStatus};

/// Event for task status updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusUpdateEvent {
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(rename = "contextId")]
    pub context_id: String,
    pub kind: String, // Always "status-update"
    pub status: TaskStatus,
    #[serde(rename = "final")]
    pub final_: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Event for task artifact updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskArtifactUpdateEvent {
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(rename = "contextId")]
    pub context_id: String,
    pub kind: String, // Always "artifact-update"
    pub artifact: Artifact,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub append: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastChunk")]
    pub last_chunk: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}
