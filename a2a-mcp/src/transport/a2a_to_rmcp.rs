//! Transport adapter that converts A2A transport to RMCP transport

use crate::error::Result;
use crate::message::MessageConverter;
use a2a_rs::domain::{message::Message, task::Task};
use rmcp::{ToolCall, ToolResponse};
use async_trait::async_trait;
use std::sync::Arc;

/// Transport adapter that bridges A2A to RMCP
pub struct A2aToRmcpTransport {
    converter: Arc<MessageConverter>,
}

impl A2aToRmcpTransport {
    /// Create a new A2A to RMCP transport adapter
    pub fn new(converter: Arc<MessageConverter>) -> Self {
        Self { converter }
    }

    /// Convert A2A message to RMCP tool call
    pub async fn convert_message_to_tool_call(&self, msg: &Message, method: &str) -> Result<ToolCall> {
        // Extract parameters from message
        let params = msg.parts.iter()
            .find_map(|part| {
                if let a2a_rs::domain::message::MessagePart::Data { data, .. } = part {
                    Some(data.clone())
                } else {
                    None
                }
            })
            .unwrap_or(serde_json::Value::Null);
        
        Ok(ToolCall {
            method: method.to_string(),
            params,
        })
    }

    /// Convert RMCP tool response to A2A message
    pub async fn convert_tool_response_to_message(&self, resp: &ToolResponse) -> Result<Message> {
        let mut parts = Vec::new();
        
        // Add data part with response result
        parts.push(a2a_rs::domain::message::MessagePart::Data { 
            data: resp.result.clone(),
            mime_type: Some("application/json".to_string()),
        });
        
        // If the result is a string, also add it as text
        if let serde_json::Value::String(text) = &resp.result {
            parts.push(a2a_rs::domain::message::MessagePart::Text { 
                text: text.clone() 
            });
        }
        
        Ok(Message {
            role: "agent".to_string(),
            parts,
        })
    }
}

/// Trait for handling A2A to RMCP message conversion
#[async_trait]
pub trait A2aToRmcpHandler {
    /// Process an A2A task as RMCP tool calls
    async fn process_a2a_task(&self, task: &Task) -> Result<ToolResponse>;
}