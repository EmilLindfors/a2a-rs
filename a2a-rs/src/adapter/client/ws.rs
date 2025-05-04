//! WebSocket client adapter for the A2A protocol

// This module is already conditionally compiled with #[cfg(feature = "ws-client")] in mod.rs

use async_trait::async_trait;
use futures::{
    SinkExt,
    stream::{Stream, StreamExt},
};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::{
    net::TcpStream,
    sync::Mutex, // Changed to tokio::sync::Mutex
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message as WsMessage,
};
use url::Url;

use crate::{
    adapter::client::WebSocketClientError,
    application::json_rpc::{
        self, A2ARequest, JSONRPCResponse, SendTaskRequest, SendTaskStreamingRequest,
        TaskResubscriptionRequest,
    },
    domain::{
        A2AError, Message, Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig,
        TaskQueryParams, TaskSendParams, TaskStatusUpdateEvent,
    },
    port::client::{AsyncA2AClient, StreamItem},
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
    async fn send_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError> {
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

    async fn send_request<'a>(&self, request: &'a A2ARequest) -> Result<JSONRPCResponse, A2AError> {
        let json = json_rpc::serialize_request(request)?;
        let response_text = self.send_raw_request(&json).await?;
        let response: JSONRPCResponse = serde_json::from_str(&response_text)?;
        Ok(response)
    }

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

    async fn subscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError> {
        // First connect to ensure we have a connection
        let mut client_clone = self.clone();
        client_clone.connect().await?;

        let params = TaskSendParams {
            id: task_id.to_string(),
            session_id: session_id.map(|s| s.to_string()),
            message: message.clone(),
            push_notification: None,
            history_length,
            metadata: None,
        };

        let request = SendTaskStreamingRequest::new(params);
        let json = json_rpc::serialize_request(&A2ARequest::SendTaskStreaming(request))?;

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
                            Err(
                                WebSocketClientError::Message(format!("WebSocket error: {}", e))
                                    .into(),
                            ),
                            conn,
                        ));
                    }
                    None => {
                        return Some((Err(WebSocketClientError::Closed.into()), conn));
                    }
                };

                // Process the message
                let result = match message {
                    WsMessage::Text(text) => {
                        // Add debug logging for received messages
                        println!("DEBUG: Received WebSocket message: {}", text);

                        // Parse the response
                        let response: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(e) => {
                                println!("DEBUG: JSON parse error: {}", e);
                                return Some((Err(A2AError::JsonParse(e)), conn));
                            }
                        };

                        // Check for errors
                        if let Some(error) = response.get("error") {
                            if error.is_object() {
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
                        }

                        // Check if it's a valid JSON-RPC message
                        if response.get("jsonrpc").is_some() && response.get("result").is_some() {
                            let result = response.get("result").cloned().unwrap_or(Value::Null);

                            // Try to parse as an initial Task response first
                            if let Ok(task) = serde_json::from_value::<Task>(result.clone()) {
                                println!("DEBUG: Parsed as Task");
                                return Some((Ok(StreamItem::Task(task)), conn));
                            }

                            // Try to parse as a status update
                            if let Ok(status_update) =
                                serde_json::from_value::<TaskStatusUpdateEvent>(result.clone())
                            {
                                println!("DEBUG: Parsed as StatusUpdate");
                                return Some((Ok(StreamItem::StatusUpdate(status_update)), conn));
                            }

                            // Try to parse as an artifact update
                            if let Ok(artifact_update) =
                                serde_json::from_value::<TaskArtifactUpdateEvent>(result)
                            {
                                println!("DEBUG: Parsed as ArtifactUpdate");
                                return Some((
                                    Ok(StreamItem::ArtifactUpdate(artifact_update)),
                                    conn,
                                ));
                            }
                        }

                        // If we got here, we couldn't parse the response
                        println!("DEBUG: Failed to parse streaming response");
                        Err(WebSocketClientError::Protocol(
                            "Failed to parse streaming response".to_string(),
                        )
                        .into())
                    }
                    _ => Err(WebSocketClientError::Protocol(
                        "Unexpected WebSocket message type".to_string(),
                    )
                    .into()),
                };

                Some((result, conn))
            })
        });

        Ok(stream)
    }

    async fn resubscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError> {
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

        // Create a stream that will process incoming messages (same as in subscribe_to_task)
        let stream = futures::stream::unfold(connection, move |conn| {
            Box::pin(async move {
                let message_result = {
                    let mut guard = conn.lock().await;
                    guard.next().await
                }; // Lock is dropped here

                let message = match message_result {
                    Some(Ok(msg)) => msg,
                    Some(Err(e)) => {
                        return Some((
                            Err(
                                WebSocketClientError::Message(format!("WebSocket error: {}", e))
                                    .into(),
                            ),
                            conn,
                        ));
                    }
                    None => {
                        return Some((Err(WebSocketClientError::Closed.into()), conn));
                    }
                };

                // Process the message
                let result = match message {
                    WsMessage::Text(text) => {
                        // Add debug logging for received messages
                        println!("DEBUG: Received WebSocket message: {}", text);

                        // Parse the response
                        let response: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(e) => {
                                println!("DEBUG: JSON parse error: {}", e);
                                return Some((Err(A2AError::JsonParse(e)), conn));
                            }
                        };

                        // Check for errors
                        if let Some(error) = response.get("error") {
                            if error.is_object() {
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
                        }

                        // Check if it's a valid JSON-RPC message
                        if response.get("jsonrpc").is_some() && response.get("result").is_some() {
                            let result = response.get("result").cloned().unwrap_or(Value::Null);

                            // Try to parse as an initial Task response first
                            if let Ok(task) = serde_json::from_value::<Task>(result.clone()) {
                                println!("DEBUG: Parsed as Task");
                                return Some((Ok(StreamItem::Task(task)), conn));
                            }

                            // Try to parse as a status update
                            if let Ok(status_update) =
                                serde_json::from_value::<TaskStatusUpdateEvent>(result.clone())
                            {
                                println!("DEBUG: Parsed as StatusUpdate");
                                return Some((Ok(StreamItem::StatusUpdate(status_update)), conn));
                            }

                            // Try to parse as an artifact update
                            if let Ok(artifact_update) =
                                serde_json::from_value::<TaskArtifactUpdateEvent>(result)
                            {
                                println!("DEBUG: Parsed as ArtifactUpdate");
                                return Some((
                                    Ok(StreamItem::ArtifactUpdate(artifact_update)),
                                    conn,
                                ));
                            }
                        }

                        // If we got here, we couldn't parse the response
                        println!("DEBUG: Failed to parse streaming response");
                        Err(WebSocketClientError::Protocol(
                            "Failed to parse streaming response".to_string(),
                        )
                        .into())
                    }
                    _ => Err(WebSocketClientError::Protocol(
                        "Unexpected WebSocket message type".to_string(),
                    )
                    .into()),
                };

                Some((result, conn))
            })
        });

        Ok(stream)
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
