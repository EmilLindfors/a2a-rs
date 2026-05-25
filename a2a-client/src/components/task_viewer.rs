//! Generic task viewing components

use a2a_rs::domain::{Task, part};
use serde::Serialize;

/// View model for a task in a list
#[derive(Debug, Serialize, Clone)]
pub struct TaskView {
    pub task_id: String,
    pub state: String,
    pub message_count: usize,
    pub last_message_preview: Option<String>,
}

impl TaskView {
    /// Create a TaskView from an A2A Task
    pub fn from_task(task: Task) -> Self {
        let message_count = task.history.len();
        let last_message_preview = task.history.last().and_then(|msg| {
            msg.parts.iter().find_map(|part| {
                part.get_text()
                    .map(|text| text.chars().take(100).collect::<String>())
            })
        });

        let state = match task.status.as_option().map(|s| &s.state) {
            Some(::buffa::EnumValue::Known(state)) => match state {
                a2a_rs::domain::TaskState::TASK_STATE_SUBMITTED => "Submitted".to_string(),
                a2a_rs::domain::TaskState::TASK_STATE_WORKING => "Working".to_string(),
                a2a_rs::domain::TaskState::TASK_STATE_INPUT_REQUIRED => "InputRequired".to_string(),
                a2a_rs::domain::TaskState::TASK_STATE_COMPLETED => "Completed".to_string(),
                a2a_rs::domain::TaskState::TASK_STATE_CANCELED => "Canceled".to_string(),
                a2a_rs::domain::TaskState::TASK_STATE_FAILED => "Failed".to_string(),
                a2a_rs::domain::TaskState::TASK_STATE_REJECTED => "Rejected".to_string(),
                a2a_rs::domain::TaskState::TASK_STATE_AUTH_REQUIRED => "AuthRequired".to_string(),
                a2a_rs::domain::TaskState::TASK_STATE_UNSPECIFIED => "Unknown".to_string(),
            },
            Some(::buffa::EnumValue::Unknown(num)) => format!("Unknown({})", num),
            None => "Unknown".to_string(),
        };

        Self {
            task_id: task.id,
            state,
            message_count,
            last_message_preview,
        }
    }
}

/// View model for a single message
#[derive(Debug, Serialize, Clone)]
pub struct MessageView {
    pub id: String,
    pub role: String,
    pub content: String,
}

impl MessageView {
    /// Create a MessageView from an A2A Message
    pub fn from_message(msg: a2a_rs::domain::Message) -> Self {
        // Extract text content from message parts
        let content = msg
            .parts
            .iter()
            .map(|part| match &part.content {
                Some(part::Content::Text(text)) => text.clone(),
                Some(part::Content::Raw(_)) => format!(
                    "[File: {}]",
                    if part.filename.is_empty() {
                        "unnamed"
                    } else {
                        &part.filename
                    }
                ),
                Some(part::Content::Url(url)) => format!(
                    "[URL/File: {}]",
                    if part.filename.is_empty() {
                        url
                    } else {
                        &part.filename
                    }
                ),
                Some(part::Content::Data(data)) => {
                    let name = serde_json::to_value(&**data)
                        .ok()
                        .and_then(|v| {
                            v.get("name")
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string())
                        })
                        .unwrap_or_else(|| "unnamed".to_string());
                    format!("[Data: {}]", name)
                }
                None => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");

        Self {
            id: msg.message_id,
            role: match &msg.role {
                ::buffa::EnumValue::Known(a2a_rs::domain::Role::ROLE_USER) => "User".to_string(),
                ::buffa::EnumValue::Known(a2a_rs::domain::Role::ROLE_AGENT) => "Agent".to_string(),
                ::buffa::EnumValue::Known(r) => format!("{:?}", r),
                ::buffa::EnumValue::Unknown(num) => format!("Unknown({})", num),
            },
            content,
        }
    }

    /// Create a MessageView with JSON parsing for structured responses
    pub fn from_message_with_json_parsing(msg: a2a_rs::domain::Message) -> Self {
        let content = msg
            .parts
            .iter()
            .filter_map(|part| part.get_text().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join("\n");

        // Try to parse as JSON for better display
        let display_content =
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(obj) = json_value.as_object() {
                    match obj.get("type").and_then(|v| v.as_str()) {
                        Some("form") => obj
                            .get("instructions")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Please fill out the form.")
                            .to_string(),
                        Some("result") => {
                            let message = obj
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Request processed.");
                            let status = obj
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            format!("{}\n\nStatus: {}", message, status)
                        }
                        _ => serde_json::to_string_pretty(&json_value).unwrap_or(content.clone()),
                    }
                } else {
                    content.clone()
                }
            } else {
                content.clone()
            };

        Self {
            id: msg.message_id,
            role: match &msg.role {
                ::buffa::EnumValue::Known(a2a_rs::domain::Role::ROLE_USER) => "User".to_string(),
                ::buffa::EnumValue::Known(a2a_rs::domain::Role::ROLE_AGENT) => "Agent".to_string(),
                ::buffa::EnumValue::Known(r) => format!("{:?}", r),
                ::buffa::EnumValue::Unknown(num) => format!("Unknown({})", num),
            },
            content: display_content,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_rs::domain::{Message, Task, TaskState, TaskStatus};

    #[test]
    fn test_task_view_from_task() {
        let mut task = Task::new("task-123".to_string(), "ctx-1".to_string());
        task.status = ::buffa::MessageField::some(TaskStatus::new(TaskState::Working, None));

        // Add a message to history
        let msg = Message::user_text("A preview text here".to_string(), "msg-1".to_string());
        task.history = vec![msg];

        let view = TaskView::from_task(task);
        assert_eq!(view.task_id, "task-123");
        assert_eq!(view.state, "Working");
        assert_eq!(view.message_count, 1);
        assert_eq!(
            view.last_message_preview,
            Some("A preview text here".to_string())
        );
    }

    #[test]
    fn test_task_view_empty_history() {
        let task = Task::new("task-123".to_string(), "ctx-1".to_string());
        let view = TaskView::from_task(task);
        assert_eq!(view.message_count, 0);
        assert_eq!(view.last_message_preview, None);
    }

    #[test]
    fn test_message_view_from_message() {
        let msg = Message::user_text("Hello world".to_string(), "msg-1".to_string());
        let view = MessageView::from_message(msg);

        assert_eq!(view.id, "msg-1");
        assert_eq!(view.role, "User");
        assert_eq!(view.content, "Hello world");
    }

    #[test]
    fn test_message_view_with_json_parsing_result() {
        let json_content = r#"{"type":"result","message":"Success!","status":"ok"}"#;
        let msg = Message::user_text(json_content.to_string(), "msg-2".to_string());

        let view = MessageView::from_message_with_json_parsing(msg);
        assert_eq!(view.content, "Success!\n\nStatus: ok");
    }

    #[test]
    fn test_message_view_with_json_parsing_form() {
        let json_content = r#"{"type":"form","instructions":"Please provide name"}"#;
        let msg = Message::user_text(json_content.to_string(), "msg-3".to_string());

        let view = MessageView::from_message_with_json_parsing(msg);
        assert_eq!(view.content, "Please provide name");
    }
}
