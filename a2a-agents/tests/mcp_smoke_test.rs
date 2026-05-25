#[cfg(feature = "mcp-server")]
mod tests {
    use a2a_agents::core::{AgentBuilder, AgentConfig};
    use a2a_rs::{
        InMemoryTaskStorage,
        domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
        port::AsyncMessageHandler,
    };
    use async_trait::async_trait;

    #[derive(Clone)]
    struct EchoHandler;

    #[async_trait]
    impl AsyncMessageHandler for EchoHandler {
        async fn process_message(
            &self,
            task_id: &str,
            message: &Message,
            _session_id: Option<&str>,
        ) -> Result<Task, A2AError> {
            let text = message
                .parts
                .iter()
                .find_map(|p| p.get_text())
                .unwrap_or("<empty>")
                .to_string();

            let response = Message::builder()
                .role(Role::Agent)
                .parts(vec![Part::text(format!("echo: {text}"))])
                .message_id(uuid::Uuid::new_v4().to_string())
                .build();

            Ok(Task::builder()
                .id(task_id.to_string())
                .context_id(message.context_id.clone())
                .status(TaskStatus::new(
                    TaskState::Completed,
                    Some(response.clone()),
                ))
                .history(vec![message.clone(), response])
                .build())
        }
    }

    #[tokio::test]
    async fn test_mcp_config_validation_and_runtime_smoke() {
        let toml_content = r#"
            [agent]
            name = "Test Smoke Agent"
            version = "0.9.9"

            [server]
            host = "127.0.0.1"
            http_port = 0
            ws_port = 0
            auth = { type = "bearer", tokens = ["secret-token-123"] }

            [features.mcp_server]
            enabled = true
            stdio = false
            name = "Custom MCP Service"
            version = "1.2.3"

            [[skills]]
            id = "smoke_skill"
            name = "Smoke Skill"
            description = "A dummy skill"
        "#;

        let config = AgentConfig::from_toml(toml_content).expect("Should parse TOML");
        assert!(
            config.validate().is_ok(),
            "Config with ports = 0 should be valid when MCP server is enabled"
        );

        // Build the agent runtime
        let runtime = AgentBuilder::from_toml(toml_content)
            .expect("Should construct builder")
            .with_handler(EchoHandler)
            .with_storage(InMemoryTaskStorage::new())
            .build()
            .expect("Should build agent runtime");

        // Run the agent. With stdio = false, it should output a log and return Ok(()) immediately.
        let run_result = runtime.run().await;
        assert!(
            run_result.is_ok(),
            "Running runtime in MCP mode with stdio=false should succeed immediately"
        );
    }
}
