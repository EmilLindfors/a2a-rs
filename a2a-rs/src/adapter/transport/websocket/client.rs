//! WebSocket client adapter for the A2A protocol

// This module is already conditionally compiled with #[cfg(feature = "ws-client")] in mod.rs

use async_trait::async_trait;
use futures::{
    SinkExt,
    stream::{Stream, StreamExt},
};
use serde_json::Value;
use std::{pin::Pin, sync::Arc, time::Duration};
use tokio::{
    net::TcpStream,
    sync::Mutex, // Changed to tokio::sync::Mutex
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message as WsMessage,
};
use url::Url;

#[cfg(feature = "tracing")]
use tracing::{debug, trace};

use crate::{
    adapter::error::WebSocketClientError,
    application::{
        JSONRPCResponse,
        json_rpc::{self, A2ARequest, SendTaskRequest, TaskResubscriptionRequest},
    },
    domain::{
        A2AError, Message, Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig,
        TaskQueryParams, TaskSendParams, TaskStatusUpdateEvent,
    },
    services::client::{AsyncA2AClient, StreamItem},
};

type WebSocketTx = Arc<Mutex<WebSocketStream<MaybeTlsStream<TcpStream>>>>;

/// WebSocket client for interacting with the A2A protocol with streaming support
pub struct WebSocketClient {
    /// Base WebSocket URL of the A2A API
    base_url: String,
    /// Authorization token, if any
    auth_token: Option<String>,
    /// Connection to the WebSocket server
    connection: Option<WebSocketTx>,
    /// Timeout in seconds
    timeout: u64,
}

impl WebSocketClient {
    /// Create a new WebSocket client with the given base URL
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            auth_token: None,
            connection: None,
            timeout: 30, // Default timeout in seconds
        }
    }

    /// Create a new WebSocket client with authentication
    pub fn with_auth(base_url: String, auth_token: String) -> Self {
        Self {
            base_url,
            auth_token: Some(auth_token),
            connection: None,
            timeout: 30,
        }
    }

    /// Set the timeout for operations
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    /// Connect to the WebSocket server
    async fn connect(&mut self) -> Result<(), A2AError> {
        if self.connection.is_some() {
            return Ok(());
        }

        let mut url = Url::parse(&self.base_url)
            .map_err(|e| WebSocketClientError::Connection(format!("Invalid URL: {}", e)))?;

        // Add auth token to URL if present
        if let Some(token) = &self.auth_token {
            url.query_pairs_mut().append_pair("token", token);
        }

        let (ws_stream, _) = connect_async(url).await.map_err(|e| {
            WebSocketClientError::Connection(format!("WebSocket connection error: {}", e))
        })?;

        self.connection = Some(Arc::new(Mutex::new(ws_stream)));
        Ok(())
    }

    /// Send a message to the WebSocket server and get a response
    async fn send_ws_message(&mut self, message: WsMessage) -> Result<WsMessage, A2AError> {
        self.connect().await?;

        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| WebSocketClientError::Connection("No connection".to_string()))?;

        // Send the message
        {
            let mut guard = conn.lock().await; // Changed to await
            guard
                .send(message)
                .await
                .map_err(|e| WebSocketClientError::Message(format!("Send error: {}", e)))?;
        }

        // Receive the response
        let response = {
            let mut guard = conn.lock().await; // Changed to await

            let timeout = Duration::from_secs(self.timeout);
            let result = tokio::time::timeout(timeout, guard.next())
                .await
                .map_err(|_| WebSocketClientError::Timeout)?;

            match result {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => {
                    return Err(
                        WebSocketClientError::Message(format!("WebSocket error: {}", e)).into(),
                    );
                }
                None => return Err(WebSocketClientError::Closed.into()),
            }
        };

        Ok(response)
    }
}

#[async_trait]
impl AsyncA2AClient for WebSocketClient {
    async fn send_raw_request(&self, request: &str) -> Result<String, A2AError> {
        let mut client = self.clone();
        let response = client
            .send_ws_message(WsMessage::Text(request.to_string()))
            .await?;

        match response {
            WsMessage::Text(text) => Ok(text),
            _ => Err(A2AError::Internal(
                "Unexpected WebSocket message type".to_string(),
            )),
        }
    }

    async fn send_request(&self, request: &A2ARequest) -> Result<JSONRPCResponse, A2AError> {
        let json = json_rpc::serialize_request(request)?;
        let response_text = self.send_raw_request(&json).await?;
        let response: JSONRPCResponse = serde_json::from_str(&response_text)?;
        Ok(response)
    }

    async fn send_task_message(
        &self,
        task_id: &str,
        message: &Message,
        session_id: Option<&str>,
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

    async fn get_task(
        &self,
        task_id: &str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        let params = TaskQueryParams {
            id: task_id.to_string(),
            history_length,
            metadata: None,
        };

        let request = json_rpc::GetTaskRequest::new(params);
        let response = self.send_request(&A2ARequest::GetTask(request)).await?;

        let Some(value) = response.result else {
            if let Some(error) = response.error {
                return Err(A2AError::JsonRpc {
                    code: error.code,
                    message: error.message,
                    data: error.data,
                });
            }
            return Err(A2AError::Internal("Empty response".to_string()));
        };

        let task: Task = serde_json::from_value(value)?;
        Ok(task)
    }

    async fn cancel_task(&self, task_id: &str) -> Result<Task, A2AError> {
        let params = TaskIdParams {
            id: task_id.to_string(),
            metadata: None,
        };

        let request = json_rpc::CancelTaskRequest::new(params);
        let response = self.send_request(&A2ARequest::CancelTask(request)).await?;

        let Some(value) = response.result else {
            if let Some(error) = response.error {
                return Err(A2AError::JsonRpc {
                    code: error.code,
                    message: error.message,
                    data: error.data,
                });
            }
            return Err(A2AError::Internal("Empty response".to_string()));
        };

        let task: Task = serde_json::from_value(value)?;
        Ok(task)
    }

    async fn set_task_push_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        let request = json_rpc::SetTaskPushNotificationRequest::new(config.clone());
        let response = self
            .send_request(&A2ARequest::SetTaskPushNotification(request))
            .await?;

        let Some(value) = response.result else {
            if let Some(error) = response.error {
                return Err(A2AError::JsonRpc {
                    code: error.code,
                    message: error.message,
                    data: error.data,
                });
            }
            return Err(A2AError::Internal("Empty response".to_string()));
        };

        let config: TaskPushNotificationConfig = serde_json::from_value(value)?;
        Ok(config)
    }

    async fn get_task_push_notification(
        &self,
        task_id: &str,
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

    async fn list_tasks(
        &self,
        params: &crate::domain::ListTasksParams,
    ) -> Result<crate::domain::ListTasksResult, A2AError> {
        let request = json_rpc::ListTasksRequest::new(Some(params.clone()));
        let response = self.send_request(&A2ARequest::ListTasks(request)).await?;

        match response.result {
            Some(value) => {
                let result: crate::domain::ListTasksResult = serde_json::from_value(value)?;
                Ok(result)
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

    async fn list_push_notification_configs(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::domain::TaskPushNotificationConfig>, A2AError> {
        use crate::domain::ListTaskPushNotificationConfigParams;

        let request = json_rpc::ListTaskPushNotificationConfigRequest::new(
            ListTaskPushNotificationConfigParams {
                id: task_id.to_string(),
                metadata: None,
            },
        );
        let response = self
            .send_request(&A2ARequest::ListTaskPushNotificationConfigs(request))
            .await?;

        match response.result {
            Some(value) => {
                let configs: Vec<crate::domain::TaskPushNotificationConfig> =
                    serde_json::from_value(value)?;
                Ok(configs)
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

    async fn get_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<crate::domain::TaskPushNotificationConfig, A2AError> {
        use crate::domain::GetTaskPushNotificationConfigParams;

        let request = json_rpc::GetTaskPushNotificationConfigRequest::new(
            GetTaskPushNotificationConfigParams {
                id: task_id.to_string(),
                push_notification_config_id: Some(config_id.to_string()),
                metadata: None,
            },
        );
        let response = self
            .send_request(&A2ARequest::GetTaskPushNotificationConfig(request))
            .await?;

        match response.result {
            Some(value) => {
                let config: crate::domain::TaskPushNotificationConfig =
                    serde_json::from_value(value)?;
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

    async fn delete_push_notification_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> Result<(), A2AError> {
        use crate::domain::DeleteTaskPushNotificationConfigParams;

        let request = json_rpc::DeleteTaskPushNotificationConfigRequest::new(
            DeleteTaskPushNotificationConfigParams {
                id: task_id.to_string(),
                push_notification_config_id: config_id.to_string(),
                metadata: None,
            },
        );
        let response = self
            .send_request(&A2ARequest::DeleteTaskPushNotificationConfig(request))
            .await?;

        // For delete operations, both Some(Null) and None are success if there's no error
        if let Some(error) = response.error {
            Err(A2AError::JsonRpc {
                code: error.code,
                message: error.message,
                data: error.data,
            })
        } else {
            Ok(())
        }
    }

    async fn subscribe_to_task(
        &self,
        task_id: &str,
        history_length: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamItem, A2AError>> + Send>>, A2AError> {
        // First connect to ensure we have a connection
        let mut client_clone = self.clone();
        client_clone.connect().await?;

        let params = TaskQueryParams {
            id: task_id.to_string(),
            history_length,
            metadata: None,
        };

        let request = TaskResubscriptionRequest::new(params);
        let json = json_rpc::serialize_request(&A2ARequest::TaskResubscription(request))?;

        // Get the connection
        let connection = client_clone
            .connection
            .as_ref()
            .ok_or_else(|| WebSocketClientError::Connection("No connection".to_string()))?
            .clone();

        // Send the request
        {
            let mut guard = connection.lock().await; // Changed to await

            guard
                .send(WsMessage::Text(json))
                .await
                .map_err(|e| WebSocketClientError::Message(format!("Send error: {}", e)))?;
        }

        // Create a stream that will process incoming messages
        let stream = futures::stream::unfold(connection, move |conn| {
            Box::pin(async move {
                // Loop until we get a non-null message or an error
                loop {
                    // Get the next message from the WebSocket
                    let message_result = {
                        let mut guard = conn.lock().await;
                        guard.next().await
                    }; // Lock is dropped here
                    // Process result outside the lock scope
                    let message = match message_result {
                        Some(Ok(msg)) => msg,
                        Some(Err(e)) => {
                            return Some((
                                Err(WebSocketClientError::Message(format!(
                                    "WebSocket error: {}",
                                    e
                                ))
                                .into()),
                                conn,
                            ));
                        }
                        None => {
                            return Some((Err(WebSocketClientError::Closed.into()), conn));
                        }
                    };

                    // Process the message
                    match message {
                        WsMessage::Text(text) => {
                            // Add debug logging for received messages
                            #[cfg(feature = "tracing")]
                            trace!("Received WebSocket message: {}", text);

                            // Parse the response
                            let response: Value = match serde_json::from_str(&text) {
                                Ok(value) => value,
                                Err(e) => {
                                    #[cfg(feature = "tracing")]
                                    debug!("JSON parse error: {}", e);
                                    return Some((Err(A2AError::JsonParse(e)), conn));
                                }
                            };

                            // Check for errors
                            if let Some(error) = response.get("error")
                                && error.is_object()
                            {
                                let response_clone = response.clone();
                                let error: JSONRPCResponse =
                                    match serde_json::from_value(response_clone) {
                                        Ok(resp) => resp,
                                        Err(e) => {
                                            return Some((Err(A2AError::JsonParse(e)), conn));
                                        }
                                    };

                                if let Some(err) = error.error {
                                    return Some((
                                        Err(A2AError::JsonRpc {
                                            code: err.code,
                                            message: err.message,
                                            data: err.data,
                                        }),
                                        conn,
                                    ));
                                }
                            }

                            // Check if it's a valid JSON-RPC message
                            if response.get("jsonrpc").is_some() && response.get("result").is_some()
                            {
                                let result = response.get("result").cloned().unwrap_or(Value::Null);

                                // If result is null, the task doesn't exist yet - keep streaming
                                if result.is_null() {
                                    #[cfg(feature = "tracing")]
                                    debug!("Task doesn't exist yet, waiting for next message");
                                    // Skip this message and wait for the next WebSocket message
                                    continue; // Continue the loop to get the next message
                                }

                                // Try to parse as an initial Task response first
                                if let Ok(task) = serde_json::from_value::<Task>(result.clone()) {
                                    #[cfg(feature = "tracing")]
                                    debug!("Parsed streaming response as Task");
                                    return Some((Ok(StreamItem::Task(task)), conn));
                                }

                                // Try to parse as a status update
                                if let Ok(status_update) =
                                    serde_json::from_value::<TaskStatusUpdateEvent>(result.clone())
                                {
                                    #[cfg(feature = "tracing")]
                                    debug!("Parsed streaming response as StatusUpdate");
                                    return Some((
                                        Ok(StreamItem::StatusUpdate(status_update)),
                                        conn,
                                    ));
                                }

                                // Try to parse as an artifact update
                                if let Ok(artifact_update) =
                                    serde_json::from_value::<TaskArtifactUpdateEvent>(result)
                                {
                                    #[cfg(feature = "tracing")]
                                    debug!("Parsed streaming response as ArtifactUpdate");
                                    return Some((
                                        Ok(StreamItem::ArtifactUpdate(artifact_update)),
                                        conn,
                                    ));
                                }
                            }

                            // If we got here, we couldn't parse the response
                            #[cfg(feature = "tracing")]
                            debug!("Failed to parse streaming response");
                            return Some((
                                Err(WebSocketClientError::Protocol(
                                    "Failed to parse streaming response".to_string(),
                                )
                                .into()),
                                conn,
                            ));
                        }
                        _ => {
                            return Some((
                                Err(WebSocketClientError::Protocol(
                                    "Unexpected WebSocket message type".to_string(),
                                )
                                .into()),
                                conn,
                            ));
                        }
                    }; // End of match
                } // End of loop
            })
        });

        Ok(Box::pin(stream))
    }
}

impl Clone for WebSocketClient {
    fn clone(&self) -> Self {
        Self {
            base_url: self.base_url.clone(),
            auth_token: self.auth_token.clone(),
            connection: self.connection.clone(),
            timeout: self.timeout,
        }
    }
}
