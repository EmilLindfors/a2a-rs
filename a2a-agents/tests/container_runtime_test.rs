//! Live end-to-end test of [`ContainerRuntime`].
//!
//! Requires a working `docker` and a built `a2a-agents:latest` image
//! (`docker build -t a2a-agents:latest -f a2a-agents/Dockerfile .` from the
//! workspace root). When either is absent — CI, this sandbox — the test prints a
//! skip notice and returns green, so it never blocks the suite. It exercises the
//! real container lifecycle: provision (`docker create`) → start → poll health
//! (card probe through the published port) → recover from a *fresh* runtime
//! (the restart case) → stop.

use std::time::Duration;

use a2a_agents::{
    AgentRuntime, AgentSpec, ContainerRuntime, EnvAllowlist, Recovered, RuntimeHealth,
};

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

/// The recovery query against a *real* engine, with no image and no container
/// required — so it runs anywhere docker does, not only where the agent image
/// has been built.
///
/// Its whole job is the half that unit tests cannot reach: that `--filter` and
/// the `--format` Go template are things the engine actually accepts. A typo in
/// either is a runtime `Backend` error, and it would only ever surface on the
/// restart path — i.e. once, in production, at the worst moment.
#[tokio::test]
async fn recover_query_is_accepted_by_a_real_engine() {
    if !docker_available() {
        eprintln!("skipping recover_query_is_accepted_by_a_real_engine: docker not available");
        return;
    }

    match ContainerRuntime::new().recover().await {
        // Content depends on what is on the machine; acceptance does not.
        Ok(Recovered::Adopted(_)) => {}
        other => panic!("the engine rejected the recovery query: {other:?}"),
    }
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

    // A "secret" that exists only in this process's env — the TOML references it
    // as `${VAR}`, and the runtime must pass it through into the container for
    // the in-container `a2a run` to expand.
    // SAFETY: test-only var, unique name, set before any threads read the env
    // concurrently in ways that matter here.
    unsafe {
        std::env::set_var("A2A_CONTAINER_TEST_SECRET", "injected-from-host");
    }

    // Config omits `host` so the in-container HOST=0.0.0.0 binds all interfaces.
    let config_path = std::env::temp_dir().join(format!("container_test_{port}.toml"));
    std::fs::write(
        &config_path,
        format!(
            r#"
[agent]
name = "Container Test Agent"
description = "${{A2A_CONTAINER_TEST_SECRET}}"

[handler]
type = "echo"

[server]
http_port = {port}
"#
        ),
    )
    .unwrap();

    // The operator explicitly permits this one variable; anything else the
    // config named would be refused at provision.
    let rt =
        ContainerRuntime::new().with_allowed_env(EnvAllowlist::new(["A2A_CONTAINER_TEST_SECRET"]));
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
    assert_eq!(
        health,
        RuntimeHealth::Healthy,
        "agent should become Healthy"
    );

    // The env ref was expanded *inside* the container from the passed-through
    // var — the on-disk TOML only ever held `${A2A_CONTAINER_TEST_SECRET}`.
    let card = a2a_rs::fetch_agent_card(&format!("http://127.0.0.1:{port}"))
        .await
        .expect("fetch agent card");
    assert_eq!(
        card.description, "injected-from-host",
        "container should expand env refs from injected pass-through vars"
    );

    // Restart-recovery: a *brand-new* runtime, as after a control-plane bounce.
    // Its map starts empty, so everything below works only if the engine's
    // labels really are the durable store.
    let restarted = ContainerRuntime::new();
    let Recovered::Adopted(adopted) = restarted.recover().await.expect("recover") else {
        panic!("the container runtime is durable and must report Adopted");
    };
    assert!(
        adopted.contains(&id),
        "recovery must find the running container: adopted {adopted:?}"
    );
    assert_eq!(
        restarted.health(&id).await.unwrap(),
        RuntimeHealth::Healthy,
        "a recovered agent must be health-checkable, port and all"
    );
    assert!(
        restarted
            .list()
            .await
            .unwrap()
            .iter()
            .any(|s| s.id == id && s.endpoint == format!("http://127.0.0.1:{port}")),
        "the published port must be recovered too, or the endpoint is wrong"
    );

    // Stopping through the recovered runtime proves adoption is real management,
    // not just visibility.
    restarted.stop(&id).await.expect("stop after recovery");
    assert_eq!(rt.health(&id).await.unwrap(), RuntimeHealth::Stopped);

    // A stopped container is still managed: `ps -a` keeps it, so a later restart
    // can still see (and clean up) it.
    let stopped = ContainerRuntime::new();
    assert!(
        stopped.recover().await.unwrap().adopted().contains(&id),
        "recovery must adopt stopped containers too, or they become unmanageable"
    );

    // Best-effort cleanup of the container and temp config.
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", &format!("a2a-agent-{id}")])
        .output();
    let _ = std::fs::remove_file(&config_path);
}
