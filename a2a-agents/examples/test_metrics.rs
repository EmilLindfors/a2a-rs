use serde_json::{Map, Value};
use uuid::Uuid;

use a2a_agents::reimbursement_agent::handler::ReimbursementHandler;
use a2a_rs::domain::{Message, Part, Role};
use a2a_rs::port::message_handler::AsyncMessageHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with detailed output
    tracing_subscriber::fmt()
        .with_env_filter("info,a2a_agents=debug")
        .with_target(false)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();

    // Initialize the handler
    let handler = ReimbursementHandler::new();

    println!("=== Testing Metrics and Logging ===\n");

    // Test 1: Valid request that gets auto-approved
    println!("Test 1: Small amount (auto-approval)");
    let message1 = Message::builder()
        .role(Role::User)
        .message_id(Uuid::new_v4().to_string())
        .parts(vec![Part::Data {
            data: {
                let mut data = Map::new();
                data.insert("date".to_string(), Value::String("2024-01-20".to_string()));
                data.insert(
                    "amount".to_string(),
                    Value::Number(serde_json::Number::from(50)),
                );
                data.insert(
                    "purpose".to_string(),
                    Value::String("Team lunch".to_string()),
                );
                data.insert("category".to_string(), Value::String("meals".to_string()));
                data
            },
            metadata: None,
        }])
        .build();

    let _task1 = handler.process_message("task1", &message1, None).await?;
    println!();

    // Test 2: Large amount (requires approval)
    println!("Test 2: Large amount (requires approval)");
    let message2 = Message::builder()
        .role(Role::User)
        .message_id(Uuid::new_v4().to_string())
        .parts(vec![Part::Data {
            data: {
                let mut data = Map::new();
                data.insert("date".to_string(), Value::String("2024-01-25".to_string()));
                data.insert(
                    "amount".to_string(),
                    Value::Number(serde_json::Number::from(500)),
                );
                data.insert(
                    "purpose".to_string(),
                    Value::String("Conference attendance".to_string()),
                );
                data.insert("category".to_string(), Value::String("travel".to_string()));
                data
            },
            metadata: None,
        }])
        .build();

    let _task2 = handler.process_message("task2", &message2, None).await?;
    println!();

    // Test 3: Invalid request (validation error)
    println!("Test 3: Invalid request (validation error)");
    let message3 = Message::builder()
        .role(Role::User)
        .message_id(Uuid::new_v4().to_string())
        .parts(vec![Part::Data {
            data: {
                let mut data = Map::new();
                data.insert("date".to_string(), Value::String("".to_string())); // Empty date
                data.insert(
                    "amount".to_string(),
                    Value::Number(serde_json::Number::from(100)),
                );
                data.insert("purpose".to_string(), Value::String("".to_string())); // Empty purpose
                data.insert("category".to_string(), Value::String("meals".to_string()));
                data
            },
            metadata: None,
        }])
        .build();

    let result3 = handler.process_message("task3", &message3, None).await;
    match result3 {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("Expected validation error: {}", e),
    }
    println!();

    // Test 4: Initial request (form generation)
    println!("Test 4: Initial request (form generation)");
    let message4 = Message::builder()
        .role(Role::User)
        .message_id(Uuid::new_v4().to_string())
        .parts(vec![Part::Text {
            text: "I need to submit a reimbursement request".to_string(),
            metadata: None,
        }])
        .build();

    let _task4 = handler.process_message("task4", &message4, None).await?;
    println!();

    // Log final metrics
    println!("=== Final Metrics ===");
    handler.log_metrics();

    // Get metrics programmatically
    let metrics = handler.get_metrics();
    println!("\nMetrics Summary:");
    println!("  Total Requests: {}", metrics.total_requests);
    println!("  Successful: {}", metrics.successful_requests);
    println!("  Validation Errors: {}", metrics.validation_errors);
    println!("  Forms Generated: {}", metrics.forms_generated);
    println!("  Approvals: {}", metrics.approvals_processed);
    println!("  Auto-Approvals: {}", metrics.auto_approvals);

    Ok(())
}
