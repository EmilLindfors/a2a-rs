//! A default request processor implementation

use std::pin::Pin;
use std::sync::Arc;
use async_trait::async_trait;
use buffa::Enumeration;

use crate::{
    domain::{
        A2AError, Task, TaskPushNotificationConfig, AgentCard,
        TaskStatusUpdateEvent, TaskArtifactUpdateEvent,
        generated::{
            A2aService, SendMessageResponse, ListTasksResponse,
            ListTaskPushNotificationConfigsResponse, StreamResponse,
            SendMessageRequestView, GetTaskRequestView, ListTasksRequestView,
            CancelTaskRequestView, SubscribeToTaskRequestView, TaskPushNotificationConfigView,
            GetTaskPushNotificationConfigRequestView, ListTaskPushNotificationConfigsRequestView,
            GetExtendedAgentCardRequestView, DeleteTaskPushNotificationConfigRequestView,
            send_message_response, stream_response, TaskState,
            TaskStatusUpdateEvent as GenTaskStatusUpdateEvent,
            TaskArtifactUpdateEvent as GenTaskArtifactUpdateEvent,
        },
    },
    port::{
        AsyncMessageHandler, AsyncNotificationManager, AsyncTaskManager,
        AsyncStreamingHandler, UpdateEvent, streaming_handler::Subscriber,
    },
    services::server::AgentInfoProvider,
};

/// Default implementation of a request processor that routes ConnectRPC requests to business handlers
#[derive(Clone)]
pub struct DefaultRequestProcessor<M, T, N, A = crate::adapter::SimpleAgentInfo, S = NoopStreamingHandler>
where
    M: AsyncMessageHandler + Send + Sync + 'static,
    T: AsyncTaskManager + Send + Sync + 'static,
    N: AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    S: AsyncStreamingHandler + Send + Sync + 'static,
{
    /// Message handler
    message_handler: Arc<M>,
    /// Task manager
    task_manager: Arc<T>,
    /// Notification manager
    notification_manager: Arc<N>,
    /// Agent info provider
    agent_info: Arc<A>,
    /// Streaming handler
    streaming_handler: Arc<S>,
}

impl<M, T, N, A> DefaultRequestProcessor<M, T, N, A, NoopStreamingHandler>
where
    M: AsyncMessageHandler + Send + Sync + 'static,
    T: AsyncTaskManager + Send + Sync + 'static,
    N: AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
{
    /// Create a new request processor with the given handlers and default NoopStreamingHandler
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
            streaming_handler: Arc::new(NoopStreamingHandler),
        }
    }
}

impl<H, A> DefaultRequestProcessor<H, H, H, A, NoopStreamingHandler>
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
            streaming_handler: Arc::new(NoopStreamingHandler),
        }
    }
}

impl<M, T, N, A, S> DefaultRequestProcessor<M, T, N, A, S>
where
    M: AsyncMessageHandler + Send + Sync + 'static,
    T: AsyncTaskManager + Send + Sync + 'static,
    N: AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    S: AsyncStreamingHandler + Send + Sync + 'static,
{
    /// Builder-style method to inject custom streaming handler support
    pub fn with_streaming_handler<NewS>(
        self,
        streaming_handler: NewS,
    ) -> DefaultRequestProcessor<M, T, N, A, NewS>
    where
        NewS: AsyncStreamingHandler + Send + Sync + 'static,
    {
        DefaultRequestProcessor {
            message_handler: self.message_handler,
            task_manager: self.task_manager,
            notification_manager: self.notification_manager,
            agent_info: self.agent_info,
            streaming_handler: Arc::new(streaming_handler),
        }
    }
}

/// Helper function to map A2AError to connectrpc::ConnectError
fn map_err(e: A2AError) -> ::connectrpc::ConnectError {
    match e {
        A2AError::TaskNotFound(msg) => ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::NotFound, msg),
        A2AError::InvalidParams(msg) => ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::InvalidArgument, msg),
        A2AError::ValidationError { field, message } => ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::InvalidArgument, format!("{}: {}", field, message)),
        A2AError::UnsupportedOperation(msg) => ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::Unimplemented, msg),
        A2AError::AuthenticatedExtendedCardNotConfigured => ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::FailedPrecondition, "Authenticated extended card not configured".to_string()),
        A2AError::MethodNotFound(msg) => ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::Unimplemented, msg),
        _ => ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::Internal, e.to_string()),
    }
}

/// Helper to map domain metadata to protobuf Struct
fn map_metadata(opt: Option<serde_json::Map<String, serde_json::Value>>) -> ::buffa::MessageField<::buffa_types::google::protobuf::Struct> {
    if let Some(map) = opt {
        let val = serde_json::Value::Object(map);
        if let Ok(struc) = serde_json::from_value::<::buffa_types::google::protobuf::Struct>(val) {
            return ::buffa::MessageField::some(struc);
        }
    }
    ::buffa::MessageField::none()
}

fn map_status_update(evt: crate::domain::events::TaskStatusUpdateEvent) -> GenTaskStatusUpdateEvent {
    GenTaskStatusUpdateEvent {
        task_id: evt.task_id,
        context_id: evt.context_id,
        status: ::buffa::MessageField::some(evt.status),
        metadata: map_metadata(evt.metadata),
        ..Default::default()
    }
}

fn map_artifact_update(evt: crate::domain::events::TaskArtifactUpdateEvent) -> GenTaskArtifactUpdateEvent {
    GenTaskArtifactUpdateEvent {
        task_id: evt.task_id,
        context_id: evt.context_id,
        artifact: ::buffa::MessageField::some(evt.artifact),
        append: evt.append.unwrap_or(false),
        last_chunk: evt.last_chunk.unwrap_or(false),
        metadata: map_metadata(evt.metadata),
        ..Default::default()
    }
}

impl<M, T, N, A, S> A2aService for DefaultRequestProcessor<M, T, N, A, S>
where
    M: AsyncMessageHandler + Send + Sync + 'static,
    T: AsyncTaskManager + Send + Sync + 'static,
    N: AsyncNotificationManager + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    S: AsyncStreamingHandler + Send + Sync + 'static,
{
    async fn send_message(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<SendMessageRequestView<'static>>,
    ) -> Result<
        (SendMessageResponse, ::connectrpc::Context),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        let message = req.message.into_option().ok_or_else(|| {
            ::connectrpc::ConnectError::new(
                ::connectrpc::ErrorCode::InvalidArgument,
                "Missing message".to_string(),
            )
        })?;
        let config = req.configuration.into_option();

        let task_id = message.task_id.clone();
        let session_id = if message.context_id.is_empty() {
            None
        } else {
            Some(message.context_id.as_str())
        };

        let mut history_limit = None;

        // If push notification configuration is provided, configure it
        if let Some(c) = config {
            if let Some(mut push_config) = c.task_push_notification_config.into_option() {
                push_config.task_id = task_id.clone();
                self.notification_manager
                    .set_task_notification_validated(&push_config)
                    .await
                    .map_err(map_err)?;
            }
            if let Some(limit) = c.history_length {
                history_limit = Some(limit as u32);
            }
        }

        let mut task = self
            .message_handler
            .process_message(&task_id, &message, session_id)
            .await
            .map_err(map_err)?;

        if let Some(limit) = history_limit {
            task = task.with_limited_history(Some(limit));
        }

        let response = SendMessageResponse {
            payload: Some(send_message_response::Payload::Task(Box::new(task))),
            ..Default::default()
        };

        Ok((response, ctx))
    }

    async fn send_streaming_message(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<SendMessageRequestView<'static>>,
    ) -> Result<
        (
            ::std::pin::Pin<
                Box<
                    dyn ::futures::Stream<
                        Item = Result<StreamResponse, ::connectrpc::ConnectError>,
                    > + Send,
                >,
            >,
            ::connectrpc::Context,
        ),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        let message = req.message.into_option().ok_or_else(|| {
            ::connectrpc::ConnectError::new(
                ::connectrpc::ErrorCode::InvalidArgument,
                "Missing message".to_string(),
            )
        })?;
        let config = req.configuration.into_option();

        let task_id = message.task_id.clone();
        let session_id = if message.context_id.is_empty() {
            None
        } else {
            Some(message.context_id.as_str())
        };

        let mut history_limit = None;

        // Setup notification if present
        if let Some(c) = config {
            if let Some(mut push_config) = c.task_push_notification_config.into_option() {
                push_config.task_id = task_id.clone();
                self.notification_manager
                    .set_task_notification_validated(&push_config)
                    .await
                    .map_err(map_err)?;
            }
            if let Some(limit) = c.history_length {
                history_limit = Some(limit as u32);
            }
        }

        // Start updates stream first so we don't miss early updates
        let update_stream = self
            .streaming_handler
            .start_task_streaming(&task_id)
            .await
            .map_err(map_err)?;

        let mut task = self
            .message_handler
            .process_message(&task_id, &message, session_id)
            .await
            .map_err(map_err)?;

        if let Some(limit) = history_limit {
            task = task.with_limited_history(Some(limit));
        }

        use futures::StreamExt;
        
        let initial_response = StreamResponse {
            payload: Some(stream_response::Payload::Task(Box::new(task))),
            ..Default::default()
        };

        let mapped_stream = update_stream.map(|item| {
            item.map(|evt| match evt {
                UpdateEvent::StatusUpdate(event) => StreamResponse {
                    payload: Some(stream_response::Payload::StatusUpdate(Box::new(map_status_update(event)))),
                    ..Default::default()
                },
                UpdateEvent::ArtifactUpdate(event) => StreamResponse {
                    payload: Some(stream_response::Payload::ArtifactUpdate(Box::new(map_artifact_update(event)))),
                    ..Default::default()
                },
            })
            .map_err(map_err)
        });

        let chained_stream = futures::stream::once(async { Ok(initial_response) })
            .chain(mapped_stream);

        Ok((Box::pin(chained_stream), ctx))
    }

    async fn get_task(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<GetTaskRequestView<'static>>,
    ) -> Result<(Task, ::connectrpc::Context), ::connectrpc::ConnectError> {
        let req = request.to_owned_message();
        let history_length = req.history_length.map(|l| l as u32);
        let task = self
            .task_manager
            .get_task(&req.id, history_length)
            .await
            .map_err(map_err)?;
        Ok((task, ctx))
    }

    async fn list_tasks(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<ListTasksRequestView<'static>>,
    ) -> Result<
        (ListTasksResponse, ::connectrpc::Context),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        
        let params = crate::domain::ListTasksParams {
            context_id: if req.context_id.is_empty() { None } else { Some(req.context_id) },
            status: match req.status.to_i32() {
                0 => None,
                val => Some(TaskState::from_i32(val).unwrap_or(TaskState::TASK_STATE_UNSPECIFIED)),
            },
            page_size: req.page_size,
            page_token: if req.page_token.is_empty() { None } else { Some(req.page_token) },
            history_length: req.history_length,
            include_artifacts: req.include_artifacts,
            status_timestamp_after: req.status_timestamp_after.as_option().map(|t| {
                let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(t.seconds, t.nanos as u32).unwrap_or_default();
                dt.to_rfc3339()
            }),
            metadata: None,
        };

        let result = self.task_manager.list_tasks_v3(&params).await.map_err(map_err)?;

        let response = ListTasksResponse {
            tasks: result.tasks,
            next_page_token: result.next_page_token,
            page_size: result.page_size,
            total_size: result.total_size,
            ..Default::default()
        };

        Ok((response, ctx))
    }

    async fn cancel_task(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<CancelTaskRequestView<'static>>,
    ) -> Result<(Task, ::connectrpc::Context), ::connectrpc::ConnectError> {
        let req = request.to_owned_message();
        let task = self
            .task_manager
            .cancel_task(&req.id)
            .await
            .map_err(map_err)?;
        Ok((task, ctx))
    }

    async fn subscribe_to_task(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<SubscribeToTaskRequestView<'static>>,
    ) -> Result<
        (
            ::std::pin::Pin<
                Box<
                    dyn ::futures::Stream<
                        Item = Result<StreamResponse, ::connectrpc::ConnectError>,
                    > + Send,
                >,
            >,
            ::connectrpc::Context,
        ),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        let task_id = req.id;

        let initial_task = match self.task_manager.get_task(&task_id, None).await {
            Ok(task) => Some(task),
            Err(A2AError::TaskNotFound(_)) => None,
            Err(e) => return Err(map_err(e)),
        };

        let update_stream = self
            .streaming_handler
            .start_task_streaming(&task_id)
            .await
            .map_err(map_err)?;

        use futures::StreamExt;

        let mapped_stream = update_stream.map(|item| {
            item.map(|evt| match evt {
                UpdateEvent::StatusUpdate(event) => StreamResponse {
                    payload: Some(stream_response::Payload::StatusUpdate(Box::new(map_status_update(event)))),
                    ..Default::default()
                },
                UpdateEvent::ArtifactUpdate(event) => StreamResponse {
                    payload: Some(stream_response::Payload::ArtifactUpdate(Box::new(map_artifact_update(event)))),
                    ..Default::default()
                },
            })
            .map_err(map_err)
        });

        if let Some(task) = initial_task {
            let initial_response = StreamResponse {
                payload: Some(stream_response::Payload::Task(Box::new(task))),
                ..Default::default()
            };
            let chained_stream = futures::stream::once(async { Ok(initial_response) })
                .chain(mapped_stream);
            Ok((Box::pin(chained_stream), ctx))
        } else {
            Ok((Box::pin(mapped_stream), ctx))
        }
    }

    async fn create_task_push_notification_config(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<TaskPushNotificationConfigView<'static>>,
    ) -> Result<
        (TaskPushNotificationConfig, ::connectrpc::Context),
        ::connectrpc::ConnectError,
    > {
        let config = request.to_owned_message();
        let created_config = self
            .notification_manager
            .set_task_notification_validated(&config)
            .await
            .map_err(map_err)?;
        Ok((created_config, ctx))
    }

    async fn get_task_push_notification_config(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<
            GetTaskPushNotificationConfigRequestView<'static>,
        >,
    ) -> Result<
        (TaskPushNotificationConfig, ::connectrpc::Context),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        let params = crate::domain::GetTaskPushNotificationConfigParams {
            id: req.task_id,
            push_notification_config_id: Some(req.id),
            metadata: None,
        };
        let config = self
            .task_manager
            .get_push_notification_config(&params)
            .await
            .map_err(map_err)?;
        Ok((config, ctx))
    }

    async fn list_task_push_notification_configs(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<
            ListTaskPushNotificationConfigsRequestView<'static>,
        >,
    ) -> Result<
        (ListTaskPushNotificationConfigsResponse, ::connectrpc::Context),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        let params = crate::domain::ListTaskPushNotificationConfigsParams {
            id: req.task_id,
            metadata: None,
        };
        let configs = self
            .task_manager
            .list_push_notification_configs(&params)
            .await
            .map_err(map_err)?;
        let response = ListTaskPushNotificationConfigsResponse {
            configs,
            ..Default::default()
        };
        Ok((response, ctx))
    }

    async fn get_extended_agent_card(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<GetExtendedAgentCardRequestView<'static>>,
    ) -> Result<(AgentCard, ::connectrpc::Context), ::connectrpc::ConnectError> {
        let _req = request.to_owned_message();
        let card = self
            .agent_info
            .get_authenticated_extended_card()
            .await
            .map_err(map_err)?;
        Ok((card, ctx))
    }

    async fn delete_task_push_notification_config(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<
            DeleteTaskPushNotificationConfigRequestView<'static>,
        >,
    ) -> Result<
        (::buffa_types::google::protobuf::Empty, ::connectrpc::Context),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        let params = crate::domain::DeleteTaskPushNotificationConfigParams {
            id: req.task_id,
            push_notification_config_id: req.id,
            metadata: None,
        };
        self.task_manager
            .delete_push_notification_config(&params)
            .await
            .map_err(map_err)?;
        Ok((::buffa_types::google::protobuf::Empty::default(), ctx))
    }
}

/// A no-op AsyncStreamingHandler implementation for request processor defaulting
#[derive(Clone, Debug, Default)]
pub struct NoopStreamingHandler;

#[async_trait]
impl AsyncStreamingHandler for NoopStreamingHandler {
    async fn add_status_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Err(A2AError::UnsupportedOperation("Streaming not supported by this processor".to_string()))
    }

    async fn add_artifact_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Err(A2AError::UnsupportedOperation("Streaming not supported by this processor".to_string()))
    }

    async fn remove_subscription(&self, _subscription_id: &str) -> Result<(), A2AError> {
        Ok(())
    }

    async fn remove_task_subscribers(&self, _task_id: &str) -> Result<(), A2AError> {
        Ok(())
    }

    async fn get_subscriber_count(&self, _task_id: &str) -> Result<usize, A2AError> {
        Ok(0)
    }

    async fn broadcast_status_update(
        &self,
        _task_id: &str,
        _update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        Ok(())
    }

    async fn broadcast_artifact_update(
        &self,
        _task_id: &str,
        _update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        Ok(())
    }

    async fn status_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<Pin<Box<dyn ::futures::Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>, A2AError> {
        Err(A2AError::UnsupportedOperation("Streaming not supported by this processor".to_string()))
    }

    async fn artifact_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<Box<dyn ::futures::Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation("Streaming not supported by this processor".to_string()))
    }

    async fn combined_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<Pin<Box<dyn ::futures::Stream<Item = Result<UpdateEvent, A2AError>> + Send>>, A2AError> {
        Err(A2AError::UnsupportedOperation("Streaming not supported by this processor".to_string()))
    }
}
