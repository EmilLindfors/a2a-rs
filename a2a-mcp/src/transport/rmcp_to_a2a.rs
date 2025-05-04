//! Transport adapter that converts RMCP transport to A2A transport

use crate::error::Result;
use crate::message::MessageConverter;
use a2a_rs::domain::{message::Message, task::Task};
use rmcp::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use async_trait::async_trait;
use std::sync::Arc;

/// Transport adapter that bridges RMCP to A2A
pub struct RmcpToA2aTransport {
    converter: Arc<MessageConverter>,
}

impl RmcpToA2aTransport {
    /// Create a new RMCP to A2A transport adapter
    pub fn new(converter: Arc<MessageConverter>) -> Self {
        Self { converter }
    }

    /// Convert RMCP request to A2A task
    pub async fn convert_request(&self, req: &ClientJsonRpcMessage, task_id: &str) -> Result<Task> {
        let message = self.converter.rmcp_to_a2a_request(req)?;
        
        // Create a new task with the converted message
        let task = Task {
            id: task_id.to_string(),
            status: a2a_rs::domain::task::TaskStatus {
                state: a2a_rs::domain::task::TaskState::Submitted,
                message: Some("Task submitted".to_string()),
            },
            messages: vec![message],
            artifacts: Vec::new(),
            history_ttl: Some(3600), // 1 hour default
            metadata: None,
        };
        
        Ok(task)
    }

    /// Convert A2A response to RMCP response
    pub async fn convert_response(&self, task: &Task, id: Option<serde_json::Value>) -> Result<ServerJsonRpcMessage> {
        // Get the last agent message
        let agent_message = self.converter.extract_agent_message(task)?;
        
        // Convert to RMCP response
        self.converter.a2a_to_rmcp_response(agent_message, id)
    }
}

/// Trait for handling RMCP to A2A message conversion
#[async_trait]
pub trait RmcpToA2aHandler {
    /// Process an RMCP request as an A2A task
    async fn process_rmcp_request(&self, req: ClientJsonRpcMessage) -> Result<ServerJsonRpcMessage>;
}