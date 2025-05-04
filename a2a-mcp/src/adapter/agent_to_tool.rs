//! Adapter that exposes A2A agents as RMCP tools

use crate::error::{Error, Result};
use crate::message::MessageConverter;
use a2a_rs::domain::agent::{AgentCard, Skill};
use rmcp::{Tool, ToolCall, ToolResponse};
use std::collections::HashMap;
use std::sync::Arc;

/// Adapts A2A agents to RMCP tool capabilities
pub struct AgentToToolAdapter {
    converter: Arc<MessageConverter>,
    agent_cache: HashMap<String, AgentCard>,
}

impl AgentToToolAdapter {
    /// Create a new adapter
    pub fn new() -> Self {
        Self {
            converter: Arc::new(MessageConverter::new()),
            agent_cache: HashMap::new(),
        }
    }

    /// Add an agent to the cache
    pub fn add_agent(&mut self, url: String, card: AgentCard) {
        self.agent_cache.insert(url, card);
    }

    /// Get an agent from the cache
    pub fn get_agent(&self, url: &str) -> Option<&AgentCard> {
        self.agent_cache.get(url)
    }

    /// Generate RMCP tools from an A2A agent
    pub fn generate_tools(&self, agent: &AgentCard, agent_url: &str) -> Vec<Tool> {
        agent.skills.iter().map(|skill| {
            self.skill_to_tool(skill, agent, agent_url)
        }).collect()
    }

    /// Convert an A2A skill to an RMCP tool
    fn skill_to_tool(&self, skill: &Skill, agent: &AgentCard, agent_url: &str) -> Tool {
        let tool_name = format!("{}:{}", agent_url, skill.name);
        
        Tool {
            name: tool_name,
            description: format!("{} - {}", agent.description, skill.description),
            parameters: None, // Could generate from skill.inputs if available
        }
    }

    /// Convert RMCP tool call to A2A task parameters
    pub fn tool_call_to_task(&self, call: &ToolCall, agent_card: &AgentCard, method: &str) -> Result<a2a_rs::domain::task::Task> {
        // Create a message from the tool call
        let message = self.converter.tool_call_to_message(call)?;
        
        // Create a task with the message
        Ok(a2a_rs::domain::task::Task {
            id: uuid::Uuid::new_v4().to_string(),
            status: a2a_rs::domain::task::TaskStatus {
                state: a2a_rs::domain::task::TaskState::Submitted,
                message: Some("Task submitted from RMCP tool call".to_string()),
            },
            messages: vec![message],
            artifacts: Vec::new(),
            history_ttl: Some(3600), // 1 hour default
            metadata: Some(serde_json::json!({
                "skill": method,
                "agent": agent_card.name.clone(),
            })),
        })
    }

    /// Convert A2A task response to RMCP tool response
    pub fn task_to_tool_response(&self, task: &a2a_rs::domain::task::Task) -> Result<ToolResponse> {
        // Extract the last agent message
        let agent_message = self.converter.extract_agent_message(task)?;
        
        // Convert to tool response
        self.converter.message_to_tool_response(agent_message)
    }

    /// Parse tool method string in format "agent_url:method"
    pub fn parse_tool_method(&self, tool_method: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = tool_method.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(Error::InvalidToolMethod(tool_method.to_string()));
        }
        
        Ok((parts[0].to_string(), parts[1].to_string()))
    }
}