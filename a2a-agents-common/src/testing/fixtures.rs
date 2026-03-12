//! Test fixtures for creating A2A protocol test data.

use serde_json::{Value, json};

/// Create a simple text message for testing.
///
/// # Example
///
/// ```
/// use a2a_agents_common::testing::test_message;
///
/// let message = test_message("Hello, agent!");
/// ```
pub fn test_message(content: &str) -> Value {
    json!({
        "role": "user",
        "parts": [
            {
                "type": "text",
                "content": content
            }
        ]
    })
}

/// Create a test message with metadata.
pub fn test_message_with_metadata(content: &str, metadata: Value) -> Value {
    let mut message = test_message(content);
    if let Some(obj) = message.as_object_mut() {
        obj.insert("metadata".to_string(), metadata);
    }
    message
}

/// Create a test task with the given ID and state.
pub fn test_task(task_id: &str, state: &str) -> Value {
    json!({
        "id": task_id,
        "state": state,
        "status": {
            "type": "message",
            "message": {
                "role": "agent",
                "parts": []
            }
        },
        "messages": []
    })
}

/// Create a test task status update event.
pub fn test_status_update(task_id: &str, state: &str, message_content: &str) -> Value {
    json!({
        "taskId": task_id,
        "state": state,
        "status": {
            "type": "message",
            "message": {
                "role": "agent",
                "parts": [
                    {
                        "type": "text",
                        "content": message_content
                    }
                ]
            }
        }
    })
}

/// Create a test artifact update event.
pub fn test_artifact_update(task_id: &str, artifact_type: &str, content: Value) -> Value {
    json!({
        "taskId": task_id,
        "artifact": {
            "type": artifact_type,
            "content": content
        }
    })
}

/// Create sample stock quote data for testing.
pub fn sample_stock_quote(symbol: &str, price: f64, change_percent: f64) -> Value {
    json!({
        "symbol": symbol,
        "price": price,
        "change": change_percent,
        "timestamp": "2025-12-06T10:00:00Z"
    })
}

/// Create sample expense data for testing.
pub fn sample_expense(amount: f64, category: &str, description: &str) -> Value {
    json!({
        "amount": amount,
        "category": category,
        "description": description,
        "date": "2025-12-06"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = test_message("Hello");
        assert_eq!(msg["role"], "user");
        assert_eq!(msg["parts"][0]["type"], "text");
        assert_eq!(msg["parts"][0]["content"], "Hello");
    }

    #[test]
    fn test_message_metadata() {
        let metadata = json!({"key": "value"});
        let msg = test_message_with_metadata("Hello", metadata.clone());
        assert_eq!(msg["metadata"], metadata);
    }

    #[test]
    fn test_task_creation() {
        let task = test_task("task-123", "working");
        assert_eq!(task["id"], "task-123");
        assert_eq!(task["state"], "working");
    }

    #[test]
    fn test_status_update_creation() {
        let update = test_status_update("task-123", "completed", "Done!");
        assert_eq!(update["taskId"], "task-123");
        assert_eq!(update["state"], "completed");
        assert_eq!(update["status"]["message"]["parts"][0]["content"], "Done!");
    }

    #[test]
    fn test_stock_quote() {
        let quote = sample_stock_quote("AAPL", 178.42, 1.22);
        assert_eq!(quote["symbol"], "AAPL");
        assert_eq!(quote["price"], 178.42);
    }

    #[test]
    fn test_expense() {
        let expense = sample_expense(150.50, "Travel", "Taxi to airport");
        assert_eq!(expense["amount"], 150.50);
        assert_eq!(expense["category"], "Travel");
    }
}
