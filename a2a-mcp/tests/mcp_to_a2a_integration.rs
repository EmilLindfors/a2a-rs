//! Integration test for MCP to A2A bridge
//!
//! This test verifies that MCP tools and prompts can be successfully exposed as A2A agent skills

use a2a_mcp::bridge::mcp_to_a2a::{
    McpToA2ABridge, ProgressClientHandler, create_prompt_call_message,
    create_resource_read_message, create_tool_call_message,
};
use a2a_rs::domain::core::agent::AgentCard;
use a2a_rs::domain::{
    Message, Part, Role, Task, TaskArtifactUpdateEvent, TaskState, TaskStatus,
    TaskStatusUpdateEvent,
};
use a2a_rs::port::streaming_handler::Subscriber;
use a2a_rs::port::{AsyncMessageHandler, AsyncStreamingHandler, SeqEvent};
use async_trait::async_trait;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::client::progress::ProgressDispatcher, model::*, service::RequestContext,
};
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

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
    let _mock_result = CallToolResult::success(vec![ContentBlock::text("42")]);

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
    tasks: rmcp::task_manager::TaskManager,
    /// Lets one waiting `ask_when_told` call through to its elicitation.
    gate: Arc<tokio::sync::Notify>,
    /// How many calls are waiting at it.
    at_gate: Arc<AtomicUsize>,
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

        // Pauses for a yes/no before doing anything, the way a data server's
        // destructive tool would.
        let drop_table = Tool::new(
            "drop_table",
            "Drops a table after asking",
            Arc::new(
                serde_json::from_value(serde_json::json!({
                    "type": "object",
                    "properties": { "table": { "type": "string" } },
                    "required": ["table"]
                }))
                .unwrap(),
            ),
        );
        // Not ready on the first call: answers with state only, and completes
        // when that state comes back.
        let slow_count = Tool::new(
            "slow_count",
            "Counts, eventually",
            Arc::new(serde_json::from_value(serde_json::json!({ "type": "object" })).unwrap()),
        );

        // Takes a while, so a client that can hold a task gets one; a client
        // that cannot gets the result when it is done, the way `strata run`
        // will behave.
        let run = Tool::new(
            "run",
            "Builds the warehouse",
            Arc::new(serde_json::from_value(serde_json::json!({ "type": "object" })).unwrap()),
        );
        // A task that asks before it acts.
        let run_after_asking = Tool::new(
            "run_after_asking",
            "Builds the warehouse after asking",
            Arc::new(serde_json::from_value(serde_json::json!({ "type": "object" })).unwrap()),
        );
        // Asks the *client* while the call is open, rather than answering
        // with `input_required` — the other way a server asks.
        let ask_in_flight = Tool::new(
            "ask_in_flight",
            "Asks the client mid-call",
            Arc::new(serde_json::from_value(serde_json::json!({ "type": "object" })).unwrap()),
        );
        // The same, held at a gate the test opens, so two calls can be open
        // at once on purpose.
        let ask_when_told = Tool::new(
            "ask_when_told",
            "Asks the client mid-call, once let through",
            Arc::new(serde_json::from_value(serde_json::json!({ "type": "object" })).unwrap()),
        );

        Self {
            tools: Arc::new(vec![
                tool,
                drop_table,
                slow_count,
                run,
                run_after_asking,
                ask_in_flight,
                ask_when_told,
            ]),
            prompts: Arc::new(vec![prompt]),
            tasks: rmcp::task_manager::TaskManager::new(),
            gate: Arc::new(tokio::sync::Notify::new()),
            at_gate: Arc::new(AtomicUsize::new(0)),
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
                .enable_resources()
                .enable_resources_subscribe()
                .enable_tasks()
                .build(),
        )
        .with_server_info(Implementation::new("test-server", "1.0.0"))
    }

    fn get_task(
        &self,
        request: GetTaskParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetTaskResult, McpError>> + Send + '_ {
        async move { Ok(GetTaskResult::new(self.tasks.get_task(&request.task_id)?)) }
    }

    fn update_task(
        &self,
        request: UpdateTaskParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send + '_ {
        async move {
            self.tasks
                .update_task(&request.task_id, request.input_responses)
        }
    }

    fn cancel_task(
        &self,
        request: CancelTaskParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send + '_ {
        async move { self.tasks.cancel_task(&request.task_id) }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move { Ok(ListToolsResult::with_all_items((*self.tools).clone())) }
    }

    fn call_tool(
        &self,
        CallToolRequestParams {
            name,
            arguments,
            input_responses,
            request_state,
            ..
        }: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResponse, McpError>> + Send + '_ {
        async move {
            if name == "drop_table" {
                let table = arguments
                    .as_ref()
                    .and_then(|a| a.get("table"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("?")
                    .to_string();
                let Some(responses) = input_responses else {
                    let schema = ElicitationSchema::builder()
                        .required_bool("confirm")
                        .build()
                        .unwrap();
                    let mut requests = InputRequests::new();
                    requests.insert(
                        "confirm".to_string(),
                        InputRequest::Elicitation(ElicitRequest::new(
                            ElicitRequestParams::FormElicitationParams {
                                meta: None,
                                message: format!("Drop table {table}? This cannot be undone."),
                                requested_schema: schema,
                            },
                        )),
                    );
                    return Ok(CallToolResponse::InputRequired(InputRequiredResult::new(
                        Some(requests),
                        Some(format!("dropping:{table}")),
                    )));
                };
                if request_state.as_deref() != Some(&format!("dropping:{table}")) {
                    return Err(McpError::invalid_params(
                        format!("request state not echoed: {request_state:?}"),
                        None,
                    ));
                }
                let answer: ElicitResult = serde_json::from_value(
                    responses
                        .get("confirm")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .map_err(|e| McpError::invalid_params(format!("no elicit result: {e}"), None))?;
                let confirmed = answer.action == ElicitationAction::Accept
                    && answer
                        .content
                        .as_ref()
                        .and_then(|c| c.get("confirm"))
                        .and_then(|c| c.as_bool())
                        .unwrap_or(false);
                let said = if confirmed {
                    format!("dropped {table}")
                } else {
                    format!("kept {table}")
                };
                return Ok(CallToolResult::success(vec![ContentBlock::text(said)]).into());
            }
            if name == "slow_count" {
                return Ok(match request_state.as_deref() {
                    None => CallToolResponse::InputRequired(
                        InputRequiredResult::from_request_state("counting"),
                    ),
                    Some("counting") => {
                        CallToolResult::success(vec![ContentBlock::text("3")]).into()
                    }
                    other => {
                        return Err(McpError::invalid_params(
                            format!("unexpected state {other:?}"),
                            None,
                        ));
                    }
                });
            }
            if name == "ask_in_flight" || name == "ask_when_told" {
                if name == "ask_when_told" {
                    self.at_gate.fetch_add(1, Ordering::SeqCst);
                    self.gate.notified().await;
                }
                let schema = ElicitationSchema::builder()
                    .required_string("answer")
                    .build()
                    .unwrap();
                let answer = ctx
                    .peer
                    .create_elicitation(ElicitRequestParams::FormElicitationParams {
                        meta: None,
                        message: "What should I call it?".to_string(),
                        requested_schema: schema,
                    })
                    .await;
                let said = match answer {
                    Ok(result) if result.action == ElicitationAction::Accept => result
                        .content
                        .as_ref()
                        .and_then(|c| c.get("answer"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("(nothing)")
                        .to_string(),
                    Ok(result) => format!("no answer: {:?}", result.action),
                    Err(e) => format!("could not ask: {e}"),
                };
                return Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                    "named {said}"
                ))])
                .into());
            }
            if name == "run" || name == "run_after_asking" {
                use rmcp::task_manager::{TaskExit, TaskOptions};
                let built = || CallToolResult::success(vec![ContentBlock::text("2 nodes built")]);
                // Under the discover lifecycle the client's capabilities ride
                // on each request, not on the peer's handshake info.
                let client_holds_tasks = ctx
                    .client_capabilities()
                    .is_some_and(|caps| caps.supports_tasks());
                if !client_holds_tasks {
                    return Ok(built().into());
                }
                let asks = name == "run_after_asking";
                let task = self.tasks.spawn(
                    TaskOptions::new().with_poll_interval_ms(10),
                    move |task| {
                        Box::pin(async move {
                            if asks {
                                let schema = ElicitationSchema::builder()
                                    .required_bool("confirm")
                                    .build()
                                    .unwrap();
                                let answer = task
                                    .request_input(
                                        "confirm",
                                        InputRequest::Elicitation(ElicitRequest::new(
                                            ElicitRequestParams::FormElicitationParams {
                                                meta: None,
                                                message: "Build the warehouse now?".to_string(),
                                                requested_schema: schema,
                                            },
                                        )),
                                    )
                                    .await?;
                                let answer: ElicitResult = serde_json::from_value(answer)
                                    .map_err(|e| TaskExit::Error(McpError::invalid_params(e.to_string(), None)))?;
                                let confirmed = answer.action == ElicitationAction::Accept
                                    && answer
                                        .content
                                        .as_ref()
                                        .and_then(|c| c.get("confirm"))
                                        .and_then(|c| c.as_bool())
                                        .unwrap_or(false);
                                if !confirmed {
                                    return Ok(CallToolResult::success(vec![ContentBlock::text(
                                        "did not build",
                                    )]));
                                }
                            }
                            for node in 1..=2 {
                                task.set_status_message(format!("node {node} of 2"));
                                tokio::select! {
                                    _ = task.cancelled() => return Err(TaskExit::Cancelled),
                                    _ = tokio::time::sleep(std::time::Duration::from_millis(30)) => {}
                                }
                            }
                            Ok(built())
                        })
                    },
                );
                return Ok(CallToolResponse::Task(CreateTaskResult::new(task)));
            }
            if name != "calculator" {
                return Err(McpError::invalid_params("unknown tool", None));
            }

            // Test progress updates if progress_token is present
            if let Some(token) = ctx.meta.get_progress_token() {
                // Send some progress notifications
                let _ = ctx
                    .peer
                    .notify_progress(
                        ProgressNotificationParam::new(token.clone(), 50.0)
                            .with_message("Step 1 done"),
                    )
                    .await;
                let _ = ctx
                    .peer
                    .notify_progress(
                        ProgressNotificationParam::new(token.clone(), 100.0)
                            .with_message("Step 2 done"),
                    )
                    .await;
            }

            Ok(CallToolResult::success(vec![ContentBlock::text("42")]).into())
        }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        async move {
            let catalogue =
                Resource::new("catalogue://views", "views").with_mime_type("text/markdown");
            let logo = Resource::new("file:///logo.bin", "logo")
                .with_mime_type("application/octet-stream");
            Ok(ListResourcesResult::with_all_items(vec![catalogue, logo]))
        }
    }

    fn read_resource(
        &self,
        ReadResourceRequestParams { uri, .. }: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResponse, McpError>> + Send + '_ {
        async move {
            match uri.as_str() {
                "catalogue://views" => Ok(ReadResourceResult::new(vec![
                    ResourceContents::text("# Views\n- harvest\n- feed", uri)
                        .with_mime_type("text/markdown"),
                ])
                .into()),
                "file:///logo.bin" => Ok(ReadResourceResult::new(vec![
                    // [0, 1, 2]
                    ResourceContents::blob("AAEC", uri).with_mime_type("application/octet-stream"),
                ])
                .into()),
                _ => Err(McpError::resource_not_found(uri, None)),
            }
        }
    }

    fn subscribe(
        &self,
        SubscribeRequestParams { uri, .. }: SubscribeRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send + '_ {
        async move {
            // The test server reports the resource changed the moment anyone
            // subscribes, so the notification path is exercised without a
            // clock.
            let _ = ctx
                .peer
                .notify_resource_updated(ResourceUpdatedNotificationParam::new(uri))
                .await;
            Ok(())
        }
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListPromptsResult, McpError>> + Send + '_ {
        async move { Ok(ListPromptsResult::with_all_items((*self.prompts).clone())) }
    }

    fn get_prompt(
        &self,
        GetPromptRequestParams { name, .. }: GetPromptRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetPromptResponse, McpError>> + Send + '_ {
        async move {
            if name != "test_prompt" {
                return Err(McpError::invalid_params("unknown prompt", None));
            }
            let pm = PromptMessage::new_text(rmcp::model::Role::Assistant, "Prompt output message");
            Ok(GetPromptResult::new(vec![pm]).into())
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
        _ctx: &a2a_rs::port::RequestContext,
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
                dyn futures::Stream<
                        Item = Result<TaskStatusUpdateEvent, a2a_rs::domain::error::A2AError>,
                    > + Send,
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
                dyn futures::Stream<
                        Item = Result<TaskArtifactUpdateEvent, a2a_rs::domain::error::A2AError>,
                    > + Send,
            >,
        >,
        a2a_rs::domain::error::A2AError,
    > {
        unimplemented!()
    }

    async fn combined_update_stream(
        &self,
        _task_id: &str,
        _from_event_id: Option<u64>,
    ) -> Result<
        Pin<
            Box<
                dyn futures::Stream<Item = Result<SeqEvent, a2a_rs::domain::error::A2AError>>
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
    let prompts = bridge.prompts().await;
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].name, "test_prompt");

    // Call the prompt via bridge
    let prompt_call_msg = create_prompt_call_message("test_prompt", serde_json::json!({}));
    let task = bridge
        .process_message(
            "task-prompt-1",
            &prompt_call_msg,
            &a2a_rs::port::RequestContext::anonymous(),
        )
        .await
        .unwrap();

    assert_eq!(task.status.state, TaskState::Completed);
    let history = &task.history;
    assert_eq!(history.len(), 2);
    // User message is history[0], assistant prompt reply is history[1]
    assert_eq!(history[1].role, Role::Agent);
    assert_eq!(
        history[1].parts[0].get_text(),
        Some("Prompt output message")
    );

    drop(mcp_client);
    let _ = server_task.await;
}

/// The third list beside tools and prompts, and a read that arrives as what
/// the resource holds rather than the address it was read from.
#[tokio::test]
async fn resources_are_listed_read_and_watched() {
    use a2a_rs::domain::generated::part;

    let (server_io, client_io) = tokio::io::duplex(4096);
    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });

    let mcp_client = ().serve(client_io).await.unwrap();
    let peer = mcp_client.peer().clone();
    let bridge = McpToA2ABridge::new(peer, NoOpHandler).await.unwrap();

    // Listed at initialize.
    let resources = bridge.resources().await;
    assert_eq!(resources.len(), 2);
    assert_eq!(resources[0].uri, "catalogue://views");

    // Read directly: the text, not the URI.
    let contents = bridge.read_resource("catalogue://views").await.unwrap();
    assert_eq!(contents.len(), 1);
    let message = bridge
        .read_resource_message("catalogue://views", Role::Agent)
        .await
        .unwrap();
    assert_eq!(
        message.parts[0].content,
        Some(part::Content::Text(
            "# Views\n- harvest\n- feed".to_string()
        ))
    );
    assert_eq!(message.parts[0].media_type, "text/markdown");

    // A blob arrives as bytes.
    let logo = bridge
        .read_resource_message("file:///logo.bin", Role::Agent)
        .await
        .unwrap();
    assert_eq!(
        logo.parts[0].content,
        Some(part::Content::Raw(vec![0, 1, 2]))
    );

    // Read through the A2A envelope, like a tool or prompt call.
    let task = bridge
        .process_message(
            "task-resource-1",
            &create_resource_read_message("catalogue://views"),
            &a2a_rs::port::RequestContext::anonymous(),
        )
        .await
        .unwrap();
    assert_eq!(task.status.state, TaskState::Completed);
    assert_eq!(task.history.len(), 2);
    assert_eq!(task.history[1].role, Role::Agent);
    assert!(
        task.history[1].parts[0]
            .get_text()
            .is_some_and(|t| t.contains("harvest"))
    );

    // A URI the server does not serve is the server's error, not a skipped entry.
    assert!(bridge.read_resource("catalogue://nope").await.is_err());

    drop(mcp_client);
    let _ = server_task.await;
}

/// `notifications/resources/updated` lands in the bridge when the bridge is
/// the client handler, and `take_updated_resources` hands the URI over once.
#[tokio::test]
async fn a_resource_update_is_recorded_for_the_consumer_to_re_read() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });

    // Build the bridge over a first connection only to have a peer to
    // construct with, then serve the bridge itself over the connection under
    // test so it receives notifications.
    let (probe_server_io, probe_client_io) = tokio::io::duplex(4096);
    let probe_server = TestMcpServer::new();
    let probe_task = tokio::spawn(async move {
        let running = probe_server.serve(probe_server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let probe_client = ().serve(probe_client_io).await.unwrap();
    let bridge = McpToA2ABridge::new(probe_client.peer().clone(), NoOpHandler)
        .await
        .unwrap();

    let running = bridge.clone().serve(client_io).await.unwrap();
    // The bridge's own peer still points at the probe connection; subscribe
    // through the connection the bridge is serving on instead.
    #[allow(deprecated)]
    running
        .peer()
        .subscribe(SubscribeRequestParams::new("catalogue://views"))
        .await
        .unwrap();

    // The test server notifies on subscribe; give the notification a moment.
    let mut updated = Vec::new();
    for _ in 0..50 {
        updated = bridge.take_updated_resources();
        if !updated.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(updated, vec!["catalogue://views".to_string()]);
    assert!(bridge.take_updated_resources().is_empty(), "taken once");

    drop(running);
    drop(probe_client);
    probe_task.abort();
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
        .process_message(
            "task-calc-1",
            &tool_call_msg,
            &a2a_rs::port::RequestContext::anonymous(),
        )
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

/// A client session at 2026-07-28, which is the only revision a server can
/// pause a call on: rmcp refuses to send an `InputRequiredResult` below it.
/// `()` and `serve()` open with `initialize`, and that handshake tops out at
/// 2025-11-25 whatever the client asks for; 2026-07-28 is reached only
/// through the discover lifecycle.
async fn current_client(
    transport: tokio::io::DuplexStream,
) -> rmcp::service::RunningService<rmcp::RoleClient, ClientInfo> {
    use rmcp::service::{ClientLifecycleMode, serve_client_with_lifecycle};
    let info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("test-client", "1.0.0"),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28);
    serve_client_with_lifecycle(
        info,
        transport,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    )
    .await
    .unwrap()
}

/// A client at 2026-07-28 that also declares the tasks extension, so a
/// server may answer a tool call with a task.
async fn tasks_client(
    transport: tokio::io::DuplexStream,
) -> rmcp::service::RunningService<rmcp::RoleClient, ClientInfo> {
    use rmcp::service::{ClientLifecycleMode, serve_client_with_lifecycle};
    let info = ClientInfo::new(
        ClientCapabilities::builder().enable_tasks().build(),
        Implementation::new("test-client", "1.0.0"),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28);
    serve_client_with_lifecycle(
        info,
        transport,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    )
    .await
    .unwrap()
}

/// A tool call the server made a task of is one A2A task: the bridge
/// watches `tasks/get` until the task settles, each status message the
/// server sets is a `Working` update on the A2A task, and the task's result
/// is the call's.
#[tokio::test]
async fn a_long_tool_call_is_one_task_the_caller_can_watch() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let mcp_client = tasks_client(client_io).await;
    let streaming_handler = TestStreamingHandler::default();
    let bridge = McpToA2ABridge::with_streaming(
        mcp_client.peer().clone(),
        NoOpHandler,
        ProgressDispatcher::new(),
        Arc::new(streaming_handler.clone()),
    )
    .await
    .unwrap();
    let ctx = a2a_rs::port::RequestContext::anonymous();

    let call = create_tool_call_message("run", serde_json::json!({}));
    let done = bridge.process_message("run-1", &call, &ctx).await.unwrap();
    assert_eq!(done.status.state, TaskState::Completed);
    assert_eq!(done.history[1].parts[0].get_text(), Some("2 nodes built"));

    let relayed: Vec<String> = streaming_handler
        .updates
        .lock()
        .unwrap()
        .iter()
        .filter(|u| u.status.state == TaskState::Working)
        .filter_map(|u| u.status.message.as_option())
        .filter_map(|m| m.parts[0].get_text().map(String::from))
        .collect();
    assert_eq!(
        relayed,
        vec!["node 1 of 2", "node 2 of 2"],
        "each status message the server set was relayed once"
    );

    // A model's tool call gets the task's result as the tool's.
    let tool_call = a2a_llm::ToolCall {
        id: "call-1".to_string(),
        name: "run".to_string(),
        arguments: "{}".to_string(),
    };
    let result = bridge
        .execute_llm_tool_call("run-2", &tool_call)
        .await
        .unwrap();
    assert_eq!(result.into_model_text(), "2 nodes built");

    drop(mcp_client);
    let _ = server_task.await;
}

/// A task that asks pauses the A2A task like a paused call does; the next
/// message answers it through `tasks/update`, and the bridge watches the
/// task on to its result.
#[tokio::test]
async fn a_tasks_question_pauses_the_task_and_tasks_update_answers_it() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let mcp_client = tasks_client(client_io).await;
    let bridge = McpToA2ABridge::new(mcp_client.peer().clone(), NoOpHandler)
        .await
        .unwrap();
    let ctx = a2a_rs::port::RequestContext::anonymous();

    let call = create_tool_call_message("run_after_asking", serde_json::json!({}));
    let paused = bridge.process_message("ask-1", &call, &ctx).await.unwrap();
    assert_eq!(paused.status.state, TaskState::InputRequired);
    let question: String = paused
        .status
        .message
        .as_option()
        .map(|m| m.parts.iter().filter_map(|p| p.get_text()).collect())
        .unwrap_or_default();
    assert!(
        question.contains("Build the warehouse now?"),
        "the task's question is the status message: {question:?}"
    );
    assert!(bridge.is_awaiting_input("ask-1"));

    let yes = Message::user_text("yes".to_string(), "answer-1".to_string());
    let done = bridge.process_message("ask-1", &yes, &ctx).await.unwrap();
    assert_eq!(done.status.state, TaskState::Completed);
    assert_eq!(done.history[1].parts[0].get_text(), Some("2 nodes built"));
    assert!(!bridge.is_awaiting_input("ask-1"));

    // Declined, the task still settles; its answer is the tool's.
    let call = create_tool_call_message("run_after_asking", serde_json::json!({}));
    bridge.process_message("ask-2", &call, &ctx).await.unwrap();
    let no = Message::user_text("decline".to_string(), "answer-2".to_string());
    let kept = bridge.process_message("ask-2", &no, &ctx).await.unwrap();
    assert_eq!(kept.status.state, TaskState::Completed);
    assert_eq!(kept.history[1].parts[0].get_text(), Some("did not build"));

    drop(mcp_client);
    let _ = server_task.await;
}

/// A client that did not declare the extension is answered in `tools/call`
/// as before; the bridge sees no task.
#[tokio::test]
async fn a_client_without_the_extension_is_answered_in_the_call() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let mcp_client = current_client(client_io).await;
    let bridge = McpToA2ABridge::new(mcp_client.peer().clone(), NoOpHandler)
        .await
        .unwrap();
    let call = create_tool_call_message("run", serde_json::json!({}));
    let done = bridge
        .process_message("run-3", &call, &a2a_rs::port::RequestContext::anonymous())
        .await
        .unwrap();
    assert_eq!(done.status.state, TaskState::Completed);
    assert_eq!(done.history[1].parts[0].get_text(), Some("2 nodes built"));

    drop(mcp_client);
    let _ = server_task.await;
}

/// A tool that asks before it acts pauses the task: `InputRequired`, with the
/// server's question as the status message. The next message on the task
/// answers it, the server gets its state back with the answer, and the call
/// completes. `bridge.process_message` is the whole path.
#[tokio::test]
async fn a_servers_question_pauses_the_task_and_the_next_message_answers_it() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let mcp_client = current_client(client_io).await;
    let bridge = McpToA2ABridge::new(mcp_client.peer().clone(), NoOpHandler)
        .await
        .unwrap();
    let ctx = a2a_rs::port::RequestContext::anonymous();

    let call = create_tool_call_message("drop_table", serde_json::json!({ "table": "lice" }));
    let paused = bridge.process_message("drop-1", &call, &ctx).await.unwrap();
    assert_eq!(paused.status.state, TaskState::InputRequired);
    let question: String = paused
        .status
        .message
        .as_option()
        .map(|m| m.parts.iter().filter_map(|p| p.get_text()).collect())
        .unwrap_or_default();
    assert!(
        question.contains("Drop table lice?") && question.contains("`confirm` (boolean), required"),
        "the question and what to answer are the status message: {question:?}"
    );
    assert!(bridge.is_awaiting_input("drop-1"));

    let yes = Message::user_text("yes".to_string(), "answer-1".to_string());
    let done = bridge.process_message("drop-1", &yes, &ctx).await.unwrap();
    assert_eq!(done.status.state, TaskState::Completed);
    assert_eq!(done.history[1].parts[0].get_text(), Some("dropped lice"));
    assert!(!bridge.is_awaiting_input("drop-1"));

    // `decline` on its own is the action, not a string the form receives.
    let call = create_tool_call_message("drop_table", serde_json::json!({ "table": "sites" }));
    bridge.process_message("drop-2", &call, &ctx).await.unwrap();
    let no = Message::user_text("decline".to_string(), "answer-2".to_string());
    let kept = bridge.process_message("drop-2", &no, &ctx).await.unwrap();
    assert_eq!(kept.status.state, TaskState::Completed);
    assert_eq!(kept.history[1].parts[0].get_text(), Some("kept sites"));

    drop(mcp_client);
    let _ = server_task.await;
}

/// A server that answers with state and no question is not asking anyone:
/// the bridge calls again with the state and nobody sees a pause.
#[tokio::test]
async fn a_state_only_round_is_retried_without_pausing() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let mcp_client = current_client(client_io).await;
    let bridge = McpToA2ABridge::new(mcp_client.peer().clone(), NoOpHandler)
        .await
        .unwrap();

    let call = create_tool_call_message("slow_count", serde_json::json!({}));
    let task = bridge
        .process_message("count-1", &call, &a2a_rs::port::RequestContext::anonymous())
        .await
        .unwrap();
    assert_eq!(task.status.state, TaskState::Completed);
    assert_eq!(task.history[1].parts[0].get_text(), Some("3"));
    assert!(!bridge.is_awaiting_input("count-1"));

    drop(mcp_client);
    let _ = server_task.await;
}

/// A client at 2026-07-28 that declares elicitation and routes what it is
/// asked to `router` — the shape a consumer uses when the bridge is not the
/// handler its peer was served with.
async fn asking_client(
    transport: tokio::io::DuplexStream,
    router: a2a_mcp::ElicitationRouter,
) -> rmcp::service::RunningService<rmcp::RoleClient, ProgressClientHandler> {
    use rmcp::service::{ClientLifecycleMode, serve_client_with_lifecycle};
    let handler = ProgressClientHandler::new(Default::default()).with_elicitations(router);
    serve_client_with_lifecycle(
        handler,
        transport,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    )
    .await
    .unwrap()
}

/// The other way a server asks: `create_elicitation` on the client while
/// `tools/call` is still open. Nothing in that request names the call, so
/// the bridge routes it to the one call it has open — the task pauses, the
/// next message answers, and the call it was holding open all along
/// finishes with that answer.
#[tokio::test]
async fn an_in_flight_elicitation_pauses_the_task_that_asked() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    let mcp_server = TestMcpServer::new();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let router = a2a_mcp::ElicitationRouter::new();
    let mcp_client = asking_client(client_io, router.clone()).await;
    let bridge = McpToA2ABridge::new(mcp_client.peer().clone(), NoOpHandler)
        .await
        .unwrap()
        .with_elicitation_router(router);
    let ctx = a2a_rs::port::RequestContext::anonymous();

    let call = create_tool_call_message("ask_in_flight", serde_json::json!({}));
    let paused = bridge.process_message("ask-1", &call, &ctx).await.unwrap();
    assert_eq!(paused.status.state, TaskState::InputRequired);
    let question: String = paused
        .status
        .message
        .as_option()
        .map(|m| m.parts.iter().filter_map(|p| p.get_text()).collect())
        .unwrap_or_default();
    assert!(
        question.contains("What should I call it?"),
        "the server's question is the status message: {question:?}"
    );
    assert!(bridge.is_awaiting_input("ask-1"));

    let answer = Message::user_text("Bergen".to_string(), "answer-1".to_string());
    let done = bridge
        .process_message("ask-1", &answer, &ctx)
        .await
        .unwrap();
    assert_eq!(done.status.state, TaskState::Completed);
    assert_eq!(done.history[1].parts[0].get_text(), Some("named Bergen"));
    assert!(!bridge.is_awaiting_input("ask-1"));

    drop(mcp_client);
    let _ = server_task.await;
}

/// Two calls open at once and a question that names neither: the bridge
/// refuses rather than pause the wrong task. The refusal reaches the server
/// as the elicitation's error, which is where a consumer can act on it.
#[tokio::test]
async fn a_question_with_two_calls_open_is_refused_rather_than_guessed() {
    let (server_io, client_io) = tokio::io::duplex(8192);
    let mcp_server = TestMcpServer::new();
    let gate = mcp_server.gate.clone();
    let at_gate = mcp_server.at_gate.clone();
    let server_task = tokio::spawn(async move {
        let running = mcp_server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let router = a2a_mcp::ElicitationRouter::new();
    let mcp_client = asking_client(client_io, router.clone()).await;
    let bridge = Arc::new(
        McpToA2ABridge::new(mcp_client.peer().clone(), NoOpHandler)
            .await
            .unwrap()
            .with_elicitation_router(router),
    );

    let both: Vec<_> = ["two-a", "two-b"]
        .into_iter()
        .map(|task_id| {
            let bridge = Arc::clone(&bridge);
            tokio::spawn(async move {
                let ctx = a2a_rs::port::RequestContext::anonymous();
                let call = create_tool_call_message("ask_when_told", serde_json::json!({}));
                bridge.process_message(task_id, &call, &ctx).await.unwrap()
            })
        })
        .collect();

    // Both calls are on the wire before either is allowed to ask, which is
    // what makes the question ambiguous rather than racy.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while at_gate.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both calls reach the gate");
    gate.notify_waiters();

    for task in both {
        let task = task.await.unwrap();
        assert_eq!(
            task.status.state,
            TaskState::Completed,
            "a refused question is not a pause"
        );
        let said = task.history[1].parts[0].get_text().unwrap_or_default();
        assert!(
            said.contains("could not ask") && said.contains("2 tool calls are open"),
            "the server is told why, got: {said}"
        );
    }

    drop(mcp_client);
    let _ = server_task.await;
}
