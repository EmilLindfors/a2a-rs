//! Live end-to-end test of the [`AgentRuntime`] port via [`LocalProcessRuntime`].
//!
//! Drives a *real* agent through its full managed lifecycle: provision → start
//! (spawning an actual `a2a run --config …` child process) → poll health until
//! the agent card answers → stop → confirm it's down and its port freed. This
//! proves the Pillar 3 keystone: agents run as supervised, health-checked units
//! behind the runtime port, not as an in-process fan-out.
//!
//! Gated on the `a2a` binary's required features so `CARGO_BIN_EXE_a2a` exists.

#![cfg(all(feature = "mcp-server", feature = "schema"))]

use std::time::Duration;

use a2a_agents::{AgentRuntime, AgentSpec, LocalProcessRuntime, RuntimeHealth};

/// Grab a currently-free TCP port by binding to `:0` and releasing it. The agent
/// child re-binds it moments later; the window is small enough for a local test.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// A minimal echo-agent config written to a temp file the child process reads.
struct TempConfig {
    path: std::path::PathBuf,
}

impl TempConfig {
    fn echo_agent(port: u16) -> Self {
        let path = std::env::temp_dir().join(format!("a2a_runtime_test_{port}.toml"));
        let toml = format!(
            r#"
[agent]
name = "Runtime Test Agent"

[handler]
type = "echo"

[server]
host = "127.0.0.1"
http_port = {port}
"#
        );
        std::fs::write(&path, toml).expect("write temp config");
        Self { path }
    }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Poll `health(id)` until it equals `want` or the attempts run out.
async fn wait_for_health(
    rt: &LocalProcessRuntime,
    id: &a2a_agents::AgentId,
    want: RuntimeHealth,
    attempts: u32,
) -> RuntimeHealth {
    let mut last = RuntimeHealth::Provisioned;
    for _ in 0..attempts {
        last = rt
            .health(id)
            .await
            .expect("health is infallible for a known id");
        if last == want {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    last
}

#[tokio::test]
async fn supervises_agent_through_full_lifecycle() {
    let port = free_port();
    let config = TempConfig::echo_agent(port);
    let endpoint = format!("http://127.0.0.1:{port}");

    let rt = LocalProcessRuntime::with_exe(env!("CARGO_BIN_EXE_a2a"));
    let spec = AgentSpec::from_config_path(&config.path).expect("spec from config");
    assert_eq!(spec.id.as_str(), "runtime-test-agent");
    assert_eq!(spec.endpoint, endpoint);

    // provision: known to the runtime, not yet running.
    let id = rt.provision(spec).await.expect("provision");
    assert_eq!(
        rt.health(&id).await.unwrap(),
        RuntimeHealth::Provisioned,
        "an un-started agent is Provisioned"
    );

    // start: spawns the real child, which binds the port and serves its card.
    rt.start(&id).await.expect("start");
    let health = wait_for_health(&rt, &id, RuntimeHealth::Healthy, 40).await;
    assert_eq!(
        health,
        RuntimeHealth::Healthy,
        "the agent should become Healthy once its card answers"
    );

    // The card is independently reachable, and list() reports the running agent.
    a2a_rs::fetch_agent_card(&endpoint)
        .await
        .expect("agent card should be fetchable while Healthy");
    let listed = rt.list().await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].health, RuntimeHealth::Healthy);

    // starting an already-running agent is rejected.
    assert!(
        rt.start(&id).await.is_err(),
        "starting a live agent must error (AlreadyRunning)"
    );

    // stop: the agent goes down and its card stops answering.
    rt.stop(&id).await.expect("stop");
    assert_eq!(
        rt.health(&id).await.unwrap(),
        RuntimeHealth::Stopped,
        "a stopped agent reports Stopped"
    );

    // the listening port is released once the child dies (bounded retry to allow
    // the OS a beat to reclaim it).
    let mut rebound = false;
    for _ in 0..20 {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            rebound = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(rebound, "the agent's port should be free after stop");
}
