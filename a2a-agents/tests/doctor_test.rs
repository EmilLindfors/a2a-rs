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
        // A machine that has never configured a model provider.
        &LLM_VARS,
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

/// Every variable the provider cascade reads, for tests that need to describe a
/// machine with no model provider configured.
const LLM_VARS: [&str; 6] = [
    "OPENROUTER_API_KEY",
    "GEMINI_API_KEY",
    "OPENAI_API_KEY",
    "AI_API_KEY",
    "OPENAI_API_BASE_URL",
    "AI_API_BASE_URL",
];

/// Write an `llm`-handler config with the given `[llm]` block (or none).
fn llm_agent(scratch: &ScratchDir, file: &str, llm_block: &str) {
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
{llm_block}
"#,
            free_port()
        ),
    );
}

/// A key that is set and unusable is not the same as no key: `a2a run` refuses
/// to start on it, so `doctor` has to say so rather than warn about a fallback
/// that will never be reached.
#[test]
fn a_broken_model_setting_is_a_problem() {
    let scratch = ScratchDir::new("brokenkey");
    llm_agent(&scratch, "chat.toml", "");

    let (ok, out) = scratch.a2a_with_env(
        &["doctor", "--config", "chat.toml"],
        &[
            ("OPENROUTER_API_KEY", "sk-or-test"),
            ("OPENROUTER_REASONING", "verry-high"),
        ],
        &LLM_VARS,
    );
    assert!(!ok, "an unusable provider must fail the check:\n{out}");
    assert!(out.contains("OPENROUTER_REASONING"), "{out}");
    assert!(
        out.contains("refuse to start"),
        "the report must say what `a2a run` will do:\n{out}"
    );
}

/// With no provider anywhere the agent still runs, answering from its
/// deterministic fallback — a warning, not a problem.
#[test]
fn an_llm_agent_without_any_key_is_a_warning() {
    let scratch = ScratchDir::new("nokeyllm");
    llm_agent(&scratch, "chat.toml", "");

    let (ok, out) = scratch.a2a_env(&["doctor", "--config", "chat.toml"], &LLM_VARS);
    assert!(ok, "a keyless llm agent still runs:\n{out}");
    assert!(out.contains("deterministic fallback"), "{out}");
}

/// `reasoning` reaches the wire on `openrouter` only. Everywhere else it is
/// dropped, and the run works — on the model's own thinking default, at a cost
/// the config did not choose. A warning, since the agent answers either way.
#[test]
fn a_reasoning_the_provider_cannot_send_is_a_warning() {
    let scratch = ScratchDir::new("reasoning");
    llm_agent(
        &scratch,
        "chat.toml",
        "\n[llm]\nprovider = \"gemini\"\napi_key = \"test-key\"\nreasoning = \"high\"\n",
    );

    let (ok, out) = scratch.a2a_env(&["doctor", "--config", "chat.toml"], &LLM_VARS);
    assert!(ok, "a dropped reasoning still runs:\n{out}");
    assert!(out.contains("reasoning"), "{out}");
    assert!(
        out.contains("gemini"),
        "the report must name the provider that drops it:\n{out}"
    );
}

/// The same setting on the provider that *can* send it says nothing, so the
/// warning above stays worth reading.
#[test]
fn a_reasoning_the_provider_sends_is_not_reported() {
    let scratch = ScratchDir::new("reasoningok");
    llm_agent(
        &scratch,
        "chat.toml",
        "\n[llm]\nprovider = \"openrouter\"\napi_key = \"sk-test\"\nreasoning = \"high\"\n",
    );

    let (ok, out) = scratch.a2a_env(&["doctor", "--config", "chat.toml"], &LLM_VARS);
    assert!(ok, "{out}");
    assert!(
        out.contains("all clear"),
        "openrouter carries `reasoning`, so there is nothing to warn about:\n{out}"
    );
}

/// A mistyped `provider` used to fall back to the environment, so the agent ran
/// on whatever key happened to be exported — or on none, answering with a stub.
#[test]
fn a_mistyped_provider_is_a_problem() {
    let scratch = ScratchDir::new("typo");
    llm_agent(
        &scratch,
        "chat.toml",
        "\n[llm]\nprovider = \"opnrouter\"\napi_key = \"sk-test\"\n",
    );

    let (ok, out) = scratch.a2a(&["doctor", "--config", "chat.toml"]);
    assert!(!ok, "an unknown provider must fail the check:\n{out}");
    assert!(out.contains("opnrouter"), "{out}");
    assert!(
        out.contains("openrouter"),
        "the report must name the valid providers:\n{out}"
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

/// A handler name this binary does not have stops `a2a run` outright, so the
/// config cannot run as written. Better caught here than at start-up.
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

/// The same config with an image behind it is the supported way to ship a
/// handler no TOML can express, so it has to come back clear — and the report
/// has to say the image is where the answers are, because this machine cannot
/// look inside it.
#[test]
fn an_agent_with_its_own_image_is_all_clear() {
    let scratch = ScratchDir::new("image");
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

[runtime]
image = "ghcr.io/acme/weather:2.0"
"#,
            free_port()
        ),
    );

    let (ok, out) = scratch.a2a(&["doctor", "--config", "custom.toml"]);
    assert!(
        ok,
        "an image supplies the handler, so this is runnable:\n{out}"
    );
    assert!(out.contains("ghcr.io/acme/weather:2.0"), "{out}");
    assert!(out.contains("all clear"), "{out}");
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
