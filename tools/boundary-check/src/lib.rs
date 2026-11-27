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

/// The reader host (`DEC-28`, `T-015-04`): `CMP-EVIDENCE`'s standalone part, which runs each public
/// network's reader as a reader service outside Core. It depends on the contracts, for the reader
/// contract, and on the provider system through its admitted paths alone ([`PROV_PATHS`]). Nothing of
/// ours depends on it, save the scenario assembly's tests: a dev-dependency of the assembly's, so
/// the simulated networks' readers run in the test harness (`T-024-25`), and never a dependency of
/// its build (`T-020-59`, its review's L6).
pub const READER_HOST: &str = "rutent-reader-host";

/// The paths of the provider system the reader host may name, under `rutent_prov::`, and nothing
/// else of it (`T-015-04`'s part 7, `DEC-28`): its egress layer (`components/prov/src/egress/`),
/// which enforces a reader's declared destinations; the provider's jail, re-exported at
/// `rutent_prov::confine` ([`PROV_CONFINE`]); and `T-014-01`'s key resolver, re-exported at
/// `rutent_prov::keys` ([`PROV_KEYS`]). The rest of `module` and `proxy` stays out of its reach.
pub const PROV_PATHS: [&str; 3] = ["egress", "confine", "keys"];

/// What `rutent_prov::confine` (`components/prov/src/confine.rs`) re-exports, and nothing more: the
/// launcher and the in-process jail a provider's process runs in, and what they take, each from the
/// module that defines it (a path under `crate::`, resolved by file layout from `src/`).
pub const PROV_CONFINE: &[(&str, &[&str])] = &[(
    "module::sandbox",
    &[
        "Limits",
        "PROVIDER_ADDRESS_SPACE",
        "Role",
        "confine",
        "isolated",
    ],
)];

/// What `rutent_prov::keys` (`components/prov/src/keys.rs`) re-exports, and nothing more: the
/// resolver interface, its vault implementation and what builds it from a process role's settings,
/// each from the module that defines it.
pub const PROV_KEYS: &[(&str, &[&str])] = &[
    ("proxy::https", &["HttpsTransport"]),
    (
        "proxy::kv",
        &["InvalidConfig", "KvConfig", "KvResolver", "Server", "Token"],
    ),
    ("proxy::resolver", &["ResolveError", "Resolver"]),
    (
        "proxy::settings",
        &[
            "Bucket",
            "InvalidSettings",
            "RoleSettings",
            "StoreAddress",
            "StoreScheme",
            "kv_bucket",
        ],
    ),
    ("proxy::transport", &["TransportError"]),
];

/// The crates of ours that `krate` may depend on (DEC-2).
///
/// The outer libraries, the evidence checker and the conformance suites import nothing of ours but
/// the contracts. Core's three crates may also reach the four libraries, but not each other: they
/// talk through the core-internal contract, in the contracts crate (`T-020-01`). The reader host
/// reaches the contracts and the provider system, for its egress layer only (`T-020-59`). The
/// scenario assembly may import every crate of ours, the reader host only as a dev-dependency. Any
/// other crate imports nothing of ours, so no crate but the assembly imports Core, none imports the
/// assembly, and none but the assembly's tests imports the reader host.
fn allowed(krate: &str) -> &'static [&'static str] {
    match krate {
        "rutent-conformance" | "rutent-router" | "rutent-stm" | "rutent-prov"
        | "rutent-evidence" => &["rutent-contracts"],
        READER_HOST => &["rutent-contracts", "rutent-prov"],
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
/// any crate, and nothing but the scenario assembly may depend on them otherwise. A crate's
/// dev-dependency on itself, by its own package name, is no edge between two crates: cargo builds
/// the same crate for its own tests, as the contracts crate does to turn on its test-only
/// `rule-switches` feature (`T-020-45`). Only a dev-dependency: a crate depending on itself
/// otherwise is reported, as any edge the boundary doesn't allow.
pub fn violations(graph: &Graph, krates: &[&str]) -> Vec<Violation> {
    let mut found = Vec::new();
    for &krate in krates {
        for dep in graph.get(krate).map(Vec::as_slice).unwrap_or_default() {
            let permitted = (krate == SCENARIOS
                && dep.name != SCENARIOS
                && (dep.name != READER_HOST || dep.dev))
                || allowed(krate).contains(&dep.name.as_str())
                || (dep.dev && dep.name == "rutent-conformance")
                || (dep.dev && dep.name == krate);
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
    /// The reader host names the provider system other than through its admitted paths
    /// ([`PROV_PATHS`]).
    ReachesBeyondItsPaths,
    /// An admitted path of the provider system re-exports more, or other, than it is admitted
    /// with ([`PROV_CONFINE`], [`PROV_KEYS`]), holds anything but those re-exports, re-exports a
    /// name its source file doesn't define, or is reached other than by plain declarations and
    /// file layout.
    WidensAnAdmittedPath,
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

/// The 1-based lines of `text`, its comments and literals blanked, that name the provider system's
/// crate, `rutent_prov`, other than as a path into one of its admitted paths ([`PROV_PATHS`]):
/// `rutent_prov::egress`, `rutent_prov::confine` or `rutent_prov::keys`, followed by `::` or by a
/// character that can't continue a name. Fails closed: a bare `rutent_prov`, a grouped
/// `use rutent_prov::{egress, ..}`, an `extern crate`, a rename or a longer name
/// (`rutent_prov::keystore`) is a finding.
fn lines_beyond_admitted(text: &str) -> Vec<usize> {
    const CRATE: &str = "rutent_prov";
    let code = blank(text, true);
    let continues = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = code[from..].find(CRATE).map(|n| from + n) {
        from = at + CRATE.len();
        let before = code[..at].chars().next_back();
        let after = &code[from..];
        if before.is_some_and(continues) || after.chars().next().is_some_and(continues) {
            continue;
        }
        let rest = after.trim_start();
        let admitted = rest
            .strip_prefix("::")
            .map(str::trim_start)
            .is_some_and(|r| {
                PROV_PATHS.iter().any(|path| {
                    r.strip_prefix(path)
                        .is_some_and(|r| !r.chars().next().is_some_and(continues))
                })
            });
        if !admitted {
            found.push(line_of(text, at));
        }
    }
    found.dedup();
    found
}

/// Whether the module file `text` declares the module `name` once, as `pub mod <name>;` or `pub(crate) mod <name>;`
/// alone: no attribute before it
/// (a `#[path]` or a `#[cfg_attr(.., path = ..)]` would mount another file under the admitted name), no inline body,
/// no second declaration. `Err` gives the 1-based line to report: the offending declaration's, or 1 where there is
/// none.
pub fn declared_plainly(text: &str, name: &str) -> Result<(), usize> {
    let code = blank(text, true);
    let continues = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = code[from..].find("mod").map(|n| from + n) {
        from = at + 3;
        let before = code[..at].chars().next_back();
        let rest = &code[from..];
        if before.is_some_and(continues) || !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        if rest
            .strip_prefix(name)
            .is_some_and(|r| !r.chars().next().is_some_and(continues))
        {
            found.push(at);
        }
    }
    let &[at] = found.as_slice() else {
        return Err(found.get(1).map_or(1, |at| line_of(text, *at)));
    };
    let plain = code[at..]
        .strip_prefix("mod")
        .map(str::trim_start)
        .and_then(|r| r.strip_prefix(name))
        .is_some_and(|r| r.trim_start().starts_with(';'));
    let before = code[..at].trim_end();
    let visible = before
        .strip_suffix("pub(crate)")
        .or_else(|| before.strip_suffix("pub"));
    let public = visible.is_some_and(|b| {
        !b.chars().next_back().is_some_and(continues)
            && b.trim_end()
                .chars()
                .next_back()
                .is_none_or(|c| c == ';' || c == '}')
    });
    if plain && public {
        Ok(())
    } else {
        Err(line_of(text, at))
    }
}

/// Whether `text`, an admitted path's file, holds its re-exports and nothing else: comments, then one
/// `pub use crate::<module>::<item>;` or `pub use crate::<module>::{<items>};` for each module
/// `admitted` names, with exactly its items, in any order. Fails closed: another item, a glob, a
/// rename, another statement, or anything but comments around them is a widening.
pub fn holds_its_reexports(text: &str, admitted: &[(&str, &[&str])]) -> bool {
    let code = blank(text, true);
    let code: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some(statements) = code.strip_suffix(';') else {
        return false;
    };
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    for statement in statements.split(';') {
        let Some(path) = statement.trim().strip_prefix("pub use crate::") else {
            return false;
        };
        let (module, items) = match path.split_once("::{") {
            Some((module, list)) => {
                let Some(list) = list.strip_suffix('}') else {
                    return false;
                };
                (
                    module.trim(),
                    list.split(',').map(str::trim).collect::<Vec<_>>(),
                )
            }
            None => match path.rsplit_once("::") {
                Some((module, item)) => (module.trim(), vec![item.trim()]),
                None => return false,
            },
        };
        let mut items: Vec<String> = items
            .into_iter()
            .filter(|i| !i.is_empty())
            .map(str::to_owned)
            .collect();
        if module.contains([' ', '{', '}', '*']) || items.iter().any(|i| !is_name(i)) {
            return false;
        }
        items.sort_unstable();
        found.push((module.replace(' ', ""), items));
    }
    found.sort_unstable();
    let mut wanted: Vec<(String, Vec<String>)> = admitted
        .iter()
        .map(|(module, items)| {
            let mut items: Vec<String> = items.iter().map(|i| (*i).to_owned()).collect();
            items.sort_unstable();
            ((*module).to_owned(), items)
        })
        .collect();
    wanted.sort_unstable();
    found == wanted
}

/// Whether `text` is one plain name: letters, digits and `_`, not starting with a digit.
fn is_name(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && text.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// The blanked code of `text`, with the brace depth at each byte, so a rule can look at the
/// file's own items alone.
fn depths(code: &str) -> Vec<usize> {
    let mut depth = 0usize;
    code.bytes()
        .map(|b| {
            let here = depth;
            match b {
                b'{' => depth += 1,
                b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
            here
        })
        .collect()
}

/// Whether the module file `text` defines `name` itself, among its own items: exactly one
/// `pub fn`, `pub const fn`, `pub struct`, `pub enum`, `pub union`, `pub const`, `pub static`,
/// `pub trait` or `pub type` of that name at the file's top level; no `pub mod <name>` or
/// `pub macro <name>` beside it; and no top-level `pub use` or `pub extern crate` that names it, or
/// that globs (`*`), so no module of the same name can ride on a value's path (a `pub use
/// crate::proxy as <name>;` beside a `pub const <name>`).
pub fn defined_in(text: &str, name: &str) -> bool {
    let code = blank(text, true);
    let depth = depths(&code);
    let continues = |c: char| c.is_alphanumeric() || c == '_';
    let word_at = |at: usize, word: &str| {
        code[at..].starts_with(word)
            && !code[..at].chars().next_back().is_some_and(continues)
            && !code[at + word.len()..]
                .chars()
                .next()
                .is_some_and(continues)
    };
    let mut definitions = 0;
    let mut from = 0;
    while let Some(at) = code[from..].find("pub").map(|n| from + n) {
        from = at + 3;
        if depth[at] != 0 || !word_at(at, "pub") {
            continue;
        }
        let statement_end = code[at..].find([';', '{']).map_or(code.len(), |n| at + n);
        let head: Vec<&str> = code[at + 3..statement_end].split_whitespace().collect();
        match head.as_slice() {
            ["use", ..] | ["extern", "crate", ..] => {
                let statement = &code[at..code[at..].find(';').map_or(code.len(), |n| at + n)];
                let names_it = statement
                    .match_indices(name)
                    .any(|(i, _)| word_at(at + i, name));
                if names_it || statement.contains('*') {
                    return false;
                }
            }
            ["mod" | "macro", item, ..] if item.trim_end_matches(['(', '!']) == name => {
                return false;
            }
            ["const" | "unsafe", "fn", item, ..]
            | [
                "fn" | "struct" | "enum" | "union" | "const" | "static" | "trait" | "type",
                item,
                ..,
            ] => {
                let item = item.split(['<', '(', ':', '=']).next().unwrap_or("");
                if item == name {
                    definitions += 1;
                }
            }
            _ => {}
        }
    }
    definitions == 1
}

/// The file a module path under `crate::` resolves to by file layout from the crate's `src`, each
/// segment declared plainly in its parent ([`declared_plainly`]): a module with children is
/// `<name>/mod.rs`, with no `<name>.rs` beside it, and a leaf is `<name>.rs`, with no `<name>/`
/// beside it. `Err` gives the file and line to report.
fn module_file(src: &Path, module: &str) -> Result<PathBuf, (PathBuf, usize)> {
    let segments: Vec<&str> = module.split("::").collect();
    let mut parent = src.join("lib.rs");
    let mut dir = src.to_path_buf();
    for (i, segment) in segments.iter().enumerate() {
        let text = std::fs::read_to_string(&parent).map_err(|_| (parent.clone(), 1))?;
        declared_plainly(&text, segment).map_err(|line| (parent.clone(), line))?;
        let leaf = i + 1 == segments.len();
        let (file, other) = if leaf {
            (dir.join(format!("{segment}.rs")), dir.join(segment))
        } else {
            (
                dir.join(segment).join("mod.rs"),
                dir.join(format!("{segment}.rs")),
            )
        };
        if !file.is_file() {
            return Err((file, 1));
        }
        if other.exists() {
            return Err((other, 1));
        }
        dir = dir.join(segment);
        parent = file;
    }
    Ok(parent)
}

/// The lines of a manifest that set the library target or a build script, in any spelling TOML allows (#348's L9):
/// a lib table however written (`[lib]`, `[ lib ]`, `["lib"]`, `[lib.x]`), a key that names it at the top level
/// (`lib = { .. }`, `lib.path`, `"lib".path`), and a build script by `build` in the package table, however written
/// (`[package] build = ..`, `package.build = ..`, `build = false` too). A target array (`[[bin]]`, `[[test]]`,
/// `[[example]]`, `[[bench]]`, however spaced or quoted) is read as one, so its `path` is never taken for the
/// library's: each of those builds a crate of its own, which never is `rutent_prov`, so it can't widen what the reader
/// host reaches. Matched on the text with its spaces and quotes taken out, so every spelling meets the same rule; a
/// comment is cut at its `#`, so a `#` inside a value makes the line read shorter, never longer.
fn moves_the_library(manifest: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut table = String::new();
    for (n, raw) in manifest.lines().enumerate() {
        let flat: String = raw
            .split('#')
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
            .collect();
        let key = if let Some(header) = flat.strip_prefix('[') {
            table = header.trim_matches(['[', ']']).to_owned();
            table.clone()
        } else if let Some((key, _)) = flat.split_once('=') {
            if table.is_empty() {
                key.to_owned()
            } else {
                format!("{table}.{key}")
            }
        } else {
            continue;
        };
        let names = |name: &str| key == name || key.starts_with(&format!("{name}."));
        if names("lib") || names("package.build") {
            found.push(n + 1);
        }
    }
    found
}

/// The findings, under the provider system's package `prov`, against what the reader host's
/// admitted paths admit ([`PROV_CONFINE`], [`PROV_KEYS`]): each admitted file and every module on
/// the way to what it re-exports is declared plainly and found by file layout alone, so no
/// `#[path]` (or `include!`) anywhere under `src/`, no lib table or key in the manifest in any
/// spelling and no build script ([`moves_the_library`]) moves one;
/// each admitted file holds its re-exports and nothing more; and each name they re-export is
/// defined in the file of the module it is re-exported from. A lint over the source, beside the
/// code review that every one of these files gets as a `DEC-20` path.
fn admitted_path_findings(prov: &Path) -> std::io::Result<Vec<(PathBuf, usize)>> {
    let mut found = Vec::new();
    let src = prov.join("src");
    let manifest = std::fs::read_to_string(prov.join("Cargo.toml"))?;
    for line in moves_the_library(&manifest) {
        found.push((prov.join("Cargo.toml"), line));
    }
    // A build script runs at build time and can write the sources the library compiles, so prov has none: a
    // `build.rs` at its root, which cargo runs unasked, is refused outright (#348's L9).
    let script = prov.join("build.rs");
    if script.exists() {
        found.push((script, 1));
    }
    for file in rust_files(&src, &[])? {
        let text = std::fs::read_to_string(&file)?;
        for (at, _) in mounted(&text) {
            found.push((file.clone(), line_of(&text, at)));
        }
    }
    for (name, admitted) in [("confine", PROV_CONFINE), ("keys", PROV_KEYS)] {
        let file = match module_file(&src, name) {
            Ok(file) => file,
            Err(at) => {
                found.push(at);
                continue;
            }
        };
        if !std::fs::read_to_string(&file).is_ok_and(|text| holds_its_reexports(&text, admitted)) {
            found.push((file, 1));
        }
        for (module, items) in admitted {
            match module_file(&src, module) {
                Ok(source) => {
                    let text = std::fs::read_to_string(&source).unwrap_or_default();
                    if !items.iter().all(|item| defined_in(&text, item)) {
                        found.push((source, 1));
                    }
                }
                Err(at) => found.push(at),
            }
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
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
/// - any `#[macro_export]` in the contract's own module;
/// - in the reader host ([`READER_HOST`]), once it is a member, any line that names the contract,
///   and any that names the provider system other than through its admitted paths
///   ([`PROV_PATHS`], `T-020-59`, `T-015-04`);
/// - in the provider system, once the reader host is a member, anything that widens an admitted
///   path: a `#[path]` or `include!` anywhere under `src/`, the library target or a build script
///   set in its manifest in any spelling, or a `build.rs` at its root ([`moves_the_library`]), a
///   module on the way declared other than plainly or found other than by file layout, an admitted
///   file that holds more or other than its re-exports, or a name they re-export that its source
///   file doesn't itself define ([`PROV_CONFINE`], [`PROV_KEYS`], [`holds_its_reexports`],
///   [`declared_plainly`], [`defined_in`]).
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
    let mut beyond_paths = Vec::new();
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
    // The reader host is outer code too, once it is a member (`T-015-04` makes it one): it never
    // names the contract, and names the provider system only through its egress layer.
    if let Some(dir) = dirs.get(READER_HOST) {
        scan(dir, &|_, _| false, false)?;
        for file in rust_files(dir, &others(dir))? {
            let text = std::fs::read_to_string(&file)?;
            for line in lines_beyond_admitted(&text) {
                beyond_paths.push(SourceFinding {
                    file: relative(&file),
                    line,
                    why: Why::ReachesBeyondItsPaths,
                });
            }
        }
    }
    // What the admitted paths admit, once the reader host is a member (`admitted_path_findings`).
    if dirs.contains_key(READER_HOST) {
        let prov = dirs
            .get("rutent-prov")
            .ok_or_else(|| missing("rutent-prov"))?;
        for (path, line) in admitted_path_findings(prov)? {
            beyond_paths.push(SourceFinding {
                file: relative(&path),
                line,
                why: Why::WidensAnAdmittedPath,
            });
        }
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
    found.extend(beyond_paths);
    found.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(found)
}
