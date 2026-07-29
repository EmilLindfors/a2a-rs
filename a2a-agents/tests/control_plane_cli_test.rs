//! `a2a deploy` / `ps` / `logs` / `stop` end-to-end against a real control plane.
//!
//! `control_plane_test.rs` drives the API and the client in-process. This drives
//! the two *binaries*, because the gap being closed here was that nothing but
//! `curl` could work a running control plane — so the thing worth pinning is
//! that the commands a person types find it, authenticate to it, and report what
//! it says. Everything is real: a supervising `a2a control-plane` process, a
//! supervised `a2a run` child under it, and an agent card probed over a socket.
//!
//! Credentials and address are passed only through `A2A_CONTROL_PLANE_TOKEN` /
//! `A2A_CONTROL_PLANE_URL`, never `--token` / `--url`, since the env path is
//! the one an operator is told to prefer (an argv token is visible to `ps`).
//!
//! Gated on the `a2a` binary's required features so `CARGO_BIN_EXE_a2a` exists.

#![cfg(all(feature = "mcp-server", feature = "schema"))]

mod common;

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::ScratchDir;

/// The token the test control plane requires.
const TOKEN: &str = "cli-test-token";

/// How long to wait for the control plane to bind, and for a deployed agent to
/// answer its card probe. Generous because a debug-build child has to start,
/// parse, and bind before either can succeed.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Run the `a2a` binary in `dir` with `envs` set, returning (success, output).
///
/// Env is set per-invocation rather than on the test process: `set_var` is
/// process-global (and `unsafe` in this edition), and these tests exist to check
/// what a *child* sees.
fn a2a_env(dir: &Path, envs: &[(&str, &str)], args: &[&str]) -> (bool, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_a2a"));
    command.current_dir(dir).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let out = command.output().expect("run a2a binary");
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), combined)
}

/// An address nothing is listening on, claimed by binding and releasing it.
///
/// Racy in principle; in practice the OS does not hand the same ephemeral port
/// out twice in the moment between here and the child binding it.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

/// A supervising `a2a control-plane` process, killed when the test ends.
///
/// `--runtime local` on purpose: it is the backend someone reaches for first,
/// and it is the one whose logs only exist because the control plane captures
/// them (a container engine keeps its own). Its output is redirected to a file
/// so a failure to start can be reported instead of vanishing.
struct ControlPlaneProcess {
    child: Child,
    url: String,
    log: std::path::PathBuf,
}

impl ControlPlaneProcess {
    fn start(dir: &Path) -> Self {
        let port = free_port();
        let log = dir.join("control-plane.out");
        let out = std::fs::File::create(&log).expect("create control-plane log");
        let err = out.try_clone().expect("clone control-plane log handle");
        let child = Command::new(env!("CARGO_BIN_EXE_a2a"))
            .current_dir(dir)
            .args([
                "control-plane",
                "--bind",
                &format!("127.0.0.1:{port}"),
                "--runtime",
                "local",
                "--config-dir",
                "./deployed",
            ])
            .env("A2A_CONTROL_PLANE_TOKEN", TOKEN)
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .expect("spawn a2a control-plane");
        Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
            log,
        }
    }

    /// Whatever the control plane printed so far — only read to explain a
    /// failure, since the child still holds the file open.
    fn output(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for ControlPlaneProcess {
    fn drop(&mut self) {
        // The control plane kills its own children on drop, so this takes the
        // deployed agents with it.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A test agent whose port is claimed the same way the control plane's is, so
/// concurrent runs of this suite do not collide.
fn agent_config(name: &str) -> String {
    format!(
        r#"
[agent]
name = "{name}"
description = "Deployed by the CLI test."

[handler]
type = "echo"

[server]
host = "127.0.0.1"
http_port = {}

[[skills]]
id = "cli-echo"
name = "Echo"
"#,
        free_port()
    )
}

/// Poll `a2a <args>` until `ready` accepts its output, returning that output.
///
/// Polling rather than sleeping a fixed amount: how long a debug-build child
/// takes to bind is a property of the machine, and a fixed wait is either flaky
/// or slow.
fn poll_until(
    dir: &Path,
    envs: &[(&str, &str)],
    args: &[&str],
    what: &str,
    ready: impl Fn(bool, &str) -> bool,
) -> String {
    let deadline = Instant::now() + READY_TIMEOUT;
    let mut last = String::new();
    while Instant::now() < deadline {
        let (ok, out) = a2a_env(dir, envs, args);
        if ready(ok, &out) {
            return out;
        }
        last = out;
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!(
        "timed out waiting for {what}; last `a2a {}`:\n{last}",
        args.join(" ")
    );
}

#[test]
fn deploy_ps_logs_stop_drive_a_running_control_plane() {
    let scratch = ScratchDir::new("cp_cli");
    scratch.write("echo.toml", &agent_config("Cli Agent"));

    let cp = ControlPlaneProcess::start(scratch.path());
    let env = [
        ("A2A_CONTROL_PLANE_URL", cp.url.as_str()),
        ("A2A_CONTROL_PLANE_TOKEN", TOKEN),
    ];

    // `ps` succeeding at all means the API bound and the token was accepted.
    let listed = poll_until(
        scratch.path(),
        &env,
        &["ps"],
        &format!("the control plane to bind (see {})", cp.log.display()),
        |ok, _| ok,
    );
    assert!(
        listed.contains("no agents running"),
        "a fresh control plane has an empty fleet, and should say so plainly:\n{listed}\n\
         control plane said:\n{}",
        cp.output()
    );

    // Deploy. The agent is not up the instant this returns — it has to be
    // spawned and bind — so health is polled rather than asserted here.
    let (ok, out) = a2a_env(scratch.path(), &env, &["deploy", "--config", "echo.toml"]);
    assert!(
        ok,
        "deploy failed:\n{out}\ncontrol plane said:\n{}",
        cp.output()
    );
    assert!(
        out.contains("cli-agent"),
        "deploy must report the id it gave the agent:\n{out}"
    );

    let listed = poll_until(
        scratch.path(),
        &env,
        &["ps"],
        "the deployed agent to answer its card probe",
        |ok, out| ok && out.contains("healthy"),
    );
    assert!(listed.contains("cli-agent"), "{listed}");

    // The agent's own output, captured by the control plane and served back.
    // Poll: the log file is written by a child that has only just started.
    let logs = poll_until(
        scratch.path(),
        &env,
        &["logs", "cli-agent"],
        "the deployed agent to log something",
        |ok, out| ok && out.contains("Cli Agent"),
    );
    assert!(
        !logs.contains("\u{1b}["),
        "captured logs must not carry terminal escape codes:\n{logs}"
    );

    // `--tail` is the control plane's to apply, so a small one must come back
    // shorter than the whole log.
    let (ok, tailed) = a2a_env(scratch.path(), &env, &["logs", "cli-agent", "--tail", "2"]);
    assert!(ok, "{tailed}");
    assert!(
        tailed.lines().count() <= 2,
        "--tail 2 returned {} lines:\n{tailed}",
        tailed.lines().count()
    );

    // Stop, and see it reflected — not merely accepted.
    let (ok, out) = a2a_env(scratch.path(), &env, &["stop", "cli-agent"]);
    assert!(ok, "stop failed:\n{out}");
    assert!(out.contains("stopped cli-agent"), "{out}");

    // It leaves the listing: an undeployed agent that keeps showing up in `ps`
    // reads as one that refused to go away.
    let (ok, listed) = a2a_env(scratch.path(), &env, &["ps"]);
    assert!(ok, "{listed}");
    assert!(
        !listed.contains("cli-agent"),
        "a stopped agent is not part of what is running:\n{listed}"
    );

    // …but it is not forgotten. `--all` is `docker ps -a`, and it is what makes
    // the log of an agent that died still reachable by id.
    let (ok, listed) = a2a_env(scratch.path(), &env, &["ps", "--all"]);
    assert!(ok, "{listed}");
    assert!(
        listed.contains("cli-agent") && listed.contains("stopped"),
        "--all must still show it, as stopped:\n{listed}"
    );
    let (ok, logs) = a2a_env(scratch.path(), &env, &["logs", "cli-agent"]);
    assert!(ok, "a stopped agent's log is still readable:\n{logs}");
}

/// The failure an operator hits first: no control plane at that address. It has
/// to say so, and say what to start — a bare connection error reads as a bug in
/// the CLI.
#[test]
fn a_missing_control_plane_is_reported_not_just_failed() {
    let scratch = ScratchDir::new("cp_cli_down");
    let url = format!("http://127.0.0.1:{}", free_port());
    let (ok, out) = a2a_env(
        scratch.path(),
        &[("A2A_CONTROL_PLANE_URL", url.as_str())],
        &["ps"],
    );
    assert!(!ok, "reaching nothing must fail:\n{out}");
    assert!(
        out.contains(&url),
        "the message must name what was dialled:\n{out}"
    );
    assert!(
        out.contains("a2a control-plane"),
        "and what to start:\n{out}"
    );
}

/// Cross-agent conflicts are caught before anything is sent. A port clash is
/// silent-wrong once deployed — one agent quietly fails to bind — and finding it
/// halfway through a fleet leaves a partial rollout to unpick.
#[test]
fn conflicting_configs_are_rejected_before_any_deploy() {
    let scratch = ScratchDir::new("cp_cli_conflict");
    let port = free_port();
    for file in ["one.toml", "two.toml"] {
        scratch.write(
            file,
            &format!(
                r#"
[agent]
name = "{file}"

[handler]
type = "echo"

[server]
host = "127.0.0.1"
http_port = {port}
"#
            ),
        );
    }

    // No control plane is running: if the check did not come first, this would
    // fail with a connection error instead of naming the conflict.
    let (ok, out) = a2a_env(
        scratch.path(),
        &[(
            "A2A_CONTROL_PLANE_URL",
            format!("http://127.0.0.1:{}", free_port()).as_str(),
        )],
        &["deploy", "--config", "one.toml", "--config", "two.toml"],
    );
    assert!(!ok, "a port clash must not deploy:\n{out}");
    assert!(out.contains("conflict"), "{out}");
    assert!(
        out.contains(&port.to_string()),
        "the message must name the contested port:\n{out}"
    );
    assert!(
        out.contains("nothing was deployed"),
        "and be clear that no partial rollout happened:\n{out}"
    );
}
