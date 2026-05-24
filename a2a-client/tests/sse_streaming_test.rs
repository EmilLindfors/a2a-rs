use a2a_agents::core::AgentBuilder;
use a2a_rs::{
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatusUpdateEvent},
    port::{AsyncMessageHandler, AsyncStreamingHandler},
    InMemoryTaskStorage,
    services::client::AsyncA2AClient,
};
use a2a_client::WebA2AClient;
use a2a_client::components::create_sse_stream;
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Clone)]
struct StreamingHandler {
    storage: Arc<InMemoryTaskStorage>,
}

#[async_trait]
impl AsyncMessageHandler for StreamingHandler {
    async fn process_message(
        &self,
        task_id: &str,
        _message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text("Working...".to_string())])
            .message_id("msg1".to_string())
            .build();

        let task = Task::builder()
            .id(task_id.to_string())
            .status(a2a_rs::domain::TaskStatus::new(
                TaskState::Working,
                Some(response.clone()),
            ))
            .build();

        // Spawn a background task to simulate a streaming delay
        let storage = self.storage.clone();
        let tid = task_id.to_string();
        
        tokio::spawn(async move {
            sleep(Duration::from_millis(1000)).await;
            let _ = storage.broadcast_status_update(&tid, TaskStatusUpdateEvent {
                task_id: tid.clone(),
                status: a2a_rs::domain::TaskStatus::new(TaskState::Completed, Some(response)),
                context_id: "".to_string(),
                kind: "status-update".to_string(),
                metadata: None,
            }).await;
        });

        Ok(task)
    }

    async fn validate_message(&self, _message: &Message) -> Result<(), A2AError> {
        Ok(())
    }
}

#[tokio::test]
#[ignore = "Requires HTTP/2 ConnectRPC streaming setup to run locally with axum"]
async fn test_sse_stream_success() {
    let toml_config = r#"
        [agent]
        name = "Test Agent"
        version = "1.0.0"

        [server]
        http_port = 19385
        
        [server.storage]
        type = "inmemory"
    "#;

    let storage = Arc::new(InMemoryTaskStorage::new());
    let handler = StreamingHandler { storage: storage.clone() };

    let runtime = AgentBuilder::from_toml(toml_config)
        .unwrap()
        .with_handler(handler)
        .with_storage((*storage).clone())
        .build()
        .unwrap();

    // Start server in background
    tokio::spawn(async move {
        let _ = runtime.run().await;
    });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    let client = Arc::new(WebA2AClient::new_http("http://127.0.0.1:19385".to_string()));
    
    // Create a task first
    let message = Message::builder()
        .role(Role::User)
        .parts(vec![Part::text("Start".to_string())])
        .message_id("start-msg".to_string())
        .build();

    let task: Task = client.http.send_task_message("test-task-1", &message, None, None).await.unwrap();
    assert_eq!(task.id, "test-task-1");

    // Check if subscribe_to_task works natively
    let mut native_stream = client.http.subscribe_to_task("test-task-1", None).await.unwrap();
    if let Some(item) = native_stream.next().await {
        println!("Native stream item: {:?}", item);
    } else {
        println!("Native stream ended without items");
    }

    // Now test SSE Stream
    // Note: Re-subscribing might only get new events, so we might need another task for SSE testing
    let task2 = client.http.send_task_message("test-task-2", &message, None, None).await.unwrap();
    assert_eq!(task2.id, "test-task-2");

    let sse = create_sse_stream(client.clone(), "test-task-2".to_string());
    
    use axum::response::IntoResponse;
    let mut response = sse.into_response();
    let body = response.into_body();
    
    // Read body chunks
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    let data = String::from_utf8(bytes.to_vec()).unwrap();
    
    println!("Received SSE data: {}", data);
    
    assert!(data.contains("event: task-update") || data.contains("event: task-status"));
}

