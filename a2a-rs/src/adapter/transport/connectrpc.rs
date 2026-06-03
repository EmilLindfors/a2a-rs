//! The ConnectRPC transport adapter.
//!
//! `ConnectRpcAdapter` is the **outer** half of the service/transport split: a
//! thin transport adapter that implements the generated [`A2aService`] surface.
//! Its only job is to decode `buffa` wire views into domain values, delegate to
//! the inner [`TaskService`], and re-encode the domain results (and map
//! [`A2AError`] onto ConnectRPC error codes). All use-case orchestration lives
//! in [`TaskService`]; this layer holds no port traits directly.
//!
//! The public constructors (`new`, `with_handler`, `with_streaming_handler`)
//! each build the inner service and wrap it.

use async_trait::async_trait;
use buffa::Enumeration;
use std::pin::Pin;

use crate::{
    application::TaskService,
    domain::{
        A2AError, AgentCard, Task, TaskArtifactUpdateEvent, TaskId, TaskPushNotificationConfig,
        TaskStatusUpdateEvent,
        generated::{
            A2aService, CancelTaskRequestView, DeleteTaskPushNotificationConfigRequestView,
            GetExtendedAgentCardRequestView, GetTaskPushNotificationConfigRequestView,
            GetTaskRequestView, ListTaskPushNotificationConfigsRequestView,
            ListTaskPushNotificationConfigsResponse, ListTasksRequest, ListTasksRequestView,
            ListTasksResponse,
            SendMessageRequestView, SendMessageResponse, StreamResponse,
            SubscribeToTaskRequestView, TaskArtifactUpdateEvent as GenTaskArtifactUpdateEvent,
            TaskPushNotificationConfigView, TaskState,
            TaskStatusUpdateEvent as GenTaskStatusUpdateEvent, send_message_response,
            stream_response,
        },
    },
    port::{
        AsyncMessageHandler, AsyncNotificationManager, AsyncStreamingHandler, AsyncTaskLifecycle,
        AsyncTaskQuery, UpdateEvent, streaming_handler::Subscriber,
    },
    services::server::AgentInfoProvider,
};

/// ConnectRPC transport adapter over a [`TaskService`].
///
/// Holds no ports directly — it owns the inner application service and forwards
/// decoded requests to it. Dispatch into the service goes through the service's
/// `Arc<dyn …>` fields, which is a cold path against the I/O each call performs.
#[derive(Clone)]
pub struct ConnectRpcAdapter {
    service: TaskService,
}

impl ConnectRpcAdapter {
    /// Create a new adapter from separate handlers, defaulting to a no-op
    /// streaming handler.
    ///
    /// `tasks` supplies both the lifecycle and query capabilities.
    pub fn new(
        message_handler: impl AsyncMessageHandler + 'static,
        tasks: impl AsyncTaskLifecycle + AsyncTaskQuery + 'static,
        notification_manager: impl AsyncNotificationManager + 'static,
        agent_info: impl AgentInfoProvider + 'static,
    ) -> Self {
        Self {
            service: TaskService::new(
                message_handler,
                tasks,
                notification_manager,
                agent_info,
                NoopStreamingHandler,
                crate::port::NoopPushNotifier,
            ),
        }
    }

    /// Create a new adapter from a single handler that implements every port,
    /// defaulting to a no-op streaming handler.
    pub fn with_handler(
        handler: impl AsyncMessageHandler
        + AsyncTaskLifecycle
        + AsyncTaskQuery
        + AsyncNotificationManager
        + 'static,
        agent_info: impl AgentInfoProvider + 'static,
    ) -> Self {
        Self {
            service: TaskService::with_handler(
                handler,
                agent_info,
                NoopStreamingHandler,
                crate::port::NoopPushNotifier,
            ),
        }
    }

    /// Builder-style method to inject custom streaming handler support.
    pub fn with_streaming_handler(
        self,
        streaming_handler: impl AsyncStreamingHandler + 'static,
    ) -> Self {
        Self {
            service: self.service.with_streaming_handler(streaming_handler),
        }
    }

    /// Builder-style method to inject a custom push notifier.
    pub fn with_push_notifier(
        self,
        push_notifier: impl crate::port::AsyncPushNotifier + 'static,
    ) -> Self {
        Self {
            service: self.service.with_push_notifier(push_notifier),
        }
    }
}

/// Helper function to map A2AError to connectrpc::ConnectError
fn map_err(e: A2AError) -> ::connectrpc::ConnectError {
    match e {
        A2AError::TaskNotFound(msg) => {
            ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::NotFound, msg)
        }
        A2AError::InvalidParams(msg) => {
            ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::InvalidArgument, msg)
        }
        A2AError::ValidationError { field, message } => ::connectrpc::ConnectError::new(
            ::connectrpc::ErrorCode::InvalidArgument,
            format!("{}: {}", field, message),
        ),
        A2AError::UnsupportedOperation(msg) => {
            ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::Unimplemented, msg)
        }
        A2AError::AuthenticatedExtendedCardNotConfigured => ::connectrpc::ConnectError::new(
            ::connectrpc::ErrorCode::FailedPrecondition,
            "Authenticated extended card not configured".to_string(),
        ),
        A2AError::MethodNotFound(msg) => {
            ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::Unimplemented, msg)
        }
        _ => ::connectrpc::ConnectError::new(::connectrpc::ErrorCode::Internal, e.to_string()),
    }
}

/// Helper to map domain metadata to protobuf Struct
fn map_metadata(
    opt: Option<serde_json::Map<String, serde_json::Value>>,
) -> ::buffa::MessageField<::buffa_types::google::protobuf::Struct> {
    if let Some(map) = opt {
        let val = serde_json::Value::Object(map);
        if let Ok(struc) = serde_json::from_value::<::buffa_types::google::protobuf::Struct>(val) {
            return ::buffa::MessageField::some(struc);
        }
    }
    ::buffa::MessageField::none()
}

fn map_status_update(
    evt: crate::domain::events::TaskStatusUpdateEvent,
) -> GenTaskStatusUpdateEvent {
    GenTaskStatusUpdateEvent {
        task_id: evt.task_id,
        context_id: evt.context_id,
        status: ::buffa::MessageField::some(evt.status),
        metadata: map_metadata(evt.metadata),
        ..Default::default()
    }
}

fn map_artifact_update(
    evt: crate::domain::events::TaskArtifactUpdateEvent,
) -> GenTaskArtifactUpdateEvent {
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

/// Map a domain [`UpdateEvent`] onto its wire [`StreamResponse`].
///
/// Shared with the JSON-RPC adapter so both transports map streaming updates
/// through one path.
pub(super) fn map_update_event(evt: UpdateEvent) -> StreamResponse {
    match evt {
        UpdateEvent::StatusUpdate(event) => StreamResponse {
            payload: Some(stream_response::Payload::StatusUpdate(Box::new(
                map_status_update(event),
            ))),
            ..Default::default()
        },
        UpdateEvent::ArtifactUpdate(event) => StreamResponse {
            payload: Some(stream_response::Payload::ArtifactUpdate(Box::new(
                map_artifact_update(event),
            ))),
            ..Default::default()
        },
    }
}

impl A2aService for ConnectRpcAdapter {
    async fn send_message(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<SendMessageRequestView<'static>>,
    ) -> Result<(SendMessageResponse, ::connectrpc::Context), ::connectrpc::ConnectError> {
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

        let (push_config, history_limit) = decode_send_config(config);

        let task = self
            .service
            .send_message(&task_id, &message, session_id, push_config, history_limit)
            .await
            .map_err(map_err)?;

        let response = SendMessageResponse {
            payload: Some(send_message_response::Payload::Task(Box::new(task))),
            ..Default::default()
        };

        Ok((response, ctx))
    }

    #[allow(clippy::result_large_err)]
    async fn send_streaming_message(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<SendMessageRequestView<'static>>,
    ) -> Result<
        (
            ::std::pin::Pin<
                Box<
                    dyn ::futures::Stream<Item = Result<StreamResponse, ::connectrpc::ConnectError>>
                        + Send,
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

        let (push_config, history_limit) = decode_send_config(config);

        let (task, update_stream) = self
            .service
            .send_streaming_message(&task_id, &message, session_id, push_config, history_limit)
            .await
            .map_err(map_err)?;

        use futures::StreamExt;

        let initial_response = StreamResponse {
            payload: Some(stream_response::Payload::Task(Box::new(task))),
            ..Default::default()
        };

        let mapped_stream =
            update_stream.map(|item| item.map(map_update_event).map_err(map_err));

        let chained_stream =
            futures::stream::once(async { Ok(initial_response) }).chain(mapped_stream);

        Ok((Box::pin(chained_stream), ctx))
    }

    async fn get_task(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<GetTaskRequestView<'static>>,
    ) -> Result<(Task, ::connectrpc::Context), ::connectrpc::ConnectError> {
        let req = request.to_owned_message();
        let history_length = req.history_length.map(|l| l as u32);
        let id: TaskId = req.id.parse().map_err(map_err)?;
        let task = self
            .service
            .get(&id, history_length)
            .await
            .map_err(map_err)?;
        Ok((task, ctx))
    }

    async fn list_tasks(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<ListTasksRequestView<'static>>,
    ) -> Result<(ListTasksResponse, ::connectrpc::Context), ::connectrpc::ConnectError> {
        let req = request.to_owned_message();
        let params = list_request_to_params(req);

        let result = self.service.list(&params).await.map_err(map_err)?;

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
        let id: TaskId = req.id.parse().map_err(map_err)?;
        let task = self.service.cancel(&id).await.map_err(map_err)?;
        Ok((task, ctx))
    }

    #[allow(clippy::result_large_err)]
    async fn subscribe_to_task(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<SubscribeToTaskRequestView<'static>>,
    ) -> Result<
        (
            ::std::pin::Pin<
                Box<
                    dyn ::futures::Stream<Item = Result<StreamResponse, ::connectrpc::ConnectError>>
                        + Send,
                >,
            >,
            ::connectrpc::Context,
        ),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();

        let (initial_task, update_stream) =
            self.service.subscribe(&req.id).await.map_err(map_err)?;

        use futures::StreamExt;

        let mapped_stream =
            update_stream.map(|item| item.map(map_update_event).map_err(map_err));

        if let Some(task) = initial_task {
            let initial_response = StreamResponse {
                payload: Some(stream_response::Payload::Task(Box::new(task))),
                ..Default::default()
            };
            let chained_stream =
                futures::stream::once(async { Ok(initial_response) }).chain(mapped_stream);
            Ok((Box::pin(chained_stream), ctx))
        } else {
            Ok((Box::pin(mapped_stream), ctx))
        }
    }

    async fn create_task_push_notification_config(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<TaskPushNotificationConfigView<'static>>,
    ) -> Result<(TaskPushNotificationConfig, ::connectrpc::Context), ::connectrpc::ConnectError>
    {
        let config = request.to_owned_message();
        let created_config = self
            .service
            .set_push_config(&config)
            .await
            .map_err(map_err)?;
        Ok((created_config, ctx))
    }

    async fn get_task_push_notification_config(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<GetTaskPushNotificationConfigRequestView<'static>>,
    ) -> Result<(TaskPushNotificationConfig, ::connectrpc::Context), ::connectrpc::ConnectError>
    {
        let req = request.to_owned_message();
        let params = crate::domain::GetTaskPushNotificationConfigParams {
            id: req.task_id,
            push_notification_config_id: Some(req.id),
            metadata: None,
        };
        let config = self
            .service
            .get_push_config(&params)
            .await
            .map_err(map_err)?;
        Ok((config, ctx))
    }

    async fn list_task_push_notification_configs(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<ListTaskPushNotificationConfigsRequestView<'static>>,
    ) -> Result<
        (
            ListTaskPushNotificationConfigsResponse,
            ::connectrpc::Context,
        ),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        let params = crate::domain::ListTaskPushNotificationConfigsParams {
            id: req.task_id,
            metadata: None,
        };
        let configs = self
            .service
            .list_push_configs(&params)
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
            .service
            .extended_agent_card()
            .await
            .map_err(map_err)?;
        Ok((card, ctx))
    }

    async fn delete_task_push_notification_config(
        &self,
        ctx: ::connectrpc::Context,
        request: ::buffa::view::OwnedView<DeleteTaskPushNotificationConfigRequestView<'static>>,
    ) -> Result<
        (
            ::buffa_types::google::protobuf::Empty,
            ::connectrpc::Context,
        ),
        ::connectrpc::ConnectError,
    > {
        let req = request.to_owned_message();
        let params = crate::domain::DeleteTaskPushNotificationConfigParams {
            id: req.task_id,
            push_notification_config_id: req.id,
            metadata: None,
        };
        self.service
            .delete_push_config(&params)
            .await
            .map_err(map_err)?;
        Ok((::buffa_types::google::protobuf::Empty::default(), ctx))
    }
}

/// Map a generated `ListTasksRequest` (proto wire message) onto the domain
/// [`ListTasksParams`]. Shared with the JSON-RPC adapter.
pub(super) fn list_request_to_params(req: ListTasksRequest) -> crate::domain::ListTasksParams {
    crate::domain::ListTasksParams {
        context_id: if req.context_id.is_empty() {
            None
        } else {
            Some(req.context_id)
        },
        status: match req.status.to_i32() {
            0 => None,
            val => Some(TaskState::from_i32(val).unwrap_or(TaskState::TASK_STATE_UNSPECIFIED)),
        },
        page_size: req.page_size,
        page_token: if req.page_token.is_empty() {
            None
        } else {
            Some(req.page_token)
        },
        history_length: req.history_length,
        include_artifacts: req.include_artifacts,
        status_timestamp_after: req.status_timestamp_after.as_option().map(|t| {
            let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(t.seconds, t.nanos as u32)
                .unwrap_or_default();
            dt.to_rfc3339()
        }),
        metadata: None,
    }
}

/// Decode the optional `SendMessageConfiguration` view into the domain push
/// config + history limit the service expects.
///
/// Shared with the JSON-RPC adapter (both decode the same generated config
/// message), so the two transports agree on the wire shape.
pub(super) fn decode_send_config(
    config: Option<crate::domain::generated::SendMessageConfiguration>,
) -> (Option<TaskPushNotificationConfig>, Option<u32>) {
    let Some(c) = config else {
        return (None, None);
    };
    let push_config = c.task_push_notification_config.into_option();
    let history_limit = c.history_length.map(|limit| limit as u32);
    (push_config, history_limit)
}

/// A no-op [`AsyncStreamingHandler`] used as the adapter's default streaming port
/// when the caller has no real streaming backend to inject.
#[derive(Clone, Debug, Default)]
pub struct NoopStreamingHandler;

#[async_trait]
impl AsyncStreamingHandler for NoopStreamingHandler {
    async fn add_status_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Streaming not supported by this processor".to_string(),
        ))
    }

    async fn add_artifact_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Streaming not supported by this processor".to_string(),
        ))
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
    ) -> Result<
        Pin<Box<dyn ::futures::Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Streaming not supported by this processor".to_string(),
        ))
    }

    async fn artifact_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<Box<dyn ::futures::Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Streaming not supported by this processor".to_string(),
        ))
    }

    async fn combined_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<Box<dyn ::futures::Stream<Item = Result<UpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        Err(A2AError::UnsupportedOperation(
            "Streaming not supported by this processor".to_string(),
        ))
    }
}
