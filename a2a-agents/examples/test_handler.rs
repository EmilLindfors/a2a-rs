use a2a_agents::reimbursement_agent::handler::ReimbursementHandler;
use a2a_rs::domain::{Message, Part, Role};
use a2a_rs::port::message_handler::AsyncMessageHandler;
use serde_json::json;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("debug").init();

    // Create handler
    let handler = ReimbursementHandler::new();

    println!("=== Testing Reimbursement Handler ===\n");

    // Test 1: Text-based request
    println!("1. Testing text-based request:");
    let text_message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text(
            "I need to get reimbursed $150.50 for client lunch at downtown restaurant".to_string(),
        )])
        .message_id(Uuid::new_v4().to_string())
        .context_id("conv-123".to_string())
        .build();

    let task_id = format!("task_{}", Uuid::new_v4().simple());
    let result = handler
        .process_message(&task_id, &text_message, None)
        .await?;
    println!("Response: {:?}\n", result.status.message);

    // Test 2: Structured data request
    println!("2. Testing structured data request:");
    let data_message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::data(
            json!({
                "date": "2024-01-15",
                "amount": 250.00,
                "purpose": "Team building dinner for Q1 planning",
                "category": "meals"
            })
            .as_object()
            .unwrap()
            .clone(),
        )])
        .message_id(Uuid::new_v4().to_string())
        .context_id("conv-456".to_string())
        .build();

    let task_id = format!("task_{}", Uuid::new_v4().simple());
    let result = handler
        .process_message(&task_id, &data_message, None)
        .await?;
    println!("Response: {:?}\n", result.status.message);

    // Test 3: Form submission
    println!("3. Testing form submission:");
    let form_message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::data(
            json!({
                "request_id": "req_12345",
                "date": "2024-01-20",
                "amount": {"amount": 500.00, "currency": "USD"},
                "purpose": "Conference registration and travel expenses",
                "category": "travel",
                "notes": "Annual tech conference in SF"
            })
            .as_object()
            .unwrap()
            .clone(),
        )])
        .message_id(Uuid::new_v4().to_string())
        .context_id("conv-789".to_string())
        .build();

    let task_id = format!("task_{}", Uuid::new_v4().simple());
    let result = handler
        .process_message(&task_id, &form_message, None)
        .await?;
    println!("Response: {:?}\n", result.status.message);

    // Test 4: Mixed content (text + file reference)
    println!("4. Testing mixed content with file:");
    let file_content = a2a_rs::domain::FileContent {
        name: Some("receipt_20240115.pdf".to_string()),
        mime_type: Some("application/pdf".to_string()),
        bytes: Some("SGVsbG8gV29ybGQh".to_string()), // Base64 encoded
        uri: None,
    };

    let mixed_message = Message::builder()
        .role(Role::User)
        .parts(vec![
            Part::text("Here's my receipt for the office supplies".to_string()),
            Part::File {
                file: file_content,
                metadata: json!({"extracted_amount": "$75.50"})
                    .as_object().cloned(),
            },
        ])
        .message_id(Uuid::new_v4().to_string())
        .build();

    let task_id = format!("task_{}", Uuid::new_v4().simple());
    let result = handler
        .process_message(&task_id, &mixed_message, None)
        .await?;
    println!("Response: {:?}\n", result.status.message);

    // Test 5: Status query
    println!("5. Testing status query:");
    let status_message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text(
            "What's the status of req_12345?".to_string(),
        )])
        .message_id(Uuid::new_v4().to_string())
        .build();

    let task_id = format!("task_{}", Uuid::new_v4().simple());
    let result = handler
        .process_message(&task_id, &status_message, None)
        .await?;
    println!("Response: {:?}\n", result.status.message);

    println!("All tests completed!");
    Ok(())
}
