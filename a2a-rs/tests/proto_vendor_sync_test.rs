//! Guards against silent drift between the vendored protos and the spec mirror.
//!
//! `a2a-rs/proto/` holds the trimmed proto set that `build.rs` compiles and that
//! `cargo publish` packages (the spec mirror in `spec/` is *not* packaged). Those
//! files duplicate `spec/a2a.proto` + the handful of `spec/google/api/*.proto`
//! they import, so they can drift apart unnoticed. This test fails when any
//! vendored file no longer matches its `spec/` counterpart byte-for-byte — update
//! both together, or this is the failure that catches it.
//!
//! When the `spec/` mirror is absent (e.g. inside a packaged/published crate
//! where only `proto/` ships), the check is skipped rather than failed.

use std::fs;
use std::path::{Path, PathBuf};

/// Collect every file under `dir`, relative to `dir`.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap_or_else(|e| panic!("read_dir {d:?}: {e}")) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(dir).unwrap().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn vendored_protos_match_spec() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let vendored = crate_dir.join("proto");
    let spec = crate_dir.join("..").join("spec");

    if !spec.is_dir() {
        eprintln!("skipping: spec/ mirror not present at {spec:?}");
        return;
    }

    let mut problems = Vec::new();
    for rel in files_under(&vendored) {
        let vendored_file = vendored.join(&rel);
        let spec_file = spec.join(&rel);

        match fs::read(&spec_file) {
            Ok(spec_bytes) => {
                let vendored_bytes = fs::read(&vendored_file).unwrap();
                if spec_bytes != vendored_bytes {
                    problems.push(format!(
                        "  {} differs from spec/{}",
                        rel.display(),
                        rel.display()
                    ));
                }
            }
            Err(_) => problems.push(format!(
                "  {} has no spec/ counterpart (spec/{} missing)",
                rel.display(),
                rel.display()
            )),
        }
    }

    assert!(
        problems.is_empty(),
        "vendored protos in a2a-rs/proto/ have drifted from spec/:\n{}\n\
         Re-sync the two trees (update both `spec/` and `a2a-rs/proto/`).",
        problems.join("\n")
    );
}
