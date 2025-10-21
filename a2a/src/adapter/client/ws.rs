//! WebSocket client adapter for the A2A protocol

#![cfg(feature = "ws-client")]

use async_trait::async_trait;
use futures::{
    stream::{Stream, StreamExt},
    SinkExt,
};
use serde_json::{json, Value};
use std::{
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, Mutex, RwLock},
    time::timeout, // Changed to tokio::sync::Mutex
};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message as WsMessage, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};
use url::Url;

use crate::{
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

type WsStreamType = WebSocketStream<MaybeTlsStream<TcpStream>>;
/// A robust stream implementation that handles reconnections and error recovery
struct RobustWebSocketStream {
    /// Connection to the WebSocket server
    client: WebSocketClient,
    /// Task ID being streamed
    task_id: String,
    /// Current connection state
    connection: Arc<Mutex<Option<WsStreamType>>>,
    /// Receiver for stream items
    receiver: mpsc::Receiver<Result<StreamItem, A2AError>>,
    /// Sender for stream items
    sender: mpsc::Sender<Result<StreamItem, A2AError>>,
    /// Flag to indicate if the stream is finished
    finished: Arc<Mutex<bool>>,
    /// Configuration settings for reconnection
    config: WebSocketClientConfig,
}

#[derive(Clone)]
/// Represents the state of a WebSocket connection
enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Connected and ready
    Connected(Arc<Mutex<WsStreamType>>),
    /// Currently attempting to connect
    Connecting,
}

impl ConnectionInfo {
    /// Create a new disconnected connection info
    fn new() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            last_used: Instant::now(),
            last_ping: None,
            is_healthy: false,
            reconnect_attempts: 0,
        }
    }

    /// Update last_used timestamp
    fn mark_used(&mut self) {
        self.last_used = Instant::now();
    }

    /// Check if the connection is likely stale and needs a health check
    fn needs_health_check(&self, idle_timeout: Duration) -> bool {
        match &self.state {
            ConnectionState::Connected(_) => self.last_used.elapsed() > idle_timeout,
            _ => false,
        }
    }

    /// Mark the connection as healthy
    fn mark_healthy(&mut self) {
        self.is_healthy = true;
        self.reconnect_attempts = 0;
    }

    /// Mark the connection as unhealthy
    fn mark_unhealthy(&mut self) {
        self.is_healthy = false;
    }

    /// Reset connection status to disconnected
    fn reset(&mut self) {
        self.state = ConnectionState::Disconnected;
        self.is_healthy = false;
        self.last_ping = None;
    }
}

/// Configuration for WebSocket client behavior
#[derive(Clone)]
pub struct WebSocketClientConfig {
    /// How long to wait for connections to establish
    pub connect_timeout: Duration,
    /// How long to wait for a response from the server
    pub response_timeout: Duration,
    /// How long to wait between automatic pings
    pub ping_interval: Duration,
    /// How long a connection can be idle before checking health
    pub idle_timeout: Duration,
    /// Maximum number of reconnection attempts
    pub max_reconnect_attempts: u32,
    /// Base time to wait between reconnection attempts (with exponential backoff)
    pub reconnect_base_delay: Duration,
}

impl Default for WebSocketClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            response_timeout: Duration::from_secs(30),
            ping_interval: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(60),
            max_reconnect_attempts: 5,
            reconnect_base_delay: Duration::from_millis(500),
        }
    }
}

/// Connection information with health checking
struct ConnectionInfo {
    /// The current state of the connection
    state: ConnectionState,
    /// When the connection was last used
    last_used: Instant,
    /// When the last ping was sent
    last_ping: Option<Instant>,
    /// If the connection is considered healthy
    is_healthy: bool,
    /// Number of failed reconnection attempts
    reconnect_attempts: u32,
}

/// WebSocket client for interacting with the A2A protocol with streaming support
pub struct WebSocketClient {
    /// Base WebSocket URL of the A2A API
    base_url: String,
    /// Authorization token, if any
    auth_token: Option<String>,
    /// Connection state information
    connection: Arc<RwLock<ConnectionInfo>>,
    /// Client configuration
    config: WebSocketClientConfig,
    /// Background connection health checker task handle
    _health_checker: Option<tokio::task::JoinHandle<()>>,
}

impl WebSocketClient {
    /// Create a new WebSocket client with the given base URL and default configuration
    pub fn new(base_url: String) -> Self {
        Self::with_config(base_url, WebSocketClientConfig::default())
    }

    /// Create a new WebSocket client with the given base URL and configuration
    pub fn with_config(base_url: String, config: WebSocketClientConfig) -> Self {
        let connection = Arc::new(RwLock::new(ConnectionInfo::new()));

        // Start a background task to periodically check connection health
        let conn_clone = connection.clone();
        let base_url_clone = base_url.clone();
        let config_clone = config.clone();

        let health_checker = tokio::spawn(async move {
            let mut interval = tokio::time::interval(config_clone.ping_interval);
            loop {
                interval.tick().await;

                // Check if the connection needs a health check
                let needs_check = {
                    let conn_info = conn_clone.read().await;
                    conn_info.needs_health_check(config_clone.idle_timeout)
                };

                if needs_check {
                    debug!("Performing health check on WebSocket connection");
                    let mut client = WebSocketClient {
                        base_url: base_url_clone.clone(),
                        auth_token: None,
                        connection: conn_clone.clone(),
                        config: config_clone.clone(),
                        _health_checker: None,
                    };

                    // Send a ping to check health
                    let _ = client.send_ping().await;
                }
            }
        });

        Self {
            base_url,
            auth_token: None,
            connection,
            config,
            _health_checker: Some(health_checker),
        }
    }

    /// Create a new WebSocket client with authentication
    pub fn with_auth(base_url: String, auth_token: String) -> Self {
        let mut client = Self::new(base_url);
        client.auth_token = Some(auth_token);
        client
    }

    /// Create a new WebSocket client with authentication and custom configuration
    pub fn with_auth_and_config(
        base_url: String,
        auth_token: String,
        config: WebSocketClientConfig,
    ) -> Self {
        let mut client = Self::with_config(base_url, config);
        client.auth_token = Some(auth_token);
        client
    }

    /// Send a ping to check connection health
    async fn send_ping(&self) -> Result<(), A2AError> {
        // Get a lock on the connection info
        let mut conn_info = self.connection.write().await;
        let state = conn_info.state.clone();
        match state {
            ConnectionState::Connected(ws_stream) => {
                let mut stream = ws_stream.lock().await;

                // Send a ping with the current timestamp as data
                let now = Instant::now();
                let ping_data = format!("{}", now.elapsed().as_millis());

                if let Err(e) = stream.send(WsMessage::Ping(ping_data.into())).await {
                    warn!("Failed to send ping: {}", e);
                    conn_info.mark_unhealthy();
                    conn_info.reset();
                    Err(A2AError::WebSocket(format!("Failed to send ping: {}", e)))
                } else {
                    conn_info.last_ping = Some(now);
                    conn_info.mark_used();
                    Ok(())
                }
            }
            _ => {
                // Not connected, try to connect
                drop(conn_info); // Release the write lock
                self.connect().await?;
                Ok(())
            }
        }
    }

    /// Connect to the WebSocket server
    async fn connect(&self) -> Result<(), A2AError> {
        // Loop until we either connect successfully or hit an error
        loop {
            // First check if we're already connected with a read lock
            {
                let conn_info = self.connection.read().await;
                if let ConnectionState::Connected(_) = &conn_info.state {
                    // Already connected
                    return Ok(());
                }
            }

            // We need a write lock to update the connection state
            let mut conn_info = self.connection.write().await;

            // Check again to handle race conditions
            if let ConnectionState::Connected(_) = &conn_info.state {
                // Another thread connected while we were waiting for the lock
                return Ok(());
            }

            // Check if we're already trying to connect
            if let ConnectionState::Connecting = &conn_info.state {
                // Another thread is already connecting
                drop(conn_info); // Release the lock

                // Wait a bit and check if connection succeeded
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue; // Loop again instead of recursing
            }

            // If we reach here, we need to establish a new connection
            conn_info.state = ConnectionState::Connecting;
            drop(conn_info); // Release the write lock for the duration of the connection attempt

            let mut retries = 0;
            let max_retries = self.config.max_reconnect_attempts;

            // Connection attempt loop
            while retries <= max_retries {
                let mut url = match Url::parse(&self.base_url) {
                    Ok(url) => url,
                    Err(e) => {
                        let mut conn_info = self.connection.write().await;
                        conn_info.reset();
                        return Err(A2AError::Internal(format!("Invalid URL: {}", e)));
                    }
                };

                // Add auth token to URL if present
                if let Some(token) = &self.auth_token {
                    url.query_pairs_mut().append_pair("token", token);
                }

                // Attempt to connect with timeout
                let connect_result = timeout(self.config.connect_timeout, connect_async(url)).await;

                match connect_result {
                    Ok(Ok((ws_stream, _))) => {
                        // Connection successful
                        let mut conn_info = self.connection.write().await;
                        conn_info.state =
                            ConnectionState::Connected(Arc::new(Mutex::new(ws_stream)));
                        conn_info.mark_healthy();
                        conn_info.mark_used();
                        info!("WebSocket connection established successfully");
                        return Ok(());
                    }
                    Ok(Err(e)) => {
                        error!("WebSocket connection error: {}", e);

                        // Handle retry logic
                        retries += 1;
                        if retries > max_retries {
                            let mut conn_info = self.connection.write().await;
                            conn_info.reset();
                            conn_info.reconnect_attempts = retries;
                            return Err(A2AError::WebSocket(format!(
                                "Failed to connect after {} attempts: {}",
                                retries, e
                            )));
                        }

                        // Exponential backoff
                        let backoff = self.config.reconnect_base_delay * 2u32.pow(retries - 1);
                        warn!(
                            "Retrying connection in {:?} (attempt {}/{})",
                            backoff, retries, max_retries
                        );
                        tokio::time::sleep(backoff).await;
                    }
                    Err(_) => {
                        // Timeout occurred
                        error!("WebSocket connection timed out");

                        // Handle retry logic
                        retries += 1;
                        if retries > max_retries {
                            let mut conn_info = self.connection.write().await;
                            conn_info.reset();
                            conn_info.reconnect_attempts = retries;
                            return Err(A2AError::WebSocket("Connection timed out".to_string()));
                        }

                        // Exponential backoff
                        let backoff = self.config.reconnect_base_delay * 2u32.pow(retries - 1);
                        warn!(
                            "Retrying connection in {:?} (attempt {}/{})",
                            backoff, retries, max_retries
                        );
                        tokio::time::sleep(backoff).await;
                    }
                }
            }

            // If we exit the retry loop without returning, something went wrong
            return Err(A2AError::WebSocket(
                "Failed to connect after exhausting all retries".to_string(),
            ));
        }
    }
    /// Get the current connection or establish a new one
    async fn get_connection(&self) -> Result<Arc<Mutex<WsStreamType>>, A2AError> {
        // Try to connect first to ensure we have a valid connection
        self.connect().await?;

        // Now get the connection handle
        let conn_info = self.connection.read().await;
        let conn_state = conn_info.state.clone();
        match conn_state {
            ConnectionState::Connected(ws_stream) => {
                // Mark as used to update the idle timer
                drop(conn_info);
                let mut conn_info = self.connection.write().await;
                conn_info.mark_used();
                Ok(ws_stream.clone())
            }
            _ => {
                // This shouldn't happen if connect() succeeded
                Err(A2AError::Internal(
                    "No WebSocket connection available".to_string(),
                ))
            }
        }
    }

    /// Prepare and handle a WebSocket stream with automatic reconnection
    async fn prepare_stream<'a>(
        &self,
        task_id: &'a str,
        initial_request_json: String,
        is_new_task: bool,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError> {
        // Create a channel for stream items
        let (tx, rx) = mpsc::channel::<Result<StreamItem, A2AError>>(32);
        
        // Get a connection to the WebSocket server
        let connection = self.get_connection().await?;
        let ws_connection = {
            let mut stream = connection.lock().await;
            
            // Send the initial request
            stream.send(WsMessage::Text(initial_request_json.clone()))
                .await
                .map_err(|e| A2AError::WebSocket(format!("Failed to send stream request: {}", e)))?;
            
            // Clone the connection for processing
            connection.clone()
        };
        
        // Create a flag for stream completion
        let finished = Arc::new(Mutex::new(false));
        let finished_clone = finished.clone();
        
        // Clone needed values for the background task
        let task_id = task_id.to_string();
        let tx_clone = tx.clone();
        
        // Clone only what we need from self
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let config = self.config.clone();
        
        // Create a new client instance for the spawned task 
        // rather than referencing self
        let client_for_task = WebSocketClient {
            base_url,
            auth_token,
            connection: Arc::new(RwLock::new(ConnectionInfo::new())),
            config: config.clone(),
            _health_checker: None,
        };
        
        
        // Spawn a task to handle incoming messages
        tokio::spawn(async move {
            // Reconnection state
            let mut reconnect_attempts = 0;
            let max_attempts = config.max_reconnect_attempts;
            
            // Process loop
            'process_loop: loop {
                // Check if stream is finished
                if *finished_clone.lock().await {
                    break;
                }
                
                // Process incoming messages
                let message_result = {
                    let mut stream = ws_connection.lock().await;
                    timeout(config.response_timeout, stream.next()).await
                };
                
                match message_result {
                    // Timeout waiting for a message
                    Err(_) => {
                        warn!("Timeout waiting for WebSocket message");
                        
                        // Send an error to the stream
                        let _ = tx_clone.send(Err(A2AError::WebSocket(
                            "Timeout waiting for WebSocket message".to_string()
                        ))).await;
                        
                        // Try to reconnect
                        if reconnect_attempts >= max_attempts {
                            error!("Maximum reconnection attempts reached");
                            break;
                        }
                        
                        reconnect_attempts += 1;
                        
                        // Exponential backoff
                        let backoff = config.reconnect_base_delay * 2u32.pow(reconnect_attempts - 1);
                        warn!("Attempting to reconnect in {:?} (attempt {}/{})", 
                            backoff, reconnect_attempts, max_attempts);
                        tokio::time::sleep(backoff).await;
                        
                        // Try to resubscribe
                        if let Err(e) = handle_reconnection(
                            &client_for_task, 
                            &task_id, 
                            is_new_task, 
                            history_length, 
                            &tx_clone
                        ).await {
                            error!("Failed to reconnect: {}", e);
                            let _ = tx_clone.send(Err(e)).await;
                            break;
                        }
                        
                        // Reset reconnection counter on successful reconnection
                        reconnect_attempts = 0;
                        continue;
                    },

                    // Received a message
                    Ok(Some(Ok(message))) => {
                        // Reset reconnection counter on successful message
                        reconnect_attempts = 0;

                        match message {
                            WsMessage::Text(text) => {
                                // Process the message
                                match process_stream_message(&text, &tx_clone).await {
                                    Ok(true) => {
                                        // Final message received
                                        debug!("Final message received, ending stream");
                                        let mut finished = finished_clone.lock().await;
                                        *finished = true;
                                        break;
                                    }
                                    Ok(false) => {
                                        // Regular message, continue processing
                                        continue;
                                    }
                                    Err(e) => {
                                        // Error processing message
                                        warn!("Error processing message: {}", e);
                                        let _ = tx_clone.send(Err(e)).await;
                                        continue;
                                    }
                                }
                            }
                            WsMessage::Ping(data) => {
                                // Respond to ping with pong
                                let mut stream = ws_connection.lock().await;
                                if let Err(e) = stream.send(WsMessage::Pong(data)).await {
                                    warn!("Failed to respond to ping: {}", e);
                                }
                                continue;
                            }
                            WsMessage::Pong(_) => {
                                // Pong received, connection is healthy
                                debug!("Pong received, connection is healthy");
                                continue;
                            }
                            WsMessage::Close(reason) => {
                                // Connection closed by server
                                let reason_str = reason.map_or_else(
                                    || "No reason given".to_string(),
                                    |r| format!("Code: {}, Reason: {}", r.code, r.reason),
                                );

                                warn!("WebSocket closed by server: {}", reason_str);

                                // Try to reconnect
                                if reconnect_attempts >= max_attempts {
                                    error!("Maximum reconnection attempts reached");
                                    let _ = tx_clone
                                        .send(Err(A2AError::WebSocket(format!(
                                            "Connection closed by server: {}",
                                            reason_str
                                        ))))
                                        .await;
                                    break;
                                }

                                reconnect_attempts += 1;

                                // Exponential backoff
                                let backoff = config.reconnect_base_delay
                                    * 2u32.pow(reconnect_attempts - 1);
                                warn!(
                                    "Attempting to reconnect in {:?} (attempt {}/{})",
                                    backoff, reconnect_attempts, max_attempts
                                );
                                tokio::time::sleep(backoff).await;

                                // Try to resubscribe
                                if let Err(e) = handle_reconnection(
                                    &client_for_task,
                                    &task_id,
                                    is_new_task,
                                    history_length,
                                    &tx_clone,
                                )
                                .await
                                {
                                    error!("Failed to reconnect: {}", e);
                                    let _ = tx_clone.send(Err(e)).await;
                                    break;
                                }

                                // Reset reconnection counter on successful reconnection
                                reconnect_attempts = 0;
                            }
                            _ => {
                                // Ignore other message types
                                continue;
                            }
                        }
                    }

                    // Error receiving message
                    Ok(Some(Err(e))) => {
                        warn!("WebSocket error: {}", e);

                        // Send error to the stream
                        let _ = tx_clone
                            .send(Err(A2AError::WebSocket(format!("WebSocket error: {}", e))))
                            .await;

                        // Try to reconnect
                        if reconnect_attempts >= max_attempts {
                            error!("Maximum reconnection attempts reached");
                            break;
                        }

                        reconnect_attempts += 1;

                        // Exponential backoff
                        let backoff =
                            config.reconnect_base_delay * 2u32.pow(reconnect_attempts - 1);
                        warn!(
                            "Attempting to reconnect in {:?} (attempt {}/{})",
                            backoff, reconnect_attempts, max_attempts
                        );
                        tokio::time::sleep(backoff).await;

                        // Try to resubscribe
                        if let Err(e) = handle_reconnection(
                            &client_for_task,
                            &task_id,
                            is_new_task,
                            history_length,
                            &tx_clone,
                        )
                        .await
                        {
                            error!("Failed to reconnect: {}", e);
                            let _ = tx_clone.send(Err(e)).await;
                            break;
                        }

                        // Reset reconnection counter on successful reconnection
                        reconnect_attempts = 0;
                    }

                    // End of stream
                    Ok(None) => {
                        warn!("WebSocket stream ended unexpectedly");

                        // Try to reconnect
                        if reconnect_attempts >= max_attempts {
                            error!("Maximum reconnection attempts reached");
                            let _ = tx_clone
                                .send(Err(A2AError::WebSocket(
                                    "WebSocket stream ended unexpectedly".to_string(),
                                )))
                                .await;
                            break;
                        }

                        reconnect_attempts += 1;

                        // Exponential backoff
                        let backoff =
                            config.reconnect_base_delay * 2u32.pow(reconnect_attempts - 1);
                        warn!(
                            "Attempting to reconnect in {:?} (attempt {}/{})",
                            backoff, reconnect_attempts, max_attempts
                        );
                        tokio::time::sleep(backoff).await;

                        // Try to resubscribe
                        if let Err(e) = handle_reconnection(
                            &client_for_task,
                            &task_id,
                            is_new_task,
                            history_length,
                            &tx_clone,
                        )
                        .await
                        {
                            error!("Failed to reconnect: {}", e);
                            let _ = tx_clone.send(Err(e)).await;
                            break;
                        }

                        // Reset reconnection counter on successful reconnection
                        reconnect_attempts = 0;
                    }
                }
            }

            debug!("WebSocket stream processing task completed");
        });

        // Create a stream from the receiver
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(stream)
    }

    /// Send a message to the WebSocket server and get a response with automatic reconnection
    async fn send_ws_message(&self, message: WsMessage) -> Result<WsMessage, A2AError> {
        // Get connection (or connect if needed)
        let connection = self.get_connection().await?;

        // Send the message
        {
            let mut stream = connection.lock().await;
            if let Err(e) = stream.send(message.clone()).await {
                // Failed to send - the connection might be broken
                drop(stream); // Release the lock

                // Mark connection as unhealthy
                let mut conn_info = self.connection.write().await;
                conn_info.mark_unhealthy();
                conn_info.reset();
                drop(conn_info); // Release the lock

                // Try to reconnect and send again
                let connection = self.get_connection().await?;
                let mut stream = connection.lock().await;
                stream.send(message).await.map_err(|e| {
                    A2AError::WebSocket(format!("Failed to send after reconnection: {}", e))
                })?;
            }
        }

        // Wait for a response with timeout
        let response = {
            let mut stream = connection.lock().await;
            let response_future = stream.next();

            match timeout(self.config.response_timeout, response_future).await {
                Ok(Some(Ok(msg))) => msg,
                Ok(Some(Err(e))) => {
                    // Connection error
                    let mut conn_info = self.connection.write().await;
                    conn_info.mark_unhealthy();
                    conn_info.reset();
                    return Err(A2AError::WebSocket(format!("WebSocket error: {}", e)));
                }
                Ok(None) => {
                    // Connection closed
                    let mut conn_info = self.connection.write().await;
                    conn_info.reset();
                    return Err(A2AError::WebSocket(
                        "Connection closed by server".to_string(),
                    ));
                }
                Err(_) => {
                    // Timeout waiting for response
                    return Err(A2AError::WebSocket(
                        "Timeout waiting for response".to_string(),
                    ));
                }
            }
        };

        // Handle pong responses to mark the connection as healthy
        if let WsMessage::Pong(_) = &response {
            let mut conn_info = self.connection.write().await;
            conn_info.mark_healthy();
        }

        Ok(response)
    }
}

#[async_trait]
impl AsyncA2AClient for WebSocketClient {
    async fn send_raw_request<'a>(&self, request: &'a str) -> Result<String, A2AError> {
        let response = self
            .send_ws_message(WsMessage::Text(request.to_string()))
            .await?;

        match response {
            WsMessage::Text(text) => Ok(text),
            WsMessage::Close(reason) => {
                // Server closed the connection
                let mut conn_info = self.connection.write().await;
                conn_info.reset();

                let reason_str = reason.map_or_else(
                    || "No reason given".to_string(),
                    |r| format!("Code: {}, Reason: {}", r.code, r.reason),
                );

                Err(A2AError::WebSocket(format!(
                    "Connection closed by server: {}",
                    reason_str
                )))
            }
            _ => Err(A2AError::WebSocket(
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
        // Prepare the request
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

        // Create and return the stream
        self.prepare_stream(task_id, json, true, history_length)
            .await
    }
    async fn resubscribe_to_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<impl Stream<Item = Result<StreamItem, A2AError>>, A2AError> {
        // Prepare the request
        let params = TaskQueryParams {
            id: task_id.to_string(),
            history_length,
            metadata: None,
        };

        let request = TaskResubscriptionRequest::new(params);
        let json = json_rpc::serialize_request(&A2ARequest::TaskResubscription(request))?;

        // Create and return the stream
        self.prepare_stream(task_id, json, false, history_length)
            .await
    }
}

/// Handle reconnection to a stream
async fn handle_reconnection(
    client: &WebSocketClient,
    task_id: &str,
    is_new_task: bool,
    history_length: Option<u32>,
    tx: &mpsc::Sender<Result<StreamItem, A2AError>>,
) -> Result<(), A2AError> {
    info!("Attempting to resubscribe to task {}", task_id);

    // If this was a new task, we can't reconnect
    if is_new_task {
        return Err(A2AError::WebSocket(
            "Cannot reconnect to a new task that was never established".to_string(),
        ));
    }

    // Use resubscribe endpoint to reconnect
    let params = TaskQueryParams {
        id: task_id.to_string(),
        history_length,
        metadata: None,
    };

    let request = TaskResubscriptionRequest::new(params);
    let json = json_rpc::serialize_request(&A2ARequest::TaskResubscription(request))?;

    // Get a new connection
    let connection = client.get_connection().await?;

    // Send the resubscription request
    {
        let mut stream = connection.lock().await;
        stream.send(WsMessage::Text(json)).await.map_err(|e| {
            A2AError::WebSocket(format!("Failed to send resubscription request: {}", e))
        })?;
    }

    info!("Successfully resubscribed to task {}", task_id);
    Ok(())
}

/// Process a WebSocket message into a stream item
async fn process_stream_message(
    text: &str,
    tx: &mpsc::Sender<Result<StreamItem, A2AError>>,
) -> Result<bool, A2AError> {
    // Parse the JSON response
    let response: Value = serde_json::from_str(text).map_err(|e| A2AError::JsonParse(e))?;

    // Check if this is an error response
    if let Some(error) = response.get("error") {
        if error.is_object() {
            let error_response: JSONRPCResponse =
                serde_json::from_value(response.clone()).map_err(|e| A2AError::JsonParse(e))?;

            if let Some(err) = error_response.error {
                return Err(A2AError::JsonRpc {
                    code: err.code,
                    message: err.message,
                    data: err.data,
                });
            }
        }
    }

    // Get the result from the response
    let result = response.get("result").cloned().unwrap_or(Value::Null);

    // If result is null, this might be an acknowledgement message
    if result.is_null() {
        debug!("Received acknowledgement message: {}", text);
        return Ok(false);
    }

    // Try to parse as a status update
    if let Ok(status_update) = serde_json::from_value::<TaskStatusUpdateEvent>(result.clone()) {
        // Send the status update to the stream
        tx.send(Ok(StreamItem::StatusUpdate(status_update.clone())))
            .await
            .map_err(|_| {
                A2AError::Internal("Failed to send status update to stream".to_string())
            })?;

        // Check if this is the final update
        return Ok(status_update.final_);
    }

    // Try to parse as an artifact update
    if let Ok(artifact_update) = serde_json::from_value::<TaskArtifactUpdateEvent>(result) {
        // Send the artifact update to the stream
        tx.send(Ok(StreamItem::ArtifactUpdate(artifact_update)))
            .await
            .map_err(|_| {
                A2AError::Internal("Failed to send artifact update to stream".to_string())
            })?;

        // Artifact updates are never final
        return Ok(false);
    }

    // Unknown message type
    warn!("Unknown message type received: {}", text);
    Err(A2AError::Internal(format!(
        "Failed to parse streaming response: {}",
        text
    )))
}
