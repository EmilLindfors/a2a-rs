//! Server that exposes RMCP tools as an A2A agent

use crate::adapter::ToolToAgentAdapter;
use crate::error::{Error, Result};
use crate::transport::rmcp_to_a2a::RmcpToA2aTransport;
use a2a_rs::domain::{agent::AgentCard, message::Message, task::{Task, TaskState, TaskStatus}};
use rmcp::{Server as RmcpServer, ToolCall, ToolResponse};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use axum::{
    routing::{get, post},
    Router, Json, extract::State,
};
use serde::{Deserialize, Serialize};
use tracing::{info, debug, error};
use uuid::Uuid;

// Shared application state
struct AppState {
    rmcp_server: Arc<RmcpServer>,
    adapter: Arc<ToolToAgentAdapter>,
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    transport: Arc<RmcpToA2aTransport>,
}

/// A server that exposes RMCP tools as an A2A agent
pub struct RmcpA2aServer {
    rmcp_server: Arc<RmcpServer>,
    adapter: Arc<ToolToAgentAdapter>,
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    transport: Arc<RmcpToA2aTransport>,
}

#[derive(Debug, Deserialize)]
struct TaskSendRequest {
    task_id: Option<String>,
    message: Message,
}

#[derive(Debug, Serialize)]
struct TaskSendResponse {
    task: Task,
}

#[derive(Debug, Deserialize)]
struct TaskGetRequest {
    task_id: String,
}

impl RmcpA2aServer {
    /// Create a new server that wraps an RMCP server
    pub fn new(rmcp_server: RmcpServer, adapter: ToolToAgentAdapter) -> Self {
        let converter = Arc::new(crate::message::MessageConverter::new());
        
        Self {
            rmcp_server: Arc::new(rmcp_server),
            adapter: Arc::new(adapter),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            transport: Arc::new(RmcpToA2aTransport::new(converter)),
        }
    }
    
    /// Start serving A2A requests
    pub async fn serve(&self, addr: SocketAddr) -> Result<()> {
        let state = AppState {
            rmcp_server: self.rmcp_server.clone(),
            adapter: self.adapter.clone(),
            tasks: self.tasks.clone(),
            transport: self.transport.clone(),
        };
        
        // Set up HTTP server for A2A protocol
        let app = Router::new()
            .route("/.well-known/agent-card", get(get_agent_card))
            .route("/tasks/send", post(handle_task_send))
            .route("/tasks/sendSubscribe", post(handle_task_send_subscribe))
            .route("/tasks/get", get(handle_task_get))
            .with_state(state);
        
        // Start the server
        info!("Starting A2A agent server on {}", addr);
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .map_err(|e| Error::Server(e.to_string()))
    }
}

// Route handlers
async fn get_agent_card(State(state): State<AppState>) -> Json<AgentCard> {
    Json(state.adapter.generate_agent_card())
}

async fn handle_task_send(
    State(state): State<AppState>,
    Json(request): Json<TaskSendRequest>,
) -> Result<Json<TaskSendResponse>> {
    // Create new task or update existing task
    let task_id = request.task_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let message = request.message;
    
    // For new tasks, create entry in the task store
    let mut tasks = state.tasks.lock().unwrap();
    if !tasks.contains_key(&task_id) {
        let task = Task {
            id: task_id.clone(),
            status: TaskStatus {
                state: TaskState::Submitted,
                message: Some("Task submitted".to_string()),
            },
            messages: vec![message.clone()],
            artifacts: Vec::new(),
            history_ttl: Some(3600), // 1 hour default
            metadata: None,
        };
        tasks.insert(task_id.clone(), task);
    } else {
        // Add message to existing task
        if let Some(task) = tasks.get_mut(&task_id) {
            task.messages.push(message.clone());
            task.status.state = TaskState::Working;
            task.status.message = Some("Processing input".to_string());
        }
    }
    
    let task = tasks.get(&task_id).unwrap().clone();
    drop(tasks);
    
    // Process task with RMCP tools
    process_task(&state, &task_id).await?;
    
    // Return the updated task
    let tasks = state.tasks.lock().unwrap();
    let updated_task = tasks.get(&task_id)
        .ok_or_else(|| Error::TaskNotFound(task_id.clone()))?
        .clone();
    
    Ok(Json(TaskSendResponse { task: updated_task }))
}

async fn handle_task_send_subscribe(
    State(state): State<AppState>,
    Json(request): Json<TaskSendRequest>,
) -> Result<Json<TaskSendResponse>> {
    // For now, just redirect to regular send
    // In a real implementation, this would set up SSE streaming
    handle_task_send(State(state), Json(request)).await
}

async fn handle_task_get(
    State(state): State<AppState>,
    Json(request): Json<TaskGetRequest>,
) -> Result<Json<Task>> {
    let tasks = state.tasks.lock().unwrap();
    let task = tasks.get(&request.task_id)
        .ok_or_else(|| Error::TaskNotFound(request.task_id.clone()))?
        .clone();
    
    Ok(Json(task))
}

// Helper function to process a task using RMCP tools
async fn process_task(state: &AppState, task_id: &str) -> Result<()> {
    // Get the task
    let task = {
        let tasks = state.tasks.lock().unwrap();
        tasks.get(task_id)
            .ok_or_else(|| Error::TaskNotFound(task_id.to_string()))?
            .clone()
    };
    
    // Extract the last user message
    let last_message = task.messages.iter()
        .filter(|msg| msg.role == "user")
        .last()
        .ok_or_else(|| Error::TaskProcessing("No user message found".into()))?;
    
    // Extract tool name and parameters from message
    let (tool_name, params) = state.adapter.extract_tool_call(last_message)?;
    
    // Update task status
    {
        let mut tasks = state.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            task.status.state = TaskState::Working;
            task.status.message = Some(format!("Calling tool: {}", tool_name));
        }
    }
    
    // Call RMCP tool
    let tool_call = ToolCall {
        method: tool_name.clone(),
        params,
    };
    
    debug!("Calling RMCP tool: {}", tool_name);
    let tool_response = state.rmcp_server.call_tool(tool_call).await
        .map_err(|e| Error::RmcpToolCall(format!("Error calling tool {}: {}", tool_name, e)))?;
    
    // Create agent response message
    let agent_message = Message {
        role: "agent".to_string(),
        parts: vec![
            a2a_rs::domain::message::MessagePart::Data { 
                data: tool_response.result.clone(),
                mime_type: Some("application/json".to_string()),
            },
        ],
    };
    
    // Update task with response
    {
        let mut tasks = state.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            task.messages.push(agent_message);
            task.status.state = TaskState::Completed;
            task.status.message = Some("Task completed".to_string());
        }
    }
    
    info!("Task {} completed successfully", task_id);
    Ok(())
}