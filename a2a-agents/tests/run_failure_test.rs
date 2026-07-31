//! What `a2a run` does when an agent cannot start.
//!
//! `fleet_test.rs` covers the checks that run *before* anything binds. This
//! covers the case those checks cannot prevent: the port was free when the
//! config was written and taken by the time the agent reached for it.
//!
//! The contract worth pinning is the exit code. `a2a run` is what supervisors
//! invoke — systemd, a container entrypoint, and this tool's own
//! `LocalProcessRuntime` — and none of them read the log stream. An agent that
//! never bound must not look like a clean start.
//!
//! Gated on the `a2a` binary's required features so `CARGO_BIN_EXE_a2a` exists.

#![cfg(all(feature = "mcp-server", feature = "schema"))]

mod common;

use std::net::TcpListener;

use common::ScratchDir;

/// Hold a port for the duration of a test, and say which one.
///
/// Bound on port 0 so the OS picks something free: hard-coding one would make
/// the test flaky against whatever else is running on the machine.
fn occupied_port() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let port = listener.local_addr().expect("read bound address").port();
    (listener, port)
}

#[test]
fn an_agent_that_cannot_bind_fails_the_command() {
    // Held for the whole test: dropping it would free the port and let the
    // agent start after all.
    let (_held, port) = occupied_port();

    let scratch = ScratchDir::new("run_bind");
    scratch.agent("Doomed", port, "doomed.toml");

    let (ok, out) = scratch.a2a(&["run", "--config", "doomed.toml"]);

    assert!(
        !ok,
        "an agent that never bound must exit non-zero, or every supervisor \
         reads it as a clean start:\n{out}"
    );
    assert!(
        out.contains("doomed.toml"),
        "the failure must name the config that failed:\n{out}"
    );
    assert!(
        out.contains("1 of 1 agent(s) stopped early"),
        "the summary must say how many agents stopped:\n{out}"
    );
}

/// A config that does not load is a different failure with the same contract:
/// nothing started, so the command must not report success.
///
/// `nope` is rejected because the config structs are `deny_unknown_fields` — a
/// mistyped key is an error rather than a silently ignored line.
#[test]
fn an_unloadable_config_fails_the_command() {
    let scratch = ScratchDir::new("run_config");
    scratch.write(
        "broken.toml",
        "[agent]\nname = \"Broken\"\ndescription = \"d\"\nnope = true\n",
    );

    let (ok, out) = scratch.a2a(&["run", "--config", "broken.toml"]);

    assert!(!ok, "a config that cannot load must fail the run:\n{out}");
    assert!(
        out.contains("broken.toml"),
        "the failure must name the config:\n{out}"
    );
}
