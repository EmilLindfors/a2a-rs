#[cfg(test)]
mod task_tests {
    use crate::domain::{Message, Task, TaskState};

    #[test]
    fn test_task_history_tracking() {
        // Create a new task
        let mut task = Task::new("test-task-1".to_string(), "test-context-1".to_string());

        // Create messages using helper methods
        let message1 = Message::user_text("Message 1".to_string(), "msg1".to_string());
        let message2 = Message::agent_text("Message 2".to_string(), "msg2".to_string());
        let message3 = Message::user_text("Message 3".to_string(), "msg3".to_string());

        // Update the task with messages
        task.update_status(TaskState::Working, Some(message1.clone()));
        task.update_status(TaskState::Working, Some(message2.clone()));
        task.update_status(TaskState::Working, Some(message3.clone()));

        // Verify history has all messages
        assert!(task.history.is_some());
        let history = task.history.as_ref().unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].parts[0].get_text().unwrap(), "Message 1");
        assert_eq!(history[1].parts[0].get_text().unwrap(), "Message 2");
        assert_eq!(history[2].parts[0].get_text().unwrap(), "Message 3");

        // Test history truncation with with_limited_history
        let task_limited = task.with_limited_history(Some(2));
        assert!(task_limited.history.is_some());
        let history_limited = task_limited.history.unwrap();
        assert_eq!(history_limited.len(), 2);

        // Should have the most recent 2 messages (message2, message3)
        assert_eq!(history_limited[0].parts[0].get_text().unwrap(), "Message 2");
        assert_eq!(history_limited[1].parts[0].get_text().unwrap(), "Message 3");

        // Test removing history entirely
        let task_no_history = task.with_limited_history(Some(0));
        assert!(task_no_history.history.is_none());
    }
}
