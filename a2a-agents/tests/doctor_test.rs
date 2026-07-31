//! `a2a doctor` end-to-end against the real binary.
//!
//! The unit tests in `core::doctor` prove which requirements a config implies.
//! This drives the command that checks them against a real machine — a port that
//! is genuinely taken, a command that is genuinely absent — because the value of
//! a pre-flight check is entirely in whether it notices.
//!
//! Gated on the `a2a` binary's required features so `CARGO_BIN_EXE_a2a` exists.

#![cfg(all(feature = "mcp-server", feature = "schema"))]

mod common;

use std::net::TcpListener;

use common::ScratchDir;

/// A port nothing is listening on, by taking one and giving it straight back.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("read local addr")
        .port()
}

#[test]
fn a_runnable_agent_is_all_clear() {
    let scratch = ScratchDir::new("clear");
    scratch.agent("Weather", free_port(), "weather.toml");

    let (ok, out) = scratch.a2a(&["doctor", "--config", "weather.toml"]);
    assert!(ok, "a scaffolded echo agent must be runnable:\n{out}");
    assert!(out.contains("is free"), "{out}");
    assert!(out.contains("all clear"), "{out}");
}

/// "All clear" has to mean the same thing on every machine. It did not: the
/// environment report warned about a missing model key and a missing container
/// engine whether or not anything being checked wanted one, so an echo agent
/// came back clean on a laptop with `OPENAI_API_KEY` exported and warned on CI.
/// A check that depends on the host's unrelated state is one people learn to
/// ignore — so absence is judged against what the config asks for.
#[test]
fn an_echo_agent_is_clear_without_a_model_key() {
    let scratch = ScratchDir::new("nokey");
    scratch.agent("Weather", free_port(), "weather.toml");

    let (ok, out) = scratch.a2a_env(
        &["doctor", "--config", "weather.toml"],
        // Every var `llm_env_var` consults, cleared: the shape of a machine that
        // has never configured a model provider.
        &[
            "OPENROUTER_API_KEY",
            "GEMINI_API_KEY",
            "OPENAI_API_KEY",
            "AI_API_KEY",
            "OPENAI_API_BASE_URL",
            "AI_API_BASE_URL",
        ],
    );

    assert!(ok, "an echo agent needs no model key:\n{out}");
    assert!(
        out.contains("all clear"),
        "an echo agent must not be warned about a provider it never calls:\n{out}"
    );
    assert!(
        !out.contains("no model key"),
        "the warning belongs to `llm` handlers, not to every run:\n{out}"
    );
}

/// The check that earns the command: the config is valid and the run still
/// cannot work, because something else already holds the port.
#[test]
fn an_occupied_port_is_a_problem() {
    let scratch = ScratchDir::new("port");
    let listener = TcpListener::bind("127.0.0.1:0").expect("hold a port");
    let port = listener.local_addr().unwrap().port();
    scratch.agent("Weather", port, "weather.toml");

    let (ok, out) = scratch.a2a(&["doctor", "--config", "weather.toml"]);
    assert!(!ok, "a taken port must fail the check:\n{out}");
    assert!(
        out.contains(&format!("cannot bind 127.0.0.1:{port}")),
        "the report must name the address:\n{out}"
    );
    drop(listener);
}

/// An MCP server whose command is not installed: the agent starts fine and its
/// tools silently are not there, which is the confusing symptom this replaces.
#[test]
fn a_missing_mcp_command_is_a_problem() {
    let scratch = ScratchDir::new("mcp");
    scratch.write(
        "tools.toml",
        &format!(
            r#"
[agent]
name = "Tooling"

[server]
host = "127.0.0.1"
http_port = {}

[features.mcp_client]
enabled = true

[[features.mcp_client.servers]]
name = "filesystem"
command = "a2a-definitely-not-installed"
"#,
            free_port()
        ),
    );

    let (ok, out) = scratch.a2a(&["doctor", "--config", "tools.toml"]);
    assert!(!ok, "a missing MCP command must fail the check:\n{out}");
    assert!(out.contains("a2a-definitely-not-installed"), "{out}");
    assert!(out.contains("filesystem"), "{out}");
}

/// An unknown handler falls back to echo at runtime, so the agent answers —
/// just not the way the config says. Silent-wrong, hence a problem.
#[test]
fn an_unknown_handler_is_a_problem() {
    let scratch = ScratchDir::new("handler");
    scratch.write(
        "custom.toml",
        &format!(
            r#"
[agent]
name = "Custom"

[server]
host = "127.0.0.1"
http_port = {}

[handler]
type = "weather"
"#,
            free_port()
        ),
    );

    let (ok, out) = scratch.a2a(&["doctor", "--config", "custom.toml"]);
    assert!(!ok, "an unknown handler must fail the check:\n{out}");
    assert!(out.contains("weather"), "{out}");
}

/// Each config can be perfectly fine on its own and still not run alongside the
/// others — the reason `doctor` looks at the whole set it was given.
#[test]
fn configs_that_cannot_run_together_are_a_problem() {
    let scratch = ScratchDir::new("together");
    let port = free_port();
    scratch.agent("Weather", port, "weather.toml");
    scratch.agent("Billing", port, "billing.toml");

    let (ok, out) = scratch.a2a(&[
        "doctor",
        "--config",
        "weather.toml",
        "--config",
        "billing.toml",
    ]);
    assert!(!ok, "two agents on one port must fail the check:\n{out}");
    assert!(out.contains("together"), "{out}");
    assert!(out.contains(&port.to_string()), "{out}");
}

/// An unset `${VAR}` is exactly the difference between `validate` (shape only,
/// deliberately lenient) and `doctor` (this machine, right now): `a2a run`
/// refuses to start until it resolves.
#[test]
fn an_unset_env_reference_is_a_problem() {
    let scratch = ScratchDir::new("env");
    scratch.write(
        "secretive.toml",
        &format!(
            r#"
[agent]
name = "Secretive"
description = "${{A2A_DOCTOR_DEFINITELY_UNSET}}"

[server]
host = "127.0.0.1"
http_port = {}
"#,
            free_port()
        ),
    );

    // `validate` accepts it: the shape is checkable without the secret.
    let (ok, _) = scratch.a2a(&["validate", "--config", "secretive.toml"]);
    assert!(ok, "validate is deliberately lenient about unset vars");

    let (ok, out) = scratch.a2a(&["doctor", "--config", "secretive.toml"]);
    assert!(!ok, "doctor checks this machine, so unset is fatal:\n{out}");
    assert!(out.contains("A2A_DOCTOR_DEFINITELY_UNSET"), "{out}");
}
