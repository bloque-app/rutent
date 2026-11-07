//! The documentation check: `cargo doc` with the `missing_docs` lint at deny level, for every crate.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/undocumented")
}

/// Runs the documentation check, the same command CI runs, on the crate at `dir`.
fn doc_check(dir: &Path, target: &Path) -> Output {
    Command::new(std::env::var_os("CARGO").unwrap_or("cargo".into()))
        .args(["doc", "--no-deps", "--quiet", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .env("RUSTDOCFLAGS", "-D warnings")
        .env("CARGO_TARGET_DIR", target)
        .output()
        .expect("cargo runs")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rutent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory is writable");
    dir
}

// verifies: REQ-MOD-2, NFR-7
#[test]
fn undocumented_public_element_fails_the_check() {
    let scratch = scratch("undocumented");
    let out = doc_check(&fixture(), &scratch.join("target"));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "the check passed on an undocumented `pub fn`"
    );
    assert!(
        stderr.contains("missing documentation for a function"),
        "failed, but not for a missing doc: {stderr}"
    );

    // The same crate with the function documented passes, so the failure above is the missing doc.
    let fixed = scratch.join("documented");
    std::fs::create_dir_all(fixed.join("src")).unwrap();
    std::fs::copy(fixture().join("Cargo.toml"), fixed.join("Cargo.toml")).unwrap();
    let lib = std::fs::read_to_string(fixture().join("src/lib.rs")).unwrap();
    let lib = lib.replace(
        "pub fn undocumented",
        "/// Now documented.\npub fn undocumented",
    );
    std::fs::write(fixed.join("src/lib.rs"), lib).unwrap();
    let out = doc_check(&fixed, &scratch.join("target"));
    assert!(
        out.status.success(),
        "documented fixture failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

// verifies: REQ-MOD-2, NFR-7
#[test]
fn every_workspace_crate_inherits_the_missing_docs_lint() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root_manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(
        root_manifest.contains("[workspace.lints.rust]\nmissing_docs = \"deny\""),
        "the workspace does not deny `missing_docs`"
    );
    for member in rutent_boundary_check::members(&root_manifest).unwrap() {
        let text = std::fs::read_to_string(root.join(&member).join("Cargo.toml")).unwrap();
        assert!(
            text.contains("\n[lints]\nworkspace = true\n"),
            "{member} does not inherit the workspace lints"
        );
    }
}
