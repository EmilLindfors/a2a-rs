//! End-to-end test for the `mcp-client` framework integration.
//!
//! Spawns the bundled `mcp_echo_server` as a real child-process MCP server,
//! connects to it through the same `McpClientManager::connect` path the
//! framework uses, and exercises tool discovery + invocation. This proves the
//! loop the integration closes: config → connected manager → tool call.

#![cfg(feature = "mcp-client")]

use a2a_agents::core::{McpClientManager, config::McpClientConfig, config::McpServerConnection};
use a2a_agents::traits::extract_tool_result_text;
use serde_json::json;

/// Build a config that spawns the compiled `mcp_echo_server` binary directly
/// (no nested `cargo`, so the test is fast and deterministic).
fn echo_server_config() -> McpClientConfig {
    McpClientConfig {
        enabled: true,
        servers: vec![McpServerConnection {
            name: "echo".to_string(),
            command: env!("CARGO_BIN_EXE_mcp_echo_server").to_string(),
            args: Vec::new(),
            env: Default::default(),
            cwd: None,
        }],
    }
}

#[tokio::test]
async fn connect_discovers_tools() {
    let mcp = McpClientManager::connect(&echo_server_config())
        .await
        .expect("connect to echo server");

    assert!(mcp.is_connected("echo").await);
    assert_eq!(mcp.connected_servers().await, vec!["echo".to_string()]);

    let tools = mcp.list_server_tools("echo").await.expect("echo tools");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(
        names.contains(&"echo"),
        "expected `echo` tool, got {names:?}"
    );
    assert!(names.contains(&"add"), "expected `add` tool, got {names:?}");
}

#[tokio::test]
async fn call_echo_tool_round_trips() {
    let mcp = McpClientManager::connect(&echo_server_config())
        .await
        .expect("connect to echo server");

    let result = mcp
        .call_tool("echo", "echo", Some(json!({ "text": "hello mcp" })))
        .await
        .expect("echo tool call");

    assert_eq!(extract_tool_result_text(&result), "hello mcp");
}

#[tokio::test]
async fn call_add_tool_computes() {
    let mcp = McpClientManager::connect(&echo_server_config())
        .await
        .expect("connect to echo server");

    let result = mcp
        .call_tool("echo", "add", Some(json!({ "a": 2, "b": 40 })))
        .await
        .expect("add tool call");
    assert_eq!(extract_tool_result_text(&result), "42");
}

#[tokio::test]
async fn call_on_unknown_server_is_not_connected() {
    let mcp = McpClientManager::connect(&echo_server_config())
        .await
        .expect("connect to echo server");

    let err = mcp
        .call_tool("does-not-exist", "echo", None)
        .await
        .expect_err("calling an unconnected server must fail");
    assert!(
        matches!(err, a2a_agents::core::McpClientError::NotConnected { .. }),
        "expected NotConnected, got {err:?}"
    );
}

#[tokio::test]
async fn disabled_config_yields_empty_manager() {
    let cfg = McpClientConfig {
        enabled: false,
        servers: Vec::new(),
    };
    let mcp = McpClientManager::connect(&cfg)
        .await
        .expect("empty manager");
    assert!(mcp.connected_servers().await.is_empty());
}
