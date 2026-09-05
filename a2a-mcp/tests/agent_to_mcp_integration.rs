//! Integration test for A2A to MCP bridge
//!
//! This test verifies that A2A agent skills can be successfully exposed as MCP tools

use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use a2a_mcp::bridge::agent_to_mcp::AgentToMcpBridge;
use a2a_mcp::converters::skill_tool::SkillToolConverter;
use a2a_mcp::{SkillSchema, SkillSchemas};
use a2a_rs::adapter::transport::http::HttpClient;
use a2a_rs::domain::core::agent::{AgentCapabilities, AgentCard, AgentSkill};
use a2a_rs::domain::{
    Message, Part, Role, Task, TaskArtifactUpdateEvent, TaskState, TaskStatus,
    TaskStatusUpdateEvent, error::A2AError,
};
use a2a_rs::port::streaming_handler::Subscriber;
use a2a_rs::port::{AsyncMessageHandler, AsyncStreamingHandler, SeqEvent, UpdateEvent};
use async_trait::async_trait;
use rmcp::service::{NotificationContext, RequestContext};
use rmcp::{ClientHandler, ErrorData as McpError, RoleClient, ServerHandler, ServiceExt, model::*};

#[tokio::test]
async fn test_agent_skills_as_mcp_tools() {
    // Create an agent card with multiple skills
    let agent_card = AgentCard::builder()
        .name("Test Agent".to_string())
        .description("A test agent with multiple skills".to_string())
        .url("https://example.com/agent".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![
            AgentSkill::new(
                "calculator".to_string(),
                "Calculator".to_string(),
                "Performs mathematical calculations".to_string(),
                vec!["math".to_string(), "calculator".to_string()],
            ),
            AgentSkill::new(
                "translator".to_string(),
                "Translator".to_string(),
                "Translates text between languages".to_string(),
                vec!["translation".to_string(), "language".to_string()],
            ),
        ])
        .build();

    // Create HTTP client (won't be used in this test, but needed for bridge construction)
    let client = HttpClient::new("https://example.com/agent".to_string());

    // Create the bridge
    let bridge = AgentToMcpBridge::new(client, agent_card.clone());

    // Get the server info
    let info = bridge.get_info();

    // Verify server info
    assert_eq!(
        info.server_info.name,
        "a2a-mcp-bridge:Test Agent".to_string()
    );
    assert_eq!(
        info.server_info.title,
        Some("A2A Agent: Test Agent".to_string())
    );
    assert!(info.capabilities.tools.is_some());

    // Note: We can't easily test list_tools without a full MCP setup since RequestContext
    // doesn't implement Default. In practice, this would be tested with a real MCP client.
    // The bridge creation and tool conversion is verified through unit tests.
}

#[tokio::test]
async fn test_tool_name_namespacing() {
    // Test that tool names are properly namespaced to avoid collisions

    let agent_card = AgentCard::builder()
        .name("Agent A".to_string())
        .description("First agent".to_string())
        .url("https://agent-a.example.com".to_string())
        .version("1.0.0".to_string())
        .capabilities(Default::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "shared_skill".to_string(),
            "Shared Skill".to_string(),
            "A skill with a common name".to_string(),
            vec![],
        )])
        .build();

    // The bridge namespaces by the agent's *name*, so the same skill on two
    // agents gives two tools, and the name is a function name a model can use.
    let tool = SkillToolConverter::skill_to_tool(&agent_card.skills[0], &agent_card.name, None);
    assert_eq!(tool.name.as_ref(), "agent_a_shared_skill");

    // And a call is answered by matching the generated name, not parsing it.
    let resolved =
        SkillToolConverter::resolve_skill(&agent_card.skills, &agent_card.name, &tool.name);
    assert_eq!(resolved.map(|s| s.id.as_str()), Some("shared_skill"));
}

#[tokio::test]
async fn test_skill_metadata_preservation() {
    // Test that skill metadata (examples, input/output modes) is preserved in tool descriptions

    let skill = AgentSkill {
        id: "test_skill".to_string(),
        name: "Test Skill".to_string(),
        description: "A test skill".to_string(),
        tags: vec!["test".to_string()],
        examples: vec![
            "Example 1: Do something".to_string(),
            "Example 2: Do something else".to_string(),
        ],
        input_modes: vec!["text".to_string(), "file".to_string()],
        output_modes: vec!["text".to_string()],
        security_requirements: Vec::new(),
        ..Default::default()
    };

    let tool = SkillToolConverter::skill_to_tool(&skill, "example", None);

    // Verify description includes examples
    let description = tool.description.as_ref().unwrap();
    assert!(description.contains("Example 1"));
    assert!(description.contains("Example 2"));

    // Verify description includes input/output modes
    assert!(description.contains("Supported input modes: text, file"));
    assert!(description.contains("Supported output modes: text"));
}

#[tokio::test]
async fn test_multiple_agents_as_bridges() {
    // Test that we can create multiple bridges for different agents
    // This simulates an MCP server that exposes multiple A2A agents

    let agent1 = AgentCard::builder()
        .name("Agent 1".to_string())
        .description("First agent".to_string())
        .url("https://agent1.example.com".to_string())
        .version("1.0.0".to_string())
        .capabilities(Default::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "skill1".to_string(),
            "Skill 1".to_string(),
            "First skill".to_string(),
            vec![],
        )])
        .build();

    let agent2 = AgentCard::builder()
        .name("Agent 2".to_string())
        .description("Second agent".to_string())
        .url("https://agent2.example.com".to_string())
        .version("1.0.0".to_string())
        .capabilities(Default::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "skill2".to_string(),
            "Skill 2".to_string(),
            "Second skill".to_string(),
            vec![],
        )])
        .build();

    let client1 = HttpClient::new("https://agent1.example.com".to_string());
    let client2 = HttpClient::new("https://agent2.example.com".to_string());

    let bridge1 = AgentToMcpBridge::new(client1, agent1);
    let bridge2 = AgentToMcpBridge::new(client2, agent2);

    // Verify each bridge has different server info (representing different agents)
    let info1 = bridge1.get_info();
    let info2 = bridge2.get_info();

    assert_ne!(info1.server_info.name, info2.server_info.name);
    assert_ne!(info1.server_info.website_url, info2.server_info.website_url);
}

/// Counts how many times the handler is invoked so we can prove the in-process
/// path actually dispatches to it (and not through some hidden HTTP fallback).
#[derive(Clone)]
struct CountingHandler {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl AsyncMessageHandler for CountingHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _ctx: &a2a_rs::port::RequestContext,
    ) -> Result<Task, A2AError> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        let echoed = message
            .parts
            .iter()
            .filter_map(|p| p.get_text())
            .collect::<Vec<_>>()
            .join(" ");
        let agent_msg = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(format!("in-process: {echoed}"))])
            .message_id("resp-1".to_string())
            .build();

        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id("ctx-1".to_string())
            .status(TaskStatus::new(TaskState::Completed, None))
            .history(vec![message.clone(), agent_msg])
            .build())
    }
}

/// Cover the in-process backend end-to-end: MCP client → AgentToMcpBridge
/// (built via `with_handler`) → handler is called directly with no HTTP hop.
#[tokio::test]
async fn test_in_process_backend_dispatches_to_handler() {
    let calls = Arc::new(AtomicUsize::new(0));
    let handler = CountingHandler {
        calls: Arc::clone(&calls),
    };

    let agent_card = AgentCard::builder()
        .name("In-Process Agent".to_string())
        .description("Exercises the in-process backend".to_string())
        // Use a URL that's not bound to any listener — proves there's no
        // HTTP traffic involved in the in-process path.
        .url("http://nonroutable.invalid".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "echo".to_string(),
            "Echo".to_string(),
            "Echoes the input".to_string(),
            vec![],
        )])
        .build();

    let bridge = AgentToMcpBridge::with_handler(handler, agent_card);

    // Pair the bridge with an MCP client over an in-memory duplex transport.
    let (server_io, client_io) = tokio::io::duplex(4096);
    let bridge_task = tokio::spawn(async move {
        let running = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });
    let mcp_client = ().serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();

    // Discover the tool and call it.
    let tools = peer.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 1);
    let tool_name = tools.tools[0].name.to_string();
    assert!(tool_name.ends_with("echo"));

    let params = CallToolRequestParams::new(tool_name).with_arguments(
        serde_json::json!({ "message": "hello in-process" })
            .as_object()
            .cloned()
            .unwrap(),
    );
    let result = peer.call_tool(params).await.unwrap();

    // Handler must have been invoked exactly once — no HTTP loopback.
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!result.is_error.unwrap_or(false));
    let text = result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
        .expect("tool result has text content");
    assert!(text.contains("in-process: hello in-process"), "got: {text}");

    drop(mcp_client);
    let _ = bridge_task.await;
}

/// An MCP client that declares elicitation and answers the agent's question
/// the way a person at the client would.
/// Answers a typed query with a data part, and remembers what it was asked.
#[derive(Clone)]
struct TypedQueryHandler {
    received: Arc<Mutex<Vec<Message>>>,
}

#[async_trait]
impl AsyncMessageHandler for TypedQueryHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _ctx: &a2a_rs::port::RequestContext,
    ) -> Result<Task, A2AError> {
        self.received.lock().unwrap().push(message.clone());
        let rows: ::buffa_types::google::protobuf::Value =
            serde_json::from_value(serde_json::json!({"rows": [{"year": 2025, "tonnes": 12.5}]}))
                .unwrap();
        let agent_msg = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text("1 row".to_string()), Part::data(rows)])
            .message_id("resp-typed".to_string())
            .build();
        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id("ctx-typed".to_string())
            .status(TaskStatus::new(TaskState::Completed, None))
            .history(vec![message.clone(), agent_msg])
            .build())
    }
}

/// A skill with a schema on the card's extension is served as a typed tool:
/// its own input schema (plus `task_id`), its output schema and hints; a call
/// reaches the agent as one data part; and the agent's data part comes back
/// as `structuredContent`.
#[tokio::test]
async fn a_typed_skill_round_trips_through_the_bridge() {
    use a2a_rs::domain::generated::part;

    let mut schemas = SkillSchemas::new();
    schemas.insert(
        "query",
        SkillSchema::new()
            .with_input_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "view": {"type": "string", "enum": ["harvest", "feed"]},
                    "year": {"type": "integer"}
                },
                "required": ["view"]
            }))
            .with_output_schema(serde_json::json!({"type": "object"}))
            .with_annotations(a2a_llm::ToolAnnotations::new().read_only(true)),
    );
    let mut agent_card = AgentCard::builder()
        .name("strata".to_string())
        .description("A data agent".to_string())
        .url("http://nonroutable.invalid".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![
            AgentSkill::new(
                "query".to_string(),
                "Query".to_string(),
                "Queries a view".to_string(),
                vec!["data".to_string()],
            ),
            AgentSkill::new(
                "chat".to_string(),
                "Chat".to_string(),
                "Untyped, takes a message".to_string(),
                vec!["chat".to_string()],
            ),
        ])
        .build();
    agent_card
        .capabilities
        .get_or_insert_default()
        .extensions
        .push(schemas.to_extension());

    let received = Arc::new(Mutex::new(Vec::new()));
    let handler = TypedQueryHandler {
        received: received.clone(),
    };
    let bridge = AgentToMcpBridge::with_handler(handler, agent_card);

    let (server_io, client_io) = tokio::io::duplex(8192);
    let bridge_task = tokio::spawn(async move {
        let running = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });
    let mcp_client = ().serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();

    let tools = peer.list_tools(None).await.unwrap().tools;
    let query = tools
        .iter()
        .find(|t| t.name == "strata_query")
        .expect("typed tool");
    assert_eq!(
        query.input_schema["properties"]["view"]["enum"][0],
        "harvest"
    );
    assert_eq!(
        query.input_schema["properties"]["task_id"]["type"],
        "string"
    );
    assert!(query.input_schema["properties"].get("message").is_none());
    assert!(query.output_schema.is_some());
    assert_eq!(
        query.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );
    let chat = tools
        .iter()
        .find(|t| t.name == "strata_chat")
        .expect("untyped tool");
    assert!(chat.input_schema["properties"].get("message").is_some());
    assert!(chat.output_schema.is_none());

    let params = CallToolRequestParams::new("strata_query").with_arguments(
        serde_json::json!({ "view": "harvest", "year": 2025 })
            .as_object()
            .cloned()
            .unwrap(),
    );
    let result = peer.call_tool(params).await.unwrap();
    assert!(!result.is_error.unwrap_or(false));

    // The agent received the arguments as one data part, not as text.
    let messages = received.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].parts.len(), 1);
    match &messages[0].parts[0].content {
        Some(part::Content::Data(value)) => {
            let value = serde_json::to_value(&**value).unwrap();
            assert_eq!(value["view"], "harvest");
            assert_eq!(value["year"], 2025.0);
            assert!(
                value.get("task_id").is_none(),
                "task_id is the bridge's, not the agent's"
            );
        }
        other => panic!("expected a data part, got {other:?}"),
    }

    // And the agent's data part is the structured result.
    let structured = result.structured_content.expect("structured content");
    assert_eq!(structured["rows"][0]["year"], 2025.0);
    assert!(
        result
            .content
            .iter()
            .any(|c| c.as_text().is_some_and(|t| t.text == "1 row"))
    );

    drop(mcp_client);
    let _ = bridge_task.await;
}

#[derive(Clone, Default)]
struct TestClientHandler {
    progress_notifications: Arc<Mutex<Vec<ProgressNotificationParam>>>,
}

#[allow(clippy::manual_async_fn)]
impl ClientHandler for TestClientHandler {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::new(
            ClientCapabilities::builder().enable_elicitation().build(),
            Implementation::new("test-client", "1.0.0"),
        )
    }

    fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> impl std::future::Future<Output = Result<ElicitResult, McpError>> + Send + '_ {
        async move {
            let ElicitRequestParams::FormElicitationParams { message, .. } = request else {
                return Err(McpError::invalid_params(
                    "expected a form elicitation",
                    None,
                ));
            };
            if !message.contains("Please provide input:") {
                return Err(McpError::invalid_params(
                    "Unexpected elicitation message",
                    None,
                ));
            }
            let mut result = ElicitResult::new(ElicitationAction::Accept);
            result.content = Some(serde_json::json!({"answer": "sampled response: 42"}));
            Ok(result)
        }
    }

    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        async move {
            self.progress_notifications.lock().unwrap().push(params);
        }
    }
}

struct MockStreamingHandler;

#[async_trait]
impl AsyncMessageHandler for MockStreamingHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _ctx: &a2a_rs::port::RequestContext,
    ) -> Result<Task, A2AError> {
        let text = message
            .parts
            .iter()
            .filter_map(|p| p.get_text().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join("\n");

        if text.contains("sampled response: 42") {
            let final_msg = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text("Final result with 42".to_string())])
                .message_id("final-msg".to_string())
                .build();
            Ok(Task::builder()
                .id(task_id.to_string())
                .context_id("ctx-1".to_string())
                .status(TaskStatus::new(
                    TaskState::Completed,
                    Some(final_msg.clone()),
                ))
                .history(vec![message.clone(), final_msg])
                .build())
        } else {
            Ok(Task::builder()
                .id(task_id.to_string())
                .context_id("ctx-1".to_string())
                .status(TaskStatus::new(TaskState::Working, None))
                .history(vec![message.clone()])
                .build())
        }
    }
}

#[async_trait]
impl AsyncStreamingHandler for MockStreamingHandler {
    async fn add_status_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Ok("sub-id".to_string())
    }

    async fn add_artifact_subscriber(
        &self,
        _task_id: &str,
        _subscriber: Box<dyn Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<String, A2AError> {
        Ok("sub-id".to_string())
    }

    async fn remove_subscription(&self, _subscription_id: &str) -> Result<(), A2AError> {
        Ok(())
    }

    async fn remove_task_subscribers(&self, _task_id: &str) -> Result<(), A2AError> {
        Ok(())
    }

    async fn get_subscriber_count(&self, _task_id: &str) -> Result<usize, A2AError> {
        Ok(1)
    }

    async fn broadcast_status_update(
        &self,
        _task_id: &str,
        _update: TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        Ok(())
    }

    async fn broadcast_artifact_update(
        &self,
        _task_id: &str,
        _update: TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        Ok(())
    }

    async fn status_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<TaskStatusUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        todo!()
    }

    async fn artifact_update_stream(
        &self,
        _task_id: &str,
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<TaskArtifactUpdateEvent, A2AError>> + Send>>,
        A2AError,
    > {
        todo!()
    }

    async fn combined_update_stream(
        &self,
        task_id: &str,
        _from_event_id: Option<u64>,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<SeqEvent, A2AError>> + Send>>, A2AError>
    {
        let task_id = task_id.to_string();
        let events = vec![
            Ok(SeqEvent::new(
                1,
                UpdateEvent::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: task_id.clone(),
                    context_id: "ctx-1".to_string(),
                    kind: "status-update".to_string(),
                    status: TaskStatus::new(
                        TaskState::Working,
                        Some(
                            Message::builder()
                                .role(Role::Agent)
                                .parts(vec![Part::text("Doing step 1".to_string())])
                                .message_id("step-1-msg".to_string())
                                .build(),
                        ),
                    ),
                    metadata: None,
                }),
            )),
            Ok(SeqEvent::new(
                2,
                UpdateEvent::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: task_id.clone(),
                    context_id: "ctx-1".to_string(),
                    kind: "status-update".to_string(),
                    status: TaskStatus::new(
                        TaskState::InputRequired,
                        Some(
                            Message::builder()
                                .role(Role::Agent)
                                .parts(vec![Part::text("Please provide input:".to_string())])
                                .message_id("elicitation-msg".to_string())
                                .build(),
                        ),
                    ),
                    metadata: None,
                }),
            )),
        ];

        Ok(Box::pin(futures::stream::iter(events)))
    }
}

#[tokio::test]
async fn test_streaming_progress_and_elicitation() {
    let agent_card = AgentCard::builder()
        .name("Streaming Agent".to_string())
        .description("Exercises streaming, progress and elicitation".to_string())
        .url("http://nonroutable.invalid".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "streamskill".to_string(),
            "Stream Skill".to_string(),
            "A skill that streams progress".to_string(),
            vec![],
        )])
        .build();

    let bridge = AgentToMcpBridge::with_handler_and_streaming(
        MockStreamingHandler,
        MockStreamingHandler,
        agent_card,
    );

    let (server_io, client_io) = tokio::io::duplex(8192);
    let bridge_task = tokio::spawn(async move {
        let running: rmcp::service::RunningService<_, _> = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });

    let client_handler = TestClientHandler::default();
    let progress_notifications = client_handler.progress_notifications.clone();
    let mcp_client = client_handler.serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();

    // Discover the tool
    let tools = peer.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 1);
    let tool_name = tools.tools[0].name.to_string();
    assert!(tool_name.ends_with("streamskill"));

    // Prepare call with progress token
    let token = ProgressToken(NumberOrString::String(Arc::from("test-progress-token")));
    let meta = RequestMetaObject::with_progress_token(token);
    let mut params = CallToolRequestParams::new(tool_name).with_arguments(
        serde_json::json!({ "message": "hello streaming" })
            .as_object()
            .cloned()
            .unwrap(),
    );
    params.meta = Some(meta);

    let result = peer.call_tool(params).await.unwrap();

    // Check final result
    assert!(!result.is_error.unwrap_or(false));
    let text = result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
        .expect("tool result has text content");
    assert!(text.contains("Final result with 42"), "got: {text}");

    // Verify progress notifications were received by client
    {
        let progress = progress_notifications.lock().unwrap();
        assert!(
            !progress.is_empty(),
            "Should have received progress notifications"
        );

        // We expect progress values of 50.0 (Working) and 75.0 (InputRequired)
        let progress_vals: Vec<f64> = progress.iter().map(|p| p.progress).collect();
        assert!(
            progress_vals.contains(&50.0),
            "Expected 50.0 in {:?}",
            progress_vals
        );
        assert!(
            progress_vals.contains(&75.0),
            "Expected 75.0 in {:?}",
            progress_vals
        );
    }

    drop(mcp_client);
    let _ = bridge_task.await;
}

/// A client that cannot be asked — no elicitation capability — gets the task
/// back suspended, with its id and the instruction to call again, instead of
/// an answer invented on the user's behalf. This was the fallback when
/// sampling was unavailable and is the only fallback now that it is gone.
#[tokio::test]
async fn a_client_without_elicitation_gets_the_task_suspended() {
    let agent_card = AgentCard::builder()
        .name("Streaming Agent".to_string())
        .description("Exercises the suspend path".to_string())
        .url("http://nonroutable.invalid".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "streamskill".to_string(),
            "Stream Skill".to_string(),
            "A skill that streams progress".to_string(),
            vec![],
        )])
        .build();

    let bridge = AgentToMcpBridge::with_handler_and_streaming(
        MockStreamingHandler,
        MockStreamingHandler,
        agent_card,
    );

    let (server_io, client_io) = tokio::io::duplex(8192);
    let bridge_task = tokio::spawn(async move {
        let running: rmcp::service::RunningService<_, _> = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });

    // `()` is a client with default info: no elicitation.
    let mcp_client = ().serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();
    let tool_name = peer.list_tools(None).await.unwrap().tools[0]
        .name
        .to_string();

    let params = CallToolRequestParams::new(tool_name).with_arguments(
        serde_json::json!({ "message": "hello" })
            .as_object()
            .cloned()
            .unwrap(),
    );
    let result = peer.call_tool(params).await.unwrap();

    let text = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Please provide input:"), "got: {text}");
    assert!(
        text.contains("[Task suspended awaiting input"),
        "got: {text}"
    );
    assert!(text.contains("`task_id`"), "got: {text}");
    assert!(
        !text.contains("Final result"),
        "nothing was answered for the user: {text}"
    );

    drop(mcp_client);
    let _ = bridge_task.await;
}

#[tokio::test]
async fn test_polling_progress_and_elicitation() {
    let agent_card = AgentCard::builder()
        .name("Polling Agent".to_string())
        .description("Exercises polling, progress and elicitation".to_string())
        .url("http://nonroutable.invalid".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "pollskill".to_string(),
            "Poll Skill".to_string(),
            "A skill that polls progress".to_string(),
            vec![],
        )])
        .build();

    let backend = Arc::new(MockPollingBackend::new());
    let bridge = AgentToMcpBridge::from_backend(
        backend.clone(),
        agent_card.clone(),
        agent_card.url().to_string(),
    );

    let (server_io, client_io) = tokio::io::duplex(8192);
    let bridge_task = tokio::spawn(async move {
        let running = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });

    let client_handler = TestClientHandler::default();
    let progress_notifications = client_handler.progress_notifications.clone();
    let mcp_client = client_handler.serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();

    // Discover the tool
    let tools = peer.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 1);
    let tool_name = tools.tools[0].name.to_string();
    assert!(tool_name.ends_with("pollskill"));

    // Prepare call with progress token
    let token = ProgressToken(NumberOrString::String(Arc::from("test-progress-token")));
    let meta = RequestMetaObject::with_progress_token(token);
    let mut params = CallToolRequestParams::new(tool_name).with_arguments(
        serde_json::json!({ "message": "hello polling" })
            .as_object()
            .cloned()
            .unwrap(),
    );
    params.meta = Some(meta);

    let result = peer.call_tool(params).await.unwrap();

    // Check final result
    assert!(!result.is_error.unwrap_or(false));
    let text = result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
        .expect("tool result has text content");
    assert!(text.contains("Final polled result with 42"), "got: {text}");

    // Verify progress notifications were received by client during polling loop
    {
        let progress = progress_notifications.lock().unwrap();
        assert!(
            !progress.is_empty(),
            "Should have received progress notifications"
        );

        // Check that we got polling-based progress messages
        let progress_msgs: Vec<String> =
            progress.iter().filter_map(|p| p.message.clone()).collect();
        assert!(
            progress_msgs
                .iter()
                .any(|msg| msg.contains("Polling task status")),
            "Expected polling status messages in {:?}",
            progress_msgs
        );
    }

    drop(mcp_client);
    let _ = bridge_task.await;
}

struct MockPollingBackend {
    invocations: Mutex<Vec<(String, Message)>>,
    poll_count: AtomicUsize,
}

impl MockPollingBackend {
    fn new() -> Self {
        Self {
            invocations: Mutex::new(Vec::new()),
            poll_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl a2a_mcp::bridge::agent_to_mcp::BridgeBackend for MockPollingBackend {
    async fn invoke(
        &self,
        task_id: &str,
        message: &Message,
        _session_id: Option<&str>,
    ) -> Result<Task, A2AError> {
        self.invocations
            .lock()
            .unwrap()
            .push((task_id.to_string(), message.clone()));

        let text = message
            .parts
            .iter()
            .filter_map(|p| p.get_text().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join("\n");

        if text.contains("sampled response: 42") {
            let final_msg = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text("Final polled result with 42".to_string())])
                .message_id("final-msg".to_string())
                .build();
            Ok(Task::builder()
                .id(task_id.to_string())
                .context_id("ctx-1".to_string())
                .status(TaskStatus::new(
                    TaskState::Completed,
                    Some(final_msg.clone()),
                ))
                .history(vec![message.clone(), final_msg])
                .build())
        } else {
            Ok(Task::builder()
                .id(task_id.to_string())
                .context_id("ctx-1".to_string())
                .status(TaskStatus::new(TaskState::Working, None))
                .history(vec![message.clone()])
                .build())
        }
    }

    async fn subscribe(
        &self,
        _task_id: &str,
    ) -> Result<
        Option<Pin<Box<dyn futures::Stream<Item = Result<a2a_rs::StreamItem, A2AError>> + Send>>>,
        A2AError,
    > {
        // Return Ok(None) to force polling fallback
        Ok(None)
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<Task>, A2AError> {
        let count = self.poll_count.fetch_add(1, Ordering::SeqCst);
        let invs = self.invocations.lock().unwrap().clone();

        // Build history from invocations
        let mut history = Vec::new();
        for (_, msg) in invs {
            history.push(msg);
        }

        // On first get_task call, return InputRequired to trigger elicitation
        if count == 0 {
            let req_msg = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text("Please provide input:".to_string())])
                .message_id("elicitation-msg".to_string())
                .build();
            Ok(Some(
                Task::builder()
                    .id(task_id.to_string())
                    .context_id("ctx-1".to_string())
                    .status(TaskStatus::new(TaskState::InputRequired, Some(req_msg)))
                    .history(history)
                    .build(),
            ))
        } else {
            // Subsequent calls return Completed after invocation updates state
            let final_msg = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text("Final polled result with 42".to_string())])
                .message_id("final-msg".to_string())
                .build();
            history.push(final_msg.clone());
            Ok(Some(
                Task::builder()
                    .id(task_id.to_string())
                    .context_id("ctx-1".to_string())
                    .status(TaskStatus::new(TaskState::Completed, Some(final_msg)))
                    .history(history)
                    .build(),
            ))
        }
    }
}

#[tokio::test]
async fn test_list_prompts_and_get_prompt() {
    let agent_card = AgentCard::builder()
        .name("Prompt Agent".to_string())
        .description("Test agent for prompts".to_string())
        .url("http://nonroutable.invalid".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "echo".to_string(),
            "Echo".to_string(),
            "Echoes the input".to_string(),
            vec![],
        )])
        .build();

    let bridge = AgentToMcpBridge::with_handler(
        CountingHandler {
            calls: Arc::new(AtomicUsize::new(0)),
        },
        agent_card,
    );

    let (server_io, client_io) = tokio::io::duplex(4096);
    let bridge_task = tokio::spawn(async move {
        let running = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });
    let mcp_client = ().serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();

    // 1. Test list_prompts
    let list_res = peer.list_prompts(None).await.unwrap();
    assert_eq!(list_res.prompts.len(), 1);
    let prompt_name = &list_res.prompts[0].name;
    assert!(prompt_name.ends_with("echo"));
    assert_eq!(list_res.prompts[0].title, Some("Echo".to_string()));

    // 2. Test get_prompt
    let args = serde_json::json!({ "message": "hello prompts" })
        .as_object()
        .cloned()
        .unwrap();
    let get_params = GetPromptRequestParams::new(prompt_name.clone()).with_arguments(args);
    let get_res = peer.get_prompt(get_params).await.unwrap();
    assert_eq!(get_res.messages.len(), 2);
    assert_eq!(get_res.messages[0].role, rmcp::model::Role::User);
    if let ContentBlock::Text(text) = &get_res.messages[0].content {
        assert_eq!(text.text, "hello prompts");
    } else {
        panic!("expected text content");
    }

    assert_eq!(get_res.messages[1].role, rmcp::model::Role::Assistant);
    if let ContentBlock::Text(text) = &get_res.messages[1].content {
        assert!(text.text.contains("in-process: hello prompts"));
    } else {
        panic!("expected text content");
    }

    drop(mcp_client);
    let _ = bridge_task.await;
}

#[tokio::test]
async fn test_list_resources_and_read_resource() {
    use a2a_rs::domain::Artifact;

    let agent_card = AgentCard::builder()
        .name("Resource Agent".to_string())
        .description("Test agent for resources".to_string())
        .url("http://nonroutable.invalid".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "generate".to_string(),
            "Generate Report".to_string(),
            "Generates a report".to_string(),
            vec![],
        )])
        .build();

    // Create a custom handler that returns a task with artifacts
    #[derive(Clone)]
    struct ResourceHandler;

    #[async_trait]
    impl AsyncMessageHandler for ResourceHandler {
        async fn process_message(
            &self,
            task_id: &str,
            _message: &Message,
            _ctx: &a2a_rs::port::RequestContext,
        ) -> Result<Task, A2AError> {
            let artifact = Artifact {
                artifact_id: "report-123".to_string(),
                name: "Monthly Report".to_string(),
                description: "Monthly financial report".to_string(),
                parts: vec![Part::text("Monthly revenue: $5000".to_string())],
                extensions: Vec::new(),
                ..Default::default()
            };

            let agent_msg = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text(
                    "Report generated successfully".to_string(),
                )])
                .message_id("resp-123".to_string())
                .build();

            Ok(Task::builder()
                .id(task_id.to_string())
                .context_id("ctx-123".to_string())
                .status(TaskStatus::new(TaskState::Completed, None))
                .history(vec![_message.clone(), agent_msg])
                .artifacts(vec![artifact])
                .build())
        }
    }

    let bridge = AgentToMcpBridge::with_handler(ResourceHandler, agent_card);

    let (server_io, client_io) = tokio::io::duplex(4096);
    let bridge_task = tokio::spawn(async move {
        let running = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });
    let mcp_client = ().serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();

    // 1. Call the tool to generate a task and cached artifact
    let tools = peer.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 1);
    let tool_name = &tools.tools[0].name;

    let args = serde_json::json!({ "message": "generate" })
        .as_object()
        .cloned()
        .unwrap();
    let call_params = CallToolRequestParams::new(tool_name.clone()).with_arguments(args);
    let call_res = peer.call_tool(call_params).await.unwrap();
    assert!(!call_res.is_error.unwrap_or(false));

    // 2. Test list_resources
    let resources_res = peer.list_resources(None).await.unwrap();
    assert_eq!(resources_res.resources.len(), 1);
    let resource_uri = &resources_res.resources[0].uri;
    assert!(resource_uri.contains("report-123"));
    assert_eq!(resources_res.resources[0].name, "Monthly Report");
    assert_eq!(
        resources_res.resources[0].mime_type,
        Some("text/plain".to_string())
    );

    // 3. Test read_resource
    let read_params = ReadResourceRequestParams::new(resource_uri.clone());
    let read_res = peer.read_resource(read_params).await.unwrap();
    assert_eq!(read_res.contents.len(), 1);
    match &read_res.contents[0] {
        ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            ..
        } => {
            assert_eq!(uri, resource_uri);
            assert_eq!(mime_type, &Some("text/plain".to_string()));
            assert_eq!(text, "Monthly revenue: $5000");
        }
        _ => panic!("Expected TextResourceContents"),
    }

    drop(mcp_client);
    let _ = bridge_task.await;
}

/// A task the bridge ran is readable through the tasks extension: `tasks/get`
/// answers with the A2A state mapped, and a completed task carries its tool
/// result inline, so a client that only has the id gets the answer.
#[tokio::test]
async fn a_bridged_task_is_readable_through_tasks_get() {
    let calls = Arc::new(AtomicUsize::new(0));
    let agent_card = AgentCard::builder()
        .name("Task Agent".to_string())
        .description("Exercises tasks/get".to_string())
        .url("http://nonroutable.invalid".to_string())
        .version("1.0.0".to_string())
        .capabilities(AgentCapabilities::default())
        .default_input_modes(vec!["text".to_string()])
        .default_output_modes(vec!["text".to_string()])
        .skills(vec![AgentSkill::new(
            "echo".to_string(),
            "Echo".to_string(),
            "Echoes the input".to_string(),
            vec![],
        )])
        .build();
    let bridge = AgentToMcpBridge::with_handler(
        CountingHandler {
            calls: Arc::clone(&calls),
        },
        agent_card,
    );

    let (server_io, client_io) = tokio::io::duplex(4096);
    let bridge_task = tokio::spawn(async move {
        let running = bridge.serve(server_io).await?;
        running.waiting().await?;
        anyhow::Ok(())
    });
    // The client has to declare the tasks extension, or `tasks/get` is
    // refused as a missing client capability.
    let client_info = ClientInfo::new(
        ClientCapabilities::builder().enable_tasks().build(),
        Implementation::new("tasks-client", "1.0.0"),
    );
    let mcp_client = client_info.serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();
    let tool_name = peer.list_tools(None).await.unwrap().tools[0]
        .name
        .to_string();

    let params = CallToolRequestParams::new(tool_name).with_arguments(
        serde_json::json!({ "message": "hello tasks", "task_id": "task-get-1" })
            .as_object()
            .cloned()
            .unwrap(),
    );
    peer.call_tool(params).await.unwrap();

    let got = peer
        .get_task(GetTaskParams::new("task-get-1"))
        .await
        .unwrap();
    assert_eq!(got.task.task.task_id, "task-get-1");
    assert_eq!(got.task.status(), rmcp::model::TaskStatus::Completed);
    let TaskPayload::Completed { result } = &got.task.payload else {
        panic!(
            "a completed task carries its result: {:?}",
            got.task.payload
        );
    };
    let result: CallToolResult = serde_json::from_value(serde_json::Value::Object(result.clone()))
        .expect("the payload is the tool result");
    let text = result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
        .expect("tool result has text content");
    assert!(text.contains("in-process: hello tasks"), "got: {text}");

    let missing = peer.get_task(GetTaskParams::new("no-such-task")).await;
    assert!(missing.is_err(), "an unknown id is an error, not a task");

    drop(mcp_client);
    let _ = bridge_task.await;
}
