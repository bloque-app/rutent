//! Tests of the licence record check, run on in-memory fixtures.

use rutent_licence_check::{
    LockFile, Member, Reason, Violation, check, check_locks, find_all_locks, find_locks,
    member_paths, package_identity, workspace_member_paths,
};
use std::fs;

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
            lockfile: "Cargo.lock".into(),
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
            lockfile: "Cargo.lock".into(),
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
            lockfile: "Cargo.lock".into(),
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
            lockfile: "Cargo.lock".into(),
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
    let one_line = "[workspace]\nmembers = [\"contracts\", \"a/*\"]\n";
    let commented = "[workspace]\nmembers = [\n    \"contracts\", # note\n]\n";
    assert!(member_paths(glob).is_err(), "glob member");
    assert!(
        member_paths(one_line).is_err(),
        "glob in a single-line array"
    );
    assert!(member_paths(commented).is_err(), "comment on a member line");
}

/// An experiment lockfile fixture: `exp-guest` has no `source`, `serde` does.
fn experiment_lock(path: &str, serde_version: &str) -> LockFile {
    LockFile {
        path: path.into(),
        text: format!(
            "version = 4\n\n[[package]]\nname = \"exp-guest\"\nversion = \"0.1.0\"\n\n\
             [[package]]\nname = \"serde\"\nversion = \"{serde_version}\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
        ),
        members: vec![Member {
            name: "exp-guest".into(),
            version: "0.1.0".into(),
        }],
    }
}

// verifies: REQ-MOD-5
#[test]
fn violation_in_an_experiment_lock_is_found_and_names_that_lock() {
    let record = format!("{HEADER}serde 1.0.219 MIT permitted licence-permissive-standing\n");
    let lock = "experiments/e/Cargo.lock";
    let found = check_locks(&[experiment_lock(lock, "1.0.300")], &record).expect("record parses");
    assert_eq!(
        found,
        vec![Violation {
            lockfile: lock.into(),
            package: "serde".into(),
            version: "1.0.300".into(),
            reason: Reason::Unrecorded,
        }]
    );
    assert!(found[0].to_string().starts_with(lock));
}

// verifies: REQ-MOD-5
#[test]
fn same_package_and_version_in_two_locks_needs_one_line() {
    let record = format!("{HEADER}serde 1.0.219 MIT permitted licence-permissive-standing\n");
    let root = LockFile {
        path: "Cargo.lock".into(),
        text: LOCK.into(),
        members: members(),
    };
    let locks = [root, experiment_lock("experiments/e/Cargo.lock", "1.0.219")];
    assert_eq!(check_locks(&locks, &record).expect("record parses"), vec![]);
}

// verifies: REQ-MOD-5
#[test]
fn lock_under_a_target_directory_is_skipped() {
    let base = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("licence-find-locks");
    let _ = fs::remove_dir_all(&base);
    for dir in [
        "experiments/a",
        "experiments/a/target/debug",
        "experiments/b/sub",
    ] {
        fs::create_dir_all(base.join(dir)).expect("fixture dir");
    }
    for lock in [
        "experiments/a/Cargo.lock",
        "experiments/a/target/debug/Cargo.lock",
        "experiments/b/sub/Cargo.lock",
    ] {
        fs::write(base.join(lock), "version = 4\n").expect("fixture lock");
    }
    let found = find_locks(&base, "experiments").expect("walk");
    fs::remove_dir_all(&base).expect("clean up");
    assert_eq!(
        found,
        vec!["experiments/a/Cargo.lock", "experiments/b/sub/Cargo.lock"]
    );
}

// verifies: REQ-MOD-5
#[test]
fn sourceless_package_outside_the_workspaces_is_checked_like_a_registry_package() {
    let lock = LockFile {
        path: "experiments/e/Cargo.lock".into(),
        text: "version = 4\n\n[[package]]\nname = \"exp-guest\"\nversion = \"0.1.0\"\n\n\
               [[package]]\nname = \"elsewhere\"\nversion = \"2.0.0\"\n"
            .into(),
        members: vec![Member {
            name: "exp-guest".into(),
            version: "0.1.0".into(),
        }],
    };
    let found = check_locks(&[lock], HEADER).expect("record parses");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].package, "elsewhere");
    assert_eq!(found[0].lockfile, "experiments/e/Cargo.lock");
}

// verifies: REQ-MOD-5
#[test]
fn workspace_member_paths_reads_a_virtual_workspace_a_package_and_fails_closed_on_neither() {
    let virtual_ws = "[workspace]\nmembers = [\n    \"a\",\n    \"b/c\",\n]\nexclude = [\"x\"]\n";
    assert_eq!(
        workspace_member_paths(virtual_ws).expect("virtual"),
        vec!["a".to_string(), "b/c".to_string()]
    );
    assert_eq!(
        workspace_member_paths("[workspace]\nmembers = [\"a\", \"b/c\"]\n").expect("inline"),
        vec!["a".to_string(), "b/c".to_string()]
    );
    assert!(workspace_member_paths("[workspace]\nmembers = [\"t/*\"]\n").is_err());
    let rooted = "[workspace]\n\n[package]\nname = \"p\"\nversion = \"0.0.0\"\n";
    assert_eq!(
        workspace_member_paths(rooted).expect("rooted"),
        vec![".".to_string()]
    );
    let lone = "[package]\nname = \"p\"\nversion = \"0.0.0\"\n";
    assert_eq!(
        workspace_member_paths(lone).expect("lone"),
        vec![".".to_string()]
    );
    assert!(workspace_member_paths("[dependencies]\n").is_err());
    assert!(workspace_member_paths("[workspace]\n").is_err());
}

// verifies: REQ-MOD-5
#[cfg(unix)]
#[test]
fn symlinked_lock_or_directory_under_experiments_is_an_error() {
    use std::os::unix::fs::symlink;
    let base = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("licence-symlinks");
    for (name, make) in [("lock", true), ("dir", false)] {
        let case = base.join(name);
        let _ = fs::remove_dir_all(&case);
        fs::create_dir_all(case.join("experiments/a")).expect("fixture dir");
        fs::write(case.join("real.lock"), "version = 4\n").expect("fixture");
        if make {
            symlink(
                case.join("real.lock"),
                case.join("experiments/a/Cargo.lock"),
            )
            .expect("link");
        } else {
            symlink(case.join("experiments/a"), case.join("experiments/b")).expect("link");
        }
        let result = find_locks(&case, "experiments");
        fs::remove_dir_all(&case).expect("clean up");
        let err = result.expect_err("a symlink is refused");
        assert!(err.contains("symbolic link"), "{err}");
    }
}

// verifies: REQ-MOD-5
#[test]
fn a_lock_under_components_is_read_like_one_under_experiments() {
    let base = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("licence-find-all-locks");
    let _ = fs::remove_dir_all(&base);
    for dir in [
        "experiments/a",
        "components/stm/vos-program",
        "components/stm/vos-program/target/release",
        "components/stm/src",
    ] {
        fs::create_dir_all(base.join(dir)).expect("fixture dir");
    }
    for lock in [
        "experiments/a/Cargo.lock",
        "components/stm/vos-program/Cargo.lock",
        "components/stm/vos-program/target/release/Cargo.lock",
    ] {
        fs::write(base.join(lock), "version = 4\n").expect("fixture lock");
    }
    let found = find_all_locks(&base).expect("walk");
    fs::remove_dir_all(&base).expect("clean up");
    assert_eq!(
        found,
        vec![
            "components/stm/vos-program/Cargo.lock",
            "experiments/a/Cargo.lock"
        ]
    );
}
