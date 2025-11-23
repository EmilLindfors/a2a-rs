use a2a_agents::reimbursement_agent::{AuthConfig, ReimbursementServer, ServerConfig};
use a2a_client::{
    WebA2AClient,
    components::{MessageView, TaskView, create_sse_stream},
};
use a2a_rs::{
    domain::{ListTasksParams, TaskState, TaskStatusUpdateEvent},
    services::AsyncA2AClient,
};
use askama::Template;
use askama_axum::IntoResponse;
use axum::{
    Form, Router,
    extract::{Multipart, Path, Query, State},
    response::Response as AxumResponse,
    routing::{get, post},
};
use clap::Parser;
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Command-line arguments for the unified A2A Reimbursement Demo
#[derive(Parser, Debug)]
#[clap(
    name = "a2a-reimbursement-demo",
    author,
    version,
    about = "A2A Reimbursement Agent Demo - Unified agent backend and web frontend",
    long_about = "Runs the A2A reimbursement agent backend (HTTP/WebSocket) and/or web frontend in a single process"
)]
struct Args {
    /// Host to bind servers to
    #[clap(long, default_value = "127.0.0.1")]
    host: String,

    /// Agent HTTP server port
    #[clap(long, default_value = "8080")]
    agent_http_port: u16,

    /// Agent WebSocket server port
    #[clap(long, default_value = "8081")]
    agent_ws_port: u16,

    /// Frontend web server port
    #[clap(long, default_value = "3000")]
    frontend_port: u16,

    /// Run mode: agent (backend only), frontend (UI only), or all (both)
    #[clap(long, default_value = "all")]
    mode: String,

    /// Agent transport mode: http, websocket, or both
    #[clap(long, default_value = "both")]
    transport: String,

    /// Agent configuration file path (JSON format)
    #[clap(long)]
    config: Option<String>,

    /// Frontend WebSocket usage (true to use WebSocket for subscriptions)
    #[clap(long, default_value = "false")]
    frontend_use_websocket: bool,
}

// Frontend AppState
struct AppState {
    client: Arc<WebA2AClient>,
    webhook_token: String,
}

// Template structs
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
    task_state: Option<String>,
}

#[derive(Template)]
#[template(path = "tasks.html")]
struct TasksTemplate {
    tasks: Vec<TaskView>,
    filter_state: Option<String>,
    total_count: usize,
}

#[derive(Template)]
#[template(path = "expense-form.html")]
struct ExpenseFormTemplate {
    category: Option<String>,
}

// Form structs
#[derive(Deserialize)]
struct NewChatForm {
    #[allow(dead_code)]
    agent_url: String,
}

#[derive(Deserialize)]
struct TasksQuery {
    state: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ExpenseQuery {
    #[serde(rename = "type")]
    expense_type: Option<String>,
}

#[derive(Deserialize)]
struct ExpenseSubmitForm {
    category: String,
    amount: String,
    date: String,
    vendor: Option<String>,
    description: String,
    project_code: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Parse command-line arguments
    let args = Args::parse();

    println!("🚀 A2A Reimbursement Agent Demo");
    println!("===============================================");
    println!();

    // Validate mode
    match args.mode.as_str() {
        "agent" | "frontend" | "all" => {}
        _ => {
            eprintln!(
                "❌ Invalid mode: {}. Use 'agent', 'frontend', or 'all'",
                args.mode
            );
            std::process::exit(1);
        }
    }

    // Start based on mode
    match args.mode.as_str() {
        "agent" => start_agent_only(args).await?,
        "frontend" => start_frontend_only(args).await?,
        "all" => start_all(args).await?,
        _ => unreachable!(),
    }

    Ok(())
}

async fn start_agent_only(args: Args) -> anyhow::Result<()> {
    println!("🤖 Starting Agent Backend Only");
    println!("───────────────────────────────");

    let config = load_agent_config(&args)?;
    print_agent_info(&config, &args);

    let server = ReimbursementServer::from_config(config);

    match args.transport.as_str() {
        "http" => {
            println!("🌐 Starting HTTP server only...");
            server
                .start_http()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }
        "websocket" | "ws" => {
            println!("🔌 Starting WebSocket server only...");
            server
                .start_websocket()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }
        "both" | "all" => {
            println!("🔄 Starting both HTTP and WebSocket servers...");
            server
                .start_all()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }
        _ => {
            eprintln!(
                "❌ Invalid transport: {}. Use 'http', 'websocket', or 'both'",
                args.transport
            );
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn start_frontend_only(args: Args) -> anyhow::Result<()> {
    println!("🌐 Starting Web Frontend Only");
    println!("──────────────────────────────");

    // User must specify agent URL via env vars
    let http_url = std::env::var("AGENT_HTTP_URL")
        .or_else(|_| std::env::var("AGENT_URL"))
        .unwrap_or_else(|_| format!("http://{}:{}", args.host, args.agent_http_port));

    let ws_url = std::env::var("AGENT_WS_URL")
        .unwrap_or_else(|_| format!("ws://{}:{}", args.host, args.agent_ws_port));

    println!(
        "📍 Frontend URL: http://{}:{}",
        args.host, args.frontend_port
    );
    println!("🔗 Connecting to agent:");
    println!("   HTTP: {}", http_url);
    if args.frontend_use_websocket {
        println!("   WebSocket: {}", ws_url);
    }
    println!();

    let webhook_token = std::env::var("WEBHOOK_TOKEN").unwrap_or_else(|_| {
        let token = format!("wh_{}", Uuid::new_v4().simple());
        info!("Generated webhook token: {}", token);
        token
    });

    start_frontend_server(
        &args.host,
        args.frontend_port,
        http_url,
        ws_url,
        args.frontend_use_websocket,
        webhook_token,
    )
    .await?;

    Ok(())
}

async fn start_all(args: Args) -> anyhow::Result<()> {
    println!("🔄 Starting Full Stack (Agent + Frontend)");
    println!("──────────────────────────────────────────");

    let config = load_agent_config(&args)?;

    // Print configuration
    println!("🤖 Agent Backend:");
    print_agent_info(&config, &args);
    println!();

    println!("🌐 Web Frontend:");
    println!("   📍 URL: http://{}:{}", args.host, args.frontend_port);
    println!(
        "   🔗 Auto-connected to: http://{}:{}",
        args.host, args.agent_http_port
    );
    if args.frontend_use_websocket {
        println!("   📡 WebSocket: ws://{}:{}", args.host, args.agent_ws_port);
    }
    println!();

    println!(
        "✅ Ready! Open http://{}:{} in your browser",
        args.host, args.frontend_port
    );
    println!();

    // Generate shared webhook token
    let webhook_token = std::env::var("WEBHOOK_TOKEN").unwrap_or_else(|_| {
        let token = format!("wh_{}", Uuid::new_v4().simple());
        info!("Generated webhook token: {}", token);
        token
    });

    // Build URLs for frontend to connect to agent
    let http_url = format!("http://{}:{}", args.host, args.agent_http_port);
    let ws_url = format!("ws://{}:{}", args.host, args.agent_ws_port);

    // Start both servers concurrently
    let agent_server = ReimbursementServer::from_config(config);

    let transport = args.transport.clone();
    let host = args.host.clone();
    let frontend_port = args.frontend_port;
    let frontend_use_websocket = args.frontend_use_websocket;

    let agent_future = async move {
        match transport.as_str() {
            "http" => agent_server
                .start_http()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            "websocket" | "ws" => agent_server
                .start_websocket()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            "both" | "all" => agent_server
                .start_all()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            _ => {
                eprintln!("❌ Invalid transport: {}", transport);
                std::process::exit(1);
            }
        }
    };

    // Give agent server a moment to start before starting frontend
    let frontend_future = async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        start_frontend_server(
            &host,
            frontend_port,
            http_url,
            ws_url,
            frontend_use_websocket,
            webhook_token,
        )
        .await
    };

    // Run both servers concurrently using tokio::select
    // If either one completes/errors, we propagate that
    tokio::select! {
        result = agent_future => result?,
        result = frontend_future => result?,
    }

    Ok(())
}

fn load_agent_config(args: &Args) -> anyhow::Result<ServerConfig> {
    let mut config = if let Some(config_path) = &args.config {
        println!("📄 Loading agent config from: {}", config_path);
        // SAFETY: We're in single-threaded initialization, before any other threads
        // could be reading environment variables
        unsafe {
            std::env::set_var("CONFIG_FILE", config_path);
        }
        ServerConfig::load().map_err(|e| anyhow::anyhow!("{}", e))?
    } else {
        ServerConfig::from_env()
    };

    // Override config with command-line arguments
    config.host = args.host.clone();
    config.http_port = args.agent_http_port;
    config.ws_port = args.agent_ws_port;

    Ok(config)
}

fn print_agent_info(config: &ServerConfig, args: &Args) {
    println!("   📍 Host: {}", config.host);
    println!("   🔌 HTTP Port: {}", config.http_port);
    println!("   📡 WebSocket Port: {}", config.ws_port);
    println!("   ⚙️  Transport: {}", args.transport);

    match &config.storage {
        a2a_agents::reimbursement_agent::StorageConfig::InMemory => {
            println!("   💾 Storage: In-memory (non-persistent)");
        }
        a2a_agents::reimbursement_agent::StorageConfig::Sqlx { url, .. } => {
            println!("   💾 Storage: SQLx ({})", url);
        }
    }

    match &config.auth {
        AuthConfig::None => {
            println!("   🔓 Authentication: None (public access)");
        }
        AuthConfig::BearerToken { tokens, format } => {
            println!(
                "   🔐 Authentication: Bearer token ({} token(s){})",
                tokens.len(),
                format
                    .as_ref()
                    .map(|f| format!(", format: {}", f))
                    .unwrap_or_default()
            );
        }
        AuthConfig::ApiKey {
            keys,
            location,
            name,
        } => {
            println!(
                "   🔐 Authentication: API key ({} {} '{}', {} key(s))",
                location,
                name,
                name,
                keys.len()
            );
        }
    }
}

async fn start_frontend_server(
    host: &str,
    port: u16,
    http_url: String,
    ws_url: String,
    use_websocket: bool,
    webhook_token: String,
) -> anyhow::Result<()> {
    let client = if use_websocket {
        info!("Using WebSocket client for subscriptions at {}", ws_url);
        info!("Using HTTP client for API calls at {}", http_url);
        WebA2AClient::new_with_websocket(http_url, ws_url)
    } else {
        info!("Using HTTP client at {}", http_url);
        WebA2AClient::new_http(http_url)
    };

    let state = AppState {
        client: Arc::new(client),
        webhook_token,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/tasks", get(tasks_page))
        .route("/expense/new", get(expense_form))
        .route("/expense/submit", post(submit_expense))
        .route("/chat/new", post(new_chat))
        .route("/chat/:task_id", get(chat_page))
        .route("/chat/:task_id/send", post(send_message))
        .route("/chat/:task_id/cancel", post(cancel_task))
        .route("/chat/:task_id/stream", get(stream_task))
        .route("/webhook/push-notification", post(handle_push_notification))
        .nest_service("/static", ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        .with_state(Arc::new(state));

    let addr = SocketAddr::from((
        host.parse::<std::net::IpAddr>()
            .unwrap_or_else(|_| std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        port,
    ));

    info!("Frontend server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

// Frontend route handlers (from frontend.rs)

async fn index() -> impl IntoResponse {
    let agent_url = std::env::var("AGENT_HTTP_URL")
        .or_else(|_| std::env::var("AGENT_URL"))
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    IndexTemplate { agent_url }
}

async fn expense_form(Query(query): Query<ExpenseQuery>) -> impl IntoResponse {
    ExpenseFormTemplate {
        category: query.expense_type,
    }
}

async fn submit_expense(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ExpenseSubmitForm>,
) -> Result<AxumResponse, AppError> {
    use a2a_rs::domain::{Message, Part, Role};

    let task_id = Uuid::new_v4().to_string();

    let expense_details = format!(
        "I need to submit an expense reimbursement:\n\n\
        Category: {}\n\
        Amount: ${}\n\
        Date: {}\n\
        {}\
        Description: {}\n\
        {}",
        form.category,
        form.amount,
        form.date,
        form.vendor
            .as_ref()
            .map(|v| format!("Vendor: {}\n", v))
            .unwrap_or_default(),
        form.description,
        form.project_code
            .as_ref()
            .map(|p| format!("Project/Cost Center: {}\n", p))
            .unwrap_or_default()
    );

    let message = Message {
        role: Role::User,
        parts: vec![Part::text(expense_details)],
        metadata: None,
        reference_task_ids: None,
        message_id: Uuid::new_v4().to_string(),
        task_id: Some(task_id.clone()),
        context_id: None,
        extensions: None,
        kind: "message".to_string(),
    };

    let response = state
        .client
        .http
        .send_task_message(&task_id, &message, None, Some(50))
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Failed to submit expense: {}", e)))?;

    info!(
        "Expense submitted for task {}, response state: {:?}",
        task_id, response.status.state
    );

    use a2a_rs::domain::{PushNotificationConfig, TaskPushNotificationConfig};

    let push_config = TaskPushNotificationConfig {
        task_id: task_id.clone(),
        push_notification_config: PushNotificationConfig {
            id: None,
            url: "http://localhost:3000/webhook/push-notification".to_string(),
            token: Some(state.webhook_token.clone()),
            authentication: None,
        },
    };

    match state
        .client
        .http
        .set_task_push_notification(&push_config)
        .await
    {
        Ok(_) => info!(
            "Push notification registered for expense task {} with authentication",
            task_id
        ),
        Err(e) => warn!("Failed to register push notification: {}", e),
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    Ok(axum::response::Redirect::to(&format!("/chat/{}", task_id)).into_response())
}

async fn new_chat(
    State(_state): State<Arc<AppState>>,
    Form(_form): Form<NewChatForm>,
) -> Result<AxumResponse, AppError> {
    let task_id = Uuid::new_v4().to_string();
    Ok(axum::response::Redirect::to(&format!("/chat/{}", task_id)).into_response())
}

async fn tasks_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TasksQuery>,
) -> Result<impl IntoResponse, AppError> {
    let mut params = ListTasksParams::default();

    if let Some(state_str) = &query.state {
        if let Ok(state) = serde_json::from_str::<TaskState>(&format!("\"{}\"", state_str)) {
            params.status = Some(state);
        }
    }

    params.page_size = query.limit.map(|l| l as i32).or(Some(50));

    let result = state
        .client
        .http
        .list_tasks(&params)
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Failed to list tasks: {}", e)))?;

    let tasks: Vec<TaskView> = result.tasks.into_iter().map(TaskView::from_task).collect();

    let template = TasksTemplate {
        tasks,
        filter_state: query.state,
        total_count: result.total_size as usize,
    };

    Ok(template)
}

async fn chat_page(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let mut retry_count = 0;
    let max_retries = 3;

    let (messages, task_state) = loop {
        match state.client.http.get_task(&task_id, Some(50)).await {
            Ok(task) => {
                info!(
                    "Retrieved task {} with {} history items",
                    task_id,
                    task.history.as_ref().map(|h| h.len()).unwrap_or(0)
                );

                let state = Some(format!("{:?}", task.status.state));
                let messages = task
                    .history
                    .unwrap_or_default()
                    .into_iter()
                    .map(MessageView::from_message_with_json_parsing)
                    .collect();
                break (messages, state);
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    warn!(
                        "Failed to get task {} after {} retries: {}",
                        task_id, max_retries, e
                    );
                    break (vec![], None);
                }
                info!(
                    "Task {} not found, retrying ({}/{})",
                    task_id, retry_count, max_retries
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        }
    };

    let template = ChatTemplate {
        task_id,
        messages,
        task_state,
    };
    Ok(template)
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<AxumResponse, AppError> {
    use a2a_rs::domain::{FileContent, Message, Part, Role};

    let mut task_id = String::new();
    let mut message_text = String::new();
    let mut parts = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Failed to read multipart field: {}", e)))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "task_id" => {
                task_id = field
                    .text()
                    .await
                    .map_err(|e| AppError(anyhow::anyhow!("Failed to read task_id: {}", e)))?;
            }
            "message" => {
                message_text = field
                    .text()
                    .await
                    .map_err(|e| AppError(anyhow::anyhow!("Failed to read message: {}", e)))?;
            }
            "receipt" => {
                let file_name = field.file_name().map(|s| s.to_string());
                let content_type = field.content_type().map(|s| s.to_string());
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError(anyhow::anyhow!("Failed to read file: {}", e)))?;

                if !data.is_empty() {
                    info!(
                        "Received file upload: name={:?}, type={:?}, size={} bytes",
                        file_name,
                        content_type,
                        data.len()
                    );

                    use base64::Engine;
                    let base64_data = base64::engine::general_purpose::STANDARD.encode(&data);

                    let file_part = Part::File {
                        file: FileContent {
                            bytes: Some(base64_data),
                            uri: None,
                            name: file_name,
                            mime_type: content_type,
                        },
                        metadata: None,
                    };
                    parts.push(file_part);
                }
            }
            _ => {
                warn!("Unknown form field: {}", name);
            }
        }
    }

    if !message_text.is_empty() {
        parts.insert(0, Part::text(message_text));
    }

    if task_id.is_empty() {
        return Err(AppError(anyhow::anyhow!("Missing task_id")));
    }

    if parts.is_empty() {
        return Err(AppError(anyhow::anyhow!("Message cannot be empty")));
    }

    let message = Message {
        role: Role::User,
        parts,
        metadata: None,
        reference_task_ids: None,
        message_id: Uuid::new_v4().to_string(),
        task_id: Some(task_id.clone()),
        context_id: None,
        extensions: None,
        kind: "message".to_string(),
    };

    let response = state
        .client
        .http
        .send_task_message(&task_id, &message, None, Some(50))
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Failed to send message: {}", e)))?;

    info!(
        "Message sent successfully for task {}, response has {} history items",
        task_id,
        response.history.as_ref().map(|h| h.len()).unwrap_or(0)
    );

    use a2a_rs::domain::{PushNotificationConfig, TaskPushNotificationConfig};

    let push_config = TaskPushNotificationConfig {
        task_id: task_id.clone(),
        push_notification_config: PushNotificationConfig {
            id: None,
            url: "http://localhost:3000/webhook/push-notification".to_string(),
            token: Some(state.webhook_token.clone()),
            authentication: None,
        },
    };

    match state
        .client
        .http
        .set_task_push_notification(&push_config)
        .await
    {
        Ok(_) => info!(
            "Push notification registered for task {} with authentication",
            task_id
        ),
        Err(e) => warn!(
            "Failed to register push notification for task {}: {}",
            task_id, e
        ),
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    Ok(axum::response::Redirect::to(&format!("/chat/{}", task_id)).into_response())
}

async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Result<AxumResponse, AppError> {
    state
        .client
        .http
        .cancel_task(&task_id)
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Failed to cancel task: {}", e)))?;

    Ok(axum::response::Redirect::to(&format!("/chat/{}", task_id)).into_response())
}

async fn stream_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> axum::response::sse::Sse<
    impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    create_sse_stream(state.client.clone(), task_id)
}

async fn handle_push_notification(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(event): axum::Json<TaskStatusUpdateEvent>,
) -> Result<AxumResponse, AppError> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    let authenticated = match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..];
            token == state.webhook_token
        }
        _ => false,
    };

    if !authenticated {
        warn!(
            "Unauthorized push notification attempt for task {}",
            event.task_id
        );
        return Err(AppError(anyhow::anyhow!("Unauthorized")));
    }

    info!(
        "✅ Authenticated push notification for task {}: state={:?}",
        event.task_id, event.status.state
    );

    Ok(axum::response::Json(serde_json::json!({
        "status": "received",
        "task_id": event.task_id,
        "authenticated": true
    }))
    .into_response())
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
