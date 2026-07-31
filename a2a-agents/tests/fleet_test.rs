//! `a2a up` / `a2a validate --fleet` end-to-end against the real binary.
//!
//! The unit tests in `core::fleet` prove the parsing and conflict rules. This
//! drives the binary a user actually runs: scaffold real agents, list them in a
//! fleet file, and assert that the fleet-level check catches what no single
//! config can — a shared port, a shared registry id, a member that is not there.
//!
//! Those three are all *silent-wrong* at runtime (one agent quietly fails to
//! bind; delegation reaches whichever agent registered last), which is the whole
//! reason `a2a up` checks before it binds.
//!
//! Gated on the `a2a` binary's required features so `CARGO_BIN_EXE_a2a` exists.

#![cfg(all(feature = "mcp-server", feature = "schema"))]

mod common;

use std::path::Path;

use common::{ScratchDir, a2a};

#[test]
fn a_sound_fleet_validates_and_reports_every_member() {
    let scratch = ScratchDir::new("ok");
    scratch.agent("Weather", 8080, "weather.toml");
    scratch.agent("Billing", 8081, "billing.toml");
    scratch.write(
        "fleet.toml",
        r#"
name = "Demo"

[[agents]]
config = "weather.toml"

[[agents]]
config = "billing.toml"
"#,
    );

    let (ok, out) = a2a(scratch.path(), &["validate", "--fleet", "fleet.toml"]);
    assert!(ok, "a sound fleet must validate:\n{out}");
    assert!(out.contains("Demo"), "the fleet name should appear:\n{out}");
    assert!(out.contains("weather.toml"), "{out}");
    assert!(out.contains("billing.toml"), "{out}");
}

/// Two agents on one port: the second silently fails to bind at runtime, so the
/// fleet check has to catch it up front and name both members.
#[test]
fn a_shared_port_is_a_conflict() {
    let scratch = ScratchDir::new("port");
    scratch.agent("Weather", 8080, "weather.toml");
    scratch.agent("Billing", 8080, "billing.toml");
    scratch.write(
        "fleet.toml",
        r#"
[[agents]]
config = "weather.toml"

[[agents]]
config = "billing.toml"
"#,
    );

    let (ok, out) = a2a(scratch.path(), &["validate", "--fleet", "fleet.toml"]);
    assert!(!ok, "a port clash must fail the check:\n{out}");
    assert!(
        out.contains("8080"),
        "the report must name the port:\n{out}"
    );
    assert!(
        out.contains("weather.toml") && out.contains("billing.toml"),
        "the report must name both members:\n{out}"
    );
}

/// Distinct names that slugify to one registry id: registration upserts, so
/// delegation by skill or agent_id reaches only whichever registered last.
#[test]
fn a_shared_agent_id_is_a_conflict() {
    let scratch = ScratchDir::new("id");
    scratch.agent("Weather Agent", 8080, "one.toml");
    scratch.agent("weather agent", 8081, "two.toml");
    scratch.write(
        "fleet.toml",
        r#"
[[agents]]
config = "one.toml"

[[agents]]
config = "two.toml"
"#,
    );

    let (ok, out) = a2a(scratch.path(), &["validate", "--fleet", "fleet.toml"]);
    assert!(!ok, "an id clash must fail the check:\n{out}");
    assert!(
        out.contains("weather-agent"),
        "the report must name the contested id:\n{out}"
    );
}

#[test]
fn a_missing_member_names_the_fleet_file_and_the_path() {
    let scratch = ScratchDir::new("missing");
    scratch.write(
        "fleet.toml",
        r#"
[[agents]]
config = "gone.toml"
"#,
    );

    let (ok, out) = a2a(scratch.path(), &["validate", "--fleet", "fleet.toml"]);
    assert!(!ok, "a missing member must fail:\n{out}");
    assert!(out.contains("gone.toml"), "{out}");
    assert!(out.contains("fleet.toml"), "{out}");
}

/// `a2a up` defaults to ./fleet.toml, so "no such file" is how most people first
/// meet fleets — the error has to answer the question it raises rather than
/// leaving an os-error-2 on screen.
#[test]
fn a_missing_fleet_file_shows_what_one_looks_like() {
    let scratch = ScratchDir::new("nofleet");

    let (ok, out) = a2a(scratch.path(), &["up"]);
    assert!(!ok, "there is no fleet file to run:\n{out}");
    assert!(out.contains("fleet.toml"), "{out}");
    assert!(
        out.contains("[[agents]]"),
        "the error should show the shape of a fleet file:\n{out}"
    );
}

/// Portability: members resolve against the fleet file, not the cwd, so a fleet
/// checked into a subdirectory runs from anywhere.
#[test]
fn member_paths_resolve_relative_to_the_fleet_file() {
    let scratch = ScratchDir::new("relative");
    std::fs::create_dir_all(scratch.path().join("stack")).unwrap();
    let (ok, out) = scratch.a2a(&[
        "new",
        "Weather",
        "--port",
        "8080",
        "--output",
        "stack/weather.toml",
    ]);
    assert!(ok, "{out}");
    scratch.write(
        "stack/fleet.toml",
        r#"
[[agents]]
config = "weather.toml"
"#,
    );

    // Run from the parent: `weather.toml` only resolves if the fleet file's own
    // directory is the base.
    let (ok, out) = a2a(scratch.path(), &["validate", "--fleet", "stack/fleet.toml"]);
    assert!(
        ok,
        "member paths must resolve against the fleet file:\n{out}"
    );
}

#[test]
fn an_unknown_key_in_the_fleet_file_is_an_error() {
    let scratch = ScratchDir::new("typo");
    scratch.agent("Weather", 8080, "weather.toml");
    scratch.write(
        "fleet.toml",
        r#"
naem = "Typo"

[[agents]]
config = "weather.toml"
"#,
    );

    let (ok, out) = a2a(scratch.path(), &["validate", "--fleet", "fleet.toml"]);
    assert!(!ok, "a mistyped fleet key must not be ignored:\n{out}");
    assert!(out.contains("naem"), "{out}");
}

/// The shipped example is the first fleet most people will run; if it drifts
/// from the schema or its members move, that has to fail here rather than in
/// someone's terminal.
#[test]
fn the_shipped_example_fleet_validates() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = a2a(crate_dir, &["validate", "--fleet", "examples/fleet.toml"]);
    assert!(ok, "examples/fleet.toml must validate:\n{out}");
}
