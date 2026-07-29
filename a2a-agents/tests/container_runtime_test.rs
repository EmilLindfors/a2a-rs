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
    AgentRuntime, AgentSpec, ContainerHardening, ContainerRuntime, EnvAllowlist, Recovered,
    RuntimeHealth,
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

/// Everything the `a2a` binary requires must be a **default** feature.
///
/// Cargo refuses to build a named bin whose required features are not all
/// enabled, and it silently *skips* such a bin on `cargo install` rather than
/// reporting it. Making `required-features` a subset of `default` is what makes
/// every way of obtaining the binary work with no `--features` incantation:
/// `cargo install a2a-agents`, the Dockerfile, and the release-binaries
/// workflow. Each of those had already drifted from the list at least once —
/// the release workflow shipped without `llm` and `schema`, and the image
/// silently had none at all after `llm` was added here and not there.
///
/// This is also the one failure the docker-gated tests below structurally
/// cannot catch: they skip green when the image is absent, and an image that
/// cannot be built is absent.
///
/// Parsed rather than hard-coded, so adding a feature to either side is what
/// fails, not a stale copy of the list in a test.
#[test]
fn every_feature_the_a2a_binary_requires_is_a_default() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read Cargo.toml");

    /// The features named by a `<key> = ["a", "b"]` line, wherever it appears.
    fn feature_list<'a>(manifest: &'a str, key: &str) -> Vec<&'a str> {
        let prefix = format!("{key} = [");
        manifest
            .lines()
            .find_map(|line| line.trim().strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("no `{key}` list in the manifest"))
            .trim_end_matches(']')
            .split(',')
            .map(|feature| feature.trim().trim_matches('"'))
            .filter(|feature| !feature.is_empty())
            .collect()
    }

    // Scoped to the `[[bin]] name = "a2a"` section: other targets have their own
    // `required-features`, and `name = "a2a"` is a prefix of `name = "a2a_x"`.
    let bin_section = manifest
        .split("[[bin]]")
        .find(|section| section.lines().any(|line| line.trim() == r#"name = "a2a""#))
        .expect("the a2a bin section");

    let required = feature_list(bin_section, "required-features");
    let default = feature_list(&manifest, "default");
    assert!(!required.is_empty(), "parsed no required features");

    for feature in &required {
        assert!(
            default.contains(feature),
            "the `a2a` binary requires `{feature}`, which is not a default feature — \
             `cargo install a2a-agents` will silently skip the binary, and `docker build` \
             will fail with \"target `a2a` requires the features\". Defaults: {default:?}"
        );
    }
}

/// The Dockerfile must not pin its own feature list.
///
/// Passing `--features` there means the image is a variant only that file knows
/// how to build, which is how it drifted from `required-features` before. With
/// the defaults covering the binary (above), the correct invocation names no
/// features at all — and then it cannot drift.
#[test]
fn the_dockerfile_builds_the_binary_with_default_features() {
    let dockerfile = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Dockerfile"))
        .expect("read Dockerfile");

    let pinned: Vec<&str> = dockerfile
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("--features") || line.contains(" --features "))
        .collect();

    assert!(
        pinned.is_empty(),
        "the Dockerfile pins features instead of relying on the defaults, so the image \
         can drift from what `cargo install` produces: {pinned:?}"
    );
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

/// `docker inspect -f <template> <container>`, trimmed.
fn inspect(container: &str, template: &str) -> String {
    let out = std::process::Command::new("docker")
        .args(["inspect", "-f", template, container])
        .output()
        .expect("run docker inspect");
    assert!(
        out.status.success(),
        "docker inspect {template} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A hardened agent still comes up.
///
/// The unit tests pin which flags are emitted; they cannot tell you the result
/// runs. That is the failure mode hardening actually has — a root filesystem
/// mounted read-only under a process that needed to write, or a dropped
/// capability something quietly relied on, both of which look like an agent
/// that never becomes `Healthy` and say nothing about why. So this asserts the
/// agent answers its card *and* that the engine really applied the restrictions
/// (a flag it ignored would pass the argv tests and protect nothing).
#[tokio::test]
async fn a_hardened_agent_still_serves_and_the_engine_applied_the_flags() {
    if !docker_available() {
        eprintln!("skipping a_hardened_agent_still_serves: docker not available");
        return;
    }
    if !image_available(IMAGE) {
        eprintln!("skipping a_hardened_agent_still_serves: image '{IMAGE}' not built");
        return;
    }

    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config_path = std::env::temp_dir().join(format!("container_hardened_{port}.toml"));
    // In-memory storage, so the read-only root filesystem applies — the strictest
    // policy this runtime will impose on anything.
    std::fs::write(
        &config_path,
        format!(
            r#"
[agent]
name = "Hardened Agent {port}"

[handler]
type = "echo"

[server]
http_port = {port}

[server.storage]
type = "inmemory"
"#
        ),
    )
    .unwrap();

    let rt = ContainerRuntime::new().with_hardening(
        ContainerHardening::default()
            .with_memory("256m")
            .with_cpus("1"),
    );
    let spec = AgentSpec::from_config_path(&config_path).expect("spec from config");
    let id = rt.provision(spec).await.expect("provision");
    rt.start(&id).await.expect("start");

    let mut health = RuntimeHealth::Provisioned;
    for _ in 0..60 {
        health = rt.health(&id).await.unwrap();
        if health == RuntimeHealth::Healthy {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let container = format!("a2a-agent-{id}");
    assert_eq!(
        health,
        RuntimeHealth::Healthy,
        "a fully hardened agent must still serve its card; container logs:\n{}",
        String::from_utf8_lossy(
            &std::process::Command::new("docker")
                .args(["logs", &container])
                .output()
                .map(|o| [o.stdout, o.stderr].concat())
                .unwrap_or_default()
        )
    );

    // The engine's own view — proof the flags were applied, not merely passed.
    assert_eq!(
        inspect(&container, "{{.HostConfig.ReadonlyRootfs}}"),
        "true"
    );
    assert_eq!(inspect(&container, "{{.HostConfig.CapDrop}}"), "[ALL]");
    assert_eq!(inspect(&container, "{{.HostConfig.PidsLimit}}"), "512");
    assert!(
        inspect(&container, "{{.HostConfig.SecurityOpt}}").contains("no-new-privileges"),
        "no-new-privileges must reach the engine"
    );
    // 256m in bytes; the engine normalizes the notation we passed through.
    assert_eq!(
        inspect(&container, "{{.HostConfig.Memory}}"),
        (256 * 1024 * 1024).to_string()
    );

    // The image runs unprivileged, which is the half of this that lives in the
    // Dockerfile rather than in `create_args`.
    let user = inspect(&container, "{{.Config.User}}");
    assert!(
        user != "0" && user != "root" && !user.is_empty(),
        "the agent image must not run as root, got {user:?}"
    );

    rt.stop(&id).await.expect("stop");
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", &container])
        .output();
    let _ = std::fs::remove_file(&config_path);
}
