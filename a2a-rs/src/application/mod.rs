//! Application services for the A2A protocol

pub mod handlers;
pub mod json_rpc;

// Re-export key types for convenience
pub use json_rpc::{
    parse_request, serialize_request, A2ARequest, CancelTaskRequest, CancelTaskResponse,
    GetTaskPushNotificationRequest, GetTaskPushNotificationResponse, GetTaskRequest,
    GetTaskResponse, SendMessageRequest, SendMessageResponse, SendMessageStreamingRequest,
    SendMessageStreamingResponse, SendTaskRequest, SendTaskResponse, SendTaskStreamingRequest,
    SendTaskStreamingResponse, SetTaskPushNotificationRequest, SetTaskPushNotificationResponse,
    TaskResubscriptionRequest,
};

// Re-export JSON-RPC protocol types from domain for backward compatibility
pub use crate::domain::{JSONRPCError, JSONRPCMessage, JSONRPCRequest, JSONRPCResponse};
