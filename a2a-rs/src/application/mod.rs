//! Application services for the A2A protocol

pub mod json_rpc;

// Re-export key types for convenience
pub use json_rpc::{
    A2ARequest, CancelTaskRequest, CancelTaskResponse, GetTaskPushNotificationRequest,
    GetTaskPushNotificationResponse, GetTaskRequest, GetTaskResponse, JSONRPCError, JSONRPCMessage,
    JSONRPCRequest, JSONRPCResponse, SendTaskRequest, SendTaskResponse, SendTaskStreamingRequest,
    SendTaskStreamingResponse, SetTaskPushNotificationRequest, SetTaskPushNotificationResponse,
    TaskResubscriptionRequest, parse_request, serialize_request,
};
