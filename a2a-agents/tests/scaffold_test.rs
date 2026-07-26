//! `a2a new` end-to-end: scaffold a config, then prove the binary accepts it.
//!
//! The unit tests in `core::template` prove each template parses. This drives
//! the *real* binary, which is what a new user meets: `a2a new` writes a file,
//! `a2a validate` accepts it, and the exit codes are right. A template that
//! drifts from the schema would pass a hand-written unit test and still leave
//! the first command someone runs producing a broken file.
//!
//! Gated on the `a2a` binary's required features so `CARGO_BIN_EXE_a2a` exists.

#![cfg(all(feature = "mcp-server", feature = "schema"))]

mod common;

use a2a_agents::core::AgentTemplate;
use common::ScratchDir;

#[test]
fn every_template_scaffolds_a_config_the_binary_accepts() {
    let scratch = ScratchDir::new("all");

    for template in AgentTemplate::ALL {
        let name = format!("Test {template}");
        let file = format!("{template}.toml");

        let (ok, out) = scratch.a2a(&[
            "new",
            &name,
            "--template",
            template.as_str(),
            "--output",
            &file,
        ]);
        assert!(ok, "`a2a new --template {template}` failed:\n{out}");

        let written = scratch.path().join(&file);
        assert!(written.exists(), "template '{template}' wrote no file");

        // The contract that matters: the thing we just generated validates.
        let (ok, out) = scratch.a2a(&["validate", "--config", &file]);
        assert!(
            ok,
            "config scaffolded from '{template}' does not validate:\n{out}"
        );

        // And it needs no environment to do so — a fresh checkout must work.
        assert!(
            !out.contains("env ("),
            "template '{template}' scaffolds unset env refs, so it is not runnable as generated:\n{out}"
        );
    }
}

#[test]
fn default_filename_is_the_slugified_name() {
    let scratch = ScratchDir::new("slug");

    let (ok, out) = scratch.a2a(&["new", "My Weather Agent"]);
    assert!(ok, "{out}");
    assert!(
        scratch.path().join("my-weather-agent.toml").exists(),
        "expected the slugified default filename, got:\n{out}"
    );
}

#[test]
fn existing_file_is_not_clobbered_without_force() {
    let scratch = ScratchDir::new("force");
    let path = scratch.path().join("keep.toml");
    std::fs::write(&path, "# precious hand-written config\n").unwrap();

    let (ok, out) = scratch.a2a(&["new", "Keep", "--output", "keep.toml"]);
    assert!(!ok, "scaffolding over an existing file must fail:\n{out}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# precious hand-written config\n",
        "the existing file must be untouched"
    );

    // --force is the documented way through.
    let (ok, out) = scratch.a2a(&["new", "Keep", "--output", "keep.toml", "--force"]);
    assert!(ok, "--force should overwrite:\n{out}");
    assert!(std::fs::read_to_string(&path).unwrap().contains("[agent]"));
}

#[test]
fn unknown_template_fails_and_lists_the_real_ones() {
    let scratch = ScratchDir::new("badtpl");

    let (ok, out) = scratch.a2a(&["new", "X", "--template", "llmm"]);
    assert!(!ok, "an unknown template must be an error:\n{out}");
    for template in AgentTemplate::ALL {
        assert!(
            out.contains(template.as_str()),
            "the error should list '{template}' as an option:\n{out}"
        );
    }
}

#[test]
fn port_override_reaches_the_generated_config() {
    let scratch = ScratchDir::new("port");

    let (ok, out) = scratch.a2a(&["new", "Ported", "--port", "8137", "--output", "p.toml"]);
    assert!(ok, "{out}");

    let (_, out) = scratch.a2a(&["validate", "--config", "p.toml"]);
    assert!(
        out.contains("port 8137"),
        "validate should report the overridden port:\n{out}"
    );
}
