//! Boundary check over the crate graph and the sources (DEC-2).
//!
//! Reads the workspace members' manifests with `std` alone and reports every dependency between
//! our own crates that the module boundary does not allow. It also reads the outer libraries'
//! sources for the one thing a crate-level check can't see: Core's internal contract, which lives
//! in the contracts crate every library reaches, named outside Core.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// The scenario assembly (`T-020-01`): the one crate that may import every crate of ours, Core's
/// three included, and that nothing imports.
const SCENARIOS: &str = "rutent-scenarios";

/// The crates of ours that `krate` may depend on (DEC-2).
///
/// The outer libraries, the evidence checker and the conformance suites import nothing of ours but
/// the contracts. Core's three crates may also reach the four libraries, but not each other: they
/// talk through the core-internal contract, in the contracts crate (`T-020-01`). The scenario
/// assembly may import every crate of ours. Any other crate imports nothing of ours, so no crate
/// but the assembly imports Core, and none imports the assembly.
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
/// any crate, and nothing but the scenario assembly may depend on them otherwise.
pub fn violations(graph: &Graph, krates: &[&str]) -> Vec<Violation> {
    let mut found = Vec::new();
    for &krate in krates {
        for dep in graph.get(krate).map(Vec::as_slice).unwrap_or_default() {
            let permitted = (krate == SCENARIOS && dep.name != SCENARIOS)
                || allowed(krate).contains(&dep.name.as_str())
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

/// Loads the directory of each member of the workspace rooted at `root`, by package name.
pub fn member_dirs(root: &Path) -> std::io::Result<BTreeMap<String, PathBuf>> {
    let bad = |m: String| std::io::Error::new(std::io::ErrorKind::InvalidData, m);
    let root_manifest = std::fs::read_to_string(root.join("Cargo.toml"))?;
    let mut dirs = BTreeMap::new();
    for member in members(&root_manifest).map_err(|e| bad(format!("Cargo.toml: {e}")))? {
        let dir = root.join(&member);
        let text = std::fs::read_to_string(dir.join("Cargo.toml"))?;
        let (name, _) = parse_manifest(&text).map_err(|e| bad(format!("{member}: {e}")))?;
        dirs.insert(name, dir);
    }
    Ok(dirs)
}

/// The name a crate-level check can't keep out of the libraries: the core-internal contract's
/// module in the contracts crate (`T-020-01`).
pub const CORE_INTERNAL: &str = "core_internal";

/// The outer libraries and the evidence checker, whose sources must never name [`CORE_INTERNAL`].
pub const OUTER: [&str; 4] = [
    "rutent-router",
    "rutent-stm",
    "rutent-prov",
    "rutent-evidence",
];

/// The conformance suites, which may name [`CORE_INTERNAL`] but never expose it in a public item.
pub const CONFORMANCE: &str = "rutent-conformance";

/// The one line of the contracts crate's root, outside the contract's own module, that may name it.
const DECLARATION: &str = "pub mod core_internal;";

/// Why a source line fails the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// It names [`CORE_INTERNAL`] where nothing may.
    Names,
    /// A public item of the conformance suites names [`CORE_INTERNAL`], so the suites would pass it
    /// on: a `pub use` or `pub type`, or a public function's, type's or trait's interface.
    Exposes,
    /// A `#[path]` or `include!` that mounts a file from outside its package, or one the check
    /// can't resolve.
    Mounts,
    /// A `#[macro_export]` in the contract's own module, which would export a macro at the
    /// contracts crate's root, out of the reach of the name.
    ExportsAMacro,
}

/// A source line that breaks the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFinding {
    /// The file, relative to the workspace root.
    pub file: PathBuf,
    /// The 1-based line.
    pub line: usize,
    /// Why.
    pub why: Why,
}

fn io_error(kind: std::io::ErrorKind, message: String) -> std::io::Error {
    std::io::Error::new(kind, message)
}

/// Every `.rs` file of the package at `dir`, sorted. It skips the package's own `target`
/// directory and the directories of the workspace's other members (`skip`), and refuses any
/// symbolic link, which could carry the check out of the tree or hide part of it.
fn rust_files(dir: &Path, skip: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    let own_target = dir.join("target");
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(io_error(
                    std::io::ErrorKind::InvalidData,
                    format!("{}: a symbolic link in a checked tree", path.display()),
                ));
            }
            if kind.is_dir() {
                if path != own_target && !skip.contains(&path) {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// `path` with `.` and `..` folded away, lexically; `None` if it climbs above its root.
fn normalise(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// `text` with its comments blanked out, and its string and character literals too when `strings`
/// is set, each blanked character a space, so byte offsets and lines stay where they were.
fn blank(text: &str, strings: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    let wipe = |out: &mut Vec<u8>, from: usize, to: usize| {
        for b in &mut out[from..to.min(bytes.len())] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    };
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let end = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
                wipe(&mut out, i, end);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let (mut depth, mut j) = (1, i + 2);
                while j < bytes.len() && depth > 0 {
                    if bytes[j] == b'/' && bytes.get(j + 1) == Some(&b'*') {
                        depth += 1;
                        j += 2;
                    } else if bytes[j] == b'*' && bytes.get(j + 1) == Some(&b'/') {
                        depth -= 1;
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                wipe(&mut out, i, j);
                i = j;
            }
            b'r' if matches!(bytes.get(i + 1), Some(b'"') | Some(b'#'))
                && !i
                    .checked_sub(1)
                    .is_some_and(|p| bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_') =>
            {
                let hashes = bytes[i + 1..].iter().take_while(|b| **b == b'#').count();
                if bytes.get(i + 1 + hashes) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let close = format!("\"{}", "#".repeat(hashes));
                let start = i + 2 + hashes;
                let end = text[start..]
                    .find(&close)
                    .map_or(bytes.len(), |n| start + n + close.len());
                if strings {
                    wipe(&mut out, start, end - close.len().min(end - start));
                }
                i = end;
            }
            b'"' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != b'"' {
                    j += if bytes[j] == b'\\' { 2 } else { 1 };
                }
                if strings {
                    wipe(&mut out, i + 1, j);
                }
                i = j + 1;
            }
            b'\'' => {
                // A character literal, not a lifetime: `'x'`, `'\n'`, `'\u{..}'`.
                let close = if bytes.get(i + 1) == Some(&b'\\') {
                    text[i + 2..].find('\'').map(|n| i + 2 + n)
                } else if bytes.get(i + 2) == Some(&b'\'') {
                    Some(i + 2)
                } else {
                    None
                };
                match close {
                    Some(end) => {
                        if strings {
                            wipe(&mut out, i + 1, end);
                        }
                        i = end + 1;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// The 1-based line of byte offset `at` in `text`.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].bytes().filter(|b| *b == b'\n').count() + 1
}

/// The string literal `text` starts with, after any space, or `None` where it starts with
/// something else.
fn leading_literal(text: &str) -> Option<&str> {
    text.trim_start()
        .strip_prefix('"')
        .and_then(|r| r.split_once('"'))
        .map(|(literal, _)| literal)
}

/// Every file `text` mounts, with the offset where it does: each attribute (`#[...]` or
/// `#![...]`, spaces allowed, over any number of lines, `cfg_attr` included) holding a `path`
/// key, and each `include!(...)`. The path is the literal given, or `None` where it isn't a plain
/// literal. Comments are skipped.
fn mounted(text: &str) -> Vec<(usize, Option<&str>)> {
    let code = blank(text, false);
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_whitespace() || bytes[j] == b'!') {
                j += 1;
            }
            if bytes.get(j) == Some(&b'[') {
                // The attribute, to its matching bracket, outside string literals.
                let (mut depth, mut k, mut quoted) = (0, j, false);
                while k < bytes.len() {
                    match bytes[k] {
                        b'\\' if quoted => k += 1,
                        b'"' => quoted = !quoted,
                        b'[' if !quoted => depth += 1,
                        b']' if !quoted => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                let attribute = &text[j..k.min(text.len())];
                let keys = attribute.match_indices("path").filter(|(at, _)| {
                    let before = attribute[..*at].trim_end().chars().last();
                    let after = attribute[at + 4..].trim_start();
                    matches!(before, Some('[') | Some('(') | Some(','))
                        && after.starts_with('=')
                        && !after.starts_with("==")
                });
                for (at, _) in keys {
                    let rest = attribute[at + 4..].trim_start()[1..].trim_start();
                    found.push((i, leading_literal(rest)));
                }
                i = k;
                continue;
            }
        }
        if bytes[i..].starts_with(b"include")
            && !i
                .checked_sub(1)
                .is_some_and(|p| bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_')
        {
            let rest = code[i + "include".len()..].trim_start();
            if let Some(inner) = rest
                .strip_prefix('!')
                .and_then(|args| args.trim_start().strip_prefix('('))
            {
                let offset = code.len() - inner.len();
                found.push((i, leading_literal(&text[offset..])));
            }
        }
        i += 1;
    }
    found
}

/// Whether `literal`, a path mounted by `file`, leaves `package`: read from the file's directory,
/// it climbs out of the package or is absolute; `None`, a path that isn't a plain literal, leaves
/// it too.
fn leaves(literal: Option<&str>, file: &Path, package: &Path) -> bool {
    let Some(literal) = literal else {
        return true;
    };
    let target = Path::new(literal);
    let base = file.parent().unwrap_or(package);
    match (normalise(&base.join(target)), normalise(package)) {
        (Some(resolved), Some(package)) => target.is_absolute() || !resolved.starts_with(package),
        _ => true,
    }
}

/// Whether `header`, an item's text before its body or its end, declares it public to other
/// crates: `pub` alone, not `pub(crate)`, `pub(super)` or `pub(in ...)`, after any attributes.
fn is_public(header: &str) -> bool {
    let mut rest = header.trim_start();
    while let Some(attr) = rest.strip_prefix('#') {
        let Some(end) = attr.find(']') else {
            return false;
        };
        rest = attr[end + 1..].trim_start();
    }
    rest.strip_prefix("pub")
        .is_some_and(|r| r.starts_with(|c: char| c.is_whitespace()))
}

/// Whether `header` begins with the keyword `word`, after any attributes and visibility.
fn declares(header: &str, word: &str) -> bool {
    header.split_whitespace().any(|w| w == word)
}

/// The 1-based lines where a public item of `text` names [`CORE_INTERNAL`]: its signature, a
/// public item's body that is its interface (an enum's variants, a trait's items, a struct's
/// fields, a trait impl, an exported macro), or a `pub use`, `pub type`, `pub const` or
/// `pub static`. Function bodies and private items aren't public, and comments and literals are
/// skipped.
fn public_items_naming(text: &str) -> Vec<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum Block {
        /// Items, each public or not by its own header.
        Items,
        /// A public item's interface: everything in it is public.
        Interface,
        /// A function's body or a private item's: nothing in it is.
        Private,
    }
    let code = blank(text, true);
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    let mut stack = vec![Block::Items];
    let (mut start, mut depth) = (0, 0i32);
    let flag = |from: usize, to: usize, found: &mut Vec<usize>| {
        if let Some(at) = code[from..to].find(CORE_INTERNAL) {
            found.push(line_of(text, from + at));
        }
    };
    for (i, b) in bytes.iter().enumerate() {
        let here = *stack.last().unwrap_or(&Block::Items);
        match b {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'{' if depth <= 0 => {
                let header = &code[start..i];
                let block = match here {
                    Block::Private => Block::Private,
                    _ if declares(header, "fn") => {
                        if here == Block::Interface || is_public(header) {
                            flag(start, i, &mut found);
                        }
                        Block::Private
                    }
                    _ if declares(header, "impl") && declares(header, "for") => {
                        flag(start, i, &mut found);
                        Block::Interface
                    }
                    _ if declares(header, "impl") || declares(header, "mod") => Block::Items,
                    _ if declares(header, "macro_rules!") => {
                        if header.contains("macro_export") {
                            Block::Interface
                        } else {
                            Block::Private
                        }
                    }
                    Block::Interface => {
                        flag(start, i, &mut found);
                        Block::Interface
                    }
                    Block::Items if is_public(header) => {
                        flag(start, i, &mut found);
                        Block::Interface
                    }
                    Block::Items => Block::Private,
                };
                stack.push(block);
                start = i + 1;
            }
            b'}' if depth <= 0 => {
                if here == Block::Interface {
                    flag(start, i, &mut found);
                }
                stack.pop();
                start = i + 1;
            }
            b';' | b',' if depth <= 0 => {
                let statement = &code[start..i];
                let public = match here {
                    Block::Interface => true,
                    Block::Items => *b == b';' && is_public(statement),
                    Block::Private => false,
                };
                if public {
                    flag(start, i, &mut found);
                }
                if *b == b';' || here != Block::Items {
                    start = i + 1;
                }
            }
            _ => {}
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Reports every line that breaks the source boundary of the core-internal contract (DEC-2):
///
/// - any line that names [`CORE_INTERNAL`] in the sources of the outer libraries and the
///   evidence checker ([`OUTER`], each package's whole tree, guests and nested workspaces
///   included), or in the contracts crate's sources outside the contract's own module, where
///   only the crate root's declaration may name it, so no other contract re-exports it;
/// - any public item that names it in the conformance suites ([`CONFORMANCE`]), which may use it
///   but never pass it on: a `pub use`, `pub type`, `pub const` or `pub static`, a public
///   function's signature, a public type's or trait's interface, a trait impl, an exported macro;
/// - any attribute with a `path` key (`#[path]`, `# [path]`, `#[cfg_attr(.., path = ..)]`, over
///   any lines) or `include!` in those trees that mounts a file from outside its package, or that
///   the check can't resolve;
/// - any `#[macro_export]` in the contract's own module.
///
/// Fails closed: it matches the plain text, comments included, so a mention in a library's
/// documentation fails too. A package [`OUTER`] names that isn't a member, and a symbolic link in
/// a checked tree, are errors. Each package's own `target` directory, and the directories of the
/// workspace's other members (such as the scenario assembly under `conformance/`), are not its.
pub fn sources_naming_core_internal(root: &Path) -> std::io::Result<Vec<SourceFinding>> {
    let missing = |name: &str| {
        io_error(
            std::io::ErrorKind::NotFound,
            format!("{name} is not a workspace member"),
        )
    };
    let dirs = member_dirs(root)?;
    let members: Vec<PathBuf> = dirs.values().cloned().collect();
    let others = |dir: &Path| -> Vec<PathBuf> {
        members
            .iter()
            .filter(|m| m.as_path() != dir)
            .cloned()
            .collect()
    };
    let mut found = Vec::new();
    let relative = |file: &Path| file.strip_prefix(root).unwrap_or(file).to_path_buf();
    let mut scan = |package: &Path,
                    names_allowed: &dyn Fn(&Path, &str) -> bool,
                    conformance: bool|
     -> std::io::Result<()> {
        for file in rust_files(package, &others(package))? {
            let text = std::fs::read_to_string(&file)?;
            let mut add = |line, why| {
                found.push(SourceFinding {
                    file: relative(&file),
                    line,
                    why,
                })
            };
            for (at, literal) in mounted(&text) {
                if leaves(literal, &file, package) {
                    add(line_of(&text, at), Why::Mounts);
                }
            }
            if conformance {
                for line in public_items_naming(&text) {
                    add(line, Why::Exposes);
                }
            } else {
                for (n, line) in text.lines().enumerate() {
                    if line.contains(CORE_INTERNAL) && !names_allowed(&file, line) {
                        add(n + 1, Why::Names);
                    }
                }
            }
        }
        Ok(())
    };
    for name in OUTER {
        let dir = dirs.get(name).ok_or_else(|| missing(name))?;
        scan(dir, &|_, _| false, false)?;
    }
    let conformance = dirs.get(CONFORMANCE).ok_or_else(|| missing(CONFORMANCE))?;
    scan(conformance, &|_, _| true, true)?;

    let contracts = dirs
        .get("rutent-contracts")
        .ok_or_else(|| missing("rutent-contracts"))?
        .join("src");
    let own = contracts.join(CORE_INTERNAL);
    let crate_root = contracts.join("lib.rs");
    let allowed = |file: &Path, line: &str| {
        file.starts_with(&own) || (file == crate_root && line.trim() == DECLARATION)
    };
    scan(&contracts, &allowed, false)?;
    for file in rust_files(&own, &[])? {
        let text = std::fs::read_to_string(&file)?;
        for (n, line) in text.lines().enumerate() {
            if line.contains("macro_export") {
                found.push(SourceFinding {
                    file: relative(&file),
                    line: n + 1,
                    why: Why::ExportsAMacro,
                });
            }
        }
    }
    found.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(found)
}
