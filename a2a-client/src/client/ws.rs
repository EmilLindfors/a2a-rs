use futures::{
    SinkExt,
    channel::mpsc::{Receiver, Sender, channel},
    stream::{Stream, StreamExt},
};
use serde_json::Value;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent, WebSocket};

use a2a_rs::{
    application::json_rpc::{
        self, A2ARequest, JSONRPCResponse, SendTaskRequest, SendTaskStreamingRequest,
        TaskResubscriptionRequest,
    },
    domain::{
        A2AError, Message, Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig,
        TaskQueryParams, TaskSendParams, TaskStatusUpdateEvent,
    },
    port::client::StreamItem,
};

use super::error::WebSocketClientError;

// Log helper for debugging
#[allow(unused)]
fn console_log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// A message broadcaster for multiple subscribers
#[derive(Clone)]
struct MessageBroadcaster {
    senders: Arc<Mutex<Vec<Sender<Result<String, WebSocketClientError>>>>>,
}

impl MessageBroadcaster {
    fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn add_subscriber(&self) -> Receiver<Result<String, WebSocketClientError>> {
        let (sender, receiver) = channel::<Result<String, WebSocketClientError>>(100);

        let mut senders = self.senders.lock().unwrap();
        senders.push(sender);

        receiver
    }

    fn broadcast(&self, message: Result<String, WebSocketClientError>) {
        let senders = self.senders.lock().unwrap();
        for sender in senders.iter() {
            let mut sender_clone = sender.clone();
            let message_clone = message.clone();
            spawn_local(async move {
                // Ignore errors from closed channels
                let _ = sender_clone.send(message_clone).await;
            });
        }
    }
}

/// A wrapper for working with WebSockets in WASM
struct WebSocketHandle {
    socket: WebSocket,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    broadcaster: MessageBroadcaster,
}

/// A WebSocket stream for handling streamed responses
pub struct WebSocketStream {
    receiver: Receiver<Result<String, WebSocketClientError>>,
}

impl Stream for WebSocketStream {
    type Item = Result<String, WebSocketClientError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.receiver).poll_next(cx)
    }
}

impl WebSocketHandle {
    /// Create a new WebSocket connection
    pub async fn connect(url: &str) -> Result<(Self, WebSocketStream), WebSocketClientError> {
        // Create message broadcaster
        let broadcaster = MessageBroadcaster::new();
        let broadcaster_clone = broadcaster.clone();

        // Create the WebSocket
        let socket = WebSocket::new(url).map_err(|e| {
            WebSocketClientError::Connection(format!("Failed to create WebSocket: {:?}", e))
        })?;

        // Set binary type to arraybuffer
        socket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        // Message handler
        let on_message = Closure::<dyn FnMut(_)>::new(move |e: MessageEvent| {
            let broadcaster = broadcaster_clone.clone();

            if let Ok(txt) = e.data().dyn_into::<js_sys::JsString>() {
                let txt_string = String::from(txt);
                broadcaster.broadcast(Ok(txt_string));
            } else {
                console_log("Received non-text message, which is not supported");
            }
        });

        // Close handler
        let broadcaster_close = broadcaster.clone();
        let on_close = Closure::<dyn FnMut(_)>::new(move |e: CloseEvent| {
            let broadcaster = broadcaster_close.clone();
            let reason = e.reason();
            broadcaster.broadcast(Err(WebSocketClientError::Closed));
            console_log(&format!("WebSocket closed: {}", reason));
        });

        // Error handler
        let broadcaster_error = broadcaster.clone();
        let on_error = Closure::<dyn FnMut(_)>::new(move |e: ErrorEvent| {
            let broadcaster = broadcaster_error.clone();
            let msg_text = e.message();
            broadcaster.broadcast(Err(WebSocketClientError::Connection(format!(
                "WebSocket error: {}",
                msg_text
            ))));
            console_log(&format!("WebSocket error: {}", msg_text));
        });

        // Set up event handlers
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        // Create promise for socket connection
        let socket_clone = socket.clone();
        let (open_tx, mut open_rx) = channel::<Result<(), WebSocketClientError>>(1);

        let on_open = Closure::wrap(Box::new(move || {
            let mut sender = open_tx.clone();
            spawn_local(async move {
                let _ = sender.send(Ok(())).await;
            });
            console_log("WebSocket connection opened");
        }) as Box<dyn FnMut()>);

        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget(); // Prevent the closure from being dropped

        // Set up timeout future
        let timeout_future = async {
            wasm_bindgen_futures::JsFuture::from(js_sys::Promise::new(&mut |resolve, _| {
                let f = Closure::once_into_js(move || {
                    resolve.call0(&JsValue::NULL).unwrap();
                });
                let window = web_sys::window().unwrap();
                window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        f.as_ref().unchecked_ref(),
                        10000, // 10 seconds timeout
                    )
                    .unwrap();
            }))
            .await
        };

        let connection_future = async {
            if let Some(result) = open_rx.next().await {
                result
            } else {
                Err(WebSocketClientError::Connection(
                    "Connection failed".to_string(),
                ))
            }
        };

        let race_result =
            futures::future::select(Box::pin(connection_future), Box::pin(timeout_future)).await;

        match race_result {
            futures::future::Either::Left((result, _)) => {
                result?;
            }
            futures::future::Either::Right((_, _)) => {
                // Timeout occurred
                return Err(WebSocketClientError::Timeout);
            }
        }

        // Create a receiver from the broadcaster for the initial connection
        let receiver = broadcaster.add_subscriber();

        // Return socket handle and stream
        Ok((
            Self {
                socket: socket_clone,
                _on_message: on_message,
                _on_close: on_close,
                _on_error: on_error,
                broadcaster,
            },
            WebSocketStream { receiver },
        ))
    }

    /// Create a new WebSocket stream for this connection
    pub fn create_stream(&self) -> WebSocketStream {
        WebSocketStream {
            receiver: self.broadcaster.add_subscriber(),
        }
    }

    /// Send a message over the WebSocket
    pub fn send(&self, message: &str) -> Result<(), WebSocketClientError> {
        self.socket
            .send_with_str(message)
            .map_err(|e| WebSocketClientError::Message(format!("Failed to send message: {:?}", e)))
    }

    /// Close the WebSocket connection
    pub fn close(&self) -> Result<(), WebSocketClientError> {
        self.socket
            .close()
            .map_err(|e| WebSocketClientError::Connection(format!("Failed to close: {:?}", e)))
    }
}

// Type for sharing the WebSocket handle
type SharedSocket = Arc<Mutex<Option<WebSocketHandle>>>;

/// WebSocket client for interacting with the A2A protocol in a browser environment
pub struct WasmWebSocketClient {
    /// Base WebSocket URL of the A2A API
    base_url: String,
    /// Authorization token, if any
    auth_token: Option<String>,
    /// Connection to the WebSocket server
    socket: SharedSocket,
    /// Timeout in seconds
    timeout: u64,
}

impl WasmWebSocketClient {
    /// Create a new WebSocket client with the given base URL
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            auth_token: None,
            socket: Arc::new(Mutex::new(None)),
            timeout: 30, // Default timeout in seconds
        }
    }

    /// Create a new WebSocket client with authentication
    pub fn with_auth(base_url: String, auth_token: String) -> Self {
        Self {
            base_url,
            auth_token: Some(auth_token),
            socket: Arc::new(Mutex::new(None)),
            timeout: 30,
        }
    }

    /// Set the timeout for operations
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    /// Connect to the WebSocket server
    async fn connect(&self) -> Result<(), A2AError> {
        // Check if already connected
        {
            let socket_lock = self.socket.lock().unwrap();
            if socket_lock.is_some() {
                return Ok(());
            }
        }

        // Prepare URL
        let mut url = self.base_url.clone();

        // Add auth token if present
        if let Some(token) = &self.auth_token {
            let separator = if url.contains('?') { "&" } else { "?" };
            url.push_str(&format!("{}token={}", separator, token));
        }

        // Create connection
        let (handle, _) = WebSocketHandle::connect(&url).await?;

        // Store connection
        {
            let mut socket_lock = self.socket.lock().unwrap();
            *socket_lock = Some(handle);
        }

        Ok(())
    }

    /// Send a message to the WebSocket server and get a response
    async fn send_and_receive(&self, message: &str) -> Result<String, A2AError> {
        // Connect if needed
        self.connect().await?;

        // Get the socket handle and create a stream
        let stream = {
            let socket_lock = self.socket.lock().unwrap();
            match &*socket_lock {
                Some(handle) => {
                    // Create a new stream to receive the response
                    let stream = handle.create_stream();

                    // Send the message
                    handle
                        .send(message)
                        .map_err(|e| A2AError::Internal(e.to_string()))?;

                    stream
                }
                None => return Err(A2AError::Internal("No WebSocket connection".to_string())),
            }
        };

        // Wait for response with timeout
        let timeout_duration = Duration::from_secs(self.timeout);
        let response = match wait_for_response(stream, timeout_duration).await {
            Ok(response) => response,
            Err(e) => {
                return Err(e);
            }
        };

        Ok(response)
    }
}

// Helper function to wait for a response with timeout
async fn wait_for_response(
    mut stream: WebSocketStream,
    _timeout: Duration,
) -> Result<String, A2AError> {
    let item = stream.next().await;

    match item {
        Some(Ok(response)) => Ok(response),
        Some(Err(e)) => Err(A2AError::Internal(e.to_string())),
        None => Err(A2AError::Internal("No response received".to_string())),
    }
}

// Custom implementation of AsyncA2AClient for the WebSocket client
// We can't implement AsyncA2AClient directly because it requires Send + Sync
// and our web-specific types (closures, etc.) don't implement these traits
pub struct A2AClientImpl {
    inner: WasmWebSocketClient,
}

impl A2AClientImpl {
    pub fn new(base_url: String) -> Self {
        Self {
            inner: WasmWebSocketClient::new(base_url),
        }
    }

    pub fn with_auth(base_url: String, auth_token: String) -> Self {
        Self {
            inner: WasmWebSocketClient::with_auth(base_url, auth_token),
        }
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.inner = self.inner.with_timeout(timeout);
        self
    }

    pub async fn send_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError> {
        self.inner.send_and_receive(request).await
    }

    pub async fn send_request<'a>(
        &self,
        request: &'a A2ARequest,
    ) -> Result<JSONRPCResponse, A2AError> {
        let json = json_rpc::serialize_request(request)?;
        let response_text = self.send_raw_request(&json).await?;
        let response: JSONRPCResponse = serde_json::from_str(&response_text)?;
        Ok(response)
    }

    pub async fn send_task_message<'a>(
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

    pub async fn get_task<'a>(
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

    pub async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError> {
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

    pub async fn set_task_push_notification<'a>(
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

    pub async fn get_task_push_notification<'a>(
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

    pub async fn subscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError> {
        self.inner.connect().await?;

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

        // Get the socket handle and create a stream
        let stream = {
            let socket_lock = self.inner.socket.lock().unwrap();
            match &*socket_lock {
                Some(handle) => {
                    // Create a new stream to receive responses
                    let stream = handle.create_stream();

                    // Send the request
                    handle
                        .send(&json)
                        .map_err(|e| A2AError::Internal(e.to_string()))?;

                    stream
                }
                None => return Err(A2AError::Internal("No WebSocket connection".to_string())),
            }
        };

        // Transform text stream to StreamItem stream
        let stream =
            stream.map(|result| {
                match result {
                    Ok(text) => {
                        // Parse the response
                        let response: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(e) => return Err(A2AError::JsonParse(e)),
                        };
                        let response_clone = response.clone();

                        // Check for errors
                        if let Some(error) = response.get("error") {
                            if error.is_object() {
                                let error: JSONRPCResponse = match serde_json::from_value(response)
                                {
                                    Ok(resp) => resp,
                                    Err(e) => return Err(A2AError::JsonParse(e)),
                                };

                                if let Some(err) = error.error {
                                    return Err(A2AError::JsonRpc {
                                        code: err.code,
                                        message: err.message,
                                        data: err.data,
                                    });
                                }
                            }
                        }

                        // Try to parse as a status update
                        if let Ok(status_update) = serde_json::from_value::<TaskStatusUpdateEvent>(
                            response_clone.get("result").cloned().unwrap_or(Value::Null),
                        ) {
                            Ok(StreamItem::StatusUpdate(status_update))
                        } else {
                            // Try to parse as an artifact update
                            if let Ok(artifact_update) =
                                serde_json::from_value::<TaskArtifactUpdateEvent>(
                                    response_clone.get("result").cloned().unwrap_or(Value::Null),
                                )
                            {
                                Ok(StreamItem::ArtifactUpdate(artifact_update))
                            } else {
                                Err(WebSocketClientError::Protocol(
                                    "Failed to parse streaming response".to_string(),
                                )
                                .into())
                            }
                        }
                    }
                    Err(e) => Err(A2AError::Internal(e.to_string())),
                }
            });

        Ok(stream)
    }

    pub async fn resubscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError> {
        self.inner.connect().await?;

        let params = TaskQueryParams {
            id: task_id.to_string(),
            history_length,
            metadata: None,
        };

        let request = TaskResubscriptionRequest::new(params);
        let json = json_rpc::serialize_request(&A2ARequest::TaskResubscription(request))?;

        // Get the socket handle and create a stream
        let stream = {
            let socket_lock = self.inner.socket.lock().unwrap();
            match &*socket_lock {
                Some(handle) => {
                    // Create a new stream to receive responses
                    let stream = handle.create_stream();

                    // Send the request
                    handle
                        .send(&json)
                        .map_err(|e| A2AError::Internal(e.to_string()))?;

                    stream
                }
                None => return Err(A2AError::Internal("No WebSocket connection".to_string())),
            }
        };

        // Transform text stream to StreamItem stream
        let stream =
            stream.map(|result| {
                match result {
                    Ok(text) => {
                        // Parse the response
                        let response: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(e) => return Err(A2AError::JsonParse(e)),
                        };
                        let response_clone = response.clone();

                        // Check for errors
                        if let Some(error) = response.get("error") {
                            if error.is_object() {
                                let error: JSONRPCResponse = match serde_json::from_value(response)
                                {
                                    Ok(resp) => resp,
                                    Err(e) => return Err(A2AError::JsonParse(e)),
                                };

                                if let Some(err) = error.error {
                                    return Err(A2AError::JsonRpc {
                                        code: err.code,
                                        message: err.message,
                                        data: err.data,
                                    });
                                }
                            }
                        }

                        // Try to parse as a status update
                        if let Ok(status_update) = serde_json::from_value::<TaskStatusUpdateEvent>(
                            response_clone.get("result").cloned().unwrap_or(Value::Null),
                        ) {
                            Ok(StreamItem::StatusUpdate(status_update))
                        } else {
                            // Try to parse as an artifact update
                            if let Ok(artifact_update) =
                                serde_json::from_value::<TaskArtifactUpdateEvent>(
                                    response_clone.get("result").cloned().unwrap_or(Value::Null),
                                )
                            {
                                Ok(StreamItem::ArtifactUpdate(artifact_update))
                            } else {
                                Err(WebSocketClientError::Protocol(
                                    "Failed to parse streaming response".to_string(),
                                )
                                .into())
                            }
                        }
                    }
                    Err(e) => Err(A2AError::Internal(e.to_string())),
                }
            });

        Ok(stream)
    }
}

impl Clone for A2AClientImpl {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

// Implementation for our internal client
impl WasmWebSocketClient {
    async fn send_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError> {
        self.send_and_receive(request).await
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
        self.connect().await?;

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

        // Get the socket handle and create a stream
        let stream = {
            let socket_lock = self.socket.lock().unwrap();
            match &*socket_lock {
                Some(handle) => {
                    // Send the request
                    handle
                        .send(&json)
                        .map_err(|e| A2AError::Internal(e.to_string()))?;

                    // Create a stream from the channel
                    let (_, receiver) = channel::<Result<String, WebSocketClientError>>(100);
                    WebSocketStream { receiver }
                }
                None => return Err(A2AError::Internal("No WebSocket connection".to_string())),
            }
        };

        // Transform text stream to StreamItem stream
        let stream =
            stream.map(|result| {
                match result {
                    Ok(text) => {
                        // Parse the response
                        let response: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(e) => return Err(A2AError::JsonParse(e)),
                        };
                        let response_clone = response.clone();

                        // Check for errors
                        if let Some(error) = response.get("error") {
                            if error.is_object() {
                                let error: JSONRPCResponse = match serde_json::from_value(response)
                                {
                                    Ok(resp) => resp,
                                    Err(e) => return Err(A2AError::JsonParse(e)),
                                };

                                if let Some(err) = error.error {
                                    return Err(A2AError::JsonRpc {
                                        code: err.code,
                                        message: err.message,
                                        data: err.data,
                                    });
                                }
                            }
                        }

                        // Try to parse as a status update
                        if let Ok(status_update) = serde_json::from_value::<TaskStatusUpdateEvent>(
                            response_clone.get("result").cloned().unwrap_or(Value::Null),
                        ) {
                            Ok(StreamItem::StatusUpdate(status_update))
                        } else {
                            // Try to parse as an artifact update
                            if let Ok(artifact_update) =
                                serde_json::from_value::<TaskArtifactUpdateEvent>(
                                    response_clone.get("result").cloned().unwrap_or(Value::Null),
                                )
                            {
                                Ok(StreamItem::ArtifactUpdate(artifact_update))
                            } else {
                                Err(WebSocketClientError::Protocol(
                                    "Failed to parse streaming response".to_string(),
                                )
                                .into())
                            }
                        }
                    }
                    Err(e) => Err(A2AError::Internal(e.to_string())),
                }
            });

        Ok(stream)
    }

    async fn resubscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError> {
        // First connect to ensure we have a connection
        self.connect().await?;

        let params = TaskQueryParams {
            id: task_id.to_string(),
            history_length,
            metadata: None,
        };

        let request = TaskResubscriptionRequest::new(params);
        let json = json_rpc::serialize_request(&A2ARequest::TaskResubscription(request))?;

        // Get the socket handle and create a stream
        let stream = {
            let socket_lock = self.socket.lock().unwrap();
            match &*socket_lock {
                Some(handle) => {
                    // Send the request
                    handle
                        .send(&json)
                        .map_err(|e| A2AError::Internal(e.to_string()))?;

                    // Create a stream from the channel
                    let (_, receiver) = channel::<Result<String, WebSocketClientError>>(100);
                    WebSocketStream { receiver }
                }
                None => return Err(A2AError::Internal("No WebSocket connection".to_string())),
            }
        };

        // Transform text stream to StreamItem stream (same as in subscribe_to_task)
        let stream =
            stream.map(|result| {
                match result {
                    Ok(text) => {
                        // Parse the response
                        let response: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(e) => return Err(A2AError::JsonParse(e)),
                        };
                        let response_clone = response.clone();

                        // Check for errors
                        if let Some(error) = response.get("error") {
                            if error.is_object() {
                                let error: JSONRPCResponse = match serde_json::from_value(response)
                                {
                                    Ok(resp) => resp,
                                    Err(e) => return Err(A2AError::JsonParse(e)),
                                };

                                if let Some(err) = error.error {
                                    return Err(A2AError::JsonRpc {
                                        code: err.code,
                                        message: err.message,
                                        data: err.data,
                                    });
                                }
                            }
                        }

                        // Try to parse as a status update
                        if let Ok(status_update) = serde_json::from_value::<TaskStatusUpdateEvent>(
                            response_clone.get("result").cloned().unwrap_or(Value::Null),
                        ) {
                            Ok(StreamItem::StatusUpdate(status_update))
                        } else {
                            // Try to parse as an artifact update
                            if let Ok(artifact_update) =
                                serde_json::from_value::<TaskArtifactUpdateEvent>(
                                    response_clone.get("result").cloned().unwrap_or(Value::Null),
                                )
                            {
                                Ok(StreamItem::ArtifactUpdate(artifact_update))
                            } else {
                                Err(WebSocketClientError::Protocol(
                                    "Failed to parse streaming response".to_string(),
                                )
                                .into())
                            }
                        }
                    }
                    Err(e) => Err(A2AError::Internal(e.to_string())),
                }
            });

        Ok(stream)
    }
}

impl Clone for WasmWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            base_url: self.base_url.clone(),
            auth_token: self.auth_token.clone(),
            socket: self.socket.clone(),
            timeout: self.timeout,
        }
    }
}
