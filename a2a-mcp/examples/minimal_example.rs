//! A minimal example of A2A-RMCP integration

use serde_json::json;

fn main() {
    println!("A2A-RMCP Integration Demo");
    println!("=========================\n");

    // Simulate RMCP request
    let rmcp_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "calculate",
        "params": {
            "operation": "add",
            "a": 5,
            "b": 7
        }
    });

    println!("RMCP Request:");
    println!("{}", serde_json::to_string_pretty(&rmcp_request).unwrap());
    println!();

    // Simulate converting to A2A message
    println!("Converting to A2A message...");
    println!("A2A Task would look like:");
    println!("{}", serde_json::to_string_pretty(&json!({
        "id": "task-123456",
        "status": {
            "state": "submitted",
            "message": "Task submitted"
        },
        "messages": [
            {
                "role": "user",
                "parts": [
                    {
                        "type": "text",
                        "text": "Call method: calculate"
                    },
                    {
                        "type": "data",
                        "mime_type": "application/json",
                        "data": {
                            "operation": "add",
                            "a": 5,
                            "b": 7
                        }
                    }
                ]
            }
        ]
    })).unwrap());
    println!();

    // Simulate A2A agent response
    println!("A2A Agent Response:");
    println!("{}", serde_json::to_string_pretty(&json!({
        "id": "task-123456",
        "status": {
            "state": "completed",
            "message": "Task completed"
        },
        "messages": [
            {
                "role": "user",
                "parts": [
                    {
                        "type": "text",
                        "text": "Call method: calculate"
                    },
                    {
                        "type": "data",
                        "mime_type": "application/json",
                        "data": {
                            "operation": "add",
                            "a": 5,
                            "b": 7
                        }
                    }
                ]
            },
            {
                "role": "agent",
                "parts": [
                    {
                        "type": "data",
                        "mime_type": "application/json",
                        "data": {
                            "result": 12
                        }
                    }
                ]
            }
        ]
    })).unwrap());
    println!();

    // Convert back to RMCP response
    let rmcp_response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "result": 12
        }
    });

    println!("Converting back to RMCP response...");
    println!("RMCP Response:");
    println!("{}", serde_json::to_string_pretty(&rmcp_response).unwrap());
}