//! Message conversion between A2A and RMCP protocols

use crate::error::{Error, Result};
use a2a_rs::domain::{message::{Message, MessagePart}, task::Task};
use rmcp::{ClientJsonRpcMessage, ServerJsonRpcMessage, ToolCall, ToolResponse};
use serde_json::Value;

/// Converts between RMCP and A2A message formats
#[derive(Debug, Default)]
pub struct MessageConverter {}

impl MessageConverter {
    /// Create a new message converter
    pub fn new() -> Self {
        Self {}
    }

    /// Convert RMCP request to A2A message
    pub fn rmcp_to_a2a_request(&self, req: &ClientJsonRpcMessage) -> Result<Message> {
        // Extract method and params from RMCP JSON-RPC request
        let method = req.method.clone();
        let params = req.params.clone();
        
        // Create A2A message with appropriate content
        let mut parts = Vec::new();
        
        // Add text part describing the tool call
        parts.push(MessagePart::Text { 
            text: format!("Call method: {}", method) 
        });
        
        // Add data part with the parameters
        if let Some(params_value) = params {
            parts.push(MessagePart::Data { 
                data: params_value.clone(),
                mime_type: Some("application/json".to_string()),
            });
        }
        
        Ok(Message {
            role: "user".to_string(),
            parts,
        })
    }
    
    /// Convert A2A message to RMCP response
    pub fn a2a_to_rmcp_response(&self, msg: &Message, id: Option<Value>) -> Result<ServerJsonRpcMessage> {
        // Extract content from A2A message parts
        let mut result_value = Value::Null;
        
        for part in &msg.parts {
            match part {
                MessagePart::Data { data, .. } => {
                    // Use the data part as the result if available
                    result_value = data.clone();
                    break;
                },
                MessagePart::Text { text } => {
                    // If only text is available, convert to string result
                    if result_value == Value::Null {
                        result_value = Value::String(text.clone());
                    }
                },
                _ => continue,
            }
        }
        
        // Create RMCP JSON-RPC response
        Ok(ServerJsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result_value),
            error: None,
        })
    }

    /// Convert RMCP tool call to A2A message
    pub fn tool_call_to_message(&self, call: &ToolCall) -> Result<Message> {
        let mut parts = Vec::new();
        
        // Add text part for the method
        parts.push(MessagePart::Text { 
            text: format!("Tool call: {}", call.method) 
        });
        
        // Add data part for the parameters
        parts.push(MessagePart::Data { 
            data: call.params.clone(),
            mime_type: Some("application/json".to_string()),
        });
        
        Ok(Message {
            role: "user".to_string(),
            parts,
        })
    }
    
    /// Convert A2A message to RMCP tool response
    pub fn message_to_tool_response(&self, msg: &Message) -> Result<ToolResponse> {
        let mut result = Value::Null;
        
        // Extract content from message parts
        for part in &msg.parts {
            match part {
                MessagePart::Data { data, .. } => {
                    result = data.clone();
                    break;
                },
                MessagePart::Text { text } => {
                    if result == Value::Null {
                        result = Value::String(text.clone());
                    }
                },
                _ => continue,
            }
        }
        
        Ok(ToolResponse { result })
    }

    /// Extract the last agent message from a task
    pub fn extract_agent_message<'a>(&self, task: &'a Task) -> Result<&'a Message> {
        task.messages.iter()
            .filter(|msg| msg.role == "agent" || msg.role == "assistant")
            .last()
            .ok_or_else(|| Error::TaskProcessing("No agent message found".into()))
    }

    /// Extract the last user message from a task
    pub fn extract_user_message<'a>(&self, task: &'a Task) -> Result<&'a Message> {
        task.messages.iter()
            .filter(|msg| msg.role == "user" || msg.role == "human")
            .last()
            .ok_or_else(|| Error::TaskProcessing("No user message found".into()))
    }
}