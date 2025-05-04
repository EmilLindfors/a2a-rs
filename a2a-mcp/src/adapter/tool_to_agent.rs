//! Adapter that exposes RMCP tools as A2A agents

use crate::error::{Error, Result};
use crate::message::MessageConverter;
use a2a_rs::domain::{agent::{AgentCard, Capabilities, Authentication, Skill}, task::{Task, TaskState, TaskStatus}, message::{Message, MessagePart}};
use rmcp::{Tool, ToolCall, ToolResponse};
use std::sync::Arc;
use uuid::Uuid;

/// Adapts RMCP tools to A2A agent capabilities
pub struct ToolToAgentAdapter {
    tools: Vec<Tool>,
    agent_name: String,
    agent_description: String,
    converter: Arc<MessageConverter>,
}

impl ToolToAgentAdapter {
    /// Create a new adapter with the given tools
    pub fn new(tools: Vec<Tool>, agent_name: String, agent_description: String) -> Self {
        Self {
            tools,
            agent_name,
            agent_description,
            converter: Arc::new(MessageConverter::new()),
        }
    }
    
    /// Generate A2A agent card from RMCP tools
    pub fn generate_agent_card(&self) -> AgentCard {
        // Create skills from tools
        let skills = self.tools.iter().map(|tool| {
            Skill {
                name: tool.name.clone(),
                description: tool.description.clone(),
                inputs: None,
                outputs: None,
                input_modes: Some(vec!["text".to_string(), "data".to_string()]),
                output_modes: Some(vec!["text".to_string(), "data".to_string()]),
                metadata: None,
            }
        }).collect();
        
        AgentCard {
            name: self.agent_name.clone(),
            description: self.agent_description.clone(),
            url: "https://example.com/agent".to_string(), // Would be configured
            version: "1.0.0".to_string(),
            capabilities: Capabilities {
                streaming: true,
                push_notifications: false,
                state_transition_history: true,
            },
            authentication: Authentication {
                schemes: vec!["Bearer".to_string()],
                // Other auth fields
            },
            default_input_modes: vec!["text".to_string()],
            default_output_modes: vec!["text".to_string()],
            skills,
            metadata: None,
        }
    }
    
    /// Map RMCP tool call to A2A task
    pub fn tool_call_to_task(&self, call: &ToolCall) -> Result<Task> {
        // Create an A2A task from an RMCP tool call
        let task_id = Uuid::new_v4().to_string();
        
        let initial_message = Message {
            role: "user".to_string(),
            parts: vec![
                MessagePart::Text { 
                    text: format!("Call tool: {}", call.method) 
                },
                MessagePart::Data { 
                    data: call.params.clone(),
                    mime_type: Some("application/json".to_string()),
                },
            ],
        };
        
        Ok(Task {
            id: task_id,
            status: TaskStatus {
                state: TaskState::Submitted,
                message: Some("Task submitted".to_string()),
            },
            messages: vec![initial_message],
            artifacts: Vec::new(),
            history_ttl: Some(3600), // 1 hour default
            metadata: None,
        })
    }
    
    /// Map A2A task result to RMCP tool response
    pub fn task_to_tool_response(&self, task: &Task) -> Result<ToolResponse> {
        // Extract the last agent message from the task
        let last_message = self.converter.extract_agent_message(task)?;
        
        // Convert to tool response
        self.converter.message_to_tool_response(last_message)
    }

    /// Find a tool by name
    pub fn find_tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    /// Extract tool name and parameters from an A2A message
    pub fn extract_tool_call(&self, message: &Message) -> Result<(String, serde_json::Value)> {
        // Try to find a text part with "Call tool: " prefix
        let tool_name = message.parts.iter()
            .find_map(|part| {
                if let MessagePart::Text { text } = part {
                    if text.starts_with("Call tool: ") {
                        Some(text.trim_start_matches("Call tool: ").to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .ok_or_else(|| Error::Translation("Unable to extract tool name from message".into()))?;
        
        // Try to find a data part
        let params = message.parts.iter()
            .find_map(|part| {
                if let MessagePart::Data { data, .. } = part {
                    Some(data.clone())
                } else {
                    None
                }
            })
            .unwrap_or(serde_json::Value::Null);
        
        Ok((tool_name, params))
    }
}