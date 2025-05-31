use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{MessageSendParams, Task, TaskSendParams};

/// Request to send a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: MessageSendParams,
}

impl SendMessageRequest {
    pub fn new(params: MessageSendParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "message/send".to_string(),
            params,
        }
    }
}

/// Request to send a task (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTaskRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: TaskSendParams,
}

impl SendTaskRequest {
    pub fn new(params: TaskSendParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/send".to_string(),
            params,
        }
    }
}

/// Response to a send message request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Task>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

/// Response to a send task request (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTaskResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Task>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

/// Request to send a message with streaming updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageStreamingRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: MessageSendParams,
}

impl SendMessageStreamingRequest {
    pub fn new(params: MessageSendParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "message/stream".to_string(),
            params,
        }
    }
}

/// Request to send a task with streaming updates (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTaskStreamingRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: TaskSendParams,
}

impl SendTaskStreamingRequest {
    pub fn new(params: TaskSendParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/sendSubscribe".to_string(),
            params,
        }
    }
}

/// Response to a send message streaming request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SendMessageStreamingResponse {
    Initial {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Value>,
        result: Box<Task>,
    },
    Update {
        jsonrpc: String,
        method: String,
        params: serde_json::Value,
    },
    Error {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Value>,
        error: crate::domain::protocols::JSONRPCError,
    },
}

/// Response to a send task streaming request (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SendTaskStreamingResponse {
    Initial {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Value>,
        result: Box<Task>,
    },
    Update {
        jsonrpc: String,
        method: String,
        params: serde_json::Value,
    },
    Error {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Value>,
        error: crate::domain::protocols::JSONRPCError,
    },
}
