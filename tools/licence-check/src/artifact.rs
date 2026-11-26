//! The artifact check (T-001-06): each package rutent builds is read through its resolved dependency graph, from
//! the output of `cargo metadata` for its lock.
//!
//! An artifact is a workspace member of a lock. Its graph is the packages reachable from it through normal and
//! build dependencies (a dev dependency is not part of what ships). The check fails on:
//! - an artifact the record does not list with its licence (`@artifact` lines), or lists with another licence than
//!   its manifest declares;
//! - an artifact whose graph holds both a crate of rutent's own core (`@core` lines) and a package outside the
//!   permissive licence set, unless an `@exception` line names that artifact and that core crate;
//! - `rutent-stm` or `rutent-contracts`, wherever they appear, whose own graph holds a package outside the
//!   permissive set;
//! - a package in a graph whose licence the record cannot tell.
//!
//! Every crate of rutent's own core stays MIT; a crate that must inherit a copyleft or source-available licence is
//! its own separately licensed crate behind the runtime-host seam.

use crate::json::{self, Json};
use crate::spdx::is_permissive;
use crate::{Record, parse_record};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The packages whose own dependency graphs hold permissive crates only, in every lock.
const PERMISSIVE_ONLY: [&str; 2] = ["rutent-stm", "rutent-contracts"];

/// The `cargo metadata` output of one lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactInput {
    /// The lock's path relative to the repository root, with `/` separators, as the record's `@artifact` lines
    /// name it.
    pub lockfile: String,
    /// The text of `cargo metadata --format-version 1` for that lock's workspace, with all features.
    pub metadata: String,
}

/// Why an artifact fails the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactReason {
    /// The record has no `@artifact` line for it in this lock.
    Unrecorded,
    /// The record's licence for it differs from the one its manifest declares.
    LicenceMismatch,
    /// Its graph holds a crate of rutent's core and a package outside the permissive set, with no exception.
    CoreWithCopyleft,
    /// `rutent-stm` or `rutent-contracts` has a package outside the permissive set in its own graph.
    CoreNotPermissive,
    /// A package in its graph has no licence the record can tell.
    UnknownLicence,
}

/// One artifact that fails the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactViolation {
    /// The lock the artifact is in.
    pub lockfile: String,
    /// The artifact's package name.
    pub artifact: String,
    /// Why it fails.
    pub reason: ArtifactReason,
    /// The package or licence the failure names.
    pub detail: String,
}

impl fmt::Display for ArtifactViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.reason {
            ArtifactReason::Unrecorded => "has no `@artifact` line in licences/RECORD",
            ArtifactReason::LicenceMismatch => {
                "is recorded with another licence than its manifest declares"
            }
            ArtifactReason::CoreWithCopyleft => {
                "holds a crate of rutent's core and a package outside the permissive licences, with no exception"
            }
            ArtifactReason::CoreNotPermissive => {
                "must hold permissive crates only, and its graph holds one that is not"
            }
            ArtifactReason::UnknownLicence => {
                "has a package in its graph whose licence the record cannot tell"
            }
        };
        write!(
            f,
            "{}: artifact {} {what}: {}",
            self.lockfile, self.artifact, self.detail
        )
    }
}

struct Pkg {
    name: String,
    version: String,
    license: Option<String>,
}

struct Meta {
    pkgs: BTreeMap<String, Pkg>,
    members: Vec<String>,
    deps: BTreeMap<String, Vec<String>>,
}

fn field<'a>(v: &'a Json, key: &str) -> Result<&'a Json, String> {
    v.get(key).ok_or_else(|| format!("no `{key}`"))
}

fn text(v: &Json, key: &str) -> Result<String, String> {
    field(v, key)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` is not a string"))
}

fn items<'a>(v: &'a Json, key: &str) -> Result<&'a [Json], String> {
    field(v, key)?
        .as_arr()
        .ok_or_else(|| format!("`{key}` is not an array"))
}

fn parse_meta(text_in: &str) -> Result<Meta, String> {
    let doc = json::parse(text_in)?;
    let mut pkgs = BTreeMap::new();
    for p in items(&doc, "packages")? {
        pkgs.insert(
            text(p, "id")?,
            Pkg {
                name: text(p, "name")?,
                version: text(p, "version")?,
                license: p.get("license").and_then(Json::as_str).map(str::to_string),
            },
        );
    }
    let members = items(&doc, "workspace_members")?
        .iter()
        .map(|m| {
            m.as_str()
                .map(str::to_string)
                .ok_or("a workspace member is not a string")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let resolve = field(&doc, "resolve")?;
    if *resolve == Json::Null {
        return Err("`resolve` is null; read the metadata without --no-deps".to_string());
    }
    let mut deps = BTreeMap::new();
    for n in items(resolve, "nodes")? {
        let mut out = Vec::new();
        for d in items(n, "deps")? {
            // An edge that is only a dev dependency is not part of what ships. A missing `dep_kinds` is a normal one.
            let only_dev = d
                .get("dep_kinds")
                .and_then(Json::as_arr)
                .is_some_and(|kinds| {
                    !kinds.is_empty()
                        && kinds
                            .iter()
                            .all(|k| k.get("kind").and_then(Json::as_str) == Some("dev"))
                });
            if !only_dev {
                out.push(text(d, "pkg")?);
            }
        }
        deps.insert(text(n, "id")?, out);
    }
    for m in &members {
        if !pkgs.contains_key(m) {
            return Err(format!("workspace member {m} is not among the packages"));
        }
    }
    Ok(Meta {
        pkgs,
        members,
        deps,
    })
}

impl Meta {
    /// The ids reachable from `root`, `root` included.
    fn graph(&self, root: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![root.to_string()];
        while let Some(id) = todo.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(next) = self.deps.get(&id) {
                todo.extend(next.iter().cloned());
            }
        }
        seen
    }
}

/// The licence of a package in a graph, from the record: the lock's own `@artifact` line, a package line at the
/// locked version, an `@artifact` line of another lock, then the manifest's declaration.
fn licence_of(record: &Record, lockfile: &str, p: &Pkg) -> Option<String> {
    record
        .artifacts
        .iter()
        .find(|a| a.lockfile == lockfile && a.name == p.name)
        .map(|a| a.licence.clone())
        .or_else(|| {
            record
                .entries
                .iter()
                .find(|e| e.name == p.name && e.version == p.version)
                .map(|e| e.licence.clone())
        })
        .or_else(|| {
            record
                .artifacts
                .iter()
                .find(|a| a.name == p.name)
                .map(|a| a.licence.clone())
        })
        .or_else(|| p.license.clone())
}

/// Checks each artifact of each lock against the record.
///
/// Returns the failing artifacts, or an error if the record or any metadata text is malformed.
pub fn check_artifacts(
    inputs: &[ArtifactInput],
    record: &str,
) -> Result<Vec<ArtifactViolation>, String> {
    let record = parse_record(record)?;
    let mut found: Vec<ArtifactViolation> = Vec::new();
    for input in inputs {
        let meta = parse_meta(&input.metadata).map_err(|e| format!("{}: {e}", input.lockfile))?;
        let mut push = |artifact: &str, reason, detail: String| {
            let v = ArtifactViolation {
                lockfile: input.lockfile.clone(),
                artifact: artifact.to_string(),
                reason,
                detail,
            };
            if !found.contains(&v) {
                found.push(v);
            }
        };
        let describe = |id: &str| {
            let p = &meta.pkgs[id];
            let l =
                licence_of(&record, &input.lockfile, p).unwrap_or_else(|| "unknown".to_string());
            format!("{} {} ({l})", p.name, p.version)
        };
        for id in &meta.members {
            let artifact = &meta.pkgs[id];
            match record
                .artifacts
                .iter()
                .find(|a| a.lockfile == input.lockfile && a.name == artifact.name)
            {
                None => push(
                    &artifact.name,
                    ArtifactReason::Unrecorded,
                    "no line".to_string(),
                ),
                Some(line) => {
                    if let Some(declared) = &artifact.license
                        && *declared != line.licence
                    {
                        push(
                            &artifact.name,
                            ArtifactReason::LicenceMismatch,
                            format!("record `{}`, manifest `{declared}`", line.licence),
                        );
                    }
                }
            }
            let graph = meta.graph(id);
            let mut copyleft = Vec::new();
            let mut cores = Vec::new();
            for g in &graph {
                let p = &meta.pkgs[g];
                match licence_of(&record, &input.lockfile, p) {
                    None => push(&artifact.name, ArtifactReason::UnknownLicence, describe(g)),
                    Some(l) if !is_permissive(&l) => copyleft.push(g.clone()),
                    Some(_) => {}
                }
                if record.cores.contains(&p.name) {
                    cores.push(&p.name);
                }
            }
            if !copyleft.is_empty() {
                for core in cores {
                    let excepted = record
                        .exceptions
                        .iter()
                        .any(|e| e.artifact == artifact.name && e.core_crates.contains(core));
                    if !excepted {
                        push(
                            &artifact.name,
                            ArtifactReason::CoreWithCopyleft,
                            format!(
                                "{core} with {}",
                                copyleft
                                    .iter()
                                    .map(|g| describe(g))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                }
            }
        }
        for (id, p) in &meta.pkgs {
            if !PERMISSIVE_ONLY.contains(&p.name.as_str()) || !meta.deps.contains_key(id) {
                continue;
            }
            for g in meta.graph(id).iter().filter(|g| *g != id) {
                let q = &meta.pkgs[g];
                match licence_of(&record, &input.lockfile, q) {
                    Some(l) if is_permissive(&l) => {}
                    Some(_) => push(&p.name, ArtifactReason::CoreNotPermissive, describe(g)),
                    None => push(&p.name, ArtifactReason::UnknownLicence, describe(g)),
                }
            }
        }
    }
    Ok(found)
}
