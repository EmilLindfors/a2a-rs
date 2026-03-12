//! Converter between A2A Task/TaskStatus and MCP CallToolResult

use crate::error::{A2aMcpError, Result};
use crate::converters::MessageConverter;
use a2a_rs::domain::{Message, Part, Role, Task, TaskState};
use rmcp::model::{CallToolResult, Content};

/// Converts between A2A Task and MCP CallToolResult
pub struct TaskResultConverter;

impl TaskResultConverter {
    /// Convert A2A Task to MCP CallToolResult
    ///
    /// Extracts the last agent message and any artifacts as the tool result
    pub fn task_to_result(task: &Task) -> Result<CallToolResult> {
        // Get history or create empty vec
        let history = task.history.as_ref().map(|h| h.as_slice()).unwrap_or(&[]);

        // Find the last agent message in the task
        let agent_message = history
            .iter()
            .rev()
            .find(|m| m.role == Role::Agent)
            .or_else(|| history.last())
            .ok_or_else(|| A2aMcpError::InvalidMessage("No messages in task history".to_string()))?;

        // Convert the message to MCP content
        let mut content = MessageConverter::message_to_content(agent_message)?;

        // Add artifacts as additional content if available
        if let Some(artifacts) = &task.artifacts {
            for artifact in artifacts {
                // Artifacts have parts, so convert each part
                for part in &artifact.parts {
                    match part {
                        Part::Text { text, .. } => {
                            let artifact_text = if let Some(ref name) = artifact.name {
                                format!("Artifact '{}': {}", name, text)
                            } else {
                                format!("Artifact: {}", text)
                            };
                            content.push(Content::text(artifact_text));
                        }
                        Part::File { file, .. } => {
                            let file_text = if let Some(ref name) = artifact.name {
                                format!("Artifact '{}': File {:?}", name, file.name)
                            } else {
                                format!("Artifact File: {:?}", file.name)
                            };
                            content.push(Content::text(file_text));
                        }
                        Part::Data { data, .. } => {
                            let data_json = serde_json::to_string_pretty(&serde_json::Value::Object(data.clone()))?;
                            let artifact_data = if let Some(ref name) = artifact.name {
                                format!("Artifact '{}' data:\n{}", name, data_json)
                            } else {
                                format!("Artifact data:\n{}", data_json)
                            };
                            content.push(Content::text(artifact_data));
                        }
                    }
                }
            }
        }

        // Determine if the task completed successfully
        let is_error = matches!(
            task.status.state,
            TaskState::Failed | TaskState::Rejected | TaskState::Canceled
        );

        Ok(if is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        })
    }

    /// Create a simple success result from text
    pub fn success_from_text(text: impl Into<String>) -> CallToolResult {
        CallToolResult::success(vec![Content::text(text.into())])
    }

    /// Create a simple error result from text
    pub fn error_from_text(text: impl Into<String>) -> CallToolResult {
        CallToolResult::error(vec![Content::text(text.into())])
    }

    /// Convert MCP content to an A2A message that can be added to a task
    pub fn content_to_task_message(content: &[Content], role: Role) -> Result<Message> {
        MessageConverter::content_to_message(content, role)
    }

    /// Check if a task is in a final state
    pub fn is_task_final(task: &Task) -> bool {
        matches!(
            task.status.state,
            TaskState::Completed | TaskState::Failed | TaskState::Rejected | TaskState::Canceled
        )
    }

    /// Check if a task completed successfully
    pub fn is_task_successful(task: &Task) -> bool {
        task.status.state == TaskState::Completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_rs::domain::{Artifact, Part, Role, TaskStatus};

    #[test]
    fn test_task_to_result_success() {
        let task = Task::builder()
            .id("task-1".to_string())
            .context_id("ctx-1".to_string())
            .status(TaskStatus {
                state: TaskState::Completed,
                message: None,
                timestamp: None,
            })
            .history(vec![Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::Text {
                    text: "Result text".to_string(),
                    metadata: None,
                }])
                .message_id("msg-1".to_string())
                .build()])
            .artifacts(vec![Artifact {
                artifact_id: "art-1".to_string(),
                name: Some("Test Artifact".to_string()),
                description: None,
                parts: vec![Part::Text {
                    text: "Additional artifact".to_string(),
                    metadata: None,
                }],
                metadata: None,
                extensions: None,
            }])
            .build();

        let result = TaskResultConverter::task_to_result(&task).unwrap();
        assert!(!result.is_error.unwrap_or(false));
        assert!(result.content.len() >= 2); // Message + artifact
    }

    #[test]
    fn test_task_to_result_error() {
        let task = Task::builder()
            .id("task-2".to_string())
            .context_id("ctx-2".to_string())
            .status(TaskStatus {
                state: TaskState::Failed,
                message: None,
                timestamp: None,
            })
            .history(vec![Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::Text {
                    text: "Error details".to_string(),
                    metadata: None,
                }])
                .message_id("msg-2".to_string())
                .build()])
            .build();

        let result = TaskResultConverter::task_to_result(&task).unwrap();
        assert!(result.is_error.unwrap_or(false));
    }

    #[test]
    fn test_is_task_final() {
        let completed = Task::builder()
            .id("1".to_string())
            .context_id("ctx-1".to_string())
            .status(TaskStatus {
                state: TaskState::Completed,
                message: None,
                timestamp: None,
            })
            .build();
        assert!(TaskResultConverter::is_task_final(&completed));

        let working = Task::builder()
            .id("2".to_string())
            .context_id("ctx-2".to_string())
            .status(TaskStatus {
                state: TaskState::Working,
                message: None,
                timestamp: None,
            })
            .build();
        assert!(!TaskResultConverter::is_task_final(&working));
    }
}
