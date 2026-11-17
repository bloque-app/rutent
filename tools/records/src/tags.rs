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
    /// The inline `mod` blocks around the test, outermost first (`["tests"]` for a `mod tests { ... }`). Read
    /// from the braces line by line, so a brace in a string or comment can only make a test read as not run.
    pub module: Vec<String>,
    /// The words after `verifies:`, or `None` when no such comment sits above the test.
    pub tag: Option<Vec<String>>,
}

/// The text of a line after its leading attributes (`#[..]`, balanced on the line); the whole line when it opens
/// with none, and an empty text when an attribute runs on past the line's end.
fn after_attributes(line: &str) -> &str {
    let mut rest = line.trim_start();
    while rest.starts_with("#[") {
        let mut depth = 0usize;
        let mut end = None;
        for (at, c) in rest.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(at + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(end) => rest = rest[end..].trim_start(),
            None => return "",
        }
    }
    rest
}

/// A line of attributes alone, or a doc comment: what may stand between a tag and its test. A line that holds
/// code after its attributes (`#[test] fn a() {}`) is a test or an item of its own, and ends the search.
fn is_attribute(line: &str) -> bool {
    line.starts_with("///") || (line.starts_with("#[") && after_attributes(line).is_empty())
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

/// A line with its string and char literals and its `//` comment cut out, so only real braces are left.
fn code_of(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => break,
            '"' => {
                while let Some(d) = chars.next() {
                    match d {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '\'' => {
                // A char literal ('{', '\\n') closes within a few characters; a lifetime ('a) does not.
                let rest: String = chars.clone().take(4).collect();
                if let Some(end) = rest.find('\'') {
                    for _ in 0..=rest[..end].chars().count() {
                        chars.next();
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// The name of the module that the code before a `{` opens inline (`mod tests`, `pub(crate) mod a`), if it does.
fn mod_opened_by(code: &str) -> Option<String> {
    let mut words = after_attributes(code).split_whitespace().peekable();
    let mut first = words.next()?;
    if first.starts_with("pub") {
        first = words.next()?;
    }
    if first != "mod" {
        return None;
    }
    let name = words.next()?;
    (words.next().is_none() && !name.is_empty() && !name.contains([';', '(', ')']))
        .then(|| name.to_string())
}

/// The name of the function a test attribute at `at` in `code` belongs to: the first `fn` after it, on the rest of
/// that line or on the lines below.
fn function_after(code: &str, at: usize, below: &[&str]) -> String {
    let name_in = |text: &str| {
        let mut from = 0;
        while let Some(found) = text[from..].find("fn ") {
            let start = from + found;
            from = start + 3;
            if text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let name: String = text[start + 3..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
        None
    };
    name_in(&code[at..])
        .or_else(|| below.iter().find_map(|l| name_in(l.trim())))
        .unwrap_or_default()
}

/// Finds the tests in one source file: every test attribute in code (not in a string or a `//` comment), wherever
/// it stands on its line. The modules around a test are read from the braces as the text is walked, so a test on
/// the line that opens its module (`mod t { #[test] fn x() {} }`) is in that module.
pub fn scan_tests(file: &str, text: &str) -> Vec<TestCase> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    // One entry per open brace: the module it opens, or `None` for any other block.
    let mut open: Vec<Option<String>> = Vec::new();
    // The code since the last `{`, `}` or `;`, across lines, which says what the next `{` opens.
    let mut segment = String::new();
    for (i, line) in lines.iter().enumerate() {
        let code = code_of(line);
        for (at, c) in code.char_indices() {
            match c {
                '{' => {
                    open.push(mod_opened_by(&segment));
                    segment.clear();
                }
                '}' => {
                    open.pop();
                    segment.clear();
                }
                ';' => segment.clear(),
                '#' if is_test_attribute(&code[at..]) => {
                    out.push(TestCase {
                        file: file.to_string(),
                        line: i + 1,
                        name: function_after(&code, at, &lines[i + 1..]),
                        module: open.iter().flatten().cloned().collect(),
                        tag: tag_above(&lines, i),
                    });
                    segment.push(c);
                }
                _ => segment.push(c),
            }
        }
        segment.push('\n');
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
