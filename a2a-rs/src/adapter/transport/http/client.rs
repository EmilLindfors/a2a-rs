//! HTTP client adapter for the A2A protocol using ConnectRPC

use async_trait::async_trait;
use futures::stream::Stream;
use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue},
};
use std::{pin::Pin, sync::Arc, time::Duration};

#[cfg(feature = "tracing")]
use tracing::{debug, instrument};

use crate::{
    adapter::error::HttpClientError,
    adapter::transport::codec::stream_response_to_item,
    domain::{
        A2AError, AgentCard, ListTasksParams, ListTasksResult, Message, SendCompletion, Task,
        TaskPushNotificationConfig,
        generated::{
            A2aServiceClient, CancelTaskRequest, DeleteTaskPushNotificationConfigRequest,
            GetExtendedAgentCardRequest, GetTaskPushNotificationConfigRequest, GetTaskRequest,
            ListTaskPushNotificationConfigsRequest, ListTasksRequest, SendMessageConfiguration,
            SendMessageRequest, SubscribeToTaskRequest, TaskState, send_message_response,
        },
    },
    port::{StreamEvent, Transport},
};

use crate::adapter::transport::resume;

/// Map a wire error back onto the domain. See `connect_wire`: exact when the
/// server is a2a-rs, a category otherwise.
fn map_connect_err(err: connectrpc::ConnectError) -> A2AError {
    crate::adapter::transport::connect_wire::from_connect_error(err)
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
    /// Create a new HTTP client with the given base URL.
    ///
    /// # Panics
    ///
    /// If `base_url` is not a valid `http::Uri`. Use [`try_new`](Self::try_new)
    /// whenever the URL came from outside the program — a CLI flag, a config
    /// file, or an agent card.
    pub fn new(base_url: String) -> Self {
        Self::try_new(base_url).expect("Invalid base URL")
    }

    /// Create a new HTTP client, reporting an unusable base URL rather than
    /// panicking.
    ///
    /// `http::Uri` is stricter than the URL parsers callers tend to validate
    /// with: `reqwest::Url` accepts an IDN host like `http://münchen.de` and
    /// normalizes it to punycode, while `http::Uri` rejects the raw bytes. A
    /// caller that checked with the former and built with `new` would panic on
    /// a URL it had just declared valid.
    pub fn try_new(base_url: String) -> Result<Self, A2AError> {
        let (transport, config) = Self::transport_for(&base_url)?;
        Ok(Self {
            base_url,
            client: Client::new(),
            connect_client: A2aServiceClient::new(transport, config),
            auth_token: None,
            timeout: 30,
        })
    }

    /// Create a new HTTP client with authentication.
    ///
    /// # Panics
    ///
    /// As [`new`](Self::new); see [`try_with_auth`](Self::try_with_auth).
    pub fn with_auth(base_url: String, auth_token: String) -> Self {
        Self::try_with_auth(base_url, auth_token).expect("Invalid base URL")
    }

    /// Create an authenticated HTTP client, reporting an unusable base URL
    /// rather than panicking. See [`try_new`](Self::try_new).
    pub fn try_with_auth(base_url: String, auth_token: String) -> Result<Self, A2AError> {
        let (transport, config) = Self::transport_for(&base_url)?;
        let config = config.default_header("authorization", format!("Bearer {}", auth_token));
        Ok(Self {
            base_url,
            client: Client::new(),
            connect_client: A2aServiceClient::new(transport, config),
            auth_token: Some(auth_token),
            timeout: 30,
        })
    }

    /// The ConnectRPC transport and base config for `base_url`, TLS-enabled for
    /// `https`. Shared so the authenticated and anonymous constructors cannot
    /// drift on which scheme gets a TLS stack.
    fn transport_for(
        base_url: &str,
    ) -> Result<
        (
            connectrpc::client::HttpClient,
            connectrpc::client::ClientConfig,
        ),
        A2AError,
    > {
        let uri = base_url
            .parse::<http::Uri>()
            .map_err(|e| A2AError::InvalidParams(format!("invalid base url {base_url}: {e}")))?;

        let transport = if uri.scheme_str() == Some("https") {
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

        let config =
            connectrpc::client::ClientConfig::new(uri).default_timeout(Duration::from_secs(30));
        Ok((transport, config))
    }

    /// Set the timeout for requests
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        *self.connect_client.config_mut() = self
            .connect_client
            .config()
            .clone()
            .default_timeout(Duration::from_secs(timeout));
        self
    }

    /// Get the headers for a request (used for reqwest)
    fn get_headers(&self) -> Result<HeaderMap, A2AError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

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
    pub async fn get_extended_agent_card(
        &self,
        tenant: Option<String>,
    ) -> Result<AgentCard, A2AError> {
        let request = GetExtendedAgentCardRequest {
            tenant: tenant.unwrap_or_default(),
            ..Default::default()
        };
        let response = self
            .connect_client
            .get_extended_agent_card(request)
            .await
            .map_err(map_connect_err)?;
        Ok(response.into_owned())
    }
}

#[async_trait]
impl Transport for HttpClient {
    fn protocol(&self) -> &str {
        "CONNECTRPC"
    }

    #[cfg_attr(
        feature = "tracing",
        instrument(skip(self, message), fields(task_id, session_id, history_length))
    )]
    async fn send_task_message(
        &self,
        task_id: Option<&str>,
        message: &Message,
        session_id: Option<&str>,
        history_length: Option<u32>,
        completion: SendCompletion,
    ) -> Result<Task, A2AError> {
        let mut msg = message.clone();
        // Left empty when the caller passed `None`: the wire treats an absent
        // task id as "server assigns one".
        if let Some(id) = task_id {
            msg.task_id = id.to_string();
        }
        if let Some(sid) = session_id {
            msg.context_id = sid.to_string();
        }

        let config = SendMessageConfiguration {
            history_length: history_length.map(|l| l as i32),
            return_immediately: completion.return_immediately(),
            ..Default::default()
        };

        let request = SendMessageRequest {
            message: ::buffa::MessageField::some(msg),
            configuration: ::buffa::MessageField::some(config),
            ..Default::default()
        };

        let response = self
            .connect_client
            .send_message(request)
            .await
            .map_err(map_connect_err)?;
        let owned_response = response.into_owned();

        match owned_response.payload {
            Some(send_message_response::Payload::Task(task)) => Ok(*task),
            _ => Err(A2AError::Internal(
                "Expected task in SendMessageResponse payload".to_string(),
            )),
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
        let response = self
            .connect_client
            .get_task(request)
            .await
            .map_err(map_connect_err)?;
        Ok(response.into_owned())
    }

    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(task_id)))]
    async fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError> {
        let request = CancelTaskRequest {
            id: task_id.to_string(),
            ..Default::default()
        };
        let response = self
            .connect_client
            .cancel_task(request)
            .await
            .map_err(map_connect_err)?;
        Ok(response.into_owned())
    }

    async fn set_task_push_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        let request = config.clone();
        let response = self
            .connect_client
            .create_task_push_notification_config(request)
            .await
            .map_err(map_connect_err)?;
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
        let response = self
            .connect_client
            .list_task_push_notification_configs(request)
            .await
            .map_err(map_connect_err)?;
        let configs = response.into_owned().configs;
        if let Some(config) = configs.into_iter().next() {
            Ok(config)
        } else {
            Err(A2AError::TaskNotFound(format!(
                "No push notification config found for task {}",
                task_id
            )))
        }
    }

    #[cfg_attr(feature = "tracing", instrument(skip(self, params)))]
    async fn list_tasks(&self, params: &ListTasksParams) -> Result<ListTasksResult, A2AError> {
        let mut request = ListTasksRequest {
            context_id: params.context_id.clone().unwrap_or_default(),
            status: ::buffa::EnumValue::from(
                params.status.unwrap_or(TaskState::TASK_STATE_UNSPECIFIED),
            ),
            page_size: params.page_size,
            page_token: params.page_token.clone().unwrap_or_default(),
            history_length: params.history_length,
            include_artifacts: params.include_artifacts,
            ..Default::default()
        };
        if let Some(ref t_str) = params.status_timestamp_after
            && let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t_str)
        {
            let utc_dt = dt.with_timezone(&chrono::Utc);
            request.status_timestamp_after =
                ::buffa::MessageField::some(::buffa_types::google::protobuf::Timestamp {
                    seconds: utc_dt.timestamp(),
                    nanos: utc_dt.timestamp_subsec_nanos() as i32,
                    ..Default::default()
                });
        }

        let response = self
            .connect_client
            .list_tasks(request)
            .await
            .map_err(map_connect_err)?;
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
        let response = self
            .connect_client
            .list_task_push_notification_configs(request)
            .await
            .map_err(map_connect_err)?;
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
        let response = self
            .connect_client
            .get_task_push_notification_config(request)
            .await
            .map_err(map_connect_err)?;
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
        self.connect_client
            .delete_task_push_notification_config(request)
            .await
            .map_err(map_connect_err)?;
        Ok(())
    }

    async fn subscribe_to_task(
        &self,
        task_id: &str,
        _history_length: Option<u32>,
        last_event_id: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>, A2AError> {
        let request = SubscribeToTaskRequest {
            id: task_id.to_string(),
            ..Default::default()
        };

        // ConnectRPC has no SSE `id:` field, so ids ride in the update's
        // metadata — but only for a client that asks, since that changes the
        // payload. See the `resume` module.
        let mut options =
            connectrpc::client::CallOptions::default().with_header(resume::EVENT_IDS_HEADER, "1");
        if let Some(id) = last_event_id {
            options = options
                .try_with_header("last-event-id", id)
                .map_err(map_connect_err)?;
        }

        let stream = self
            .connect_client
            .subscribe_to_task_with_options(request, options)
            .await
            .map_err(map_connect_err)?;

        // A Connect streaming call answers HTTP 200 before the handler has
        // run, so a server that refuses the subscription outright says so in
        // the END_STREAM envelope — which the client library parks in
        // `ServerStream::error()` behind an ordinary `Ok(None)`. Read it
        // there, once, or a refusal is indistinguishable from a task that
        // settled with nothing to say.
        let mapped = futures::stream::unfold((stream, false), |(mut s, ended)| async move {
            if ended {
                return None;
            }
            match s.message().await {
                Ok(Some(view)) => {
                    let resp = view.to_owned_message();
                    if let Some(mut item) = stream_response_to_item(resp) {
                        let event_id = resume::take_event_id(&mut item);
                        Some((Ok(StreamEvent::new(event_id, item)), (s, false)))
                    } else {
                        Some((
                            Err(A2AError::Internal(
                                "Empty or unhandled stream response payload".to_string(),
                            )),
                            (s, false),
                        ))
                    }
                }
                Ok(None) => {
                    let trailing = s.error().cloned()?;
                    Some((Err(map_connect_err(trailing)), (s, true)))
                }
                Err(e) => Some((Err(map_connect_err(e)), (s, false))),
            }
        });

        Ok(Box::pin(mapped))
    }
}
