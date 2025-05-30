use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{A2AError, JSONRPCRequest};

// Re-export handler types
pub use crate::application::handlers::{
    CancelTaskRequest, CancelTaskResponse, GetTaskPushNotificationRequest,
    GetTaskPushNotificationResponse, GetTaskRequest, GetTaskResponse, SendMessageRequest,
    SendMessageResponse, SendMessageStreamingRequest, SendMessageStreamingResponse,
    SendTaskRequest, SendTaskResponse, SendTaskStreamingRequest, SendTaskStreamingResponse,
    SetTaskPushNotificationRequest, SetTaskPushNotificationResponse, TaskResubscriptionRequest,
};

/// Any A2A protocol request
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum A2ARequest {
    SendMessage(SendMessageRequest),
    SendMessageStreaming(SendMessageStreamingRequest),
    SendTask(SendTaskRequest),
    SendTaskStreaming(SendTaskStreamingRequest),
    GetTask(GetTaskRequest),
    CancelTask(CancelTaskRequest),
    SetTaskPushNotification(SetTaskPushNotificationRequest),
    GetTaskPushNotification(GetTaskPushNotificationRequest),
    TaskResubscription(TaskResubscriptionRequest),
    Generic(JSONRPCRequest),
}

// Custom deserializer for A2ARequest to handle method-based routing
impl<'de> Deserialize<'de> for A2ARequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // First deserialize into a JSONRPCRequest to get the method
        let json_req = JSONRPCRequest::deserialize(deserializer)?;

        // Based on the method field, determine the appropriate variant
        let result = match json_req.method.as_str() {
            "message/send" => {
                // Re-parse as SendMessageRequest
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req =
                    SendMessageRequest::deserialize(value).map_err(serde::de::Error::custom)?;
                A2ARequest::SendMessage(req)
            }
            "message/stream" => {
                // Re-parse as SendMessageStreamingRequest
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = SendMessageStreamingRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                A2ARequest::SendMessageStreaming(req)
            }
            "tasks/send" => {
                // Re-parse as SendTaskRequest (legacy)
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = SendTaskRequest::deserialize(value).map_err(serde::de::Error::custom)?;
                A2ARequest::SendTask(req)
            }
            "tasks/get" => {
                // Re-parse as GetTaskRequest
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = GetTaskRequest::deserialize(value).map_err(serde::de::Error::custom)?;
                A2ARequest::GetTask(req)
            }
            "tasks/cancel" => {
                // Re-parse as CancelTaskRequest
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req =
                    CancelTaskRequest::deserialize(value).map_err(serde::de::Error::custom)?;
                A2ARequest::CancelTask(req)
            }
            "tasks/pushNotificationConfig/set" => {
                // Re-parse as SetTaskPushNotificationRequest
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = SetTaskPushNotificationRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                A2ARequest::SetTaskPushNotification(req)
            }
            "tasks/pushNotificationConfig/get" => {
                // Re-parse as GetTaskPushNotificationRequest
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = GetTaskPushNotificationRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                A2ARequest::GetTaskPushNotification(req)
            }
            "tasks/resubscribe" => {
                // Re-parse as TaskResubscriptionRequest
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = TaskResubscriptionRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                A2ARequest::TaskResubscription(req)
            }
            "tasks/sendSubscribe" => {
                // Re-parse as SendTaskStreamingRequest (legacy)
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = SendTaskStreamingRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                A2ARequest::SendTaskStreaming(req)
            }
            _ => {
                // For other methods, use Generic variant
                A2ARequest::Generic(json_req)
            }
        };

        Ok(result)
    }
}

impl A2ARequest {
    /// Get the method of the request
    pub fn method(&self) -> &str {
        match self {
            A2ARequest::SendMessage(req) => &req.method,
            A2ARequest::SendMessageStreaming(req) => &req.method,
            A2ARequest::SendTask(req) => &req.method,
            A2ARequest::SendTaskStreaming(req) => &req.method,
            A2ARequest::GetTask(req) => &req.method,
            A2ARequest::CancelTask(req) => &req.method,
            A2ARequest::SetTaskPushNotification(req) => &req.method,
            A2ARequest::GetTaskPushNotification(req) => &req.method,
            A2ARequest::TaskResubscription(req) => &req.method,
            A2ARequest::Generic(req) => &req.method,
        }
    }

    /// Get the ID of the request, if any
    pub fn id(&self) -> Option<&Value> {
        match self {
            A2ARequest::SendMessage(req) => req.id.as_ref(),
            A2ARequest::SendMessageStreaming(req) => req.id.as_ref(),
            A2ARequest::SendTask(req) => req.id.as_ref(),
            A2ARequest::SendTaskStreaming(req) => req.id.as_ref(),
            A2ARequest::GetTask(req) => req.id.as_ref(),
            A2ARequest::CancelTask(req) => req.id.as_ref(),
            A2ARequest::SetTaskPushNotification(req) => req.id.as_ref(),
            A2ARequest::GetTaskPushNotification(req) => req.id.as_ref(),
            A2ARequest::TaskResubscription(req) => req.id.as_ref(),
            A2ARequest::Generic(req) => req.id.as_ref(),
        }
    }
}

/// Parse a JSON string as an A2A request
pub fn parse_request(json: &str) -> Result<A2ARequest, A2AError> {
    match serde_json::from_str::<A2ARequest>(json) {
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
