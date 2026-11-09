//! The tag check (`DEC-21`): every Rust test carries a `// verifies:` comment naming identifiers that the
//! documents define and have not withdrawn.

use crate::{Finding, ids::Defined, ids::is_id};
use std::path::{Path, PathBuf};

/// One Rust test and the identifiers its `// verifies:` comment names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    /// The source file.
    pub file: String,
    /// The 1-based line of the `#[test]` attribute.
    pub line: usize,
    /// The function name.
    pub name: String,
    /// The words after `verifies:`, or `None` when no such comment sits above the test.
    pub tag: Option<Vec<String>>,
}

fn is_attribute(line: &str) -> bool {
    line.starts_with("#[") || line.starts_with("///")
}

/// The words of a `// verifies:` comment block ending just above line index `at`, attributes and doc comments
/// in between allowed. A tag may continue on the `//` lines under it.
fn tag_above(lines: &[&str], at: usize) -> Option<Vec<String>> {
    let mut i = at;
    while i > 0 && is_attribute(lines[i - 1].trim()) {
        i -= 1;
    }
    let mut block_start = i;
    while block_start > 0
        && lines[block_start - 1].trim().starts_with("//")
        && !is_attribute(lines[block_start - 1].trim())
    {
        block_start -= 1;
    }
    let block = &lines[block_start..i];
    let first = block.iter().position(|l| verifies_body(l).is_some())?;
    let mut words = Vec::new();
    for (n, line) in block[first..].iter().enumerate() {
        let body = if n == 0 {
            verifies_body(line)?
        } else {
            line.trim().trim_start_matches('/').trim()
        };
        words.extend(
            body.split(|c: char| c == ',' || c.is_whitespace())
                .map(|w| w.trim_matches(|c: char| c == '`' || c == '.' || c == ';'))
                .filter(|w| !w.is_empty())
                .map(str::to_string),
        );
    }
    Some(words)
}

fn verifies_body(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("//")?.trim_start();
    rest.strip_prefix("verifies:")
}

/// True for an attribute that makes a test: `#[test]`, a path ending in `::test` (`#[tokio::test(...)]`),
/// `#[rstest]` or `#[test_case(...)]`, with or without arguments.
fn is_test_attribute(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("#[") else {
        return false;
    };
    let path: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
        .collect();
    ["test", "rstest", "test_case"]
        .iter()
        .any(|name| path == *name || path.ends_with(&format!("::{name}")))
}

/// Finds the tests in one source file: lines that start a test attribute.
pub fn scan_tests(file: &str, text: &str) -> Vec<TestCase> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !is_test_attribute(line.trim()) {
            continue;
        }
        let name = lines[i..]
            .iter()
            .find_map(|l| {
                let l = l.trim();
                let at = l.find("fn ")?;
                let name: String = l[at + 3..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                (!name.is_empty()).then_some(name)
            })
            .unwrap_or_default();
        out.push(TestCase {
            file: file.to_string(),
            line: i + 1,
            name,
            tag: tag_above(&lines, i),
        });
    }
    out
}

/// Reports tests without a tag, and tags naming an unknown or withdrawn identifier.
pub fn check_tests(tests: &[TestCase], defined: &Defined) -> Vec<Finding> {
    let mut out = Vec::new();
    for test in tests {
        let location = format!("{}:{}", test.file, test.line);
        let words = match &test.tag {
            Some(words) if !words.is_empty() => words,
            _ => {
                out.push(Finding {
                    rule: "untagged-test",
                    location,
                    message: format!(
                        "test `{}` has no `// verifies:` comment naming an id directly above it",
                        test.name
                    ),
                });
                continue;
            }
        };
        for word in words {
            if defined.withdrawn.contains(word) {
                out.push(Finding {
                    rule: "withdrawn-id",
                    location: location.clone(),
                    message: format!("test `{}` cites {word}, which is withdrawn", test.name),
                });
            } else if !is_id(word) || !defined.is_defined(word) {
                out.push(Finding {
                    rule: "unknown-id",
                    location: location.clone(),
                    message: format!(
                        "test `{}` cites {word}, which no document defines",
                        test.name
                    ),
                });
            }
        }
    }
    out
}

/// Every `.rs` file under `root`, sorted, skipping `target` and hidden directories.
pub fn find_rust_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                if name != "target" && !name.starts_with('.') {
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
