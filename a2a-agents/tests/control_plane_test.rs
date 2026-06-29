//! HTTP-level test of the control-plane API.
//!
//! Serves [`control_plane_router`] over an `InMemoryAgentRuntime` (no child
//! processes) on an ephemeral port, then drives the real `POST/GET/DELETE
//! /agents` surface with `reqwest` — the same shape the Terraform provider will
//! call. Proves deploy → discover → undeploy round-trips over the wire.

use std::sync::Arc;

use a2a_agents::{
    AgentRegistry, AgentRuntime, ControlPlane, DeployedAgent, InMemoryAgentRegistry,
    InMemoryAgentRuntime, RuntimeHealth, control_plane_router,
};

/// Temp dir the API writes deployed configs into. Removed on drop.
struct TempDir {
    path: std::path::PathBuf,
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tokio::test]
async fn deploy_list_status_undeploy_over_http() {
    let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
    let runtime: Arc<dyn AgentRuntime> = Arc::new(InMemoryAgentRuntime::new());
    let cp = Arc::new(ControlPlane::new(runtime, registry));

    let config_dir = TempDir {
        path: std::env::temp_dir().join(format!("cp_http_{}", std::process::id())),
    };
    let router = control_plane_router(cp, config_dir.path.clone());

    // Bind first, then serve, so requests can connect immediately.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // POST /agents — deploy from rendered TOML.
    let toml = r#"
[agent]
name = "Http Agent"

[handler]
type = "echo"

[server]
host = "127.0.0.1"
http_port = 8200
"#;
    let resp = client
        .post(format!("{base}/agents"))
        .json(&serde_json::json!({ "config_toml": toml }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let deployed: DeployedAgent = resp.json().await.unwrap();
    assert_eq!(deployed.id, "http-agent");
    assert_eq!(deployed.endpoint, "http://127.0.0.1:8200");
    assert_eq!(deployed.health, RuntimeHealth::Healthy);

    // GET /agents — lists the deployed agent as Healthy.
    let listed: Vec<DeployedAgent> = client
        .get(format!("{base}/agents"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].health, RuntimeHealth::Healthy);

    // GET /agents/:id — health of a single agent.
    let resp = client
        .get(format!("{base}/agents/http-agent"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // GET an unknown agent → 404.
    let resp = client
        .get(format!("{base}/agents/nope"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    // DELETE /agents/:id — undeploy.
    let resp = client
        .delete(format!("{base}/agents/http-agent"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // It is now Stopped.
    let listed: Vec<DeployedAgent> = client
        .get(format!("{base}/agents"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed[0].health, RuntimeHealth::Stopped);
}
