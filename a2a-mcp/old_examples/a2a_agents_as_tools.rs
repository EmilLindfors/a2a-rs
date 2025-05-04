//! Example showing how to use A2A agents as RMCP tools

use a2a_mcp::A2aRmcpClient;
use a2a_rs::port::client::AsyncA2AClient;
use rmcp::ToolCall;
use std::error::Error;
use serde_json::json;

struct MockA2AClient;

#[async_trait::async_trait]
impl AsyncA2AClient for MockA2AClient {
    async fn fetch_agent_card(&self, url: &str) -> Result<a2a_rs::domain::agent::AgentCard, a2a_rs::Error> {
        println!("Fetching agent card from: {}", url);
        
        // Create a mock agent card for demonstration
        Ok(a2a_rs::domain::agent::AgentCard {
            name: "Mock Agent".to_string(),
            description: "A mock A2A agent for testing".to_string(),
            url: url.to_string(),
            version: "1.0.0".to_string(),
            capabilities: a2a_rs::domain::agent::Capabilities {
                streaming: true,
                push_notifications: false,
                state_transition_history: true,
            },
            authentication: a2a_rs::domain::agent::Authentication {
                schemes: vec!["Bearer".to_string()],
            },
            default_input_modes: vec!["text".to_string()],
            default_output_modes: vec!["text".to_string()],
            skills: vec![
                a2a_rs::domain::agent::Skill {
                    name: "mockSkill".to_string(),
                    description: "A mock skill".to_string(),
                    inputs: None,
                    outputs: None,
                    input_modes: None,
                    output_modes: None,
                    metadata: None,
                }
            ],
            metadata: None,
        })
    }

    async fn send_task(&self, agent_url: &str, task: a2a_rs::domain::task::Task) -> Result<a2a_rs::domain::task::Task, a2a_rs::Error> {
        println!("Sending task to agent at {}: {:?}", agent_url, task.id);
        
        // Create a mock response task
        Ok(a2a_rs::domain::task::Task {
            id: task.id.clone(),
            status: a2a_rs::domain::task::TaskStatus {
                state: a2a_rs::domain::task::TaskState::Working,
                message: Some("Task received and processing".to_string()),
            },
            messages: task.messages,
            artifacts: Vec::new(),
            history_ttl: None,
            metadata: None,
        })
    }

    async fn wait_for_completion(&self, agent_url: &str, task_id: &str) -> Result<a2a_rs::domain::task::Task, a2a_rs::Error> {
        println!("Waiting for completion of task {} at agent {}", task_id, agent_url);
        
        // Create a mock completed task
        Ok(a2a_rs::domain::task::Task {
            id: task_id.to_string(),
            status: a2a_rs::domain::task::TaskStatus {
                state: a2a_rs::domain::task::TaskState::Completed,
                message: Some("Task completed successfully".to_string()),
            },
            messages: vec![
                a2a_rs::domain::message::Message {
                    role: "user".to_string(),
                    parts: vec![
                        a2a_rs::domain::message::MessagePart::Text { 
                            text: "Initial request".to_string() 
                        },
                    ],
                },
                a2a_rs::domain::message::Message {
                    role: "agent".to_string(),
                    parts: vec![
                        a2a_rs::domain::message::MessagePart::Text { 
                            text: "Task completed successfully".to_string() 
                        },
                        a2a_rs::domain::message::MessagePart::Data { 
                            data: json!({
                                "result": "This is the result from the A2A agent",
                                "timestamp": chrono::Utc::now().to_rfc3339(),
                            }),
                            mime_type: Some("application/json".to_string()),
                        },
                    ],
                },
            ],
            artifacts: Vec::new(),
            history_ttl: None,
            metadata: None,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize the mock A2A client
    let a2a_client = MockA2AClient;
    
    // Create A2A-RMCP client
    let client = A2aRmcpClient::new(a2a_client);
    
    // Discover A2A agents
    let agent_urls = vec![
        "https://example.com/mock-agent".to_string(),
    ];
    
    let tools = client.discover_agents(&agent_urls).await?;
    println!("Discovered {} tools from A2A agents:", tools.len());
    for tool in &tools {
        println!("  - {}: {}", tool.name, tool.description);
    }
    
    // Call an agent as a tool
    let call = ToolCall {
        method: "https://example.com/mock-agent:mockSkill".to_string(),
        params: json!({
            "query": "test query",
            "options": {
                "detailed": true,
                "format": "json"
            }
        }),
    };
    
    println!("\nCalling A2A agent as RMCP tool...");
    let response = client.call_agent_as_tool(call).await?;
    
    println!("\nReceived response:");
    println!("{}", serde_json::to_string_pretty(&response.result)?);
    
    Ok(())
}