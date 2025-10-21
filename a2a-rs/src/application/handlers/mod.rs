//! Request and response handlers for the A2A protocol

pub mod agent;
pub mod message;
pub mod notification;
pub mod task;

pub use agent::{
    GetAuthenticatedExtendedCardRequest, GetAuthenticatedExtendedCardResponse,
    GetExtendedCardRequest, GetExtendedCardResponse,
};
pub use message::{
    SendMessageRequest, SendMessageResponse, SendMessageStreamingRequest,
    SendMessageStreamingResponse, SendTaskRequest, SendTaskResponse, SendTaskStreamingRequest,
    SendTaskStreamingResponse,
};
pub use notification::{
    GetTaskPushNotificationRequest, GetTaskPushNotificationResponse,
    SetTaskPushNotificationRequest, SetTaskPushNotificationResponse,
};
pub use task::{
    CancelTaskRequest, CancelTaskResponse, DeleteTaskPushNotificationConfigRequest,
    DeleteTaskPushNotificationConfigResponse, GetTaskPushNotificationConfigRequest,
    GetTaskPushNotificationConfigResponse, GetTaskRequest, GetTaskResponse,
    ListTaskPushNotificationConfigRequest, ListTaskPushNotificationConfigResponse,
    ListTasksRequest, ListTasksResponse, TaskResubscriptionRequest,
};
