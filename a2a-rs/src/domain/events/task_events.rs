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

impl From<crate::domain::generated::TaskStatusUpdateEvent> for TaskStatusUpdateEvent {
    fn from(event: crate::domain::generated::TaskStatusUpdateEvent) -> Self {
        let metadata = event.metadata.into_option().and_then(|s| {
            if let Ok(serde_json::Value::Object(map)) = serde_json::to_value(s) {
                Some(map)
            } else {
                None
            }
        });
        Self {
            task_id: event.task_id,
            context_id: event.context_id,
            kind: "status-update".to_string(),
            status: event.status.into_option().unwrap_or_default(),
            metadata,
        }
    }
}

impl From<crate::domain::generated::TaskArtifactUpdateEvent> for TaskArtifactUpdateEvent {
    fn from(event: crate::domain::generated::TaskArtifactUpdateEvent) -> Self {
        let metadata = event.metadata.into_option().and_then(|s| {
            if let Ok(serde_json::Value::Object(map)) = serde_json::to_value(s) {
                Some(map)
            } else {
                None
            }
        });
        Self {
            task_id: event.task_id,
            context_id: event.context_id,
            kind: "artifact-update".to_string(),
            artifact: event.artifact.into_option().unwrap_or_default(),
            append: Some(event.append),
            last_chunk: Some(event.last_chunk),
            metadata,
        }
    }
}
