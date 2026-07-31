//! Shared harness for the tests that drive the real `a2a` binary.
//!
//! `new`/`validate`/`doctor`/`up` are only meaningfully tested through the
//! process a user actually runs — exit codes and printed output are the
//! contract — so several suites need the same two things: somewhere disposable
//! to write configs, and a way to invoke the binary there.

// Each integration test compiles this module into its own binary and uses only
// the parts it needs, so unused-item warnings here are structural rather than a
// sign of dead code.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// A scratch directory removed on drop, so generated configs never leak into the
/// repo (and a failing test cannot poison the next run).
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Create an empty scratch directory, tagged so concurrent suites and
    /// processes do not collide.
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("a2a_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch dir");
        Self { path }
    }

    /// The directory itself — the cwd every `a2a` invocation runs in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Scaffold an agent config here with `a2a new`.
    pub fn agent(&self, name: &str, port: u16, file: &str) {
        let port = port.to_string();
        let (ok, out) = self.a2a(&["new", name, "--port", &port, "--output", file]);
        assert!(ok, "scaffolding {name} failed:\n{out}");
    }

    /// Write a file here, creating parent directories as needed.
    pub fn write(&self, file: &str, content: &str) {
        let path = self.path.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(path, content).expect("write file");
    }

    /// Run the `a2a` binary in this directory.
    pub fn a2a(&self, args: &[&str]) -> (bool, String) {
        a2a(&self.path, args)
    }

    /// Run the `a2a` binary here with `unset` removed from its environment.
    ///
    /// For checks whose result depends on the host's environment: clearing the
    /// vars in the *child* pins the machine the test is describing, where
    /// `std::env::remove_var` would mutate the shared environment of every
    /// other test in the binary — which run on threads alongside it.
    pub fn a2a_env(&self, args: &[&str], unset: &[&str]) -> (bool, String) {
        a2a_env(&self.path, args, unset)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Run the `a2a` binary in `dir`, returning (success, stdout+stderr).
///
/// Output is combined because the commands under test deliberately split it —
/// reports go to stdout, `tracing` to stderr — and a test asserting on what the
/// user sees should not have to know which.
pub fn a2a(dir: &Path, args: &[&str]) -> (bool, String) {
    a2a_env(dir, args, &[])
}

/// As [`a2a`], with `unset` removed from the child's environment.
pub fn a2a_env(dir: &Path, args: &[&str], unset: &[&str]) -> (bool, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_a2a"));
    command.current_dir(dir).args(args);
    for var in unset {
        command.env_remove(var);
    }
    let out = command.output().expect("run a2a binary");
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), combined)
}
