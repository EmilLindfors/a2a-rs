use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{TaskIdParams, TaskPushNotificationConfig};

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
            method: "tasks/pushNotificationConfig/set".to_string(),
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
    pub error: Option<crate::domain::protocols::JSONRPCError>,
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
            method: "tasks/pushNotificationConfig/get".to_string(),
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
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}
