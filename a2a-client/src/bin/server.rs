use a2a_rs::{
    domain::Part as MessagePart,
    services::AsyncA2AClient,
    HttpClient,
};
use askama::Template;
use askama_axum::IntoResponse;
use axum::{
    extract::{Path, State},
    response::{Html, Response as AxumResponse},
    routing::{get, post},
    Form, Router,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{error, info};
use uuid::Uuid;

struct AppState {
    a2a_client: HttpClient,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    agent_url: String,
}

#[derive(Template)]
#[template(path = "chat.html")]
struct ChatTemplate {
    task_id: String,
    messages: Vec<MessageView>,
}

#[derive(Debug, Serialize)]
struct MessageView {
    id: String,
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct SendMessageForm {
    task_id: String,
    message: String,
}

#[derive(Deserialize)]
struct NewChatForm {
    agent_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let agent_url = std::env::var("AGENT_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    let a2a_client = HttpClient::new(agent_url);
    let state = AppState { a2a_client };

    let app = Router::new()
        .route("/", get(index))
        .route("/chat/new", post(new_chat))
        .route("/chat/:task_id", get(chat_page))
        .route("/chat/:task_id/send", post(send_message))
        .route("/static/*path", get(serve_static))
        .nest_service("/styles.css", ServeDir::new("src"))
        .layer(CorsLayer::permissive())
        .with_state(Arc::new(state));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn index() -> impl IntoResponse {
    let template = IndexTemplate {
        agent_url: std::env::var("AGENT_URL")
            .unwrap_or_else(|_| "http://localhost:8080".to_string()),
    };
    template
}

async fn new_chat(
    State(_state): State<Arc<AppState>>,
    Form(_form): Form<NewChatForm>,
) -> Result<AxumResponse, AppError> {
    // Create a new task
    let task_id = Uuid::new_v4().to_string();
    
    // Redirect to the chat page
    Ok(axum::response::Redirect::to(&format!("/chat/{}", task_id)).into_response())
}

async fn chat_page(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    // Try to get existing task messages
    let messages = match state.a2a_client.get_task(&task_id, Some(50)).await {
        Ok(task) => {
            task.history
                .unwrap_or_default()
                .into_iter()
                .map(|msg| {
                    // Extract text content from message parts
                    let content = msg.parts.iter()
                        .filter_map(|part| match part {
                            MessagePart::Text { text, .. } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    
                    MessageView {
                        id: msg.message_id,
                        role: format!("{:?}", msg.role),
                        content,
                    }
                })
                .collect()
        }
        Err(_) => {
            // New task, no messages yet
            vec![]
        }
    };

    let template = ChatTemplate { task_id, messages };
    Ok(template)
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    Form(form): Form<SendMessageForm>,
) -> Result<AxumResponse, AppError> {
    use a2a_rs::domain::{Message, Role, Part};
    
    let message = Message {
        role: Role::User,
        parts: vec![Part::text(form.message)],
        metadata: None,
        reference_task_ids: None,
        message_id: Uuid::new_v4().to_string(),
        task_id: Some(form.task_id.clone()),
        context_id: None,
        kind: "message".to_string(),
    };

    state.a2a_client
        .send_task_message(&form.task_id, &message, None, Some(50))
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Failed to send message: {}", e)))?;

    // Redirect back to the chat page
    Ok(axum::response::Redirect::to(&format!("/chat/{}", form.task_id)).into_response())
}

async fn serve_static(Path(path): Path<String>) -> impl IntoResponse {
    // Serve static files
    Html(format!("Static file: {}", path))
}

#[derive(Debug)]
struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> AxumResponse {
        error!("Application error: {}", self.0);
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Internal server error: {}", self.0),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}