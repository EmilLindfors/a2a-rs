//! Converter between A2A Task/TaskStatus and MCP CallToolResult

use crate::converters::MessageConverter;
use crate::error::{A2aMcpError, Result};
use a2a_rs::domain::{Message, Role, Task, TaskState};
use rmcp::model::{CallToolResult, Content};

/// Converts between A2A Task and MCP CallToolResult
pub struct TaskResultConverter;

impl TaskResultConverter {
    /// Convert A2A Task to MCP CallToolResult
    ///
    /// Extracts the last agent message and any artifacts as the tool result
    pub fn task_to_result(task: &Task) -> Result<CallToolResult> {
        // Get history
        let history = &task.history;

        // Find the last agent message in the task
        let agent_message = history
            .iter()
            .rev()
            .find(|m| m.role == buffa::enumeration::EnumValue::Known(Role::ROLE_AGENT))
            .or_else(|| history.last())
            .ok_or_else(|| {
                A2aMcpError::InvalidMessage("No messages in task history".to_string())
            })?;

        // Convert the message to MCP content
        let mut content = MessageConverter::message_to_content(agent_message)?;

        // Add artifacts as additional content if available
        for artifact in &task.artifacts {
            // Artifacts have parts, so convert each part
            for part in &artifact.parts {
                use a2a_rs::domain::generated::part;
                match &part.content {
                    Some(part::Content::Text(text)) => {
                        let artifact_text = if !artifact.name.is_empty() {
                            format!("Artifact '{}': {}", artifact.name, text)
                        } else {
                            format!("Artifact: {}", text)
                        };
                        content.push(Content::text(artifact_text));
                    }
                    Some(part::Content::Raw(_)) | Some(part::Content::Url(_)) => {
                        let file_text = if !artifact.name.is_empty() {
                            format!("Artifact '{}': File {:?}", artifact.name, part.filename)
                        } else {
                            format!("Artifact File: {:?}", part.filename)
                        };
                        content.push(Content::text(file_text));
                    }
                    Some(part::Content::Data(value)) => {
                        let data_json = serde_json::to_string_pretty(&value)?;
                        let artifact_data = if !artifact.name.is_empty() {
                            format!("Artifact '{}' data:\n{}", artifact.name, data_json)
                        } else {
                            format!("Artifact data:\n{}", data_json)
                        };
                        content.push(Content::text(artifact_data));
                    }
                    None => {}
                }
            }
        }

        let is_input_required = task
            .status
            .as_option()
            .map(|s| {
                s.state
                    == buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_INPUT_REQUIRED)
            })
            .unwrap_or(false);

        if is_input_required {
            let prompt = format!(
                "\n\n[Task suspended awaiting input. To continue, please call this tool again with `task_id`: '{}' and provide the requested information in the `message` parameter.]",
                task.id
            );
            content.push(Content::text(prompt));
        }

        // Determine if the task completed successfully
        let is_error = task
            .status
            .as_option()
            .map(|s| {
                matches!(
                    s.state,
                    buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_FAILED)
                        | buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_REJECTED)
                        | buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_CANCELED)
                )
            })
            .unwrap_or(false);

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
        task.status
            .as_option()
            .map(|s| {
                matches!(
                    s.state,
                    buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_COMPLETED)
                        | buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_FAILED)
                        | buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_REJECTED)
                        | buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_CANCELED)
                )
            })
            .unwrap_or(false)
    }

    /// Check if a task completed successfully
    pub fn is_task_successful(task: &Task) -> bool {
        task.status
            .as_option()
            .map(|s| {
                s.state == buffa::enumeration::EnumValue::Known(TaskState::TASK_STATE_COMPLETED)
            })
            .unwrap_or(false)
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
            .status(TaskStatus::new(TaskState::TASK_STATE_COMPLETED, None))
            .history(vec![Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text("Result text".to_string())])
                .message_id("msg-1".to_string())
                .build()])
            .artifacts(vec![Artifact {
                artifact_id: "art-1".to_string(),
                name: "Test Artifact".to_string(),
                description: String::new(),
                parts: vec![Part::text("Additional artifact".to_string())],
                metadata: None.into(),
                extensions: Vec::new(),
                ..Default::default()
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
            .status(TaskStatus::new(TaskState::TASK_STATE_FAILED, None))
            .history(vec![Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text("Error details".to_string())])
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
            .status(TaskStatus::new(TaskState::TASK_STATE_COMPLETED, None))
            .build();
        assert!(TaskResultConverter::is_task_final(&completed));

        let working = Task::builder()
            .id("2".to_string())
            .context_id("ctx-2".to_string())
            .status(TaskStatus::new(TaskState::TASK_STATE_WORKING, None))
            .build();
        assert!(!TaskResultConverter::is_task_final(&working));
    }
}
