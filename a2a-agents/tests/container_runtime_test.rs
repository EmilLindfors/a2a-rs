//! Live end-to-end test of [`ContainerRuntime`].
//!
//! Requires a working `docker` and a built `a2a-agents:latest` image
//! (`docker build -t a2a-agents:latest -f a2a-agents/Dockerfile .` from the
//! workspace root). When either is absent — CI, this sandbox — the test prints a
//! skip notice and returns green, so it never blocks the suite. It exercises the
//! real container lifecycle: provision (`docker create`) → start → poll health
//! (card probe through the published port) → stop.

use std::time::Duration;

use a2a_agents::{AgentRuntime, AgentSpec, ContainerRuntime, RuntimeHealth};

const IMAGE: &str = "a2a-agents:latest";

/// True if `docker version` succeeds.
fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// True if the base image is present locally.
fn image_available(image: &str) -> bool {
    std::process::Command::new("docker")
        .args(["image", "inspect", image])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn container_runtime_full_lifecycle() {
    if !docker_available() {
        eprintln!("skipping container_runtime_full_lifecycle: docker not available");
        return;
    }
    if !image_available(IMAGE) {
        eprintln!("skipping container_runtime_full_lifecycle: image '{IMAGE}' not built");
        return;
    }

    // A free port the container publishes; written into the agent config.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    // Config omits `host` so the in-container HOST=0.0.0.0 binds all interfaces.
    let config_path = std::env::temp_dir().join(format!("container_test_{port}.toml"));
    std::fs::write(
        &config_path,
        format!(
            r#"
[agent]
name = "Container Test Agent"

[handler]
type = "echo"

[server]
http_port = {port}
"#
        ),
    )
    .unwrap();

    let rt = ContainerRuntime::new();
    let spec = AgentSpec::from_config_path(&config_path).expect("spec from config");
    let id = rt.provision(spec).await.expect("provision");

    assert_eq!(
        rt.health(&id).await.unwrap(),
        RuntimeHealth::Provisioned,
        "a created-but-unstarted container is Provisioned"
    );

    rt.start(&id).await.expect("start");

    let mut health = RuntimeHealth::Provisioned;
    for _ in 0..60 {
        health = rt.health(&id).await.unwrap();
        if health == RuntimeHealth::Healthy {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert_eq!(health, RuntimeHealth::Healthy, "agent should become Healthy");

    rt.stop(&id).await.expect("stop");
    assert_eq!(rt.health(&id).await.unwrap(), RuntimeHealth::Stopped);

    // Best-effort cleanup of the container and temp config.
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", &format!("a2a-agent-{id}")])
        .output();
    let _ = std::fs::remove_file(&config_path);
}
