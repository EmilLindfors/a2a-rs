//! `a2a run` against the real binary, for what only the running process shows.
//!
//! Gated on the `a2a` binary's required features so `CARGO_BIN_EXE_a2a` exists.

#![cfg(all(feature = "mcp-server", feature = "schema"))]

mod common;

use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::ScratchDir;

/// A port nothing is listening on, by taking one and giving it straight back.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("read local addr")
        .port()
}

/// Wait up to `budget` for `child` to exit. `None` means it is still running,
/// which for `a2a run` means it started serving.
fn wait_for_exit(child: &mut Child, budget: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        match child.try_wait().expect("poll a2a run") {
            Some(status) => return Some(status),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    None
}

/// An `llm` agent whose provider comes from the environment.
fn llm_agent(scratch: &ScratchDir, file: &str) {
    scratch.write(
        file,
        &format!(
            r#"
[agent]
name = "Chat"

[server]
host = "127.0.0.1"
http_port = {}

[handler]
type = "llm"
"#,
            free_port()
        ),
    );
}

/// A provider that is configured and unusable stops the run. It used to warn
/// once and fall through to the non-LLM fallback, so the agent came up healthy,
/// answered every message with the stub reply, and nothing said why.
#[test]
fn an_unusable_provider_stops_the_run() {
    let scratch = ScratchDir::new("runllm");
    llm_agent(&scratch, "chat.toml");

    let mut child = Command::new(env!("CARGO_BIN_EXE_a2a"))
        .current_dir(scratch.path())
        .args(["run", "--config", "chat.toml"])
        .env("OPENROUTER_API_KEY", "sk-or-test")
        .env("OPENROUTER_REASONING", "verry-high")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn a2a run");

    let status = wait_for_exit(&mut child, Duration::from_secs(30));
    // Both streams: the failure is a report (stdout) and the context around it
    // is `tracing` (stderr).
    let mut printed = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut printed);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut printed);
    }

    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        panic!("`a2a run` served an agent whose provider cannot be built");
    };
    assert!(
        !status.success(),
        "an unusable provider must fail the run:\n{printed}"
    );
    assert!(
        printed.contains("OPENROUTER_REASONING"),
        "the failure must name the setting that caused it:\n{printed}"
    );
}
