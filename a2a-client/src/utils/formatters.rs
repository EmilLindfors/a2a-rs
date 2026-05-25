//! Formatting utilities for displaying A2A types

use a2a_rs::domain::{Part, TaskState};

/// Format a task state for display
pub fn format_task_state(state: &TaskState) -> String {
    match *state {
        TaskState::Submitted => "Submitted",
        TaskState::Working => "Working",
        TaskState::InputRequired => "Input Required",
        TaskState::Completed => "Completed",
        TaskState::Canceled => "Canceled",
        TaskState::Failed => "Failed",
        TaskState::Rejected => "Rejected",
        TaskState::AuthRequired => "Auth Required",
        TaskState::Unknown => "Unknown",
    }
    .to_string()
}

/// Extract and format text content from message parts
pub fn format_message_content(parts: &[Part]) -> String {
    use a2a_rs::domain::part::Content;
    parts
        .iter()
        .map(|part| match &part.content {
            Some(Content::Text(text)) => text.clone(),
            Some(Content::Raw(_)) => format!(
                "[File: {}]",
                if part.filename.is_empty() { "unnamed" } else { &part.filename }
            ),
            Some(Content::Url(url)) => format!(
                "[File URI: {} ({})]",
                if part.filename.is_empty() { "unnamed" } else { &part.filename },
                url
            ),
            Some(Content::Data(data)) => {
                let name = serde_json::to_value(&**data).ok()
                    .and_then(|v| v.get("name").cloned())
                    .map(|v| {
                        if let serde_json::Value::String(s) = v {
                            s
                        } else {
                            v.to_string()
                        }
                    })
                    .unwrap_or_else(|| "unnamed".to_string());
                format!("[Data: {}]", name)
            }
            None => "[Empty Part]".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Truncate text for preview display
pub fn truncate_preview(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", text.chars().take(max_len).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_rs::domain::Part;

    #[test]
    fn test_format_task_state() {
        assert_eq!(format_task_state(&TaskState::Submitted), "Submitted");
        assert_eq!(format_task_state(&TaskState::Working), "Working");
        assert_eq!(format_task_state(&TaskState::InputRequired), "Input Required");
        assert_eq!(format_task_state(&TaskState::Completed), "Completed");
        assert_eq!(format_task_state(&TaskState::Canceled), "Canceled");
        assert_eq!(format_task_state(&TaskState::Failed), "Failed");
        assert_eq!(format_task_state(&TaskState::Rejected), "Rejected");
        assert_eq!(format_task_state(&TaskState::AuthRequired), "Auth Required");
        assert_eq!(format_task_state(&TaskState::Unknown), "Unknown");
    }

    #[test]
    fn test_format_message_content_text() {
        let parts = vec![Part::text("Hello".to_string())];
        assert_eq!(format_message_content(&parts), "Hello");
    }

    #[test]
    fn test_format_message_content_multiple() {
        let parts = vec![
            Part::text("Here is file".to_string()),
            Part::file_from_bytes(vec![], Some("test.txt".to_string()), None),
            Part::data({
                let mut map = serde_json::Map::new();
                map.insert("name".to_string(), serde_json::Value::String("metadata".to_string()));
                serde_json::from_value(serde_json::Value::Object(map)).unwrap()
            })
        ];
        
        let expected = "Here is file\n[File: test.txt]\n[Data: metadata]";
        assert_eq!(format_message_content(&parts), expected);
    }

    #[test]
    fn test_truncate_preview() {
        assert_eq!(truncate_preview("Short", 10), "Short");
        assert_eq!(truncate_preview("ExactLength", 11), "ExactLength");
        assert_eq!(truncate_preview("This is too long", 10), "This is to...");
        
        // Multi-byte character test
        assert_eq!(truncate_preview("你好世界", 2), "你好...");
    }
}
