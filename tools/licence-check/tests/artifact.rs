//! Tests of the per-lock and per-artifact licence rules (T-001-06), on in-memory fixtures: no network, no cargo.

use rutent_licence_check::{
    ArtifactInput, ArtifactReason, ArtifactViolation, LockFile, Member, Reason, Violation, check,
    check_artifacts, check_locks, is_permissive,
};

const VOS_PROGRAM: &str = "components/stm/vos-program/Cargo.lock";
const VOS_CLIENT: &str = "components/stm/vos-program/client/Cargo.lock";

/// A lock with one registry package, `vos-api`, and one with a permissive one.
fn lock(path: &str, package: &str, version: &str) -> LockFile {
    LockFile {
        path: path.to_string(),
        text: format!(
            "version = 4\n\n[[package]]\nname = \"{package}\"\nversion = \"{version}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
        ),
        members: Vec::<Member>::new(),
    }
}

const AGPL_RECORD: &str = "vos-api 0.1.0 AGPL-3.0-or-later permitted DEC-6,counsel-licences\n\
serde 1.0.219 MIT OR Apache-2.0 permitted licence-permissive-standing\n";

fn found(locks: &[LockFile], record: &str) -> Vec<Violation> {
    check_locks(locks, record).expect("fixture parses")
}

// verifies: DEC-6
#[test]
fn agpl_package_in_the_root_lock_fails_the_licence_check_whatever_the_record_says() {
    // The record permits `vos-api` and even names the root lock for it: the root lock still refuses it.
    let record = format!("{AGPL_RECORD}@holds vos-api 0.1.0 Cargo.lock\n");
    let got = found(&[lock("Cargo.lock", "vos-api", "0.1.0")], &record);
    assert!(
        got.iter().any(|v| v.reason == Reason::RefusedLock
            && v.lockfile == "Cargo.lock"
            && v.package == "vos-api"),
        "the root lock must refuse an AGPL package, got {got:?}"
    );
    assert!(
        got.iter()
            .any(|v| v.reason == Reason::HoldsRefused && v.lockfile == "Cargo.lock"),
        "a permission naming the root lock must fail too, got {got:?}"
    );
    // An MIT package's lock outside the VOS trees refuses it as well.
    let got = found(
        &[lock(
            "components/prov/module-guests/Cargo.lock",
            "vos-api",
            "0.1.0",
        )],
        &format!("{AGPL_RECORD}@holds vos-api 0.1.0 components/prov/module-guests/Cargo.lock\n"),
    );
    assert!(
        got.iter().any(|v| v.reason == Reason::RefusedLock),
        "got {got:?}"
    );
}

// verifies: DEC-6
#[test]
fn copyleft_permission_holds_only_in_the_lockfiles_it_names() {
    let record = format!("{AGPL_RECORD}@holds vos-api 0.1.0 {VOS_PROGRAM}\n");
    let ok = found(&[lock(VOS_PROGRAM, "vos-api", "0.1.0")], &record);
    assert_eq!(ok, vec![], "the named lock holds it");
    // The client's lock is under the VOS tree too, but the permission does not name it.
    let got = found(&[lock(VOS_CLIENT, "vos-api", "0.1.0")], &record);
    assert_eq!(
        got,
        vec![Violation {
            lockfile: VOS_CLIENT.into(),
            package: "vos-api".into(),
            version: "0.1.0".into(),
            reason: Reason::NotHeld,
        }]
    );
    // A permitted package outside the permissive set with no `@holds` line fails at the record.
    let got = found(&[], AGPL_RECORD);
    assert_eq!(
        got,
        vec![Violation {
            lockfile: "licences/RECORD".into(),
            package: "vos-api".into(),
            version: "0.1.0".into(),
            reason: Reason::HoldsNothing,
        }]
    );
}

// verifies: DEC-6
#[test]
fn permissive_package_passes_in_any_lock() {
    for path in [
        "Cargo.lock",
        VOS_PROGRAM,
        "components/prov/module-guests/Cargo.lock",
    ] {
        assert_eq!(
            found(&[lock(path, "serde", "1.0.219")], AGPL_RECORD_WITH_HOLDS),
            vec![],
            "{path}"
        );
    }
    for expr in [
        "MIT OR Apache-2.0",
        "Apache-2.0 WITH LLVM-exception",
        "MIT/Apache-2.0",
        "(MIT OR Apache-2.0) AND Unicode-3.0",
        "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
    ] {
        assert!(is_permissive(expr), "{expr}");
    }
    for expr in [
        "AGPL-3.0-or-later",
        "BUSL-1.1",
        "MIT AND GPL-3.0-only",
        "(MIT",
        "",
        "MIT WITH Classpath-exception-2.0",
    ] {
        assert!(!is_permissive(expr), "{expr}");
    }
}

const AGPL_RECORD_WITH_HOLDS: &str = "vos-api 0.1.0 AGPL-3.0-or-later permitted DEC-6,counsel-licences\n\
@holds vos-api 0.1.0 components/stm/vos-program/Cargo.lock\n\
serde 1.0.219 MIT OR Apache-2.0 permitted licence-permissive-standing\n";

/// One package of a `cargo metadata` fixture: name, version, declared licence.
type P = (&'static str, &'static str, Option<&'static str>);

/// A `cargo metadata` text for the VOS program's lock: see [`metadata_in`].
fn metadata(pkgs: &[P], members: &[&str], edges: &[(&str, &str, Option<&str>)]) -> String {
    metadata_in("components/stm/vos-program", pkgs, members, edges)
}

/// A `cargo metadata` text for the workspace in `lockdir`: the packages, the workspace members (by name), and edges
/// `(from, to, kind)`. A package at version `0.0.0` is a path package in `lockdir/<name>`; any other is a registry one.
fn metadata_in(
    lockdir: &str,
    pkgs: &[P],
    members: &[&str],
    edges: &[(&str, &str, Option<&str>)],
) -> String {
    let id = |n: &str| {
        let p = pkgs.iter().find(|p| p.0 == n).expect("fixture package");
        format!("{} {}", p.0, p.1)
    };
    let packages: Vec<String> = pkgs
        .iter()
        .map(|(n, v, l)| {
            let license = l.map_or("null".to_string(), |l| format!("\"{l}\""));
            let source = if *v == "0.0.0" {
                "null".to_string()
            } else {
                "\"registry+https://github.com/rust-lang/crates.io-index\"".to_string()
            };
            format!(
                "{{\"id\":\"{n} {v}\",\"name\":\"{n}\",\"version\":\"{v}\",\"license\":{license},\"source\":{source},\"manifest_path\":\"/repo/{lockdir}/{n}/Cargo.toml\"}}"
            )
        })
        .collect();
    let nodes: Vec<String> = pkgs
        .iter()
        .map(|(n, ..)| {
            let deps: Vec<String> = edges
                .iter()
                .filter(|e| e.0 == *n)
                .map(|(_, to, kind)| {
                    let kind = kind.map_or("null".to_string(), |k| format!("\"{k}\""));
                    format!("{{\"pkg\":\"{}\",\"name\":\"{to}\",\"dep_kinds\":[{{\"kind\":{kind},\"target\":null}}]}}", id(to))
                })
                .collect();
            format!("{{\"id\":\"{}\",\"deps\":[{}]}}", id(n), deps.join(","))
        })
        .collect();
    let members: Vec<String> = members.iter().map(|m| format!("\"{}\"", id(m))).collect();
    format!(
        "{{\"packages\":[{}],\"workspace_root\":\"/repo/{lockdir}\",\"workspace_members\":[{}],\"resolve\":{{\"nodes\":[{}],\"root\":null}}}}",
        packages.join(","),
        members.join(","),
        nodes.join(",")
    )
}

fn artifacts(lockfile: &str, metadata: String, record: &str) -> Vec<ArtifactViolation> {
    check_artifacts(
        &[ArtifactInput {
            lockfile: lockfile.into(),
            metadata,
        }],
        record,
    )
    .expect("fixture parses")
}

const CORE: &str = "@core rutent-stm\n@core rutent-contracts\n@core rutent-core-orch\n\
vos-sdk 0.1.0 AGPL-3.0-or-later permitted DEC-6,counsel-licences\n\
serde 1.0.219 MIT OR Apache-2.0 permitted licence-permissive-standing\n";

const PKGS: [P; 6] = [
    ("host", "0.0.0", Some("AGPL-3.0-or-later")),
    ("rutent-stm", "0.0.0", Some("MIT")),
    ("rutent-contracts", "0.0.0", Some("MIT")),
    ("rutent-core-orch", "0.0.0", Some("MIT")),
    ("vos-sdk", "0.1.0", Some("AGPL-3.0-or-later")),
    ("serde", "1.0.219", Some("MIT OR Apache-2.0")),
];

// verifies: DEC-6
#[test]
fn artifact_holding_a_core_crate_and_an_agpl_package_fails_the_licence_check() {
    let record = format!("{CORE}@artifact {VOS_PROGRAM} host AGPL-3.0-or-later\n");
    let edges = [
        ("host", "rutent-core-orch", None),
        ("host", "vos-sdk", None),
        ("rutent-core-orch", "serde", None),
    ];
    let got = artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &record);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].reason, ArtifactReason::CoreWithCopyleft);
    assert_eq!(got[0].artifact, "host");
    assert!(
        got[0].detail.contains("rutent-core-orch") && got[0].detail.contains("vos-sdk"),
        "{got:?}"
    );
    // An exception names the artifact and the core crates it may hold: `rutent-stm` does not excuse `rutent-core-orch`.
    let excused = format!(
        "{record}@exception stm-inside-agpl-program {VOS_PROGRAM} host rutent-stm,rutent-contracts\n"
    );
    let got = artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &excused);
    assert_eq!(got.len(), 1, "{got:?}");
    // The named crates pass.
    let edges = [
        ("host", "rutent-stm", None),
        ("host", "rutent-contracts", None),
        ("host", "vos-sdk", None),
    ];
    let got = artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &excused);
    assert_eq!(got, vec![], "the exception holds for the artifact it names");
    // Without the exception the same graph fails, once per core crate.
    let got = artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &record);
    assert_eq!(got.len(), 2, "{got:?}");
    // A dev dependency is not part of the artifact.
    let edges = [
        ("host", "rutent-core-orch", Some("dev")),
        ("host", "vos-sdk", None),
    ];
    assert_eq!(
        artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &record),
        vec![]
    );
}

// verifies: DEC-6
#[test]
fn rutent_stm_or_rutent_contracts_gaining_a_non_permissive_dependency_fails_the_licence_check() {
    let record = format!(
        "{CORE}@artifact {VOS_PROGRAM} host AGPL-3.0-or-later\n@exception stm-inside-agpl-program {VOS_PROGRAM} host rutent-stm,rutent-contracts\n"
    );
    // The exception covers `host` embedding `rutent-stm`, but not `rutent-stm` depending on VOS itself.
    let edges = [
        ("host", "rutent-stm", None),
        ("rutent-stm", "vos-sdk", None),
        ("rutent-stm", "serde", None),
    ];
    let got = artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &record);
    assert!(
        got.iter()
            .any(|v| v.reason == ArtifactReason::CoreNotPermissive
                && v.artifact == "rutent-stm"
                && v.detail.contains("vos-sdk")),
        "{got:?}"
    );
    // Through a middle crate as well, and for `rutent-contracts`.
    let edges = [
        ("host", "rutent-contracts", None),
        ("rutent-contracts", "rutent-core-orch", None),
        ("rutent-core-orch", "vos-sdk", None),
    ];
    let got = artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &record);
    assert!(
        got.iter()
            .any(|v| v.reason == ArtifactReason::CoreNotPermissive
                && v.artifact == "rutent-contracts"),
        "{got:?}"
    );
    // Permissive dependencies pass.
    let edges = [
        ("host", "rutent-stm", None),
        ("rutent-stm", "rutent-contracts", None),
        ("rutent-stm", "serde", None),
        ("host", "vos-sdk", None),
    ];
    assert_eq!(
        artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &record),
        vec![]
    );
}

// verifies: DEC-6
#[test]
fn every_artifact_is_listed_with_its_licence() {
    let edges = [("host", "serde", None)];
    let got = artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), CORE);
    assert_eq!(
        got.iter().map(|v| v.reason).collect::<Vec<_>>(),
        vec![ArtifactReason::Unrecorded]
    );
    let record = format!("{CORE}@artifact {VOS_PROGRAM} host MIT\n");
    let got = artifacts(VOS_PROGRAM, metadata(&PKGS, &["host"], &edges), &record);
    assert_eq!(
        got.iter().map(|v| v.reason).collect::<Vec<_>>(),
        vec![ArtifactReason::LicenceMismatch]
    );
}

// verifies: DEC-6
#[test]
fn a_package_whose_licence_is_unknown_fails_the_artifact() {
    let pkgs = [("host", "0.0.0", Some("MIT")), ("mystery", "1.0.0", None)];
    let record = format!("{CORE}@artifact {VOS_PROGRAM} host MIT\n");
    let got = artifacts(
        VOS_PROGRAM,
        metadata(&pkgs, &["host"], &[("host", "mystery", None)]),
        &record,
    );
    assert_eq!(
        got.iter().map(|v| v.reason).collect::<Vec<_>>(),
        vec![ArtifactReason::UnknownLicence]
    );
}

// verifies: DEC-6
#[test]
fn malformed_directives_and_metadata_are_errors() {
    for bad in [
        "@holds vos-api 0.1.0\n",
        "@artifact lock\n",
        "@core\n",
        "@exception a b\n",
        "@nope x\n",
    ] {
        assert!(check_locks(&[], bad).is_err(), "{bad}");
    }
    assert!(
        check_locks(&[], "@holds ghost 0.1.0 Cargo.lock\n").is_err(),
        "a `@holds` for no package line"
    );
    assert!(
        check_artifacts(
            &[ArtifactInput {
                lockfile: "Cargo.lock".into(),
                metadata: "{".into()
            }],
            ""
        )
        .is_err()
    );
    assert!(
        check_artifacts(
            &[ArtifactInput {
                lockfile: "Cargo.lock".into(),
                metadata: "{\"packages\":[],\"workspace_members\":[],\"resolve\":null}".into()
            }],
            ""
        )
        .is_err()
    );
    let _ = check("version = 4\n", "", &[]);
}

// verifies: DEC-6
#[test]
fn a_root_workspace_crate_in_neither_core_list_fails_the_licence_check() {
    let pkgs: [P; 3] = [
        ("rutent-a", "0.0.0", Some("MIT")),
        ("rutent-b", "0.0.0", Some("MIT")),
        ("rutent-c", "0.0.0", Some("MIT")),
    ];
    let root = |record: &str| {
        artifacts(
            "Cargo.lock",
            metadata_in("", &pkgs, &["rutent-a", "rutent-b", "rutent-c"], &[]),
            record,
        )
        .into_iter()
        .filter(|v| v.lockfile == "licences/RECORD")
        .map(|v| (v.artifact, v.reason))
        .collect::<Vec<_>>()
    };
    let listed = "@core rutent-a\n@noncore rutent-b a tool of the repository's own checks\n@noncore rutent-c a tool\n";
    assert_eq!(root(listed), vec![]);
    // A crate in neither list, as a new crate of the workspace would be.
    let got = root("@core rutent-a\n@noncore rutent-b a tool\n");
    assert_eq!(
        got,
        vec![("rutent-c".to_string(), ArtifactReason::CoreUnclassified)]
    );
    // A crate in both lists.
    let got = root(&format!("{listed}@core rutent-c\n"));
    assert_eq!(
        got,
        vec![("rutent-c".to_string(), ArtifactReason::CoreUnclassified)]
    );
}

// verifies: DEC-6
#[test]
fn a_core_or_noncore_name_that_is_not_a_root_workspace_crate_fails_the_licence_check() {
    let pkgs: [P; 1] = [("rutent-a", "0.0.0", Some("MIT"))];
    for record in [
        "@core rutent-a\n@core rutent-ghost\n",
        "@core rutent-a\n@noncore rutent-ghost a tool\n",
    ] {
        let got: Vec<_> = artifacts(
            "Cargo.lock",
            metadata_in("", &pkgs, &["rutent-a"], &[]),
            record,
        )
        .into_iter()
        .filter(|v| v.lockfile == "licences/RECORD")
        .map(|v| (v.artifact, v.reason))
        .collect();
        assert_eq!(
            got,
            vec![("rutent-ghost".to_string(), ArtifactReason::CoreNotMember)],
            "{record}"
        );
    }
}

// verifies: DEC-6
#[test]
fn a_dependency_is_read_as_the_stricter_of_its_record_and_its_manifest() {
    let edges = [
        ("host", "rutent-stm", None),
        ("host", "dep", None),
        ("rutent-stm", "rutent-contracts", None),
    ];
    let base = "@core rutent-stm\n@core rutent-contracts\n@artifact components/stm/vos-program/Cargo.lock host MIT\n";
    let run = |manifest: &'static str, recorded: &str| {
        let pkgs: [P; 4] = [
            ("host", "0.0.0", Some("MIT")),
            ("rutent-stm", "0.0.0", Some("MIT")),
            ("rutent-contracts", "0.0.0", Some("MIT")),
            ("dep", "1.0.0", Some(manifest)),
        ];
        let record = format!("{base}dep 1.0.0 {recorded} permitted licence-permissive-standing\n");
        artifacts(VOS_PROGRAM, metadata(&pkgs, &["host"], &edges), &record)
            .iter()
            .map(|v| v.reason)
            .collect::<Vec<_>>()
    };
    assert_eq!(run("MIT", "MIT"), vec![]);
    // The manifest says AGPL, the record MIT: the stricter decides.
    assert_eq!(
        run("AGPL-3.0-or-later", "MIT"),
        vec![ArtifactReason::CoreWithCopyleft; 2]
    );
    // The manifest says MIT, the record AGPL: the same.
    assert_eq!(
        run("MIT", "AGPL-3.0-or-later"),
        vec![ArtifactReason::CoreWithCopyleft; 2]
    );
}

// verifies: DEC-6
#[test]
fn an_exception_holds_in_the_lock_it_names_only() {
    let edges = [
        ("host", "rutent-stm", None),
        ("host", "vos-sdk", None),
        ("rutent-stm", "rutent-contracts", None),
    ];
    let record = format!(
        "{CORE}@artifact {VOS_PROGRAM} host AGPL-3.0-or-later\n@artifact {VOS_CLIENT} host AGPL-3.0-or-later\n\
         @exception stm-inside-agpl-program {VOS_PROGRAM} host rutent-stm,rutent-contracts\n"
    );
    let run = |lock: &str, dir: &str| {
        check_artifacts(
            &[ArtifactInput {
                lockfile: lock.into(),
                metadata: metadata_in(dir, &PKGS, &["host"], &edges),
            }],
            &record,
        )
        .expect("fixture parses")
    };
    assert_eq!(run(VOS_PROGRAM, "components/stm/vos-program"), vec![]);
    // The same artifact name in the client's lock does not inherit it.
    let got = run(VOS_CLIENT, "components/stm/vos-program/client");
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(
        got.iter()
            .all(|v| v.lockfile == VOS_CLIENT && v.reason == ArtifactReason::CoreWithCopyleft)
    );
}

// verifies: DEC-6
#[test]
fn a_same_named_artifact_of_another_lock_does_not_speak_for_a_path_package() {
    let pkgs: [P; 3] = [
        ("host", "0.0.0", Some("MIT")),
        ("helper", "0.0.0", Some("MIT")),
        ("rutent-stm", "0.0.0", Some("MIT")),
    ];
    let edges = [("host", "helper", None), ("host", "rutent-stm", None)];
    let record = |helper_lock: &str| {
        format!(
            "@core rutent-stm\n@artifact tools/x/Cargo.lock host MIT\n@artifact {helper_lock} helper AGPL-3.0-or-later\n"
        )
    };
    let run = |helper_lock: &str| {
        check_artifacts(
            &[ArtifactInput {
                lockfile: "tools/x/Cargo.lock".into(),
                metadata: metadata_in("tools/x", &pkgs, &["host"], &edges),
            }],
            &record(helper_lock),
        )
        .expect("fixture parses")
        .iter()
        .map(|v| v.reason)
        .collect::<Vec<_>>()
    };
    // `helper` is in `tools/x`, whose lock has no line for it; the other lock's line is not its own.
    assert!(!run("experiments/y/Cargo.lock").contains(&ArtifactReason::CoreWithCopyleft));
    // The line of the lock that owns it is read.
    assert!(run("tools/x/Cargo.lock").contains(&ArtifactReason::CoreWithCopyleft));
}

// verifies: DEC-6
#[test]
fn the_locks_the_tool_reads_are_the_locks_git_lists() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "*Cargo.lock",
        ])
        .current_dir(&root)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git ls-files failed");
    let mut listed: Vec<String> = String::from_utf8(out.stdout)
        .expect("utf-8 paths")
        .lines()
        .filter(|l| l.rsplit('/').next() == Some("Cargo.lock"))
        .map(str::to_string)
        .collect();
    listed.sort();
    let mut read = rutent_licence_check::find_all_locks(&root).expect("listing");
    read.push("Cargo.lock".to_string());
    read.sort();
    assert_eq!(
        read, listed,
        "the tool must read exactly the locks of the repository"
    );
}
