//! Example demonstrating the new builder patterns for Message and Task

use a2a_rs::domain::{Message, Part, Role, Task, TaskState, TaskStatus};
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== A2A Builder Patterns Demo ===\n");

    // 1. Building Messages with the new builder pattern
    println!("1. Building a Message with the builder pattern:");

    let message_id = format!("msg-{}", Uuid::new_v4());
    let message = Message::builder()
        .role(Role::User)
        .message_id(message_id.clone())
        .parts(vec![
            Part::text("Hello, agent!".to_string()),
            Part::data(serde_json::Map::new()),
        ])
        .task_id("task-123".to_string())
        .build();

    // Validate the message
    message.validate()?;
    println!(
        "  ✓ Built and validated message with ID: {}",
        message.message_id
    );
    println!("  ✓ Message has {} parts", message.parts.len());

    // 2. Building a more complex message with file part
    println!("\n2. Building a Message with file part:");

    let file_part = Part::file_builder()
        .name("example.txt".to_string())
        .mime_type("text/plain".to_string())
        .bytes("SGVsbG8gV29ybGQ=".to_string()) // "Hello World" in base64
        .build()?;

    let complex_message_id = format!("msg-{}", Uuid::new_v4());
    let complex_message = Message::builder()
        .role(Role::Agent)
        .message_id(complex_message_id.clone())
        .parts(vec![
            Part::text("Here's a file for you:".to_string()),
            file_part,
        ])
        .context_id("conversation-456".to_string())
        .build();

    complex_message.validate()?;
    println!("  ✓ Built complex message with file attachment");
    println!("  ✓ Message ID: {}", complex_message.message_id);

    // 3. Building Tasks with the builder pattern
    println!("\n3. Building a Task with the builder pattern:");

    let task_id = format!("task-{}", Uuid::new_v4());
    let context_id = format!("ctx-{}", Uuid::new_v4());

    let task = Task::builder()
        .id(task_id.clone())
        .context_id(context_id.clone())
        .history(vec![message, complex_message])
        .build();

    task.validate()?;
    println!("  ✓ Built and validated task with ID: {}", task.id);
    println!(
        "  ✓ Task has {} messages in history",
        task.history.as_ref().unwrap().len()
    );
    println!("  ✓ Task status: {:?}", task.status.state);

    // 4. Building a Task with custom status
    println!("\n4. Building a Task with custom status:");

    let custom_task_id = format!("task-{}", Uuid::new_v4());
    let working_message_id = format!("msg-{}", Uuid::new_v4());
    let status_message = Message::builder()
        .role(Role::Agent)
        .message_id(working_message_id)
        .parts(vec![Part::text("I'm working on this task...".to_string())])
        .build();

    let working_task = Task::builder()
        .id(custom_task_id.clone())
        .context_id(context_id.clone())
        .status(TaskStatus {
            state: TaskState::Working,
            message: Some(status_message.clone()),
            timestamp: Some(chrono::Utc::now()),
        })
        .history(vec![status_message])
        .build();

    working_task.validate()?;
    println!("  ✓ Built working task with custom status");
    println!("  ✓ Task status: {:?}", working_task.status.state);
    println!(
        "  ✓ Status message: {:?}",
        working_task.status.message.as_ref().unwrap().parts[0]
    );

    // 5. Demonstrating builder flexibility with metadata
    println!("\n5. Building with metadata:");

    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "priority".to_string(),
        serde_json::Value::String("high".to_string()),
    );
    metadata.insert(
        "category".to_string(),
        serde_json::Value::String("support".to_string()),
    );

    let metadata_message_id = format!("msg-{}", Uuid::new_v4());
    let metadata_message = Message::builder()
        .role(Role::User)
        .message_id(metadata_message_id)
        .parts(vec![Part::text(
            "This is a high priority support request".to_string(),
        )])
        .metadata(metadata.clone())
        .reference_task_ids(vec![
            "related-task-1".to_string(),
            "related-task-2".to_string(),
        ])
        .build();

    metadata_message.validate()?;
    println!("  ✓ Built message with metadata and references");
    println!(
        "  ✓ Metadata keys: {:?}",
        metadata_message
            .metadata
            .as_ref()
            .unwrap()
            .keys()
            .collect::<Vec<_>>()
    );
    println!(
        "  ✓ Referenced tasks: {:?}",
        metadata_message.reference_task_ids.as_ref().unwrap()
    );

    println!("\n🎉 All builder patterns work correctly!");
    Ok(())
}
