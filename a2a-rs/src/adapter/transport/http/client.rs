//! HTTP client adapter for the A2A protocol

// This module is already conditionally compiled with #[cfg(feature = "http-client")] in mod.rs

use async_trait::async_trait;
use futures::stream::Stream;
use reqwest::{
    Client,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue},
};
use std::{pin::Pin, time::Duration};

#[cfg(feature = "tracing")]
use tracing::instrument;

use crate::{
    adapter::error::HttpClientError,
    application::{json_rpc::{self, A2ARequest, SendTaskRequest}, JSONRPCResponse},
    domain::{
        A2AError, Message, Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig, 
        TaskQueryParams, TaskSendParams, TaskStatusUpdateEvent,
    },
    services::client::{AsyncA2AClient, StreamItem},
};

/// HTTP client for interacting with the A2A protocol
pub struct HttpClient {
    /// Base URL of the A2A API
    base_url: String,
    /// HTTP client
    client: Client,
    /// Authorization token, if any
    auth_token: Option<String>,
    /// Timeout in seconds
    timeout: u64,
}

impl HttpClient {
    /// Create a new HTTP client with the given base URL
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
            auth_token: None,
            timeout: 30, // Default timeout in seconds
        }
    }

    /// Create a new HTTP client with authentication
    pub fn with_auth(base_url: String, auth_token: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
            auth_token: Some(auth_token),
            timeout: 30,
        }
    }

    /// Set the timeout for requests
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    /// Get the headers for a request
    fn get_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(token) = &self.auth_token {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
            );
        }

        headers
    }
}

#[async_trait]
impl AsyncA2AClient for HttpClient {
    #[cfg_attr(feature = "tracing", instrument(skip(self, request), fields(url = %self.base_url, request_len = request.len())))]
    async fn send_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError> {
        let response = self
            .client
            .post(&self.base_url)
            .headers(self.get_headers())
            .body(request.to_string())
            .timeout(Duration::from_secs(self.timeout))
            .send()
            .await
            .map_err(HttpClientError::Reqwest)?;

        if response.status().is_success() {
            let body = response.text().await.map_err(HttpClientError::Reqwest)?;
            Ok(body)
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

    #[cfg_attr(feature = "tracing", instrument(skip(self, request), fields(method = ?request)))]
    async fn send_request<'a>(&self, request: &'a A2ARequest) -> Result<JSONRPCResponse, A2AError> {
        let json = json_rpc::serialize_request(request)?;
        let response_text = self.send_raw_request(&json).await?;
        let response: JSONRPCResponse = serde_json::from_str(&response_text)?;
        Ok(response)
    }

    #[cfg_attr(feature = "tracing", instrument(skip(self, message), fields(task_id, session_id, history_length)))]
    async fn send_task_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        let params = TaskSendParams {
            id: task_id.to_string(),
            session_id: session_id.map(|s| s.to_string()),
            message: message.clone(),
            push_notification: None,
            history_length,
            metadata: None,
        };

        let request = SendTaskRequest::new(params);
        let response = self.send_request(&A2ARequest::SendTask(request)).await?;

        match response.result {
            Some(value) => {
                let task: Task = serde_json::from_value(value)?;
                Ok(task)
            }
            None => {
                if let Some(error) = response.error {
                    Err(A2AError::JsonRpc {
                        code: error.code,
                        message: error.message,
                        data: error.data,
                    })
                } else {
                    Err(A2AError::Internal("Empty response".to_string()))
                }
            }
        }
    }

    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(task_id, history_length)))]
    async fn get_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        let params = TaskQueryParams {
            id: task_id.to_string(),
            history_length,
            metadata: None,
        };

        let request = json_rpc::GetTaskRequest::new(params);
        let response = self.send_request(&A2ARequest::GetTask(request)).await?;

        match response.result {
            Some(value) => {
                let task: Task = serde_json::from_value(value)?;
                Ok(task)
            }
            None => {
                if let Some(error) = response.error {
                    Err(A2AError::JsonRpc {
                        code: error.code,
                        message: error.message,
                        data: error.data,
                    })
                } else {
                    Err(A2AError::Internal("Empty response".to_string()))
                }
            }
        }
    }

    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(task_id)))]
    async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError> {
        let params = TaskIdParams {
            id: task_id.to_string(),
            metadata: None,
        };

        let request = json_rpc::CancelTaskRequest::new(params);
        let response = self.send_request(&A2ARequest::CancelTask(request)).await?;

        match response.result {
            Some(value) => {
                let task: Task = serde_json::from_value(value)?;
                Ok(task)
            }
            None => {
                if let Some(error) = response.error {
                    Err(A2AError::JsonRpc {
                        code: error.code,
                        message: error.message,
                        data: error.data,
                    })
                } else {
                    Err(A2AError::Internal("Empty response".to_string()))
                }
            }
        }
    }

    async fn set_task_push_notification<'a>(
        &self,
        config: &'a TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        let request = json_rpc::SetTaskPushNotificationRequest::new(config.clone());
        let response = self
            .send_request(&A2ARequest::SetTaskPushNotification(request))
            .await?;

        match response.result {
            Some(value) => {
                let config: TaskPushNotificationConfig = serde_json::from_value(value)?;
                Ok(config)
            }
            None => {
                if let Some(error) = response.error {
                    Err(A2AError::JsonRpc {
                        code: error.code,
                        message: error.message,
                        data: error.data,
                    })
                } else {
                    Err(A2AError::Internal("Empty response".to_string()))
                }
            }
        }
    }

    async fn get_task_push_notification<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        let params = TaskIdParams {
            id: task_id.to_string(),
            metadata: None,
        };

        let request = json_rpc::GetTaskPushNotificationRequest::new(params);
        let response = self
            .send_request(&A2ARequest::GetTaskPushNotification(request))
            .await?;

        match response.result {
            Some(value) => {
                let config: TaskPushNotificationConfig = serde_json::from_value(value)?;
                Ok(config)
            }
            None => {
                if let Some(error) = response.error {
                    Err(A2AError::JsonRpc {
                        code: error.code,
                        message: error.message,
                        data: error.data,
                    })
                } else {
                    Err(A2AError::Internal("Empty response".to_string()))
                }
            }
        }
    }

    // HTTP clients can't directly support streaming, so this method returns
    // an error indicating that streaming is not supported via HTTP
    async fn subscribe_to_task<'a>(
        &self,
        _task_id: &'a str,
        _history_length: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamItem, A2AError>> + Send>>, A2AError> {
        Err(A2AError::UnsupportedOperation(
            "Streaming is not supported with HTTP client".to_string(),
        ))
    }
}
