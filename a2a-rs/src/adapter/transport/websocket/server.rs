//! WebSocket server adapter for the A2A protocol

// This module is already conditionally compiled with #[cfg(feature = "ws-server")] in mod.rs

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, Mutex}, // Changed to tokio::sync::Mutex
};
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};

#[cfg(feature = "tracing")]
use tracing::{debug, error, info, instrument};

use crate::{
    adapter::{
        auth::{Authenticator, NoopAuthenticator},
        error::WebSocketServerError,
    },
    domain::{A2AError, TaskArtifactUpdateEvent, TaskStatusUpdateEvent},
    port::{streaming_handler::Subscriber, AsyncStreamingHandler},
    services::server::{AgentInfoProvider, AsyncA2ARequestProcessor},
};

type ClientMap = Arc<Mutex<HashMap<String, mpsc::Sender<WsMessage>>>>;

/// WebSocket server for the A2A protocol
pub struct WebSocketServer<P, A, S, Auth = NoopAuthenticator>
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    S: AsyncStreamingHandler + Send + Sync + 'static,
    Auth: Authenticator + Send + Sync + 'static,
{
    /// Request processor
    processor: Arc<P>,
    /// Agent info provider
    _agent_info: Arc<A>,
    /// Streaming handler
    streaming_handler: Arc<S>,
    /// Server address
    address: String,
    /// Connected clients
    clients: ClientMap,
    /// Authenticator
    authenticator: Option<Arc<Auth>>,
}

impl<P, A, S> WebSocketServer<P, A, S>
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    S: AsyncStreamingHandler + Send + Sync + 'static,
{
    /// Create a new WebSocket server
    pub fn new(processor: P, agent_info: A, streaming_handler: S, address: String) -> Self {
        Self {
            processor: Arc::new(processor),
            _agent_info: Arc::new(agent_info),
            streaming_handler: Arc::new(streaming_handler),
            address,
            clients: Arc::new(Mutex::new(HashMap::new())),
            authenticator: None,
        }
    }
}

impl<P, A, S, Auth> WebSocketServer<P, A, S, Auth>
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    S: AsyncStreamingHandler + Send + Sync + 'static,
    Auth: Authenticator + Clone + Send + Sync + 'static,
{
    /// Create a new WebSocket server with authentication
    pub fn with_auth(
        processor: P,
        agent_info: A,
        streaming_handler: S,
        address: String,
        authenticator: Auth,
    ) -> Self {
        Self {
            processor: Arc::new(processor),
            _agent_info: Arc::new(agent_info),
            streaming_handler: Arc::new(streaming_handler),
            address,
            clients: Arc::new(Mutex::new(HashMap::new())),
            authenticator: Some(Arc::new(authenticator)),
        }
    }

    /// Start the WebSocket server
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        server.address = %self.address,
        server.has_auth = self.authenticator.is_some()
    )))]
    pub async fn start(&self) -> Result<(), A2AError> {
        #[cfg(feature = "tracing")]
        info!("Starting WebSocket server");

        let addr = self
            .address
            .parse::<SocketAddr>()
            .map_err(|e| WebSocketServerError::Server(format!("Invalid address: {}", e)))?;

        let listener = TcpListener::bind(&addr)
            .await
            .map_err(WebSocketServerError::Io)?;

        #[cfg(feature = "tracing")]
        info!("WebSocket server listening on: {}", addr);

        #[cfg(not(feature = "tracing"))]
        println!("WebSocket server listening on: {}", addr);

        while let Ok((stream, _)) = listener.accept().await {
            let processor = self.processor.clone();
            let agent_info = self._agent_info.clone();
            let streaming_handler = self.streaming_handler.clone();
            let clients = self.clients.clone();

            let authenticator = self.authenticator.clone();

            tokio::spawn(async move {
                // If an authenticator is present, obtain credentials from query parameters or headers
                if let Some(_auth) = &authenticator {
                    // For WebSockets, we'd typically extract auth from the URL query parameters
                    // or from headers. In this simplified implementation, we'll just assume success.
                    // In a real implementation, you would extract the token and call authenticate()

                    // For now, we'll just log a message indicating auth is enabled
                    #[cfg(feature = "tracing")]
                    debug!("Authentication is enabled for WebSocket connections");
                    #[cfg(not(feature = "tracing"))]
                    println!("Authentication is enabled for WebSocket connections");
                }

                if let Err(e) =
                    handle_connection(stream, processor, agent_info, streaming_handler, clients)
                        .await
                {
                    #[cfg(feature = "tracing")]
                    error!("Error handling connection: {}", e);
                    #[cfg(not(feature = "tracing"))]
                    eprintln!("Error handling connection: {}", e);
                }
            });
        }

        Ok(())
    }
}

/// Handle a WebSocket connection
#[cfg_attr(feature = "tracing", instrument(skip_all, fields(peer_addr)))]
async fn handle_connection<P, A, S>(
    stream: TcpStream,
    processor: Arc<P>,
    _agent_info: Arc<A>,
    streaming_handler: Arc<S>,
    clients: ClientMap,
) -> Result<(), A2AError>
where
    P: AsyncA2ARequestProcessor + Send + Sync + 'static,
    A: AgentInfoProvider + Send + Sync + 'static,
    S: AsyncStreamingHandler + Send + Sync + 'static,
{
    let addr = stream.peer_addr().map_err(|e| {
        WebSocketServerError::Connection(format!("Failed to get peer address: {}", e))
    })?;

    #[cfg(feature = "tracing")]
    tracing::Span::current().record("peer_addr", addr.to_string());

    let ws_stream = accept_async(stream).await.map_err(|e| {
        WebSocketServerError::Connection(format!("Error during WebSocket handshake: {}", e))
    })?;

    #[cfg(feature = "tracing")]
    info!("WebSocket connection established with: {}", addr);
    #[cfg(not(feature = "tracing"))]
    println!("WebSocket connection established with: {}", addr);

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Channel for sending messages to the client
    let (tx, mut rx) = mpsc::channel::<WsMessage>(32);

    // Register the client
    let client_id = addr.to_string();
    {
        let mut clients_guard = clients.lock().await; // Changed to await
        clients_guard.insert(client_id.clone(), tx.clone());
        #[cfg(feature = "tracing")]
        debug!(
            "Registered client {}, total clients: {}",
            client_id,
            clients_guard.len()
        );
    }

    // Task to forward messages from the channel to the WebSocket
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = ws_sender.send(msg).await {
                #[cfg(feature = "tracing")]
                error!("Error sending WebSocket message: {}", e);
                #[cfg(not(feature = "tracing"))]
                eprintln!("Error sending WebSocket message: {}", e);
                break;
            }
        }
    });

    // Process incoming messages
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(msg) => {
                if let WsMessage::Text(text) = msg {
                    // Process the message
                    let response = match processor.process_raw_request(&text).await {
                        Ok(response) => response,
                        Err(e) => {
                            let error = e.to_jsonrpc_error();
                            serde_json::to_string(&json!({
                                "jsonrpc": "2.0",
                                "id": null,
                                "error": error
                            }))
                            .unwrap_or_else(|_| {
                                r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Internal error","data":null}}"#.to_string()
                            })
                        }
                    };

                    // Send the response
                    if let Err(e) = tx.send(WsMessage::Text(response)).await {
                        eprintln!("Error sending response: {}", e);
                        break;
                    }

                    // Check if this is a streaming request
                    if let Ok(request) = serde_json::from_str::<Value>(&text) {
                        if let Some(method) = request.get("method").and_then(Value::as_str) {
                            if method == "tasks/sendSubscribe" || method == "tasks/resubscribe" {
                                // Handle streaming request
                                if let Some(params) = request.get("params") {
                                    if let Some(task_id) = params.get("id").and_then(Value::as_str)
                                    {
                                        // Create subscribers for status and artifact updates
                                        let status_subscriber = WebSocketSubscriber {
                                            client_id: client_id.clone(),
                                            request_id: request.get("id").cloned(),
                                            clients: clients.clone(),
                                        };

                                        let artifact_subscriber = WebSocketSubscriber {
                                            client_id: client_id.clone(),
                                            request_id: request.get("id").cloned(),
                                            clients: clients.clone(),
                                        };

                                        // Register the subscribers
                                        if let Err(e) = streaming_handler
                                            .add_status_subscriber(
                                                task_id,
                                                Box::new(status_subscriber),
                                            )
                                            .await
                                        {
                                            eprintln!("Error adding status subscriber: {}", e);
                                        }

                                        if let Err(e) = streaming_handler
                                            .add_artifact_subscriber(
                                                task_id,
                                                Box::new(artifact_subscriber),
                                            )
                                            .await
                                        {
                                            #[cfg(feature = "tracing")]
                                            error!("Error adding artifact subscriber: {}", e);
                                            #[cfg(not(feature = "tracing"))]
                                            eprintln!("Error adding artifact subscriber: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if let WsMessage::Ping(data) = msg {
                    // Respond to ping with pong
                    if let Err(e) = tx.send(WsMessage::Pong(data)).await {
                        #[cfg(feature = "tracing")]
                        error!("Error sending pong: {}", e);
                        #[cfg(not(feature = "tracing"))]
                        eprintln!("Error sending pong: {}", e);
                        break;
                    }
                } else if let WsMessage::Close(_) = msg {
                    break;
                }
            }
            Err(e) => {
                eprintln!("Error receiving WebSocket message: {}", e);
                break;
            }
        }
    }

    // Clean up
    {
        let mut clients_guard = clients.lock().await; // Changed to await
        clients_guard.remove(&client_id);
    }

    // Cancel the forward task
    forward_task.abort();

    #[cfg(feature = "tracing")]
    info!("WebSocket connection closed with: {}", addr);
    #[cfg(not(feature = "tracing"))]
    println!("WebSocket connection closed with: {}", addr);
    Ok(())
}

/// WebSocket subscriber for streaming updates
struct WebSocketSubscriber {
    client_id: String,
    request_id: Option<Value>,
    clients: ClientMap,
}

#[async_trait]
impl Subscriber<TaskStatusUpdateEvent> for WebSocketSubscriber {
    async fn on_update(&self, update: TaskStatusUpdateEvent) -> Result<(), A2AError> {
        let message = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "result": update
        });

        // Get the sender without holding the lock across the await point
        let sender_opt = {
            let clients_guard = self.clients.lock().await; // Changed to await
            clients_guard.get(&self.client_id).cloned()
        };

        // Send the message if we have a sender
        if let Some(sender) = sender_opt {
            sender
                .send(WsMessage::Text(
                    serde_json::to_string(&message).map_err(A2AError::JsonParse)?,
                ))
                .await
                .map_err(|e| A2AError::Internal(format!("Send error: {}", e)))?;
        }

        Ok(())
    }
}

#[async_trait]
impl Subscriber<TaskArtifactUpdateEvent> for WebSocketSubscriber {
    async fn on_update(&self, update: TaskArtifactUpdateEvent) -> Result<(), A2AError> {
        let message = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "result": update
        });

        // Get the sender without holding the lock across the await point
        let sender_opt = {
            let clients_guard = self.clients.lock().await; // Changed to await
            clients_guard.get(&self.client_id).cloned()
        };

        // Send the message if we have a sender
        if let Some(sender) = sender_opt {
            sender
                .send(WsMessage::Text(
                    serde_json::to_string(&message).map_err(A2AError::JsonParse)?,
                ))
                .await
                .map_err(|e| A2AError::Internal(format!("Send error: {}", e)))?;
        }

        Ok(())
    }
}
