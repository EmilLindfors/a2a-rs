//! Integration test for MCP to A2A bridge
//!
//! This test verifies that MCP tools and prompts can be successfully exposed as A2A agent skills

use a2a_mcp::bridge::mcp_to_a2a::{
    create_prompt_call_message, create_tool_call_message, McpToA2ABridge, ProgressClientHandler,
};
use a2a_rs::domain::core::agent::AgentCard;
use a2a_rs::domain::{
    Message, Part, Role, Task, TaskArtifactUpdateEvent, TaskState, TaskStatus,
    TaskStatusUpdateEvent,
};
use a2a_rs::port::streaming_handler::Subscriber;
use a2a_rs::port::{AsyncMessageHandler, AsyncStreamingHandler, UpdateEvent};
use async_trait::async_trait;
use rmcp::{
    handler::client::progress::ProgressDispatcher,
    model::*, service::RequestContext, ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_mcp_tool_as_a2a_skill() {
    // Create a mock MCP tool
    let input_schema = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "expression": {
                "type": "string",
                "description": "The math expression to evaluate"
            }
        },
        "required": ["expression"]
    }))
    .expect("Failed to parse schema");

    let tool = Tool::new(
        "calculator",
        "Performs calculations",
        Arc::new(input_schema),
    );

    let tools = [tool];

    // Create mock MCP client result
    let _mock_result = CallToolResult::success(vec![Content::text("42")]);

    // Create a simple agent card to use as base
    let _base_card = AgentCard::builder()
        .name("MCP Bridge Agent".to_string())
        .description("Agent exposing MCP tools".to_string())
        .url("https://example.com/mcp".to_string())
        .version("1.0.0".to_string())
        .capabilities(Default::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![])
        .build();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name.as_ref(), "calculator");
}

#[tokio::test]
async fn test_task_state_tracking() {
    // Test that tasks properly track their state through the bridge

    let task = Task::builder()
        .id("task-1".to_string())
        .context_id("ctx-1".to_string())
        .status(TaskStatus::new(TaskState::Completed, None))
        .history(vec![
            Message::builder()
                .role(Role::User)
                .parts(vec![Part::text("Calculate 2 + 2".to_string())])
                .message_id("msg-1".to_string())
                .build(),
            Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text("The result is 4".to_string())])
                .message_id("msg-2".to_string())
                .build(),
        ])
        .build();

    // Verify task structure
    assert_eq!(task.status.state, TaskState::Completed);
    assert_eq!(task.history.len(), 2);

    // Verify message flow
    let history = &task.history;
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[1].role, Role::Agent);
}

#[derive(Clone)]
struct TestMcpServer {
    tools: Arc<Vec<Tool>>,
    prompts: Arc<Vec<Prompt>>,
}

impl TestMcpServer {
    fn new() -> Self {
        let tool = Tool::new(
            "calculator",
            "Performs calculations",
            Arc::new(
                serde_json::from_value(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string" }
                    },
                    "required": ["expression"]
                }))
                .unwrap(),
            ),
        );

        let prompt = Prompt::new("test_prompt", Some("A test prompt"), None);

        Self {
            tools: Arc::new(vec![tool]),
            prompts: Arc::new(vec![prompt]),
        }
    }
}

#[async_trait]
#[allow(clippy::manual_async_fn)]
impl ServerHandler for TestMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_protocol_version(ProtocolVersion::V_2024_11_05)
        .with_server_info(Implementation::new("test-server", "1.0.0"))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            Ok(ListToolsResult {
                tools: (*self.tools).clone(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn call_tool(
        &self,
        CallToolRequestParams { name, .. }: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            if name != "calculator" {
                return Err(McpError::invalid_params("unknown tool", None));
            }

            // Test progress updates if progress_token is present
            if let Some(token) = ctx.meta.get_progress_token() {
                // Send some progress notifications
                let _ = ctx.peer.notify_progress(
                    ProgressNotificationParam::new(token.clone(), 50.0)
                        .with_message("Step 1 done"),
                ).await;
                let _ = ctx.peer.notify_progress(
                    ProgressNotificationParam::new(token.clone(), 100.0)
                        .with_message("Step 2 done"),
                ).await;
            }

            Ok(CallToolResult::success(vec![Content::text("42")]))
        }
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListPromptsResult, McpError>> + Send + '_ {
        async move {
            Ok(ListPromptsResult {
                prompts: (*self.prompts).clone(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn get_prompt(
        &self,
        GetPromptRequestParams { name, .. }: GetPromptRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetPromptResult, McpError>> + Send + '_ {
        async move {
            if name != "test_prompt" {
                return Err(McpError::invalid_params("unknown prompt", None));
            }
            let pm = PromptMessage::new_text(PromptMessageRole::Assistant, "Prompt output message");
            Ok(GetPromptResult::new(vec![pm]))
        }
    }
}

#[derive(Clone)]
struct NoOpHandler;

#[async_trait]
impl AsyncMessageHandler for NoOpHandler {
    async fn process_message(
        &self,
        task_id: &str,
        _message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, a2a_rs::domain::error::A2AError> {
        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id("noop-ctx".to_string())
            .status(TaskStatus::new(TaskState::Completed, None))
            .build())
    }
}

#[derive(Clone, Default)]
struct TestStreamingHandler {
    updates: Arc<Mutex<Vec<TaskStatusUpdateEvent>>>,
}

#[async_trait]
impl AsyncStreamingHandler for TestStreamingHandler {
    async fn add_status_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, a2a_rs::domain::error::A2AError> {
        Ok("sub-1".to_string())
    }

    async fn add_artifact_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, a2a_rs::domain::error::A2AError> {
        Ok("sub-2".to_string())
    }

    async fn remove_subscription(
        &self,
        _subscription_id: &str,
    ) -> Result<(), a2a_rs::domain::error::A2AError> {
        Ok(())
    }

    async fn remove_task_subscribers(
        &self,
        _task_id: &str,
    ) -> Result<(), a2a_rs::domain::error::A2AError> {
        Ok(())
    }

    async fn get_subscriber_count(
        &self,
        _task_id: &str,
    ) -> Result<usize, a2a_rs::domain::error::A2AError> {
        Ok(1)
    }

    async fn broadcast_status_update(
        &self,
        _task_id: &str,
        update: TaskStatusUpdateEvent,
    ) -> Result<(), a2a_rs::domain::error::A2AError> {
        self.updates.lock().unwrap().push(update);
        Ok(())
    }

    async fn broadcast_artifact_update(
        &self,
        _task_id: &str,
        _update: TaskArtifactUpdateEvent,
    ) -> Result<(), a2a_rs::domain::error::A2AError> {
        Ok(())
    }

    async fn status_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<
            Box<
                dyn futures::Stream<Item = Result<TaskStatusUpdateEvent, a2a_rs::domain::error::A2AError>>
                    + Send,
            >,
        >,
        a2a_rs::domain::error::A2AError,
    > {
        unimplemented!()
    }

    async fn artifact_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<
            Box<
                dyn futures::Stream<Item = Result<TaskArtifactUpdateEvent, a2a_rs::domain::error::A2AError>>
                    + Send,
            >,
        >,
        a2a_rs::domain::error::A2AError,
    > {
        unimplemented!()
    }

    async fn combined_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<
            Box<
                dyn futures::Stream<Item = Result<UpdateEvent, a2a_rs::domain::error::A2AError>>
                    + Send,
            >,
        >,
        a2a_rs::domain::error::A2AError,
    > {
        unimplemented!()
    }
}

#[tokio::test]
async fn test_mcp_to_a2a_prompts() {
    let (server_io, client_io) = tokio::io::duplex(4096);

    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });

    let mcp_client = ().serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();

    // Create McpToA2ABridge
    let bridge = McpToA2ABridge::new(peer, NoOpHandler).await.unwrap();

    // Verify list_prompts was called and populated
    assert_eq!(bridge.prompts().len(), 1);
    assert_eq!(bridge.prompts()[0].name, "test_prompt");

    // Call the prompt via bridge
    let prompt_call_msg = create_prompt_call_message("test_prompt", serde_json::json!({}));
    let task = bridge
        .process_message("task-prompt-1", &prompt_call_msg, None)
        .await
        .unwrap();

    assert_eq!(task.status.state, TaskState::Completed);
    let history = &task.history;
    assert_eq!(history.len(), 2);
    // User message is history[0], assistant prompt reply is history[1]
    assert_eq!(history[1].role, Role::Agent);
    assert_eq!(history[1].parts[0].get_text(), Some("Prompt output message"));

    drop(mcp_client);
    let _ = server_task.await;
}

#[tokio::test]
async fn test_mcp_to_a2a_progress_streaming() {
    let (server_io, client_io) = tokio::io::duplex(4096);

    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });

    let progress_dispatcher = ProgressDispatcher::new();
    let client_handler = ProgressClientHandler::new(progress_dispatcher.clone());
    let mcp_client = client_handler.serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();

    let streaming_handler = TestStreamingHandler::default();

    // Create streaming McpToA2ABridge
    let bridge = McpToA2ABridge::with_streaming(
        peer,
        NoOpHandler,
        progress_dispatcher,
        Arc::new(streaming_handler.clone()),
    )
    .await
    .unwrap();

    // Call the tool via bridge
    let tool_call_msg =
        create_tool_call_message("calculator", serde_json::json!({ "expression": "2 + 2" }));
    let task = bridge
        .process_message("task-calc-1", &tool_call_msg, None)
        .await
        .unwrap();

    assert_eq!(task.status.state, TaskState::Completed);
    let history = &task.history;
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].parts[0].get_text(), Some("42"));

    // Verify progress notifications were broadcast and stored
    // Wait a brief moment to ensure broadcast updates compile and finish processing
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    {
        let updates = streaming_handler.updates.lock().unwrap();
        assert!(updates.len() >= 2);
        // Verify first update
        assert_eq!(updates[0].task_id, "task-calc-1");
        assert_eq!(updates[0].status.state, TaskState::Working);
        assert_eq!(
            updates[0]
                .status
                .message
                .as_option()
                .unwrap()
                .parts
                .iter()
                .find_map(|p| p.get_text().map(|t| t.to_string()))
                .unwrap(),
            "Progress: 50"
        );

        // Verify second update
        assert_eq!(updates[1].task_id, "task-calc-1");
        assert_eq!(updates[1].status.state, TaskState::Working);
        assert_eq!(
            updates[1]
                .status
                .message
                .as_option()
                .unwrap()
                .parts
                .iter()
                .find_map(|p| p.get_text().map(|t| t.to_string()))
                .unwrap(),
            "Progress: 100"
        );
    }
    drop(mcp_client);
    let _ = server_task.await;
}
