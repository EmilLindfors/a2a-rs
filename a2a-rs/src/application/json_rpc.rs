use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{A2AError, JSONRPCRequest};

// Re-export handler types
pub use crate::application::handlers::{
    CancelTaskRequest, CancelTaskResponse, DeleteTaskPushNotificationConfigRequest,
    DeleteTaskPushNotificationConfigResponse, GetAuthenticatedExtendedCardRequest,
    GetAuthenticatedExtendedCardResponse, GetExtendedCardRequest, GetExtendedCardResponse,
    GetTaskPushNotificationConfigRequest, GetTaskPushNotificationConfigResponse,
    GetTaskPushNotificationRequest, GetTaskPushNotificationResponse, GetTaskRequest,
    GetTaskResponse, ListTaskPushNotificationConfigRequest, ListTaskPushNotificationConfigResponse,
    ListTasksRequest, ListTasksResponse, SendMessageRequest, SendMessageResponse,
    SendMessageStreamingRequest, SendMessageStreamingResponse, SendTaskRequest, SendTaskResponse,
    SendTaskStreamingRequest, SendTaskStreamingResponse, SetTaskPushNotificationRequest,
    SetTaskPushNotificationResponse, TaskResubscriptionRequest,
};

/// Union type representing any A2A protocol request.\n///\n/// This enum provides a unified interface for all possible A2A protocol requests,\n/// automatically handling method-based routing during deserialization. The enum\n/// covers all standard A2A operations including message sending, task management,\n/// and notification configuration.\n///\n/// # Supported Request Types\n/// - `SendMessage`: Send a message to an agent\n/// - `SendMessageStreaming`: Send a message with streaming response\n/// - `SendTask`: Legacy task sending (replaced by SendMessage)\n/// - `SendTaskStreaming`: Legacy streaming task (replaced by SendMessageStreaming)\n/// - `GetTask`: Retrieve task status and information\n/// - `CancelTask`: Cancel a running task\n/// - `SetTaskPushNotification`: Configure push notifications for a task\n/// - `GetTaskPushNotification`: Retrieve push notification configuration\n/// - `TaskResubscription`: Re-subscribe to task updates\n/// - `GetExtendedCard`: Get extended agent card (v0.3.0)\n/// - `ListTasks`: List tasks with filtering and pagination (v0.3.0)\n/// - `GetTaskPushNotificationConfig`: Get specific push notification config (v0.3.0)\n/// - `ListTaskPushNotificationConfigs`: List all push notification configs (v0.3.0)\n/// - `DeleteTaskPushNotificationConfig`: Delete a push notification config (v0.3.0)\n/// - `GetAuthenticatedExtendedCard`: Get authenticated extended card (v0.3.0)\n/// - `Generic`: Fallback for custom or unknown requests
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
    GetExtendedCard(GetExtendedCardRequest),
    // v0.3.0 new methods
    ListTasks(ListTasksRequest),
    GetTaskPushNotificationConfig(GetTaskPushNotificationConfigRequest),
    ListTaskPushNotificationConfigs(ListTaskPushNotificationConfigRequest),
    DeleteTaskPushNotificationConfig(DeleteTaskPushNotificationConfigRequest),
    GetAuthenticatedExtendedCard(GetAuthenticatedExtendedCardRequest),
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
            "agent/getExtendedCard" => {
                // Re-parse as GetExtendedCardRequest (v0.3.0)
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req =
                    GetExtendedCardRequest::deserialize(value).map_err(serde::de::Error::custom)?;
                A2ARequest::GetExtendedCard(req)
            }
            "agent/getAuthenticatedExtendedCard" => {
                // Re-parse as GetAuthenticatedExtendedCardRequest (v0.3.0)
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = GetAuthenticatedExtendedCardRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                A2ARequest::GetAuthenticatedExtendedCard(req)
            }
            "tasks/list" => {
                // Re-parse as ListTasksRequest (v0.3.0)
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = ListTasksRequest::deserialize(value).map_err(serde::de::Error::custom)?;
                A2ARequest::ListTasks(req)
            }
            "tasks/pushNotificationConfig/list" => {
                // Re-parse as ListTaskPushNotificationConfigRequest (v0.3.0)
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = ListTaskPushNotificationConfigRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                A2ARequest::ListTaskPushNotificationConfigs(req)
            }
            "tasks/pushNotificationConfig/delete" => {
                // Re-parse as DeleteTaskPushNotificationConfigRequest (v0.3.0)
                let value = serde_json::to_value(&json_req).map_err(serde::de::Error::custom)?;
                let req = DeleteTaskPushNotificationConfigRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                A2ARequest::DeleteTaskPushNotificationConfig(req)
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
            A2ARequest::GetExtendedCard(req) => &req.method,
            A2ARequest::ListTasks(req) => &req.method,
            A2ARequest::GetTaskPushNotificationConfig(req) => &req.method,
            A2ARequest::ListTaskPushNotificationConfigs(req) => &req.method,
            A2ARequest::DeleteTaskPushNotificationConfig(req) => &req.method,
            A2ARequest::GetAuthenticatedExtendedCard(req) => &req.method,
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
            A2ARequest::GetExtendedCard(req) => req.id.as_ref(),
            A2ARequest::ListTasks(req) => req.id.as_ref(),
            A2ARequest::GetTaskPushNotificationConfig(req) => req.id.as_ref(),
            A2ARequest::ListTaskPushNotificationConfigs(req) => req.id.as_ref(),
            A2ARequest::DeleteTaskPushNotificationConfig(req) => req.id.as_ref(),
            A2ARequest::GetAuthenticatedExtendedCard(req) => req.id.as_ref(),
            A2ARequest::Generic(req) => req.id.as_ref(),
        }
    }
}

/// Parse a JSON string as an A2A protocol request.
///
/// This function deserializes a JSON string into the appropriate A2ARequest variant
/// based on the JSON-RPC method field. It automatically handles method-based routing
/// and validates the request structure according to the A2A specification.
///
/// # Arguments
/// * `json` - JSON string containing the request
///
/// # Returns
/// * `Ok(A2ARequest)` - Successfully parsed request
/// * `Err(A2AError::JsonParse)` - JSON parsing or validation failed
///
/// # Example
/// ```rust
/// use a2a_rs::application::parse_request;
///
/// let json = r#"{
///     "jsonrpc": "2.0",
///     "id": "req-123",
///     "method": "agent/sendMessage",
///     "params": { "taskId": "task-456", "message": {...} }
/// }"#;
///
/// match parse_request(json) {
///     Ok(request) => println!("Parsed request: {:?}", request),
///     Err(err) => eprintln!("Parse error: {}", err),
/// }
/// ```
pub fn parse_request(json: &str) -> Result<A2ARequest, A2AError> {
    match serde_json::from_str::<A2ARequest>(json) {
        Ok(request) => Ok(request),
        Err(err) => Err(A2AError::JsonParse(err)),
    }
}

/// Serialize an A2A request to a JSON string.
///
/// This function converts an A2ARequest into its JSON representation
/// according to the JSON-RPC 2.0 specification and A2A protocol requirements.
///
/// # Arguments
/// * `request` - The A2A request to serialize
///
/// # Returns
/// * `Ok(String)` - JSON representation of the request
/// * `Err(A2AError::JsonParse)` - Serialization failed
///
/// # Example
/// ```rust
/// use a2a_rs::application::{serialize_request, A2ARequest};
///
/// // Assuming you have an A2ARequest instance
/// // let request = A2ARequest::SendMessage(...);
///
/// // match serialize_request(&request) {
/// //     Ok(json) => println!("Request JSON: {}", json),
/// //     Err(err) => eprintln!("Serialization error: {}", err),
/// // }
/// ```
pub fn serialize_request(request: &A2ARequest) -> Result<String, A2AError> {
    match serde_json::to_string(request) {
        Ok(json) => Ok(json),
        Err(err) => Err(A2AError::JsonParse(err)),
    }
}
