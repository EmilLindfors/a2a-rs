//! SSE streaming example with Axum
//!
//! This example demonstrates how to build a web server that streams task updates
//! to clients using Server-Sent Events (SSE). It shows how to integrate the
//! a2a-client library with an Axum web application.
//!
//! # Running the Example
//!
//! 1. Start an A2A agent with WebSocket support:
//!    ```bash
//!    cd ../a2a-agents
//!    cargo run --bin reimbursement_demo
//!    ```
//!
//! 2. Run this example:
//!    ```bash
//!    cargo run --example sse_streaming
//!    ```
//!
//! 3. Open your browser to http://localhost:3000 and send a message.
//!    The response will stream in real-time via SSE.

use a2a_client::{components::create_sse_stream, WebA2AClient};
use a2a_rs::domain::{Message, Part, Role};
use a2a_rs::services::AsyncA2AClient;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};

/// Application state shared across handlers
type AppState = Arc<WebA2AClient>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting SSE streaming example server");

    // Create the A2A client with WebSocket support
    let client = Arc::new(
        WebA2AClient::builder()
            .http_url("http://localhost:8080")
            .ws_url("ws://localhost:8080/ws")
            .build(),
    );

    if client.has_websocket() {
        println!("✓ WebSocket support enabled");
    } else {
        println!("! WebSocket not configured, will use HTTP polling fallback");
    }

    // Build the Axum router
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/send", post(send_message_handler))
        .route("/stream/:task_id", get(stream_handler))
        .with_state(client);

    // Start the server
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server listening on http://{}", addr);
    println!("Open your browser to http://localhost:3000");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Serve the HTML frontend
async fn index_handler() -> Html<&'static str> {
    Html(include_str!("sse_streaming_index.html"))
}

#[derive(Deserialize)]
struct SendMessageRequest {
    text: String,
}

#[derive(Serialize)]
struct SendMessageResponse {
    task_id: String,
}

/// Handle sending a message to the agent
async fn send_message_handler(
    State(client): State<AppState>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, StatusCode> {
    println!("Sending message: {}", payload.text);

    let message = Message::builder()
        .message_id(uuid::Uuid::new_v4().to_string())
        .role(Role::User)
        .parts(vec![Part::text(payload.text)])
        .build();

    // Create a new task ID for this conversation
    let task_id = uuid::Uuid::new_v4().to_string();

    match client.http.send_task_message(&task_id, &message, None, None).await {
        Ok(task) => {
            println!("Created task: {}", task.id);
            Ok(Json(SendMessageResponse { task_id: task.id }))
        }
        Err(e) => {
            eprintln!("Failed to send message: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Stream task updates via SSE
async fn stream_handler(
    State(client): State<AppState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    println!("Client subscribed to task stream: {}", task_id);
    create_sse_stream(client, task_id)
}
