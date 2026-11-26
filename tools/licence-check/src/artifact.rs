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
    /// Its manifest gives a `license-file` and no `license`, and the file cannot be read or has no SPDX header.
    LicenceFileUnread,
    /// Its graph holds a crate of rutent's core and a package outside the permissive set, with no exception.
    CoreWithCopyleft,
    /// `rutent-stm` or `rutent-contracts` has a package outside the permissive set in its own graph.
    CoreNotPermissive,
    /// A package in its graph has no licence the record can tell.
    UnknownLicence,
    /// A crate of the root workspace is in neither the `@core` nor the `@noncore` lines, or is in both. The
    /// violation names `licences/RECORD` as its lockfile.
    CoreUnclassified,
    /// An `@core` or `@noncore` name is not a crate of the root workspace. The violation names `licences/RECORD`.
    CoreNotMember,
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
            ArtifactReason::LicenceFileUnread => {
                "gives a license-file and no license, and the file has no readable SPDX header"
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
            ArtifactReason::CoreUnclassified => {
                "is a crate of the root workspace that the record lists as neither `@core` nor `@noncore`, or as both"
            }
            ArtifactReason::CoreNotMember => {
                "is listed `@core` or `@noncore` but is not a crate of the root workspace"
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
    /// The `license_file` of its manifest, and its manifest's path, for a package that gives no `license`.
    license_file: Option<String>,
    manifest: Option<String>,
    /// For a path package (no `source`), the directory of its manifest relative to the repository root.
    dir: Option<String>,
    path: bool,
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

/// The directory of a path package's manifest relative to the repository root, from the lock's workspace root
/// (`lockdir` is the lock's own directory relative to the repository root, `""` for the root lock).
fn package_dir(
    workspace_root: Option<&str>,
    lockdir: &str,
    manifest: Option<&str>,
) -> Option<String> {
    let root = workspace_root?;
    let repo = if lockdir.is_empty() {
        root
    } else {
        root.strip_suffix(lockdir)?.trim_end_matches('/')
    };
    let rel = manifest?.strip_prefix(repo)?.trim_start_matches('/');
    let dir = rel.strip_suffix("Cargo.toml")?.trim_end_matches('/');
    Some(dir.to_string())
}

fn parse_meta(text_in: &str, lockfile: &str) -> Result<Meta, String> {
    let doc = json::parse(text_in)?;
    let lockdir = lockfile
        .strip_suffix("Cargo.lock")
        .unwrap_or_default()
        .trim_end_matches('/');
    let workspace_root = doc.get("workspace_root").and_then(Json::as_str);
    let mut pkgs = BTreeMap::new();
    for p in items(&doc, "packages")? {
        let path = p.get("source").is_none_or(|s| *s == Json::Null);
        let manifest = p.get("manifest_path").and_then(Json::as_str);
        pkgs.insert(
            text(p, "id")?,
            Pkg {
                name: text(p, "name")?,
                version: text(p, "version")?,
                license: p.get("license").and_then(Json::as_str).map(str::to_string),
                license_file: p
                    .get("license_file")
                    .and_then(Json::as_str)
                    .map(str::to_string),
                manifest: manifest.map(str::to_string),
                dir: if path {
                    package_dir(workspace_root, lockdir, manifest)
                } else {
                    None
                },
                path,
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

/// The licences a package is read with: for a path package, the `@artifact` line of the lock that owns it (the
/// line of that name whose lock directory is the longest prefix of the package's directory, so a same-named
/// artifact of another lock is never taken); for any package, the record's line at its locked version, and the
/// manifest's declaration.
fn licences_of(record: &Record, p: &Pkg) -> Vec<String> {
    let mut out = Vec::new();
    if p.path
        && let Some(dir) = &p.dir
    {
        let owner = record
            .artifacts
            .iter()
            .filter(|a| a.name == p.name)
            .filter_map(|a| {
                let lockdir = a.lockfile.strip_suffix("Cargo.lock")?.trim_end_matches('/');
                let inside = lockdir.is_empty()
                    || dir == lockdir
                    || dir
                        .strip_prefix(lockdir)
                        .is_some_and(|r| r.starts_with('/'));
                inside.then_some((lockdir.len(), a))
            })
            .max_by_key(|(n, _)| *n);
        out.extend(owner.map(|(_, a)| a.licence.clone()));
    }
    out.extend(
        record
            .entries
            .iter()
            .find(|e| e.name == p.name && e.version == p.version)
            .map(|e| e.licence.clone()),
    );
    out.extend(p.license.clone());
    out
}

/// Whether a package is permissive: every licence it is read with must be, so the stricter of the record and the
/// manifest decides. `None` when there is no licence to read.
fn permissive_of(record: &Record, p: &Pkg) -> Option<bool> {
    let all = licences_of(record, p);
    (!all.is_empty()).then(|| all.iter().all(|l| is_permissive(l)))
}

/// Checks each artifact of each lock against the record.
///
/// Returns the failing artifacts, or an error if the record or any metadata text is malformed.
pub fn check_artifacts(
    inputs: &[ArtifactInput],
    record: &str,
) -> Result<Vec<ArtifactViolation>, String> {
    check_artifacts_with(inputs, record, &|_| None)
}

/// A licence text for comparison: a `/` read as ` OR `, and runs of whitespace as one space, so that `Apache-2.0/MIT`
/// and `Apache-2.0 OR MIT` are the same text, and nothing else is.
fn canon(licence: &str) -> String {
    licence
        .replace('/', " OR ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The text of the SPDX header of a licence file: the expression after `SPDX-License-Identifier:` in its first
/// lines, without a comment closer.
fn spdx_header(text: &str) -> Option<String> {
    text.lines().take(30).find_map(|line| {
        let rest = line.split_once("SPDX-License-Identifier:")?.1;
        let rest = rest
            .trim()
            .trim_end_matches("*/")
            .trim_end_matches("-->")
            .trim();
        (!rest.is_empty()).then(|| rest.to_string())
    })
}

/// As [`check_artifacts`], reading a manifest's `license-file` through `read` (given the file's path, absolute or
/// as the metadata gives it): an artifact with a `license-file` and no `license` is compared by the file's SPDX
/// header, and fails with [`ArtifactReason::LicenceFileUnread`] when `read` gives nothing or the header is missing.
pub fn check_artifacts_with(
    inputs: &[ArtifactInput],
    record: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<ArtifactViolation>, String> {
    let record = parse_record(record)?;
    let mut found: Vec<ArtifactViolation> = Vec::new();
    check_core_lists(inputs, &record, &mut found)?;
    for input in inputs {
        let mut meta = parse_meta(&input.metadata, &input.lockfile)
            .map_err(|e| format!("{}: {e}", input.lockfile))?;
        // A member that gives a `license-file` and no `license` is read by the file's SPDX header, or fails.
        let mut unread = Vec::new();
        for id in meta.members.clone() {
            let p = meta.pkgs.get_mut(&id).expect("members are packages");
            if p.license.is_some() {
                continue;
            }
            let Some(file) = p.license_file.clone() else {
                continue;
            };
            let path = if file.starts_with('/') {
                file
            } else {
                let dir = p
                    .manifest
                    .as_deref()
                    .and_then(|m| m.rsplit_once('/'))
                    .map_or("", |d| d.0);
                format!("{dir}/{file}")
            };
            match read(&path).and_then(|t| spdx_header(&t)) {
                Some(header) => p.license = Some(header),
                None => unread.push((p.name.clone(), path)),
            }
        }
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
            let l = licences_of(&record, p);
            let l = if l.is_empty() {
                "unknown".to_string()
            } else {
                let mut l = l;
                l.dedup();
                l.join(" / ")
            };
            format!("{} {} ({l})", p.name, p.version)
        };
        for (name, path) in unread {
            push(&name, ArtifactReason::LicenceFileUnread, path);
        }
        // Every locked package whose record licence differs from its manifest's fails, dependencies too.
        for p in meta.pkgs.values() {
            let entry = record
                .entries
                .iter()
                .find(|e| e.name == p.name && e.version == p.version);
            if let (Some(e), Some(declared)) = (entry, &p.license)
                && canon(&e.licence) != canon(declared)
            {
                push(
                    &p.name,
                    ArtifactReason::LicenceMismatch,
                    format!("record `{}`, manifest `{declared}`", e.licence),
                );
            }
        }
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
                        && canon(declared) != canon(&line.licence)
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
                match permissive_of(&record, p) {
                    None => push(&artifact.name, ArtifactReason::UnknownLicence, describe(g)),
                    Some(false) => copyleft.push(g.clone()),
                    Some(true) => {}
                }
                if record.cores.contains(&p.name) {
                    cores.push(&p.name);
                }
            }
            if !copyleft.is_empty() {
                for core in cores {
                    let excepted = record.exceptions.iter().any(|e| {
                        e.lockfile == input.lockfile
                            && e.artifact == artifact.name
                            && e.core_crates.contains(core)
                    });
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
                match permissive_of(&record, q) {
                    Some(true) => {}
                    Some(false) => push(&p.name, ArtifactReason::CoreNotPermissive, describe(g)),
                    None => push(&p.name, ArtifactReason::UnknownLicence, describe(g)),
                }
            }
        }
    }
    Ok(found)
}

/// Every crate of the root workspace is `@core` or `@noncore`, and not both; every such name is a crate of the root
/// workspace. An error when no input is the root lock (`Cargo.lock`), which the tool always gives.
fn check_core_lists(
    inputs: &[ArtifactInput],
    record: &Record,
    found: &mut Vec<ArtifactViolation>,
) -> Result<(), String> {
    let Some(root) = inputs.iter().find(|i| i.lockfile == "Cargo.lock") else {
        return Err(
            "no input is the root lock `Cargo.lock`, so the core lists cannot be checked"
                .to_string(),
        );
    };
    let meta =
        parse_meta(&root.metadata, &root.lockfile).map_err(|e| format!("Cargo.lock: {e}"))?;
    let members: Vec<&str> = meta
        .members
        .iter()
        .map(|m| meta.pkgs[m].name.as_str())
        .collect();
    let mut push = |name: &str, reason, detail: &str| {
        found.push(ArtifactViolation {
            lockfile: "licences/RECORD".to_string(),
            artifact: name.to_string(),
            reason,
            detail: detail.to_string(),
        });
    };
    for m in &members {
        let (core, non) = (
            record.cores.iter().any(|c| c == m),
            record.noncores.iter().any(|c| c == m),
        );
        if core == non {
            push(
                m,
                ArtifactReason::CoreUnclassified,
                if core {
                    "listed as both"
                } else {
                    "listed as neither"
                },
            );
        }
    }
    for n in record.cores.iter().chain(&record.noncores) {
        if !members.contains(&n.as_str()) {
            push(
                n,
                ArtifactReason::CoreNotMember,
                "no such crate in the root workspace",
            );
        }
    }
    Ok(())
}
