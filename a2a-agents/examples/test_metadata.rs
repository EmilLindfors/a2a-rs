use serde_json::{json, Map, Value};
use uuid::Uuid;

use a2a_rs::domain::{Message, Part, Role};
use a2a_agents::reimbursement_agent::handler::ReimbursementHandler;
use a2a_rs::port::message_handler::AsyncMessageHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the handler
    let handler = ReimbursementHandler::new();

    // Example 1: Text part with metadata hints
    println!("=== Example 1: Text with metadata ===");
    let mut metadata1 = Map::new();
    metadata1.insert("expense_type".to_string(), Value::String("travel".to_string()));
    metadata1.insert("currency".to_string(), Value::String("EUR".to_string()));
    metadata1.insert("priority".to_string(), Value::String("high".to_string()));

    let message1 = Message::builder()
        .role(Role::User)
        .message_id(Uuid::new_v4().to_string())
        .parts(vec![
            Part::Text {
                text: "I need reimbursement for 150 euros spent on hotel in Paris on 2024-01-15".to_string(),
                metadata: Some(metadata1),
            }
        ])
        .build();

    let task1 = handler.process_message("task1", &message1, None).await?;
    println!("Response: {:?}\n", task1.status.message);

    // Example 2: Data part with metadata
    println!("=== Example 2: Data with metadata ===");
    let mut data2 = Map::new();
    data2.insert("date".to_string(), Value::String("2024-01-20".to_string()));
    data2.insert("amount".to_string(), Value::Number(serde_json::Number::from(75)));
    data2.insert("purpose".to_string(), Value::String("Client dinner meeting".to_string()));

    let mut metadata2 = Map::new();
    metadata2.insert("category_hint".to_string(), Value::String("meals".to_string()));
    metadata2.insert("auto_approve".to_string(), Value::Bool(true));

    let message2 = Message::builder()
        .role(Role::User)
        .message_id(Uuid::new_v4().to_string())
        .parts(vec![
            Part::Data {
                data: data2,
                metadata: Some(metadata2),
            }
        ])
        .build();

    let task2 = handler.process_message("task2", &message2, None).await?;
    println!("Response: {:?}\n", task2.status.message);

    // Example 3: File part with metadata
    println!("=== Example 3: File with metadata ===");
    let file_content = a2a_rs::domain::FileContent {
        name: Some("receipt_hotel.pdf".to_string()),
        mime_type: Some("application/pdf".to_string()),
        bytes: Some("SGVsbG8gV29ybGQh".to_string()), // Base64 encoded
        uri: None,
    };

    let mut file_metadata = Map::new();
    file_metadata.insert("file_name".to_string(), Value::String("receipt_hotel.pdf".to_string()));
    file_metadata.insert("size_bytes".to_string(), Value::Number(serde_json::Number::from(12345)));
    file_metadata.insert("uploaded_by".to_string(), Value::String("john.doe@company.com".to_string()));

    let mut data3 = Map::new();
    data3.insert("date".to_string(), Value::String("2024-01-25".to_string()));
    data3.insert("amount".to_string(), json!({"amount": 250.0, "currency": "USD"}));
    data3.insert("purpose".to_string(), Value::String("Hotel stay for conference".to_string()));
    data3.insert("category".to_string(), Value::String("travel".to_string()));

    let message3 = Message::builder()
        .role(Role::User)
        .message_id(Uuid::new_v4().to_string())
        .parts(vec![
            Part::Data {
                data: data3,
                metadata: None,
            },
            Part::File {
                file: file_content,
                metadata: Some(file_metadata),
            }
        ])
        .build();

    let task3 = handler.process_message("task3", &message3, None).await?;
    println!("Response: {:?}\n", task3.status.message);

    Ok(())
}