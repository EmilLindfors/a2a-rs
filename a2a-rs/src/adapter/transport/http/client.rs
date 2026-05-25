//! HTTP client adapter for the A2A protocol using ConnectRPC

use async_trait::async_trait;
use futures::stream::Stream;
use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue},
};
use std::{pin::Pin, time::Duration, sync::Arc};

#[cfg(feature = "tracing")]
use tracing::{debug, instrument};

use crate::{
    adapter::error::HttpClientError,
    domain::{
        A2AError, AgentCard, ListTasksParams, ListTasksResult, Message, Task,
        TaskPushNotificationConfig,
        generated::{
            A2aServiceClient, SendMessageRequest, SendMessageConfiguration,
            GetTaskRequest, ListTasksRequest, CancelTaskRequest,
            SubscribeToTaskRequest, GetTaskPushNotificationConfigRequest,
            DeleteTaskPushNotificationConfigRequest, ListTaskPushNotificationConfigsRequest,
            GetExtendedAgentCardRequest,
            stream_response, send_message_response, TaskState,
        },
    },
    services::client::{AsyncA2AClient, StreamItem},
};

fn map_connect_err(err: connectrpc::ConnectError) -> A2AError {
    let code = match err.code {
        connectrpc::ErrorCode::NotFound => crate::domain::error::TASK_NOT_FOUND,
        connectrpc::ErrorCode::Unimplemented => crate::domain::error::METHOD_NOT_FOUND,
        connectrpc::ErrorCode::InvalidArgument => crate::domain::error::INVALID_PARAMS,
        connectrpc::ErrorCode::Internal => crate::domain::error::INTERNAL_ERROR,
        connectrpc::ErrorCode::FailedPrecondition => crate::domain::error::AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED,
        _ => {
            let code_val = err.code as i32;
            if code_val != 0 { code_val } else { crate::domain::error::INTERNAL_ERROR }
        }
    };
    A2AError::JsonRpc {
        code,
        message: err.message.clone().unwrap_or_default(),
        data: None,
    }
}

fn map_stream_response(resp: crate::domain::generated::StreamResponse) -> Option<StreamItem> {
    match resp.payload {
        Some(stream_response::Payload::Task(task)) => {
            Some(StreamItem::Task(*task))
        }
        Some(stream_response::Payload::StatusUpdate(update)) => {
            Some(StreamItem::StatusUpdate((*update).into()))
        }
        Some(stream_response::Payload::ArtifactUpdate(update)) => {
            Some(StreamItem::ArtifactUpdate((*update).into()))
        }
        _ => None,
    }
}

/// HTTP client for interacting with the A2A protocol via ConnectRPC
pub struct HttpClient {
    /// Base URL of the A2A API
    base_url: String,
    /// reqwest Client for standard GET operations like agent card
    client: Client,
    /// ConnectRPC Client
    connect_client: A2aServiceClient<connectrpc::client::HttpClient>,
    /// Authorization token, if any
    auth_token: Option<String>,
    /// Timeout in seconds
    timeout: u64,
}

impl HttpClient {
    /// Create a new HTTP client with the given base URL
    pub fn new(base_url: String) -> Self {
        let uri = base_url.parse::<http::Uri>().expect("Invalid base URL");
        let is_https = uri.scheme_str() == Some("https");

        let transport = if is_https {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let tls_config = rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();
            connectrpc::client::HttpClient::with_tls(Arc::new(tls_config))
        } else {
            connectrpc::client::HttpClient::plaintext()
        };

        let mut config = connectrpc::client::ClientConfig::new(uri);
        config = config.default_timeout(Duration::from_secs(30));

        let connect_client = A2aServiceClient::new(transport, config);

        Self {
            base_url,
            client: Client::new(),
            connect_client,
            auth_token: None,
            timeout: 30,
        }
    }

    /// Create a new HTTP client with authentication
    pub fn with_auth(base_url: String, auth_token: String) -> Self {
        let uri = base_url.parse::<http::Uri>().expect("Invalid base URL");
        let is_https = uri.scheme_str() == Some("https");

        let transport = if is_https {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let tls_config = rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();
            connectrpc::client::HttpClient::with_tls(Arc::new(tls_config))
        } else {
            connectrpc::client::HttpClient::plaintext()
        };

        let mut config = connectrpc::client::ClientConfig::new(uri);
        config = config.default_timeout(Duration::from_secs(30))
            .default_header("authorization", format!("Bearer {}", auth_token));

        let connect_client = A2aServiceClient::new(transport, config);

        Self {
            base_url,
            client: Client::new(),
            connect_client,
            auth_token: Some(auth_token),
            timeout: 30,
        }
    }

    /// Set the timeout for requests
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        *self.connect_client.config_mut() = self.connect_client.config().clone().default_timeout(Duration::from_secs(timeout));
        self
    }

    /// Get the headers for a request (used for reqwest)
    fn get_headers(&self) -> Result<HeaderMap, A2AError> {
        let mut headers = HeaderMap::new();
        headers.insert(reqwest::header::CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(token) = &self.auth_token {
            let auth_value = HeaderValue::from_str(&format!("Bearer {}", token)).map_err(|e| {
                A2AError::Internal(format!("Invalid auth token for HTTP header: {}", e))
            })?;
            headers.insert(reqwest::header::AUTHORIZATION, auth_value);
        }

        Ok(headers)
    }

    /// Get the base URL of the client
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Fetch the agent card from the agent's `/agent-card` endpoint (plain HTTP GET)
    pub async fn get_agent_card(&self) -> Result<AgentCard, A2AError> {
        let url = if self.base_url.ends_with('/') {
            format!("{}agent-card", self.base_url)
        } else {
            match reqwest::Url::parse(&self.base_url) {
                Ok(parsed) => {
                    if !parsed.path().ends_with('/') {
                        match parsed.join("/agent-card") {
                            Ok(resolved) => resolved.to_string(),
                            Err(_) => format!("{}/agent-card", self.base_url),
                        }
                    } else {
                        match parsed.join("agent-card") {
                            Ok(resolved) => resolved.to_string(),
                            Err(_) => format!("{}/agent-card", self.base_url),
                        }
                    }
                }
                Err(_) => format!("{}/agent-card", self.base_url),
            }
        };

        #[cfg(feature = "tracing")]
        debug!("Fetching agent card from URL: {}", url);

        let response = self
            .client
            .get(&url)
            .headers(self.get_headers()?)
            .timeout(Duration::from_secs(self.timeout))
            .send()
            .await
            .map_err(HttpClientError::Reqwest)?;

        if response.status().is_success() {
            let card: AgentCard = response.json().await.map_err(|e| {
                A2AError::Internal(format!("Failed to parse agent card JSON: {}", e))
            })?;
            Ok(card)
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(HttpClientError::Response {
                status: status.as_u16(),
                message: body,
            }
            .into())
        }
    }

    /// Fetch the extended agent card using ConnectRPC
    pub async fn get_extended_agent_card(&self, tenant: Option<String>) -> Result<AgentCard, A2AError> {
        let request = GetExtendedAgentCardRequest {
            tenant: tenant.unwrap_or_default(),
            ..Default::default()
        };
        let response = self.connect_client.get_extended_agent_card(request).await.map_err(map_connect_err)?;
        Ok(response.into_owned())
    }
}

#[async_trait]
impl AsyncA2AClient for HttpClient {
    #[cfg_attr(
        feature = "tracing",
        instrument(skip(self, message), fields(task_id, session_id, history_length))
    )]
    async fn send_task_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        let mut msg = message.clone();
        msg.task_id = task_id.to_string();
        if let Some(sid) = session_id {
            msg.context_id = sid.to_string();
        }

        let config = SendMessageConfiguration {
            history_length: history_length.map(|l| l as i32),
            ..Default::default()
        };

        let request = SendMessageRequest {
            message: ::buffa::MessageField::some(msg),
            configuration: ::buffa::MessageField::some(config),
            ..Default::default()
        };

        let response = self.connect_client.send_message(request).await.map_err(map_connect_err)?;
        let owned_response = response.into_owned();

        match owned_response.payload {
            Some(send_message_response::Payload::Task(task)) => {
                Ok(*task)
            }
            _ => Err(A2AError::Internal("Expected task in SendMessageResponse payload".to_string())),
        }
    }

    #[cfg_attr(
        feature = "tracing",
        instrument(skip(self), fields(task_id, history_length))
    )]
    async fn get_task(&self, task_id: &str, history_length: Option<u32>) -> Result<Task, A2AError> {
        let request = GetTaskRequest {
            id: task_id.to_string(),
            history_length: history_length.map(|l| l as i32),
            ..Default::default()
        };
        let response = self.connect_client.get_task(request).await.map_err(map_connect_err)?;
        Ok(response.into_owned())
    }

    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(task_id)))]
    async fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError> {
        let request = CancelTaskRequest {
            id: task_id.to_string(),
            ..Default::default()
        };
        let response = self.connect_client.cancel_task(request).await.map_err(map_connect_err)?;
        Ok(response.into_owned())
    }

    async fn set_task_push_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        let request = config.clone();
        let response = self.connect_client.create_task_push_notification_config(request).await.map_err(map_connect_err)?;
        Ok(response.into_owned())
    }

    async fn get_task_push_notification(
        &self,
        task_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        let request = ListTaskPushNotificationConfigsRequest {
            task_id: task_id.to_string(),
            ..Default::default()
        };
        let response = self.connect_client.list_task_push_notification_configs(request).await.map_err(map_connect_err)?;
        let configs = response.into_owned().configs;
        if let Some(config) = configs.into_iter().next() {
            Ok(config)
        } else {
            Err(A2AError::TaskNotFound(format!("No push notification config found for task {}", task_id)))
        }
    }

    #[cfg_attr(feature = "tracing", instrument(skip(self, params)))]
    async fn list_tasks(&self, params: &ListTasksParams) -> Result<ListTasksResult, A2AError> {
        let mut request = ListTasksRequest {
            context_id: params.context_id.clone().unwrap_or_default(),
            status: ::buffa::EnumValue::from(params.status.unwrap_or(TaskState::TASK_STATE_UNSPECIFIED)),
            page_size: params.page_size,
            page_token: params.page_token.clone().unwrap_or_default(),
            history_length: params.history_length,
            include_artifacts: params.include_artifacts,
            ..Default::default()
        };
        if let Some(ref t_str) = params.status_timestamp_after {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t_str) {
                let utc_dt = dt.with_timezone(&chrono::Utc);
                request.status_timestamp_after = ::buffa::MessageField::some(::buffa_types::google::protobuf::Timestamp {
                    seconds: utc_dt.timestamp(),
                    nanos: utc_dt.timestamp_subsec_nanos() as i32,
                    ..Default::default()
                });
            }
        }

        let response = self.connect_client.list_tasks(request).await.map_err(map_connect_err)?;
        let owned = response.into_owned();
        Ok(ListTasksResult {
            tasks: owned.tasks,
            total_size: owned.total_size,
            page_size: owned.page_size,
            next_page_token: owned.next_page_token,
        })
    }

    async fn list_push_notification_configs(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        let request = ListTaskPushNotificationConfigsRequest {
            task_id: task_id.to_string(),
            ..Default::default()
        };
        let response = self.connect_client.list_task_push_notification_configs(request).await.map_err(map_connect_err)?;
        Ok(response.into_owned().configs)
    }

    async fn get_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        let request = GetTaskPushNotificationConfigRequest {
            task_id: task_id.to_string(),
            id: config_id.to_string(),
            ..Default::default()
        };
        let response = self.connect_client.get_task_push_notification_config(request).await.map_err(map_connect_err)?;
        Ok(response.into_owned())
    }

    async fn delete_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<(), A2AError> {
        let request = DeleteTaskPushNotificationConfigRequest {
            task_id: task_id.to_string(),
            id: config_id.to_string(),
            ..Default::default()
        };
        self.connect_client.delete_task_push_notification_config(request).await.map_err(map_connect_err)?;
        Ok(())
    }

    async fn subscribe_to_task(
        &self,
        task_id: &str,
        _history_length: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamItem, A2AError>> + Send>>, A2AError> {
        let request = SubscribeToTaskRequest {
            id: task_id.to_string(),
            ..Default::default()
        };
        let stream = self.connect_client.subscribe_to_task(request).await.map_err(map_connect_err)?;

        let mapped = futures::stream::unfold(stream, |mut s| async move {
            match s.message().await {
                Ok(Some(view)) => {
                    let resp = view.to_owned_message();
                    if let Some(item) = map_stream_response(resp) {
                        Some((Ok(item), s))
                    } else {
                        Some((Err(A2AError::Internal("Empty or unhandled stream response payload".to_string())), s))
                    }
                }
                Ok(None) => None,
                Err(e) => Some((Err(map_connect_err(e)), s)),
            }
        });

        Ok(Box::pin(mapped))
    }
}
