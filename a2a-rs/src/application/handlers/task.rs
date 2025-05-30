use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{Task, TaskIdParams, TaskQueryParams};

/// Request to get a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTaskRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: TaskQueryParams,
}

impl GetTaskRequest {
    pub fn new(params: TaskQueryParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/get".to_string(),
            params,
        }
    }
}

/// Response to a get task request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTaskResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Task>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

/// Request to cancel a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelTaskRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: TaskIdParams,
}

impl CancelTaskRequest {
    pub fn new(params: TaskIdParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/cancel".to_string(),
            params,
        }
    }
}

/// Response to a cancel task request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelTaskResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Task>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

/// Request for task resubscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResubscriptionRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: TaskQueryParams,
}

impl TaskResubscriptionRequest {
    pub fn new(params: TaskQueryParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/resubscribe".to_string(),
            params,
        }
    }
}
