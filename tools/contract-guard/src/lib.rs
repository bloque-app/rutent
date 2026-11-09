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
//! - Every new bullet in any changelog names a `DEC-<n>`.
//! - A change to a shared file (under `contracts/`, owned by no contract) needs a new bullet naming a `DEC-<n>` in
//!   `contracts/CHANGELOG.md`, or a version bump of some contract in the same change.

use std::collections::BTreeSet;

const CONTRACTS_PATH: &str = "contracts/CONTRACTS";
const SHARED: &str = "shared";
const SHARED_CHANGELOG: &str = "contracts/CHANGELOG.md";

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

/// Whether a bullet names a `DEC-<n>`: `DEC-` stands alone (no letter, digit or underscore just before it), at least
/// one digit follows, and the digits end the token (no letter, digit or underscore just after them).
fn names_decision(bullet: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    bullet.match_indices("DEC-").any(|(index, _)| {
        let standalone = !bullet[..index].chars().next_back().is_some_and(is_word);
        let digits = bullet[index + 4..]
            .chars()
            .take_while(char::is_ascii_digit)
            .count();
        let ends_token = !bullet[index + 4 + digits..]
            .chars()
            .next()
            .is_some_and(is_word);
        standalone && digits > 0 && ends_token
    })
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
            if !names_decision(&bullet) {
                violations.push(entry_violation(name, &bullet));
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

    if !shared_paths.is_empty() {
        let decided = new_bullets(
            files.base(SHARED_CHANGELOG).as_deref(),
            files.head(SHARED_CHANGELOG).as_deref(),
        )
        .iter()
        .any(|bullet| names_decision(bullet));
        if !decided && bumped.is_empty() {
            violations.push(violation(
                SHARED,
                format!(
                    "{} changed without a new {SHARED_CHANGELOG} entry naming a DEC-<n>, or a version bump of a contract",
                    shared_paths.join(", ")
                ),
            ));
        }
    }
    Ok(violations)
}

fn violation(subject: &str, message: String) -> Violation {
    Violation {
        subject: subject.to_string(),
        message,
    }
}

fn entry_violation(subject: &str, bullet: &str) -> Violation {
    let first = bullet.lines().next().unwrap_or(bullet);
    violation(
        subject,
        format!("changelog entry names no DEC-<n> (no plan decision): {first}"),
    )
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
