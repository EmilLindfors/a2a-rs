//! A default request processor implementation

// This module is already conditionally compiled with #[cfg(feature = "server")] in mod.rs

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    application::{
        json_rpc::{
            self, A2ARequest, CancelTaskRequest, GetExtendedCardRequest,
            GetTaskPushNotificationRequest, GetTaskRequest, SendTaskRequest,
            SendTaskStreamingRequest, SetTaskPushNotificationRequest, TaskResubscriptionRequest,
        },
        JSONRPCError, JSONRPCResponse,
    },
    domain::A2AError,
    port::{AsyncMessageHandler, AsyncNotificationManager, AsyncTaskManager},
    services::server::{AgentInfoProvider, AsyncA2ARequestProcessor},
};

/// Default implementation of a request processor that routes requests to business handlers
#[derive(Clone)]
pub struct DefaultRequestProcessor<M, T, N, A = crate::adapter::SimpleAgentInfo>
where
    M: AsyncMessageHandler + Send + Sync + 'static,
    T: AsyncTaskManager + Send + Sync + 'static,
    N: AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    /// Message handler
    message_handler: Arc<M>,
    /// Task manager
    task_manager: Arc<T>,
    /// Notification manager
    notification_manager: Arc<N>,
    /// Agent info provider
    agent_info: Arc<A>,
}

impl<M, T, N, A> DefaultRequestProcessor<M, T, N, A>
where
    M: AsyncMessageHandler + Send + Sync + 'static,
    T: AsyncTaskManager + Send + Sync + 'static,
    N: AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    /// Create a new request processor with the given handlers
    pub fn new(
        message_handler: M,
        task_manager: T,
        notification_manager: N,
        agent_info: A,
    ) -> Self {
        Self {
            message_handler: Arc::new(message_handler),
            task_manager: Arc::new(task_manager),
            notification_manager: Arc::new(notification_manager),
            agent_info: Arc::new(agent_info),
        }
    }
}

impl<H, A> DefaultRequestProcessor<H, H, H, A>
where
    H: AsyncMessageHandler + AsyncTaskManager + AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    /// Create a new request processor with a single handler that implements all traits
    pub fn with_handler(handler: H, agent_info: A) -> Self {
        let handler_arc = Arc::new(handler);
        Self {
            message_handler: handler_arc.clone(),
            task_manager: handler_arc.clone(),
            notification_manager: handler_arc,
            agent_info: Arc::new(agent_info),
        }
    }
}

impl<M, T, N, A> DefaultRequestProcessor<M, T, N, A>
where
    M: AsyncMessageHandler + Send + Sync + 'static,
    T: AsyncTaskManager + Send + Sync + 'static,
    N: AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    /// Process a send task request
    async fn process_send_task(
        &self,
        request: &SendTaskRequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        let params = &request.params;
        let session_id = params.session_id.as_deref();

        let task = self
            .message_handler
            .process_message(&params.id, &params.message, session_id)
            .await?;

        Ok(JSONRPCResponse::success(
            request.id.clone(),
            serde_json::to_value(task)?,
        ))
    }

    /// Process a get task request
    async fn process_get_task(
        &self,
        request: &GetTaskRequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        let params = &request.params;
        let task = self
            .task_manager
            .get_task(&params.id, params.history_length)
            .await?;

        Ok(JSONRPCResponse::success(
            request.id.clone(),
            serde_json::to_value(task)?,
        ))
    }

    /// Process a cancel task request
    async fn process_cancel_task(
        &self,
        request: &CancelTaskRequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        let params = &request.params;
        let task = self.task_manager.cancel_task(&params.id).await?;

        Ok(JSONRPCResponse::success(
            request.id.clone(),
            serde_json::to_value(task)?,
        ))
    }

    /// Process a set task push notification request
    async fn process_set_push_notification(
        &self,
        request: &SetTaskPushNotificationRequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        let config = self
            .notification_manager
            .set_task_notification(&request.params)
            .await?;

        Ok(JSONRPCResponse::success(
            request.id.clone(),
            serde_json::to_value(config)?,
        ))
    }

    /// Process a get task push notification request
    async fn process_get_push_notification(
        &self,
        request: &GetTaskPushNotificationRequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        let params = &request.params;
        let config = self
            .notification_manager
            .get_task_notification(&params.id)
            .await?;

        Ok(JSONRPCResponse::success(
            request.id.clone(),
            serde_json::to_value(config)?,
        ))
    }

    /// Process a task resubscription request
    async fn process_task_resubscription(
        &self,
        request: &TaskResubscriptionRequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        // For resubscription, we return an initial success response,
        // and then the streaming updates are handled separately
        let params = &request.params;
        let task = self
            .task_manager
            .get_task(&params.id, params.history_length)
            .await?;

        Ok(JSONRPCResponse::success(
            request.id.clone(),
            serde_json::to_value(task)?,
        ))
    }

    /// Process a send task streaming request
    async fn process_send_task_streaming(
        &self,
        request: &SendTaskStreamingRequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        // For streaming, we process the message and return an initial success response,
        // and then the streaming updates are handled separately
        let params = &request.params;
        let session_id = params.session_id.as_deref();

        let task = self
            .message_handler
            .process_message(&params.id, &params.message, session_id)
            .await?;

        Ok(JSONRPCResponse::success(
            request.id.clone(),
            serde_json::to_value(task)?,
        ))
    }

    /// Process a get extended card request (v0.3.0)
    async fn process_get_extended_card(
        &self,
        request: &GetExtendedCardRequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        // Get the agent card from the agent info provider
        // For v0.3.0, this method should return extended information
        // that may only be available to authenticated clients.
        // Authentication checking should be handled by middleware.
        let card = self.agent_info.get_agent_card().await?;

        Ok(JSONRPCResponse::success(
            request.id.clone(),
            serde_json::to_value(card)?,
        ))
    }
}

#[async_trait]
impl<M, T, N, A> AsyncA2ARequestProcessor for DefaultRequestProcessor<M, T, N, A>
where
    M: AsyncMessageHandler + Send + Sync + 'static,
    T: AsyncTaskManager + Send + Sync + 'static,
    N: AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    async fn process_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError> {
        // Parse the request
        let request = match json_rpc::parse_request(request) {
            Ok(req) => req,
            Err(e) => {
                // Return a JSON-RPC error response
                let error = JSONRPCError::from(e);
                let response = JSONRPCResponse::error(None, error);
                return Ok(serde_json::to_string(&response)?);
            }
        };

        // Process the request
        let response = match self.process_request(&request).await {
            Ok(resp) => resp,
            Err(e) => {
                // Return a JSON-RPC error response
                let error = JSONRPCError::from(e);
                let response = JSONRPCResponse::error(request.id().cloned(), error);
                return Ok(serde_json::to_string(&response)?);
            }
        };

        // Serialize the response
        Ok(serde_json::to_string(&response)?)
    }

    async fn process_request<'a>(
        &self,
        request: &'a A2ARequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        match request {
            A2ARequest::SendTask(req) => self.process_send_task(req).await,
            A2ARequest::SendMessage(_req) => {
                // Convert MessageSendParams to TaskSendParams for backwards compatibility
                // TODO: Implement proper message handling
                Err(A2AError::UnsupportedOperation(
                    "Message sending not yet implemented".to_string(),
                ))
            }
            A2ARequest::GetTask(req) => self.process_get_task(req).await,
            A2ARequest::CancelTask(req) => self.process_cancel_task(req).await,
            A2ARequest::SetTaskPushNotification(req) => {
                self.process_set_push_notification(req).await
            }
            A2ARequest::GetTaskPushNotification(req) => {
                self.process_get_push_notification(req).await
            }
            A2ARequest::TaskResubscription(req) => self.process_task_resubscription(req).await,
            A2ARequest::SendTaskStreaming(req) => self.process_send_task_streaming(req).await,
            A2ARequest::SendMessageStreaming(_req) => {
                // Convert MessageSendParams to TaskSendParams for backwards compatibility
                // TODO: Implement proper message streaming
                Err(A2AError::UnsupportedOperation(
                    "Message streaming not yet implemented".to_string(),
                ))
            }
            A2ARequest::GetExtendedCard(req) => self.process_get_extended_card(req).await,
            A2ARequest::Generic(req) => {
                // Handle unknown method
                Err(A2AError::MethodNotFound(format!(
                    "Method '{}' not found",
                    req.method
                )))
            }
        }
    }
}
