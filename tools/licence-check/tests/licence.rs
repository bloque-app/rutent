//! Tests of the licence record check, run on in-memory fixtures.

use rutent_licence_check::{Member, Reason, Violation, check, member_paths, package_identity};

/// A lockfile with one workspace member (no source) and one registry package.
const LOCK: &str = "version = 4\n\
\n\
[[package]]\n\
name = \"rutent-contracts\"\n\
version = \"0.1.0\"\n\
\n\
[[package]]\n\
name = \"serde\"\n\
version = \"1.0.219\"\n\
source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
checksum = \"0000000000000000000000000000000000000000000000000000000000000000\"\n";

const HEADER: &str = "# header comment, ignored\n\n";

/// The workspace members of the fixture: `rutent-contracts` at the version the lockfile locks.
fn members() -> Vec<Member> {
    vec![Member {
        name: "rutent-contracts".into(),
        version: "0.1.0".into(),
    }]
}

// verifies: REQ-MOD-5
#[test]
fn unrecorded_dependency_fails_the_check() {
    let record = format!("{HEADER}vos not-adopted AGPL-3.0-or-later pending DEC-6,ADR-0004\n");
    let found = check(LOCK, &record, &members()).expect("fixture record parses");
    assert_eq!(
        found,
        vec![Violation {
            package: "serde".into(),
            version: "1.0.219".into(),
            reason: Reason::Unrecorded,
        }]
    );
}

// verifies: REQ-MOD-5
#[test]
fn dependency_with_a_pending_decision_fails_the_check() {
    let record = format!("{HEADER}serde 1.0.219 MIT pending licence-permissive-standing\n");
    let found = check(LOCK, &record, &members()).expect("fixture record parses");
    assert_eq!(
        found,
        vec![Violation {
            package: "serde".into(),
            version: "1.0.219".into(),
            reason: Reason::Pending,
        }]
    );
}

// verifies: REQ-MOD-5
#[test]
fn recorded_dependency_with_a_decision_passes() {
    let record = format!(
        "{HEADER}serde 1.0.219 MIT OR Apache-2.0 permitted licence-permissive-standing\n\
         vault not-adopted BUSL-1.1 pending DEC-9,ADR-0004\n"
    );
    assert_eq!(
        check(LOCK, &record, &members()).expect("fixture record parses"),
        vec![]
    );
}

// verifies: REQ-MOD-5
#[test]
fn duplicate_record_line_fails_the_check() {
    // The permitted line comes first: a first-match lookup would pass it and never see the pending one.
    let record = format!(
        "{HEADER}serde 1.0.219 MIT OR Apache-2.0 permitted licence-permissive-standing\n\
         serde 1.0.219 MIT pending licence-permissive-standing\n"
    );
    let result = check(LOCK, &record, &members());
    assert!(
        matches!(&result, Err(e) if e.contains("duplicate")),
        "a duplicate record line must be an error, got {result:?}"
    );
}

// verifies: REQ-MOD-5
#[test]
fn locked_package_outside_the_workspace_fails_the_check() {
    // `helper` has no `source` (a path dependency) and is not a workspace member, so it is external.
    let lock = format!("{LOCK}\n[[package]]\nname = \"helper\"\nversion = \"0.1.0\"\n");
    let record =
        format!("{HEADER}serde 1.0.219 MIT OR Apache-2.0 permitted licence-permissive-standing\n");
    let found = check(&lock, &record, &members()).expect("fixture record parses");
    assert_eq!(
        found,
        vec![Violation {
            package: "helper".into(),
            version: "0.1.0".into(),
            reason: Reason::Unrecorded,
        }]
    );
}

// verifies: REQ-MOD-5
#[test]
fn workspace_member_at_another_version_fails_the_check() {
    // `rutent-contracts 0.2.0` has no `source` and shares a member's name, but the member is 0.1.0.
    // It is not that member, so it is external and needs a record line.
    let lock = format!("{LOCK}\n[[package]]\nname = \"rutent-contracts\"\nversion = \"0.2.0\"\n");
    let record =
        format!("{HEADER}serde 1.0.219 MIT OR Apache-2.0 permitted licence-permissive-standing\n");
    let found = check(&lock, &record, &members()).expect("fixture record parses");
    assert_eq!(
        found,
        vec![Violation {
            package: "rutent-contracts".into(),
            version: "0.2.0".into(),
            reason: Reason::Unrecorded,
        }]
    );
}

const ROOT: &str = "[workspace]\n\
members = [\n\
    \"contracts\",\n\
    \"tools/licence-check\",\n\
]\n\
\n\
[workspace.package]\n\
version = \"0.3.0\"\n";

// verifies: REQ-MOD-5
#[test]
fn member_identity_reads_name_and_version() {
    let manifest = "[package]\nname = \"rutent-contracts\"\nversion = \"0.1.0\"\n";
    assert_eq!(
        package_identity(manifest, ROOT).expect("plain identity reads"),
        Member {
            name: "rutent-contracts".into(),
            version: "0.1.0".into(),
        }
    );
}

// verifies: REQ-MOD-5
#[test]
fn member_version_inherited_from_workspace_is_resolved() {
    let manifest = "[package]\nname = \"rutent-contracts\"\nversion.workspace = true\n";
    assert_eq!(
        package_identity(manifest, ROOT).expect("inherited version resolves"),
        Member {
            name: "rutent-contracts".into(),
            version: "0.3.0".into(),
        }
    );
}

// verifies: REQ-MOD-5
#[test]
fn member_identity_fails_closed_without_a_resolvable_version() {
    let no_version = "[package]\nname = \"x\"\n";
    let inline_table = "[package]\nname = \"x\"\nversion = { workspace = true }\n";
    let no_name = "[package]\nversion = \"0.1.0\"\n";
    let inherits_but_root_has_none = "[package]\nname = \"x\"\nversion.workspace = true\n";
    let root_without_version = "[workspace]\nmembers = []\n";
    assert!(
        package_identity(no_version, ROOT).is_err(),
        "missing version"
    );
    assert!(
        package_identity(inline_table, ROOT).is_err(),
        "inline table form"
    );
    assert!(package_identity(no_name, ROOT).is_err(), "missing name");
    assert!(
        package_identity(inherits_but_root_has_none, root_without_version).is_err(),
        "inherited version with no workspace version"
    );
}

// verifies: REQ-MOD-5
#[test]
fn member_paths_reads_the_members_array() {
    assert_eq!(
        member_paths(ROOT).expect("plain members array reads"),
        vec!["contracts".to_string(), "tools/licence-check".to_string()]
    );
}

// verifies: REQ-MOD-5
#[test]
fn member_paths_fails_closed_on_a_glob_or_unknown_syntax() {
    let glob = "[workspace]\nmembers = [\n    \"tools/*\",\n]\n";
    let one_line = "[workspace]\nmembers = [\"contracts\"]\n";
    let commented = "[workspace]\nmembers = [\n    \"contracts\", # note\n]\n";
    assert!(member_paths(glob).is_err(), "glob member");
    assert!(member_paths(one_line).is_err(), "single-line array");
    assert!(member_paths(commented).is_err(), "comment on a member line");
}
