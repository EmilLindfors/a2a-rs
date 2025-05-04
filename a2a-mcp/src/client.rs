//! Client for accessing A2A agents as RMCP tools

use crate::adapter::AgentToToolAdapter;
use crate::error::{Error, Result};
use a2a_rs::domain::agent::AgentCard;
use a2a_rs::port::client::AsyncA2AClient;
use rmcp::{Tool, ToolCall, ToolResponse};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{info, debug, error};

/// A client that accesses A2A agents as RMCP tools
pub struct A2aRmcpClient<C: AsyncA2AClient> {
    a2a_client: C,
    adapter: Arc<Mutex<AgentToToolAdapter>>,
}

impl<C: AsyncA2AClient> A2aRmcpClient<C> {
    /// Create a new client that discovers A2A agents
    pub fn new(a2a_client: C) -> Self {
        Self {
            a2a_client,
            adapter: Arc::new(Mutex::new(AgentToToolAdapter::new())),
        }
    }
    
    /// Discover A2A agents and convert to RMCP tools
    pub async fn discover_agents(&self, urls: &[String]) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        
        for url in urls {
            debug!("Discovering agent at {}", url);
            
            // Fetch agent card
            let agent_card = self.a2a_client.fetch_agent_card(url).await
                .map_err(|e| Error::A2a(format!("Failed to fetch agent card from {}: {}", url, e)))?;
            
            info!("Discovered agent: {} with {} skills", 
                  agent_card.name, 
                  agent_card.skills.len());
            
            // Cache the agent card
            let mut adapter = self.adapter.lock().unwrap();
            adapter.add_agent(url.clone(), agent_card.clone());
            
            // Convert agent capabilities to tools
            let agent_tools = adapter.generate_tools(&agent_card, url);
            tools.extend(agent_tools);
        }
        
        Ok(tools)
    }
    
    /// Call an A2A agent as an RMCP tool
    pub async fn call_agent_as_tool(&self, call: ToolCall) -> Result<ToolResponse> {
        // Parse the tool call to extract agent URL and method
        let adapter = self.adapter.lock().unwrap();
        let (agent_url, method) = adapter.parse_tool_method(&call.method)?;
        
        // Get agent card from cache
        let agent_card = adapter.get_agent(&agent_url)
            .ok_or_else(|| Error::AgentNotFound(agent_url.clone()))?
            .clone();
        
        drop(adapter); // Release the lock before async calls
        
        debug!("Calling agent {} with method {}", agent_card.name, method);
        
        // Convert RMCP tool call to A2A task
        let adapter = self.adapter.lock().unwrap();
        let task = adapter.tool_call_to_task(&call, &agent_card, &method)?;
        let task_id = task.id.clone();
        drop(adapter);
        
        // Send task to A2A agent
        let response = self.a2a_client.send_task(&agent_url, task).await
            .map_err(|e| Error::A2a(format!("Failed to send task to agent: {}", e)))?;
        
        debug!("Task {} sent to agent, waiting for completion", task_id);
        
        // Wait for task completion
        let completed_task = self.a2a_client.wait_for_completion(&agent_url, &response.id).await
            .map_err(|e| Error::A2a(format!("Failed to wait for task completion: {}", e)))?;
        
        info!("Task {} completed with status {:?}", 
              completed_task.id, 
              completed_task.status.state);
        
        // Convert A2A task result to RMCP tool response
        let adapter = self.adapter.lock().unwrap();
        adapter.task_to_tool_response(&completed_task)
    }
}