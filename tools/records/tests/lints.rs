//! Tests of the documentation lint's coverage (`T-024-24`), on fixtures and on the real repository.

use rutent_records::lints::{check, sub_workspaces, uncovered};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gaps(dirs: &[&str], workflow: &[&str]) -> Vec<String> {
    let dirs: Vec<String> = dirs.iter().map(ToString::to_string).collect();
    uncovered(&dirs, &[("ci.yml".to_string(), workflow.join("\n"))])
        .iter()
        .map(ToString::to_string)
        .collect()
}

// verifies: NFR-7
#[test]
fn a_sub_workspace_no_doc_step_names_is_reported() {
    let workflow = [
        "    steps:",
        "      - name: Documentation check",
        "        run: cargo doc --workspace --no-deps",
        "        env:",
        "          RUSTDOCFLAGS: -D warnings",
        "      - name: Documentation check, guests",
        "        run: cargo doc --manifest-path components/guests/Cargo.toml --no-deps",
        "        env:",
        "          RUSTDOCFLAGS: -D warnings",
    ];
    assert_eq!(
        gaps(&["components/guests"], &workflow),
        Vec::<String>::new()
    );
    let found = gaps(&["components/guests", "components/wallet"], &workflow);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("doc-lint-gap") && found[0].contains("components/wallet"));
}

// verifies: NFR-7
#[test]
fn a_step_that_names_the_directory_but_does_not_document_or_deny_warnings_covers_nothing() {
    let workflow = [
        "      - name: Lint, guests",
        "        run: cargo clippy --manifest-path components/guests/Cargo.toml -- -D warnings",
        "      - name: Documentation check, guests",
        "        run: cargo doc --manifest-path components/guests/Cargo.toml --no-deps",
        "      - name: Documentation check, a longer name",
        "        run: cargo doc --manifest-path components/guests-more/Cargo.toml",
        "        env:",
        "          RUSTDOCFLAGS: -D warnings",
    ];
    let found = gaps(&["components/guests"], &workflow);
    assert_eq!(found.len(), 1, "{found:?}");
}

// verifies: NFR-7
#[test]
fn the_real_repositorys_sub_workspaces_are_found_and_the_fixture_and_experiments_are_not() {
    let subs = sub_workspaces(&root()).expect("the walk");
    for dir in [
        "components/prov/module-guests",
        "components/prov/guests/plain-wallet",
        "components/stm/vos-program",
        "components/stm/vos-program/client",
    ] {
        assert!(subs.iter().any(|s| s == dir), "{dir} in {subs:?}");
    }
    assert!(
        subs.iter()
            .all(|s| !s.starts_with("experiments") && !s.contains("fixtures")),
        "{subs:?}"
    );
}

// verifies: NFR-7
#[test]
fn documentation_lint_covers_every_sub_workspace() {
    let found: Vec<String> = check(&root())
        .expect("the workflows are readable")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

// verifies: NFR-7
#[test]
fn a_commented_out_or_conditional_or_failure_tolerant_doc_step_covers_nothing() {
    let covering = [
        "        run: cargo doc --manifest-path components/guests/Cargo.toml --no-deps",
        "        env:",
        "          RUSTDOCFLAGS: -D warnings",
    ];
    let with = |extra: &[&str]| -> Vec<String> {
        let mut w = vec!["      - name: Documentation check, guests"];
        w.extend_from_slice(extra);
        w.extend_from_slice(&covering);
        gaps(&["components/guests"], &w)
    };
    assert!(with(&[]).is_empty());
    // A step with its `cargo doc` line commented out, and the comment block above the next step.
    let commented = [
        "      - name: Documentation check, guests",
        "        run: |",
        "          # cargo doc --manifest-path components/guests/Cargo.toml --no-deps",
        "        env:",
        "          RUSTDOCFLAGS: -D warnings",
    ];
    assert_eq!(gaps(&["components/guests"], &commented).len(), 1);
    assert_eq!(with(&["        if: false"]).len(), 1);
    assert_eq!(with(&["        if: github.event_name == 'push'"]).len(), 1);
    assert_eq!(with(&["        continue-on-error: true"]).len(), 1);
}

// verifies: NFR-7
#[test]
fn a_step_naming_a_nested_workspace_does_not_cover_its_parent() {
    let workflow = [
        "      - name: Documentation check, the client",
        "        run: cd components/vos/client && cargo doc --workspace --no-deps",
        "        env:",
        "          RUSTDOCFLAGS: -D warnings",
    ];
    let found = gaps(&["components/vos", "components/vos/client"], &workflow);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("components/vos is"), "{found:?}");
}
