//! Licence record check (REQ-MOD-5, T-001-04).
//!
//! The check reads every `Cargo.lock` in the repository (the root one, and each one under `experiments/` or `components/`,
//! found by walking those trees and skipping `target` directories), the record `licences/RECORD` and the root manifest's
//! workspace members as text. It fails on a locked external package that the record does not list at its locked
//! version, and on one whose recorded decision is still `pending`; each violation names the lockfile it came from.
//! In the root lock a package is external unless it is a workspace member without a `source`, so a path
//! dependency from outside the workspace is checked like any registry package. An experiment's lock is checked
//! the same way against the members of the workspace beside it plus the root workspace's. One record serves
//! all the locks: a package locked at the same name and version in several of them needs one line, and a name
//! at several versions needs one line per version. Lines marked `not-adopted` never fail the check. The
//! record's format is explained in its own header.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Why a locked dependency fails the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The record has no line for this name at this locked version.
    Unrecorded,
    /// The record has a line for it, with its decision still pending.
    Pending,
}

/// One locked dependency that fails the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The lockfile the package is locked in, as a path relative to the repository root.
    pub lockfile: String,
    /// The package name, as `Cargo.lock` gives it.
    pub package: String,
    /// The locked version.
    pub version: String,
    /// Why it fails.
    pub reason: Reason,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            Reason::Unrecorded => write!(
                f,
                "{}: {} {}: not in licences/RECORD at this version",
                self.lockfile, self.package, self.version
            ),
            Reason::Pending => write!(
                f,
                "{}: {} {}: decision pending",
                self.lockfile, self.package, self.version
            ),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Pending,
    Permitted,
}

#[derive(Debug)]
struct Entry {
    name: String,
    version: String,
    decision: Decision,
}

/// A package as `Cargo.lock` locks it.
#[derive(Debug)]
struct Locked {
    name: String,
    version: String,
    has_source: bool,
}

/// A workspace member, as its own manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The `[package]` name.
    pub name: String,
    /// The `[package]` version, resolved from `[workspace.package]` when inherited.
    pub version: String,
}

/// One lockfile to check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockFile {
    /// The path relative to the repository root, with `/` separators. Violations name it.
    pub path: String,
    /// The file's text.
    pub text: String,
    /// The workspace members that may appear in this lock without a `source`. A package without a `source` that
    /// is not one of them is checked like any registry package.
    pub members: Vec<Member>,
}

/// Checks the record text against the root lockfile text, given the workspace members.
///
/// Returns the failing dependencies, or an error if any text is malformed. A malformed text is an
/// error, never a silent pass.
pub fn check(lock: &str, record: &str, members: &[Member]) -> Result<Vec<Violation>, String> {
    check_locks(
        &[LockFile {
            path: "Cargo.lock".to_string(),
            text: lock.to_string(),
            members: members.to_vec(),
        }],
        record,
    )
}

/// Checks the record text against every given lockfile, in order.
///
/// Returns the failing dependencies, each naming its lockfile, or an error if any text is malformed.
pub fn check_locks(locks: &[LockFile], record: &str) -> Result<Vec<Violation>, String> {
    let entries = parse_record(record)?;
    let mut found = Vec::new();
    for lock in locks {
        check_one(lock, &entries, &mut found)?;
    }
    Ok(found)
}

fn check_one(lock: &LockFile, entries: &[Entry], found: &mut Vec<Violation>) -> Result<(), String> {
    let locked = parse_lock(&lock.text).map_err(|e| format!("{}: {e}", lock.path))?;
    for pkg in locked {
        let is_member = !pkg.has_source
            && lock
                .members
                .iter()
                .any(|m| m.name == pkg.name && m.version == pkg.version);
        if is_member {
            continue;
        }
        let entry = entries
            .iter()
            .find(|e| e.name == pkg.name && e.version == pkg.version);
        match entry {
            None => found.push(Violation {
                lockfile: lock.path.clone(),
                package: pkg.name,
                version: pkg.version,
                reason: Reason::Unrecorded,
            }),
            Some(e) if e.decision == Decision::Pending => found.push(Violation {
                lockfile: lock.path.clone(),
                package: pkg.name,
                version: pkg.version,
                reason: Reason::Pending,
            }),
            Some(_) => {}
        }
    }
    Ok(())
}

/// The trees whose own `Cargo.lock`s the check reads beside the root one: the experiments, and the packages of
/// `components/` that are workspaces of their own, such as the VOS program's (`T-010-05`).
pub const LOCK_DIRS: [&str; 2] = ["experiments", "components"];

/// The paths of every `Cargo.lock` under [`LOCK_DIRS`], relative to `root`, sorted. See [`find_locks`].
pub fn find_all_locks(root: &Path) -> Result<Vec<String>, String> {
    let mut all = Vec::new();
    for dir in LOCK_DIRS {
        all.extend(find_locks(root, dir)?);
    }
    all.sort();
    Ok(all)
}

/// The paths of every `Cargo.lock` under `dir`, relative to `root`, sorted. A directory named `target` is not
/// entered. A symbolic link that is a `Cargo.lock` or a directory is an error, since the check would otherwise
/// pass over it silently. A missing `dir` has none.
pub fn find_locks(root: &Path, dir: &str) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    let start = root.join(dir);
    if start.is_dir() {
        walk(&start, &mut found)?;
    }
    let mut rel: Vec<String> = found
        .iter()
        .map(|p| {
            let tail = p.strip_prefix(root).unwrap_or(p);
            let parts: Vec<_> = tail.iter().map(|c| c.to_string_lossy()).collect();
            parts.join("/")
        })
        .collect();
    rel.sort();
    Ok(rel)
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let read = |e: std::io::Error| format!("cannot read {}: {e}", dir.display());
    for entry in fs::read_dir(dir).map_err(read)? {
        let entry = entry.map_err(read)?;
        let kind = entry.file_type().map_err(read)?;
        let path = entry.path();
        let is_target = entry.file_name() == "target";
        if kind.is_symlink() {
            if is_target && path.is_dir() {
                continue;
            }
            if entry.file_name() == "Cargo.lock" || path.is_dir() {
                return Err(format!(
                    "{} is a symbolic link; the licence check reads real files and directories only",
                    path.display()
                ));
            }
        } else if kind.is_dir() {
            if !is_target {
                walk(&path, found)?;
            }
        } else if kind.is_file() && entry.file_name() == "Cargo.lock" {
            found.push(path);
        }
    }
    Ok(())
}

/// Every package in a lockfile, with whether it has a `source`.
fn parse_lock(lock: &str) -> Result<Vec<Locked>, String> {
    let mut packages = Vec::new();
    let mut block: Option<(Option<String>, Option<String>, bool)> = None;
    for (n, raw) in lock.lines().enumerate() {
        let line = raw.trim();
        if line == "[[package]]" {
            flush(&mut block, &mut packages, n)?;
            block = Some((None, None, false));
        } else if line.starts_with('[') {
            flush(&mut block, &mut packages, n)?;
        } else if let Some(cur) = block.as_mut() {
            if let Some(v) = quoted(line, "name") {
                cur.0 = Some(v);
            } else if let Some(v) = quoted(line, "version") {
                cur.1 = Some(v);
            } else if quoted(line, "source").is_some() {
                cur.2 = true;
            }
        }
    }
    flush(&mut block, &mut packages, lock.lines().count())?;
    Ok(packages)
}

fn flush(
    block: &mut Option<(Option<String>, Option<String>, bool)>,
    packages: &mut Vec<Locked>,
    line: usize,
) -> Result<(), String> {
    if let Some((name, version, has_source)) = block.take() {
        let (Some(name), Some(version)) = (name, version) else {
            return Err(format!(
                "Cargo.lock: [[package]] before line {} has no name or version",
                line + 1
            ));
        };
        packages.push(Locked {
            name,
            version,
            has_source,
        });
    }
    Ok(())
}

/// The value of `key = "value"`, if the line is that key.
fn quoted(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    let inner = rest.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}

/// The lines of the record. Each line is `name version licence... decision reference`.
/// A second line for the same name and version is an error: the decision it would give is ambiguous.
fn parse_record(record: &str) -> Result<Vec<Entry>, String> {
    let mut entries: Vec<Entry> = Vec::new();
    for (n, raw) in record.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 {
            return Err(format!(
                "licences/RECORD line {}: expected name, version, licence, decision and reference",
                n + 1
            ));
        }
        let decision = match fields[fields.len() - 2] {
            "pending" => Decision::Pending,
            "permitted" => Decision::Permitted,
            other => {
                return Err(format!(
                    "licences/RECORD line {}: unknown decision `{other}`",
                    n + 1
                ));
            }
        };
        let name = fields[0];
        let version = fields[1];
        if entries
            .iter()
            .any(|e| e.name == name && e.version == version)
        {
            return Err(format!(
                "licences/RECORD line {}: duplicate line for {name} {version}",
                n + 1
            ));
        }
        entries.push(Entry {
            name: name.to_string(),
            version: version.to_string(),
            decision,
        });
    }
    Ok(entries)
}

/// The workspace member paths listed in a root manifest's `[workspace]` `members` array.
///
/// Only two plain forms are read: `members = [` on its own line, then one double-quoted path per line, then `]`;
/// or the whole array on one line, `members = ["a", "b"]`.
/// A glob, a comment on a member line, or any other form is an error, so no member is silently missed.
pub fn member_paths(root_manifest: &str) -> Result<Vec<String>, String> {
    let mut in_workspace = false;
    let mut collecting = false;
    let mut found_members = false;
    let mut paths = Vec::new();
    for raw in root_manifest.lines() {
        let line = raw.trim();
        if collecting {
            if line == "]" {
                collecting = false;
                found_members = true;
                continue;
            }
            if line.is_empty() {
                continue;
            }
            let item = line.strip_suffix(',').unwrap_or(line).trim_end();
            paths.push(plain_member(item)?);
        } else if line.starts_with('[') {
            in_workspace = line == "[workspace]";
        } else if in_workspace && line.starts_with("members") {
            if let Some(inline) = inline_members(line) {
                for item in inline.split(',').map(str::trim).filter(|i| !i.is_empty()) {
                    paths.push(plain_member(item)?);
                }
                found_members = true;
                continue;
            }
            if !quoted_array_start(line) {
                return Err(format!(
                    "Cargo.toml: unsupported `members` syntax `{line}`; use `members = [` on its own line"
                ));
            }
            collecting = true;
        }
    }
    if collecting {
        return Err("Cargo.toml: the `members` array is not closed".to_string());
    }
    if !found_members {
        return Err("Cargo.toml: no `members` array under [workspace]".to_string());
    }
    Ok(paths)
}

/// The paths of the packages a workspace owns, relative to its manifest's directory, from that manifest's text.
///
/// A manifest with a `[package]` owns it (path `.`), and a `[workspace]` with a `members` array owns those
/// paths too (see [`member_paths`]). A manifest with neither a `[workspace]` nor a `[package]` is an error, and
/// so is a `[workspace]` with neither a `[package]` nor a `members` array, so a lock is never matched to nothing.
pub fn workspace_member_paths(manifest: &str) -> Result<Vec<String>, String> {
    let has = |header: &str| manifest.lines().any(|l| l.trim() == header);
    let (workspace, package) = (has("[workspace]"), has("[package]"));
    if !workspace && !package {
        return Err("Cargo.toml: neither a [workspace] nor a [package]".to_string());
    }
    let has_members = manifest.lines().any(|l| l.trim().starts_with("members"));
    let mut paths = if workspace && (!package || has_members) {
        member_paths(manifest)?
    } else {
        Vec::new()
    };
    if package {
        paths.push(".".to_string());
    }
    Ok(paths)
}

/// The text between the brackets of a one-line `members = ["a", "b"]`, when the line is that form.
fn inline_members(line: &str) -> Option<&str> {
    line.strip_prefix("members")?
        .trim_start()
        .strip_prefix('=')?
        .trim()
        .strip_prefix('[')?
        .strip_suffix(']')
}

/// A member path from its double-quoted item, or an error for any other form.
fn plain_member(item: &str) -> Result<String, String> {
    item.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .filter(|p| !p.is_empty() && !p.contains(['"', '\\', '*', '#', '[', ']', ',']))
        .map(str::to_string)
        .ok_or_else(|| {
            format!("Cargo.toml: unsupported workspace member `{item}`; list plain paths only")
        })
}

/// Whether a `members` line opens the plain array form, `members = [`.
fn quoted_array_start(line: &str) -> bool {
    line.strip_prefix("members")
        .map(str::trim_start)
        .and_then(|s| s.strip_prefix('='))
        .map(str::trim)
        == Some("[")
}

/// The `name` and `version` of a workspace member, from its manifest text.
///
/// The version may be inherited (`version.workspace = true`), in which case `root_manifest`'s
/// `[workspace.package]` supplies it. Any other form of `name` or `version` is an error, and so is a
/// missing field, so a member is never read with a guessed identity.
pub fn package_identity(manifest: &str, root_manifest: &str) -> Result<Member, String> {
    let mut section = "";
    let mut name: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut inherits = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if section != "[package]" || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = key_value(line) else {
            continue;
        };
        match key {
            "name" => {
                if name.is_some() {
                    return Err("Cargo.toml: more than one `name` in [package]".to_string());
                }
                name = Some(plain_string(value).ok_or("Cargo.toml: `name` is not a plain string")?);
            }
            "version" => {
                if version.is_some() {
                    return Err("Cargo.toml: more than one `version` in [package]".to_string());
                }
                version =
                    Some(plain_string(value).ok_or("Cargo.toml: `version` is not a plain string")?);
            }
            "version.workspace" if value == "true" => inherits = true,
            k if k == "name" || k.starts_with("name.") || k.starts_with("version") => {
                return Err(format!("Cargo.toml: unsupported `{k}` in [package]"));
            }
            _ => {}
        }
    }
    let name = name
        .filter(|n| !n.is_empty())
        .ok_or("Cargo.toml: no plain `name` in [package]")?;
    let version = match (version, inherits) {
        (Some(_), true) => {
            return Err("Cargo.toml: `version` is both set and inherited".to_string());
        }
        (Some(v), false) => v.to_string(),
        (None, true) => workspace_version(root_manifest)?,
        (None, false) => return Err("Cargo.toml: no `version` in [package]".to_string()),
    };
    Ok(Member {
        name: name.to_string(),
        version,
    })
}

/// The version under the root manifest's `[workspace.package]`, which members may inherit.
fn workspace_version(root_manifest: &str) -> Result<String, String> {
    let mut section = "";
    for raw in root_manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if section == "[workspace.package]"
            && let Some(("version", value)) = key_value(line)
        {
            return plain_string(value)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    "Cargo.toml: [workspace.package] `version` is not a plain string".to_string()
                });
        }
    }
    Err("Cargo.toml: a member inherits `version`, but [workspace.package] has none".to_string())
}

/// A `key = value` line, with both sides trimmed.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim(), value.trim()))
}

/// The inside of a double-quoted TOML string, when the value is exactly one.
fn plain_string(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}
