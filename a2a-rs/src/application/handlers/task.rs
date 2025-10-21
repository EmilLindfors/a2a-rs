use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{
    DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigParams, ListTasksParams, ListTasksResult, Task, TaskIdParams,
    TaskPushNotificationConfig, TaskQueryParams,
};

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

// ===== v0.3.0 New API Methods =====

/// Request to list tasks with filtering and pagination (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTasksRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<ListTasksParams>,
}

impl ListTasksRequest {
    pub fn new(params: Option<ListTasksParams>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/list".to_string(),
            params,
        }
    }
}

/// Response for tasks/list method (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTasksResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ListTasksResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

/// Request to get push notification config(s) for a task (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTaskPushNotificationConfigRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<GetTaskPushNotificationConfigParams>,
}

impl GetTaskPushNotificationConfigRequest {
    pub fn new(params: GetTaskPushNotificationConfigParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/pushNotificationConfig/get".to_string(),
            params: Some(params),
        }
    }
}

/// Response for tasks/pushNotificationConfig/get (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTaskPushNotificationConfigResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskPushNotificationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

/// Request to list all push notification configs for a task (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTaskPushNotificationConfigRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: ListTaskPushNotificationConfigParams,
}

impl ListTaskPushNotificationConfigRequest {
    pub fn new(params: ListTaskPushNotificationConfigParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/pushNotificationConfig/list".to_string(),
            params,
        }
    }
}

/// Response for tasks/pushNotificationConfig/list (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTaskPushNotificationConfigResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Vec<TaskPushNotificationConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

/// Request to delete a push notification config (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteTaskPushNotificationConfigRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    pub params: DeleteTaskPushNotificationConfigParams,
}

impl DeleteTaskPushNotificationConfigRequest {
    pub fn new(params: DeleteTaskPushNotificationConfigParams) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "tasks/pushNotificationConfig/delete".to_string(),
            params,
        }
    }
}

/// Response for tasks/pushNotificationConfig/delete (v0.3.0)
/// Returns null on success
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteTaskPushNotificationConfigResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}
