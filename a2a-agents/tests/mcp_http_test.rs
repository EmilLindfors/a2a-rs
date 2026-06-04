//! End-to-end tests for the MCP Streamable HTTP transport.
//!
//! Boots a TOML-configured agent in MCP-server mode with the HTTP transport
//! enabled, then exercises real behavior over the wire:
//! * a full MCP `initialize` handshake (happy path), and
//! * the `allowed_hosts` DNS-rebinding knob (reject vs. allow), driven over a
//!   raw socket so the `Host` header is fully under test control.

#![cfg(feature = "mcp-server")]

use a2a_agents::core::AgentBuilder;
use a2a_rs::{
    InMemoryTaskStorage,
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::AsyncMessageHandler,
};
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
            .status(TaskStatus::new(TaskState::Completed, Some(response.clone())))
            .history(vec![message.clone(), response])
            .build())
    }
}

/// Grab a free TCP port by binding to :0 and immediately releasing it.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local_addr")
        .port()
}

/// Build + spawn an MCP/HTTP agent from a TOML fragment on the given port.
fn spawn_agent(http_section: &str, port: u16) -> tokio::task::JoinHandle<()> {
    let toml_content = format!(
        r#"
        [agent]
        name = "HTTP MCP Agent"
        version = "0.1.0"

        [server]
        host = "127.0.0.1"
        http_port = 0

        [features.mcp_server]
        enabled = true
        stdio = false

        [features.mcp_server.http]
        enabled = true
        host = "127.0.0.1"
        port = {port}
        path = "/mcp"
        {http_section}

        [[skills]]
        id = "echo"
        name = "Echo"
        description = "Echoes input"
    "#
    );

    let runtime = AgentBuilder::from_toml(&toml_content)
        .expect("build builder")
        .with_handler(EchoHandler)
        .with_storage(InMemoryTaskStorage::new())
        .build()
        .expect("build runtime");

    tokio::spawn(async move {
        let _ = runtime.run().await;
    })
}

/// Poll until the server accepts TCP connections (or give up).
async fn wait_listening(port: u16) {
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("server on port {port} never started listening");
}

const INIT_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a2a-test-client","version":"0.1.0"}}}"#;

/// Send a raw HTTP/1.1 `initialize` POST with a chosen `Host` header and return
/// the first response line (e.g. `HTTP/1.1 200 OK`).
async fn raw_initialize(port: u16, host_header: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    let request = format!(
        "POST /mcp HTTP/1.1\r\n\
         Host: {host}\r\n\
         Accept: application/json, text/event-stream\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        host = host_header,
        len = INIT_BODY.len(),
        body = INIT_BODY,
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");

    // The status line + headers arrive promptly; read one chunk with a timeout
    // (a 200 reply opens an SSE stream that would otherwise keep us reading).
    let mut buf = vec![0u8; 1024];
    let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
        .await
        .expect("response within timeout")
        .expect("read response");
    let text = String::from_utf8_lossy(&buf[..n]);
    text.lines().next().unwrap_or_default().trim().to_string()
}

#[tokio::test]
async fn streamable_http_initialize_handshake() {
    let port = free_port();
    let server = spawn_agent("", port);
    wait_listening(port).await;

    let url = format!("http://127.0.0.1:{port}/mcp");
    let client = reqwest::Client::new();
    let init_body: serde_json::Value = serde_json::from_str(INIT_BODY).unwrap();

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&init_body)
        .send()
        .await
        .expect("initialize request");

    assert!(
        response.status().is_success(),
        "initialize should return 2xx, got {}",
        response.status()
    );
    assert!(
        response.headers().contains_key("mcp-session-id"),
        "stateful server must return an Mcp-Session-Id header"
    );

    let body = response.text().await.expect("read body");
    assert!(
        body.contains("\"result\"") && body.contains("serverInfo"),
        "initialize response should carry a JSON-RPC result with serverInfo, got: {body}"
    );

    server.abort();
}

#[tokio::test]
async fn default_config_rejects_non_loopback_host() {
    // No allowed_hosts override → secure default (loopback only).
    let port = free_port();
    let server = spawn_agent("", port);
    wait_listening(port).await;

    let status = raw_initialize(port, "evil.example.com").await;
    assert!(
        status.contains("403"),
        "non-loopback Host should be rejected with 403, got: {status:?}"
    );

    server.abort();
}

#[tokio::test]
async fn empty_allowed_hosts_permits_any_host() {
    // allowed_hosts = [] disables Host validation entirely.
    let port = free_port();
    let server = spawn_agent("allowed_hosts = []", port);
    wait_listening(port).await;

    let status = raw_initialize(port, "evil.example.com").await;
    assert!(
        status.contains("200"),
        "with Host validation disabled any Host should be accepted, got: {status:?}"
    );

    server.abort();
}
