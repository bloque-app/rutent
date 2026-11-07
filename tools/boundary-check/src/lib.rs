//! Boundary check over the crate graph (DEC-2).
//!
//! Reads the workspace members' manifests with `std` alone and reports every dependency between
//! our own crates that the module boundary does not allow.

use std::collections::BTreeMap;
use std::path::Path;

/// A dependency edge from one of our crates to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    /// Package name of the crate depended on.
    pub name: String,
    /// True for a dev-dependency.
    pub dev: bool,
}

/// Our crates and what each depends on.
pub type Graph = BTreeMap<String, Vec<Dep>>;

/// A dependency the boundary does not allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The crate that holds the dependency.
    pub krate: String,
    /// The crate it depends on.
    pub dep: String,
}

/// Every crate of ours is named with this prefix.
const OURS: &str = "rutent-";

/// The crates of ours that `krate` may depend on (DEC-2).
///
/// The outer libraries, the evidence checker and the conformance suites import nothing of ours but
/// the contracts. Core's three crates may also reach the four libraries, but not each other: their
/// internal interfaces are settled later by their own contract task. Any other crate imports
/// nothing of ours.
fn allowed(krate: &str) -> &'static [&'static str] {
    match krate {
        "rutent-conformance" | "rutent-router" | "rutent-stm" | "rutent-prov"
        | "rutent-evidence" => &["rutent-contracts"],
        "rutent-core-orch" | "rutent-core-authz" | "rutent-core-audit" => &[
            "rutent-contracts",
            "rutent-router",
            "rutent-stm",
            "rutent-prov",
            "rutent-evidence",
        ],
        _ => &[],
    }
}

/// Reports the dependencies of the crates named in `krates` that the boundary does not allow.
///
/// Every edge counts, whatever its kind; only the conformance suites may be a dev-dependency of
/// any crate, and nothing may depend on them otherwise.
pub fn violations(graph: &Graph, krates: &[&str]) -> Vec<Violation> {
    let mut found = Vec::new();
    for &krate in krates {
        for dep in graph.get(krate).map(Vec::as_slice).unwrap_or_default() {
            let permitted = allowed(krate).contains(&dep.name.as_str())
                || (dep.dev && dep.name == "rutent-conformance");
            if !permitted {
                found.push(Violation {
                    krate: krate.to_string(),
                    dep: dep.name.clone(),
                });
            }
        }
    }
    found
}

const DEP_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Parses one manifest into its package name and the dependencies of ours it declares.
///
/// Fails closed: it reads the plain `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]`
/// tables, one bare `key = value` per line, and reports as an error any other form that could name
/// a dependency (target tables, sub-tables, quoted keys, `package =` renames, multi-line values),
/// never skipping it.
pub fn parse_manifest(text: &str) -> Result<(String, Vec<Dep>), String> {
    let mut name = None;
    let mut deps: Vec<Dep> = Vec::new();
    let mut section = "";
    for (n, raw) in text.lines().enumerate() {
        let at = |why: &str| format!("line {}: {why}: `{}`", n + 1, raw.trim());
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let header = header.trim();
            if header.starts_with("target")
                || header.contains("dependencies") && !DEP_TABLES.contains(&header)
            {
                return Err(at("unrecognised table header"));
            }
            section = header;
        } else if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if section.is_empty()
                && (key.starts_with("target") || DEP_TABLES.iter().any(|t| key.starts_with(t)))
            {
                return Err(at("unrecognised dependency key"));
            }
            if section == "package" && key == "name" {
                name = Some(value.trim().trim_matches('"').to_string());
            } else if DEP_TABLES.contains(&section) {
                if key.contains(['"', '\'']) {
                    return Err(at("quoted dependency key"));
                }
                let (dep, rest) = key.split_once('.').unwrap_or((key, ""));
                let dep = dep.trim();
                let count = |c| value.matches(c).count();
                if count('{') != count('}') || count('[') != count(']') {
                    return Err(at("value continues past the line"));
                }
                if rest.trim() == "package" || has_package_key(value) {
                    return Err(at("dependency rename with `package`"));
                }
                let dev = section == "dev-dependencies";
                if dep.starts_with(OURS) && !deps.iter().any(|d| d.name == dep && d.dev == dev) {
                    deps.push(Dep {
                        name: dep.to_string(),
                        dev,
                    });
                }
            }
        }
    }
    name.map(|n| (n, deps))
        .ok_or_else(|| "no package name".to_string())
}

/// True when an inline table has a `package` key.
fn has_package_key(value: &str) -> bool {
    value.split(['{', ',']).any(|part| {
        part.split_once('=')
            .is_some_and(|(k, _)| k.trim().trim_matches(['"', '\'']) == "package")
    })
}

/// Loads the graph of the workspace rooted at `root`, from the `members` of its root manifest.
pub fn load(root: &Path) -> std::io::Result<Graph> {
    let bad = |m: String| std::io::Error::new(std::io::ErrorKind::InvalidData, m);
    let root_manifest = std::fs::read_to_string(root.join("Cargo.toml"))?;
    let mut graph = Graph::new();
    for member in members(&root_manifest).map_err(|e| bad(format!("Cargo.toml: {e}")))? {
        let text = std::fs::read_to_string(root.join(&member).join("Cargo.toml"))?;
        let (name, deps) = parse_manifest(&text).map_err(|e| bad(format!("{member}: {e}")))?;
        graph.insert(name, deps);
    }
    Ok(graph)
}

/// The paths in the `members = [ ... ]` array of the `[workspace]` table of a root manifest.
///
/// Matches the `members` key itself, so `default-members` and comments do not count.
pub fn members(root_manifest: &str) -> Result<Vec<String>, String> {
    let mut in_workspace = false;
    let mut array: Option<String> = None;
    for raw in root_manifest.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some(buf) = array.as_mut() {
            buf.push_str(line);
        } else if line.starts_with('[') {
            in_workspace = line == "[workspace]";
        } else if in_workspace
            && let Some(("members", rest)) = line.split_once('=').map(|(k, v)| (k.trim(), v))
        {
            array = Some(rest.trim().to_string());
        }
        if array.as_ref().is_some_and(|b| b.contains(']')) {
            break;
        }
    }
    let array = array.ok_or("no `members` in `[workspace]`")?;
    let inner = array
        .strip_prefix('[')
        .and_then(|a| a.split_once(']'))
        .ok_or("`members` is not an array")?
        .0;
    Ok(inner
        .split(',')
        .map(|m| m.trim().trim_matches('"').to_string())
        .filter(|m| !m.is_empty())
        .collect())
}
