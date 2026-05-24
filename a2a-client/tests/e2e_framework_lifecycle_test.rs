use a2a_client::WebA2AClient;
use a2a_rs::domain::{A2AError, Message, Task, TaskState};
use a2a_rs::port::AsyncMessageHandler;
use a2a_rs::services::AsyncA2AClient;
use a2a_rs::adapter::{DefaultRequestProcessor, HttpServer, InMemoryTaskStorage, SimpleAgentInfo};
use a2a_agents::{AgentBuilder, AgentPlugin, SkillDefinition};
use async_trait::async_trait;
use std::time::Duration;
use tokio::sync::oneshot;

use a2a_rs::port::AsyncTaskManager;

/// Simple mock agent for E2E tests
#[derive(Clone)]
struct EchoAgent {
    storage: InMemoryTaskStorage,
}

impl AgentPlugin for EchoAgent {
    fn name(&self) -> &str {
        "Echo E2E Agent"
    }
    
    fn description(&self) -> &str {
        "E2E Test Agent"
    }
    
    fn skills(&self) -> Vec<SkillDefinition> {
        vec![]
    }
}

#[async_trait]
impl AsyncMessageHandler for EchoAgent {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        // Find text in message
        let mut text = String::new();
        for part in &message.parts {
            if let Some(a2a_rs::domain::part::Content::Text(t)) = &part.content {
                text = t.clone();
                break;
            }
        }
        
        let reply_text = if text.is_empty() {
            "No message received".to_string()
        } else {
            format!("Echo: {}", text)
        };

        // Create or get the task using storage
        let task = if !self.storage.task_exists(task_id).await? {
            self.storage.create_task(task_id, "context-1").await?
        } else {
            self.storage.get_task(task_id, None).await?
        };
        
        let reply_msg = Message::agent_text(reply_text, "msg-res-1".to_string());
        
        self.storage.update_task_status(
            task_id,
            TaskState::Completed,
            Some(reply_msg.clone()),
        ).await?;

        let mut t = task;
        t.update_status(TaskState::Completed, Some(reply_msg));
        Ok(t)
    }
}

#[tokio::test]
async fn test_framework_lifecycle_e2e() {
    // 1. Boot Agent Server
    let agent_info = SimpleAgentInfo::new(
        "Echo E2E Agent".to_string(),
        "http://localhost:8185".to_string(),
    );
    let storage = InMemoryTaskStorage::new();

    let processor = DefaultRequestProcessor::new(
        EchoAgent { storage: storage.clone() },
        storage.clone(),
        storage,
        agent_info.clone(),
    );
    let server = HttpServer::new(processor, agent_info, "127.0.0.1:8185".to_string());
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // 2. Client connecting via WebA2AClient
    let client = WebA2AClient::new_http("http://localhost:8185".to_string());

    // 3. Client submit task
    let task_id = "test-e2e-task-1".to_string();
    let message = Message::user_text("Hello Framework!".to_string(), "msg-1".to_string());
    
    let task = client
        .http
        .send_task_message(&task_id, &message, None, None)
        .await
        .expect("Failed to send task");
        
    assert_eq!(task.id, task_id);
    assert!(task.status.state == TaskState::Working || task.status.state == TaskState::Completed);

    // Give it a moment to process the task since it is async behind the scenes
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. Client fetches final result
    let final_task = client
        .http
        .get_task(&task_id, None)
        .await
        .expect("Failed to fetch task");

    assert_eq!(final_task.status.state, TaskState::Completed);
    let last_reply = final_task.history.last().unwrap();
    
    let mut received_text = String::new();
    for part in &last_reply.parts {
        if let Some(a2a_rs::domain::part::Content::Text(t)) = &part.content {
            received_text = t.clone();
            break;
        }
    }
    
    assert_eq!(received_text, "Echo: Hello Framework!");

    // 5. Cleanup
    let _ = shutdown_tx.send(());
    let _ = server_handle.await;
}
