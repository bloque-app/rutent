//! The crate graph as cargo resolves it, the one the build uses: it keeps the log's handles inside the assembly.

use rutent_boundary_check::load;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Our crates and the crates of ours each depends on, with whether the edge is a dev-dependency.
type Edges = BTreeMap<String, BTreeSet<(String, bool)>>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The workspace members' edges between our crates, read from `cargo metadata`: every table, renames and target
/// tables included, by the package each edge names.
fn metadata() -> Edges {
    let out = Command::new(std::env::var_os("CARGO").unwrap_or("cargo".into()))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(root().join("Cargo.toml"))
        .output()
        .expect("cargo runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("cargo metadata is JSON");
    let members: BTreeSet<&str> = json["workspace_members"]
        .as_array()
        .expect("workspace members")
        .iter()
        .map(|m| m.as_str().expect("a member id"))
        .collect();
    let mut edges = Edges::new();
    for package in json["packages"].as_array().expect("packages") {
        if !members.contains(package["id"].as_str().expect("a package id")) {
            continue;
        }
        let name = package["name"].as_str().expect("a package name");
        let deps = edges.entry(name.to_string()).or_default();
        for dep in package["dependencies"].as_array().expect("dependencies") {
            let dep_name = dep["name"].as_str().expect("a dependency name");
            if dep_name.starts_with("rutent-") {
                deps.insert((dep_name.to_string(), dep["kind"] == "dev"));
            }
        }
    }
    edges
}

/// Every crate of ours `krate` reaches through edges of any kind.
fn reaches(edges: &Edges, krate: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut next = vec![krate.to_string()];
    while let Some(k) = next.pop() {
        for (dep, _) in edges.get(&k).into_iter().flatten() {
            if seen.insert(dep.clone()) {
                next.push(dep.clone());
            }
        }
    }
    seen
}

const CORE: [&str; 3] = ["rutent-core-orch", "rutent-core-authz", "rutent-core-audit"];
const SCENARIOS: &str = "rutent-scenarios";

/// The manifest check reads the same graph cargo does, edge for edge.
// verifies: DEC-2
#[test]
fn the_manifest_graph_is_the_graph_cargo_resolves() {
    let from_manifests: Edges = load(&root())
        .expect("workspace manifests are readable")
        .into_iter()
        .map(|(k, deps)| (k, deps.into_iter().map(|d| (d.name, d.dev)).collect()))
        .collect();
    assert_eq!(from_manifests, metadata());
}

/// `CMP-CORE-AUDIT` builds the log, and with it both its handles; `CMP-CORE-AUTHZ` holds policy's. No part of Core,
/// nor anything but the assembly, reaches either crate, directly or through another, so the assembly alone hands
/// out the handles, and policy's only to the policy engine.
// verifies: DEC-2, REQ-AUZ-1
#[test]
fn only_the_assembly_reaches_the_log_and_the_policy_engine() {
    let edges = metadata();
    for krate in CORE {
        let reached = reaches(&edges, krate);
        for other in CORE.iter().chain([&SCENARIOS]).filter(|&&o| o != krate) {
            assert!(!reached.contains(*other), "{krate} reaches {other}");
        }
    }
    for minted in ["rutent-core-audit", "rutent-core-authz"] {
        let reaching: Vec<&String> = edges
            .keys()
            .filter(|k| reaches(&edges, k).contains(minted))
            .collect();
        assert_eq!(reaching, vec![SCENARIOS], "the crates that reach {minted}");
    }
    assert!(
        edges
            .keys()
            .all(|k| !reaches(&edges, k).contains(SCENARIOS))
    );
}
