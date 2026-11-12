//! The contract guard (`T-001-05`, `DEC-4`): a change under `contracts/` needs a version bump and a changelog entry
//! naming a `DEC-*`. The core is a pure function over the changed paths, the head `CONTRACTS` text and a lookup of
//! file text at the base and at the head. It reads no git state itself.
//!
//! Rules, as `contracts/README.md` states them:
//! - A path belongs to the contract whose artefact or module directory is its longest matching prefix, and a file
//!   directly under `contracts/tests/` named after a contract (`provider_role.rs`, `host.rs`) belongs to it.
//! - A path touches every contract that either the base or the head `CONTRACTS` assigns it to.
//! - A contract's version is the first `## <semver>` heading of its `CHANGELOG.md`, at the base and at the head.
//! - A change to a touched contract needs a greater head version, or the contract is new (absent from the base
//!   `CONTRACTS`). The new top section needs a bullet that the base's top section does not have.
//! - Every new bullet in any changelog, the shared one included, names a `DEC-<n>`, and every `DEC-<n>` it names is
//!   defined in `PLAN.md`'s decisions table.
//! - A change to a shared file (under `contracts/`, owned by no contract) needs a new bullet naming a `DEC-<n>` in
//!   `contracts/CHANGELOG.md`. A contract's version bump does not stand in for it.
//! - A change to the root `Cargo.lock` that changes the name, version, source or checksum of a package the contracts
//!   crate reaches (the crate itself, its dependencies and theirs, dev-dependencies included), or adds or drops one,
//!   touches every contract, since they ship as one crate.
//! - A change that only adds or drops dependency edges of such packages (a feature another crate turns on) needs the
//!   shared changelog's entry naming a `DEC-<n>`, like a shared file, and no contract's bump.
//! - A contract whose major version moves changes every shape that references it: a contract whose head schemas
//!   name its URNs (`urn:rutent:<name>:`), or whose Rust module uses its module (`crate::<module>`, re-exports
//!   included), needs a major bump too, though none of its own paths changed. A contract new at the base needs none.
//!   Only direct references count: a chain of them moves one major at a time. A changelog whose first heading isn't
//!   a version is refused, touched or not, so it never hides a major.

use std::collections::{BTreeMap, BTreeSet};

const CONTRACTS_PATH: &str = "contracts/CONTRACTS";
const SHARED: &str = "shared";
const SHARED_CHANGELOG: &str = "contracts/CHANGELOG.md";
const PLAN_PATH: &str = "PLAN.md";
const LOCK_PATH: &str = "Cargo.lock";
/// The package whose dependencies the lockfile rule follows.
const CONTRACTS_CRATE: &str = "rutent-contracts";

/// One contract, as `contracts/CONTRACTS` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    /// The name the `CONTRACTS` file gives it, such as `provider-role`.
    pub name: String,
    /// The directory of its language-neutral artefacts and its `CHANGELOG.md`.
    pub artefact_dir: String,
    /// The directory of its Rust module.
    pub module_dir: String,
}

/// A file's text at the base and at the head. `None` means the file does not exist there.
pub trait Files {
    /// The file's text at the base ref, or `None` if it does not exist there.
    fn base(&self, path: &str) -> Option<String>;
    /// The file's text at the head, or `None` if it does not exist there.
    fn head(&self, path: &str) -> Option<String>;
    /// Every file path at the base ref. An error means the base cannot be listed, and the guard then fails.
    fn base_files(&self) -> Result<Vec<String>, String>;
}

/// One rule broken. `subject` is a contract's name, or `shared` for the files no contract owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The contract's name, or `shared`.
    pub subject: String,
    /// What rule was broken, in a sentence a person can act on.
    pub message: String,
}

/// Splits the output of `git diff -z --name-only`: names are NUL-terminated and never quoted.
pub fn split_nul_names(text: &str) -> Vec<String> {
    text.split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// Reads `contracts/CONTRACTS`: one contract per line, `name artefact-dir module-dir`, `#` for comments.
pub fn parse_contracts(text: &str) -> Result<Vec<Contract>, String> {
    let mut out: Vec<Contract> = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 3 {
            return Err(format!(
                "CONTRACTS line {}: expected a name, an artefact directory and a module directory",
                number + 1
            ));
        }
        if out.iter().any(|c| c.name == fields[0]) {
            return Err(format!(
                "CONTRACTS line {}: contract {} is named twice",
                number + 1,
                fields[0]
            ));
        }
        out.push(Contract {
            name: fields[0].to_string(),
            artefact_dir: fields[1].to_string(),
            module_dir: fields[2].to_string(),
        });
    }
    if out.is_empty() {
        return Err("CONTRACTS names no contract".to_string());
    }
    // Owners are found by longest prefix, so two contracts on one directory would hide one behind the other.
    // Nesting is fine; equal directories are not.
    let mut seen: Vec<&str> = Vec::new();
    for contract in &out {
        for dir in [contract.artefact_dir.as_str(), contract.module_dir.as_str()] {
            if seen.contains(&dir) {
                return Err(format!(
                    "CONTRACTS: directory {dir} is named by more than one contract"
                ));
            }
            seen.push(dir);
        }
    }
    Ok(out)
}

fn within(path: &str, dir: &str) -> bool {
    path.len() > dir.len() && path.starts_with(dir) && path.as_bytes()[dir.len()] == b'/'
}

/// The index of the contract that owns `path` under one mapping: the one whose directory is its longest matching
/// prefix, or, for a file directly under `contracts/tests/`, the one whose name is the file's stem (`-` and `_` alike).
fn owner(path: &str, contracts: &[Contract]) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (index, contract) in contracts.iter().enumerate() {
        for dir in [&contract.artefact_dir, &contract.module_dir] {
            if within(path, dir) && best.is_none_or(|(_, length)| dir.len() > length) {
                best = Some((index, dir.len()));
            }
        }
    }
    best.map(|(index, _)| index)
        .or_else(|| test_file_owner(path, contracts))
}

/// The contract a file directly under `contracts/tests/` belongs to, by its stem.
fn test_file_owner(path: &str, contracts: &[Contract]) -> Option<usize> {
    let stem = path.strip_prefix("contracts/tests/")?.strip_suffix(".rs")?;
    if stem.contains('/') {
        return None;
    }
    let stem = stem.replace('-', "_");
    contracts
        .iter()
        .position(|contract| contract.name.replace('-', "_") == stem)
}

/// The sections of a changelog: each `## <heading>` and the lines under it.
struct Section {
    version: String,
    body: Vec<String>,
}

fn sections(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            out.push(Section {
                version: heading.trim().to_string(),
                body: Vec::new(),
            });
        } else if let Some(section) = out.last_mut() {
            section.body.push(line.to_string());
        }
    }
    out
}

/// The bullets of some lines. A bullet runs from its `- ` line to the next bullet; blank lines are dropped.
fn bullets(body: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body {
        if line.starts_with("- ") || line.starts_with("* ") {
            out.push(line.clone());
        } else if line.trim().is_empty() {
            continue;
        } else if let Some(last) = out.last_mut() {
            last.push('\n');
            last.push_str(line);
        }
    }
    out
}

/// Every bullet of a changelog, wherever it sits: the shared changelog has bullets under no version heading.
fn all_bullets(text: &str) -> Vec<String> {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    bullets(&lines)
}

/// A plain `MAJOR.MINOR.PATCH` version, digits only.
fn semver(text: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut numbers = [0u64; 3];
    for (slot, part) in numbers.iter_mut().zip(parts) {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    Some((numbers[0], numbers[1], numbers[2]))
}

/// The `DEC-<n>` tokens a bullet names, as numbers: `DEC-` stands alone (no letter, digit or underscore just before
/// it), at least one digit follows, and the digits end the token (no letter, digit or underscore just after them).
fn decisions_named(bullet: &str) -> Vec<String> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out: Vec<String> = Vec::new();
    for (index, _) in bullet.match_indices("DEC-") {
        let standalone = !bullet[..index].chars().next_back().is_some_and(is_word);
        let digits: String = bullet[index + 4..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let ends_token = !bullet[index + 4 + digits.len()..]
            .chars()
            .next()
            .is_some_and(is_word);
        if standalone && !digits.is_empty() && ends_token {
            out.push(digits);
        }
    }
    out
}

/// The decisions `PLAN.md` defines: the rows of its decisions table, which start with ``| `DEC-<n>` ``.
fn defined_decisions(plan: &str) -> BTreeSet<String> {
    plan.lines()
        .filter_map(|line| line.strip_prefix("| `DEC-"))
        .filter_map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            (!digits.is_empty() && rest[digits.len()..].starts_with('`')).then_some(digits)
        })
        .collect()
}

/// A bullet is decided when it names at least one `DEC-<n>` and every one it names is in `defined`. `Err` is the
/// sentence that says why not.
fn check_decisions(bullet: &str, defined: &BTreeSet<String>) -> Result<(), String> {
    let first = bullet.lines().next().unwrap_or(bullet);
    let named = decisions_named(bullet);
    if named.is_empty() {
        return Err(format!(
            "changelog entry names no DEC-<n> (no plan decision): {first}"
        ));
    }
    if let Some(unknown) = named.iter().find(|n| !defined.contains(*n)) {
        return Err(format!(
            "changelog entry names DEC-{unknown}, which {PLAN_PATH} does not define: {first}"
        ));
    }
    Ok(())
}

/// A package of `Cargo.lock`, reduced to what a move of it changes.
struct LockPackage {
    name: String,
    version: String,
    /// Every line but the dependency list (name, version, source, checksum): equal exactly when the package itself
    /// did not move.
    core: String,
    /// The dependency list's lines: equal exactly when no edge was added or dropped.
    edges: String,
    /// Dependency entries: a name, or a name and a version.
    dependencies: Vec<(String, Option<String>)>,
}

/// The bodies of the `[[package]]` blocks. Any other header (`[[patch.unused]]`, `[metadata]`) ends the block before
/// it and is dropped.
fn package_blocks(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut in_package = false;
    for line in text.lines() {
        if line.trim_start().starts_with('[') {
            in_package = line.trim() == "[[package]]";
            if in_package {
                blocks.push(String::new());
            }
        } else if in_package && let Some(block) = blocks.last_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    blocks
}

fn lock_packages(text: &str) -> Vec<LockPackage> {
    let mut out: Vec<LockPackage> = Vec::new();
    for block in package_blocks(text) {
        let mut name = String::new();
        let mut version = String::new();
        let mut dependencies: Vec<(String, Option<String>)> = Vec::new();
        let mut in_dependencies = false;
        let mut core = String::new();
        let mut edges = String::new();
        for line in block.lines() {
            let line = line.trim();
            if in_dependencies || line.starts_with("dependencies = [") {
                edges.push_str(line);
                edges.push('\n');
            } else if !line.is_empty() {
                core.push_str(line);
                core.push('\n');
            }
            if in_dependencies {
                if line == "]" {
                    in_dependencies = false;
                } else if let Some(entry) = line.trim_end_matches(',').strip_prefix('"') {
                    let entry = entry.trim_end_matches('"');
                    let mut parts = entry.split_whitespace();
                    let dep = parts.next().unwrap_or_default().to_string();
                    dependencies.push((dep, parts.next().map(str::to_string)));
                }
            } else if line.starts_with("dependencies = [") {
                in_dependencies = !line.ends_with(']');
            } else if let Some(value) = line.strip_prefix("name = ") {
                name = value.trim_matches('"').to_string();
            } else if let Some(value) = line.strip_prefix("version = ") {
                version = value.trim_matches('"').to_string();
            }
        }
        if !name.is_empty() {
            out.push(LockPackage {
                name,
                version,
                core,
                edges,
                dependencies,
            });
        }
    }
    out
}

/// Every package the contracts crate reaches in a lockfile, itself included: what the package is (`core`) mapped to
/// its dependency list (`edges`).
fn contracts_closure(text: &str) -> BTreeMap<String, String> {
    let packages = lock_packages(text);
    let mut reached: BTreeSet<usize> = BTreeSet::new();
    let mut queue: Vec<usize> = packages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.name == CONTRACTS_CRATE)
        .map(|(i, _)| i)
        .collect();
    while let Some(index) = queue.pop() {
        if !reached.insert(index) {
            continue;
        }
        for (dep, version) in &packages[index].dependencies {
            for (i, candidate) in packages.iter().enumerate() {
                if &candidate.name == dep
                    && version.as_ref().is_none_or(|v| v == &candidate.version)
                {
                    queue.push(i);
                }
            }
        }
    }
    reached
        .into_iter()
        .map(|i| (packages[i].core.clone(), packages[i].edges.clone()))
        .collect()
}

/// The name and version a package's `core` text starts with.
fn core_label(core: &str) -> String {
    let field = |key: &str| {
        core.lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_default()
            .trim_matches('"')
            .to_string()
    };
    format!("{} {}", field("name = "), field("version = "))
}

/// What differs between two lockfiles among the packages the contracts crate reaches.
#[derive(Debug, Default)]
struct LockMoves {
    /// Packages that appear, vanish or change their name, version, source or checksum.
    moved: BTreeSet<String>,
    /// Packages that stay as they are and only gain or lose dependency edges.
    edges_only: BTreeSet<String>,
}

fn lock_moves(base: Option<&str>, head: Option<&str>) -> LockMoves {
    let before = base.map(contracts_closure).unwrap_or_default();
    let after = head.map(contracts_closure).unwrap_or_default();
    let mut moves = LockMoves::default();
    for (core, edges) in &before {
        match after.get(core) {
            None => {
                moves.moved.insert(core_label(core));
            }
            Some(head_edges) if head_edges != edges => {
                moves.edges_only.insert(core_label(core));
            }
            Some(_) => {}
        }
    }
    for core in after.keys().filter(|core| !before.contains_key(*core)) {
        moves.moved.insert(core_label(core));
    }
    moves
}

/// Bullets present in the head changelog but not the base one.
fn new_bullets(base: Option<&str>, head: Option<&str>) -> Vec<String> {
    let before = base.map(all_bullets).unwrap_or_default();
    head.map(all_bullets)
        .unwrap_or_default()
        .into_iter()
        .filter(|bullet| !before.contains(bullet))
        .collect()
}

/// Checks the changed paths against the contract rules. `contracts_text` is the head `CONTRACTS`; the base one is read
/// through `files`. `Err` means a `CONTRACTS` text itself is unreadable.
pub fn check(
    changed: &[String],
    contracts_text: &str,
    files: &dyn Files,
) -> Result<Vec<Violation>, String> {
    let head_contracts = parse_contracts(contracts_text)?;
    let plan = files
        .head(PLAN_PATH)
        .ok_or_else(|| format!("cannot read {PLAN_PATH}, which defines the decisions"))?;
    let defined = defined_decisions(&plan);
    let base_contracts = match files.base(CONTRACTS_PATH) {
        Some(text) => parse_contracts(&text).map_err(|error| format!("base {error}"))?,
        None => Vec::new(),
    };
    let mut violations: Vec<Violation> = Vec::new();

    // A changed path touches every contract that either mapping assigns it to; a path neither assigns is shared.
    let mut touched: BTreeSet<String> = BTreeSet::new();
    let mut shared_paths: Vec<&str> = Vec::new();
    for path in changed {
        if !path.starts_with("contracts/") {
            continue;
        }
        let in_head = owner(path, &head_contracts).map(|i| head_contracts[i].name.clone());
        let in_base = owner(path, &base_contracts).map(|i| base_contracts[i].name.clone());
        if in_head.is_none() && in_base.is_none() {
            shared_paths.push(path);
        }
        touched.extend(in_head.into_iter().chain(in_base));
    }

    // The contracts ship as one crate, so a moved dependency of it touches every contract.
    let LockMoves { moved, edges_only } = lock_moves(
        files.base(LOCK_PATH).as_deref(),
        files.head(LOCK_PATH).as_deref(),
    );
    if !moved.is_empty() {
        touched.extend(head_contracts.iter().map(|c| c.name.clone()));
    }

    let mut names: Vec<String> = head_contracts.iter().map(|c| c.name.clone()).collect();
    for contract in &base_contracts {
        if !names.contains(&contract.name) {
            names.push(contract.name.clone());
        }
    }

    // A contract listed at the base that owns no file there is new; one that owns files but has no changelog is not.
    let base_files = files.base_files()?;
    let owned_at_base: BTreeSet<String> = base_files
        .iter()
        .filter_map(|path| owner(path, &base_contracts))
        .map(|index| base_contracts[index].name.clone())
        .collect();

    let mut bumped: BTreeSet<String> = BTreeSet::new();
    for name in &names {
        let head_contract = head_contracts.iter().find(|c| &c.name == name);
        let base_contract = base_contracts.iter().find(|c| &c.name == name);
        let head_log = head_contract.map(|c| format!("{}/CHANGELOG.md", c.artefact_dir));
        let base_log = base_contract.map(|c| format!("{}/CHANGELOG.md", c.artefact_dir));
        let head_text = head_log.as_deref().and_then(|path| files.head(path));
        let base_text = base_log.as_deref().and_then(|path| files.base(path));
        for bullet in new_bullets(base_text.as_deref(), head_text.as_deref()) {
            if let Err(message) = check_decisions(&bullet, &defined) {
                violations.push(violation(name, message));
            }
        }
        if !touched.contains(name) {
            continue;
        }
        if head_contract.is_none() {
            violations.push(violation(
                name,
                "its files changed, but it is no longer in CONTRACTS at the head".to_string(),
            ));
            continue;
        }
        let log = head_log.clone().unwrap_or_default();
        let Some(head_text) = head_text else {
            violations.push(violation(name, format!("no {log} at the head")));
            continue;
        };
        let Some(head_top) = sections(&head_text).into_iter().next() else {
            violations.push(violation(
                name,
                format!("{log} has no version heading at the head"),
            ));
            continue;
        };
        let Some(head_version) = semver(&head_top.version) else {
            violations.push(violation(
                name,
                format!(
                    "the first heading of {log} is not a version: {}",
                    head_top.version
                ),
            ));
            continue;
        };
        let base_top = base_text
            .as_deref()
            .and_then(|text| sections(text).into_iter().next());
        let base_top_bullets = base_top
            .as_ref()
            .map(|s| bullets(&s.body))
            .unwrap_or_default();
        if !bullets(&head_top.body)
            .iter()
            .any(|bullet| !base_top_bullets.contains(bullet))
        {
            violations.push(violation(
                name,
                format!(
                    "the {} section of {log} adds no bullet that the base's top section lacks",
                    head_top.version
                ),
            ));
        }
        if base_contract.is_none() {
            // A contract absent from the base CONTRACTS is new: its first version needs no bump.
            continue;
        }
        if base_text.is_none() {
            if owned_at_base.contains(name) {
                violations.push(violation(
                    name,
                    format!("{name} owns files at the base but has no changelog at the base"),
                ));
            }
            // Listed at the base with no file there yet: its first version is new and needs no bump.
            continue;
        }
        match base_top.and_then(|s| semver(&s.version).map(|v| (s.version, v))) {
            None => violations.push(violation(
                name,
                format!("the base {log} has no version heading to bump from"),
            )),
            Some((base_label, base_version)) => {
                if head_version <= base_version {
                    violations.push(violation(
                        name,
                        format!(
                            "version not bumped: the base is {base_label} and the head is {}; a change needs a greater version",
                            head_top.version
                        ),
                    ));
                } else {
                    bumped.insert(name.clone());
                }
            }
        }
    }

    // A referenced contract's major bump changes the shapes and the Rust API that reference it, though no path of
    // theirs changed: by a schema naming its URNs, or by the referencing module using its Rust module
    // (`crate::<module>`, a re-export included).
    let majors =
        |contract: &Contract| -> Result<Option<((u64, u64, u64), (u64, u64, u64))>, String> {
            let log = format!("{}/CHANGELOG.md", contract.artefact_dir);
            let (Some(base), Some(head)) = (files.base(&log), files.head(&log)) else {
                return Ok(None);
            };
            let top = |text: &str| {
                sections(text)
                    .into_iter()
                    .next()
                    .map(|s| (semver(&s.version), s.version))
            };
            match (top(&base), top(&head)) {
                (Some((Some(b), _)), Some((Some(h), _))) => Ok(Some((b, h))),
                (_, Some((None, label))) | (Some((None, label)), _) => Err(format!(
                    "the first heading of {log} is not a version: {label}, so a major moved by it can't be read"
                )),
                _ => Err(format!(
                    "{log} has no version heading, so a major moved by it can't be read"
                )),
            }
        };
    let candidate_paths: BTreeSet<&String> = base_files.iter().chain(changed).collect();
    let module_paths: Vec<(usize, String)> = head_contracts
        .iter()
        .enumerate()
        .filter_map(|(index, c)| {
            c.module_dir
                .strip_prefix("contracts/src/")
                .map(|m| (index, m.replace('/', "::")))
        })
        .collect();
    for (moved_index, moved) in head_contracts.iter().enumerate() {
        let (base_version, head_version) = match majors(moved) {
            Ok(Some(versions)) => versions,
            Ok(None) => continue,
            Err(message) => {
                // A touched contract's heading is refused above, with the same reason; one untouched is refused
                // here, so a heading that isn't a version never lets a major go unseen.
                if !touched.contains(&moved.name) {
                    violations.push(violation(&moved.name, message));
                }
                continue;
            }
        };
        if head_version.0 <= base_version.0 {
            continue;
        }
        let urn = format!("urn:rutent:{}:", moved.name);
        for (index, referencing) in head_contracts.iter().enumerate() {
            if index == moved_index || !base_contracts.iter().any(|c| c.name == referencing.name) {
                continue;
            }
            let schemas = format!("{}/schemas", referencing.artefact_dir);
            let owned = |path: &&&String| owner(path, &head_contracts) == Some(index);
            let by_schema = candidate_paths.iter().filter(owned).any(|path| {
                within(path, &schemas)
                    && path.ends_with(".json")
                    && files.head(path).is_some_and(|text| text.contains(&urn))
            });
            let by_module = candidate_paths.iter().filter(owned).any(|path| {
                within(path, &referencing.module_dir)
                    && path.ends_with(".rs")
                    && files.head(path).is_some_and(|text| {
                        crate_paths(&text)
                            .iter()
                            .any(|used| module_owner(used, &module_paths) == Some(moved_index))
                    })
            });
            let how = match (by_schema, by_module) {
                (false, false) => continue,
                (true, false) => "its schemas reference",
                (false, true) => "its Rust module uses",
                (true, true) => "its schemas reference, and its Rust module uses,",
            };
            let (own_base, own_head) = match majors(referencing) {
                Ok(Some(versions)) => versions,
                Ok(None) | Err(_) => continue,
            };
            if own_head.0 <= own_base.0 {
                violations.push(violation(
                    &referencing.name,
                    format!(
                        "{how} {}, whose major moved from {} to {}, so its shapes changed with it and need a major bump too (the base is {}.{}.{} and the head is {}.{}.{})",
                        moved.name,
                        base_version.0,
                        head_version.0,
                        own_base.0,
                        own_base.1,
                        own_base.2,
                        own_head.0,
                        own_head.1,
                        own_head.2
                    ),
                ));
            }
        }
    }

    let shared_base = files.base(SHARED_CHANGELOG);
    let shared_head = files.head(SHARED_CHANGELOG);
    let shared_new = new_bullets(shared_base.as_deref(), shared_head.as_deref());
    let mut shared_decided = false;
    for bullet in &shared_new {
        match check_decisions(bullet, &defined) {
            Ok(()) => shared_decided = true,
            Err(message) => violations.push(violation(SHARED, message)),
        }
    }
    if !shared_paths.is_empty() && !shared_decided {
        violations.push(violation(
            SHARED,
            format!(
                "{} changed without a new {SHARED_CHANGELOG} entry naming a DEC-<n> that {PLAN_PATH} defines; a contract's version bump does not stand in for it",
                shared_paths.join(", ")
            ),
        ));
    }
    if !edges_only.is_empty() && !shared_decided {
        violations.push(violation(
            SHARED,
            format!(
                "{LOCK_PATH} changes only the dependency edges of {}, which the contracts crate reaches, without a new {SHARED_CHANGELOG} entry naming a DEC-<n> that {PLAN_PATH} defines",
                edges_only.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    if !moved.is_empty() {
        let unbumped: Vec<&str> = base_contracts
            .iter()
            .map(|c| c.name.as_str())
            .filter(|name| {
                head_contracts.iter().any(|c| c.name == *name) && !bumped.contains(*name)
            })
            .collect();
        if !unbumped.is_empty() {
            violations.push(violation(
                LOCK_PATH,
                format!(
                    "{LOCK_PATH} moves {}, which the contracts crate reaches, and {} {} not bumped",
                    moved.iter().cloned().collect::<Vec<_>>().join(", "),
                    unbumped.join(", "),
                    if unbumped.len() == 1 { "is" } else { "are" }
                ),
            ));
        }
    }
    Ok(violations)
}

/// Every path a Rust file names under `crate::`, braces expanded one level: `crate::a::B`, `crate::{a::B, c}`.
fn crate_paths(text: &str) -> Vec<String> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let path_at = |rest: &str| -> String {
        let mut out = String::new();
        let mut chars = rest;
        loop {
            let len = chars.find(|c: char| !ident(c)).unwrap_or(chars.len());
            if len == 0 {
                break;
            }
            if !out.is_empty() {
                out.push_str("::");
            }
            out.push_str(&chars[..len]);
            chars = &chars[len..];
            match chars.strip_prefix("::") {
                Some(next) if next.starts_with(ident) => chars = next,
                _ => break,
            }
        }
        out
    };
    let mut paths = Vec::new();
    for (at, _) in text.match_indices("crate::") {
        if at > 0 && text[..at].ends_with(ident) {
            continue;
        }
        let rest = &text[at + "crate::".len()..];
        if let Some(group) = rest.strip_prefix('{') {
            let end = group.find('}').unwrap_or(group.len());
            for item in group[..end].split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    paths.push(path_at(item));
                }
            }
        } else {
            paths.push(path_at(rest));
        }
    }
    paths
}

/// The contract whose Rust module is the longest prefix of `path`, as `module_paths` lists them.
fn module_owner(path: &str, module_paths: &[(usize, String)]) -> Option<usize> {
    module_paths
        .iter()
        .filter(|(_, m)| path == m || path.starts_with(&format!("{m}::")))
        .max_by_key(|(_, m)| m.len())
        .map(|(index, _)| *index)
}

fn violation(subject: &str, message: String) -> Violation {
    Violation {
        subject: subject.to_string(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: DEC-4
    #[test]
    fn longest_matching_prefix_owns_the_path() {
        let contracts = parse_contracts("state-machine contracts/state-machine contracts/src/state_machine\nhost contracts/state-machine/host contracts/src/state_machine/host\n").unwrap();
        assert_eq!(
            owner("contracts/state-machine/host/x.json", &contracts),
            Some(1)
        );
        assert_eq!(
            owner("contracts/src/state_machine/host/mod.rs", &contracts),
            Some(1)
        );
        assert_eq!(owner("contracts/state-machine/x.json", &contracts), Some(0));
        assert_eq!(owner("contracts/state-machinery/x.json", &contracts), None);
    }
}
