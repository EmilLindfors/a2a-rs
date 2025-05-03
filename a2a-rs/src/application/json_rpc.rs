use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{
    error::A2AError,
    task::{
        Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams,
        TaskSendParams, TaskStatusUpdateEvent,
    },
};

/// Standard JSON-RPC 2.0 message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

impl Default for JSONRPCMessage {
    fn default() -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
        }
    }
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<A2AError> for JSONRPCError {
    fn from(error: A2AError) -> Self {
        let value = error.to_jsonrpc_error();

        // Extract the fields from the JSON value
        if let Value::Object(map) = value {
            let code = map
                .get("code")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32)
                .unwrap_or(-32603); // Internal error code as fallback

            let message = map
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Internal error")
                .to_string();

            let data = map.get("data").cloned();

            Self {
                code,
                message,
                data,
            }
        } else {
            // Fallback to internal error if the JSON structure is unexpected
            Self {
                code: -32603,
                message: "Internal error".to_string(),
                data: None,
            }
        }
    }
}

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JSONRPCRequest {
    /// Create a new JSON-RPC request with the given method and parameters
    pub fn new(method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method,
            params,
        }
    }

    /// Create a new JSON-RPC request with the given method, parameters, and ID
    pub fn with_id(method: String, params: Option<Value>, id: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method,
            params,
        }
    }
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JSONRPCError>,
}

impl JSONRPCResponse {
    /// Create a new successful JSON-RPC response
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create a new error JSON-RPC response
    pub fn error(id: Option<Value>, error: JSONRPCError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Create a new error JSON-RPC response from an A2AError
    pub fn from_error(id: Option<Value>, error: A2AError) -> Self {
        Self::error(id, JSONRPCError::from(error))
    }
}

// A2A-specific request types

/// Request to send a task
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

/// Response to a send task request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTaskResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Task>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JSONRPCError>,
}

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
    pub error: Option<JSONRPCError>,
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
    pub error: Option<JSONRPCError>,
}

/// Request to set task push notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTaskPushNotificationRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: TaskPushNotificationConfig,
}

impl SetTaskPushNotificationRequest {
    pub fn new(params: TaskPushNotificationConfig) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/pushNotification/set".to_string(),
            params,
        }
    }
}

/// Response to a set task push notification request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTaskPushNotificationResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskPushNotificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JSONRPCError>,
}

/// Request to get task push notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTaskPushNotificationRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: TaskIdParams,
}

impl GetTaskPushNotificationRequest {
    pub fn new(params: TaskIdParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/pushNotification/get".to_string(),
            params,
        }
    }
}

/// Response to a get task push notification request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTaskPushNotificationResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskPushNotificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JSONRPCError>,
}

/// Request to send a task with streaming
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

/// Response to a send task streaming request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SendTaskStreamingResponse {
    Status {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Value>,
        result: TaskStatusUpdateEvent,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<JSONRPCError>,
    },
    Artifact {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Value>,
        result: TaskArtifactUpdateEvent,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<JSONRPCError>,
    },
    Error {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        error: JSONRPCError,
    },
}

/// Request to resubscribe to a task
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

/// Any A2A protocol request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum A2ARequest {
    SendTask(SendTaskRequest),
    GetTask(GetTaskRequest),
    CancelTask(CancelTaskRequest),
    SetTaskPushNotification(SetTaskPushNotificationRequest),
    GetTaskPushNotification(GetTaskPushNotificationRequest),
    TaskResubscription(TaskResubscriptionRequest),
    SendTaskStreaming(SendTaskStreamingRequest),
    Generic(JSONRPCRequest),
}

impl A2ARequest {
    /// Get the method of the request
    pub fn method(&self) -> &str {
        match self {
            A2ARequest::SendTask(req) => &req.method,
            A2ARequest::GetTask(req) => &req.method,
            A2ARequest::CancelTask(req) => &req.method,
            A2ARequest::SetTaskPushNotification(req) => &req.method,
            A2ARequest::GetTaskPushNotification(req) => &req.method,
            A2ARequest::TaskResubscription(req) => &req.method,
            A2ARequest::SendTaskStreaming(req) => &req.method,
            A2ARequest::Generic(req) => &req.method,
        }
    }

    /// Get the ID of the request, if any
    pub fn id(&self) -> Option<&Value> {
        match self {
            A2ARequest::SendTask(req) => req.id.as_ref(),
            A2ARequest::GetTask(req) => req.id.as_ref(),
            A2ARequest::CancelTask(req) => req.id.as_ref(),
            A2ARequest::SetTaskPushNotification(req) => req.id.as_ref(),
            A2ARequest::GetTaskPushNotification(req) => req.id.as_ref(),
            A2ARequest::TaskResubscription(req) => req.id.as_ref(),
            A2ARequest::SendTaskStreaming(req) => req.id.as_ref(),
            A2ARequest::Generic(req) => req.id.as_ref(),
        }
    }
}

/// Parse a JSON string as an A2A request
pub fn parse_request(json: &str) -> Result<A2ARequest, A2AError> {
    match serde_json::from_str(json) {
        Ok(request) => Ok(request),
        Err(err) => Err(A2AError::JsonParse(err)),
    }
}

/// Serialize an A2A request to a JSON string
pub fn serialize_request(request: &A2ARequest) -> Result<String, A2AError> {
    match serde_json::to_string(request) {
        Ok(json) => Ok(json),
        Err(err) => Err(A2AError::JsonParse(err)),
    }
}
