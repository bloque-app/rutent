//! The documentation lint's coverage (`T-024-24`, `NFR-7`): every workspace of its own in the repository, outside
//! `experiments/`, has a step of a workflow that runs `cargo doc` over it with rustdoc's warnings denied.
//!
//! The root workspace's step (`cargo doc --workspace`) never sees a crate with its own `[workspace]` table (the module
//! guests, the wallet guest, the VOS program and its client), so each needs a step of its own, which names its directory.

use crate::Finding;
use std::path::Path;

/// The directories under `root` that hold a workspace of their own: a `Cargo.toml` with a `[workspace]` table of its
/// own, not `root`'s, not under `experiments/`, and not one the root manifest excludes (a fixture), as paths under
/// `root` with `/` between parts, sorted.
pub fn sub_workspaces(root: &Path) -> std::io::Result<Vec<String>> {
    let excluded = crate::evidence::excluded_dirs(root)?;
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !path.is_dir() || name == "target" || name == "experiments" || name.starts_with('.')
            {
                continue;
            }
            let under = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            if excluded.contains(&under) {
                continue;
            }
            let own = std::fs::read_to_string(path.join("Cargo.toml"))
                .is_ok_and(|t| t.lines().any(|l| l.trim() == "[workspace]"));
            if own {
                out.push(
                    under
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
            stack.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// The steps of a workflow: the lines from each `- name:`, `- uses:` or `- run:` item to the next.
fn steps(workflow: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in workflow.lines() {
        let item = line.trim_start();
        if ["- name:", "- uses:", "- run:"]
            .iter()
            .any(|p| item.starts_with(p))
        {
            out.push(String::new());
        }
        if let Some(step) = out.last_mut() {
            step.push_str(line);
            step.push('\n');
        }
    }
    out
}

/// True when `text` names `dir` as a whole path (not as the head or the tail of a longer name).
fn names(text: &str, dir: &str) -> bool {
    let part = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.');
    text.match_indices(dir).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(part)
            && !text[at + dir.len()..].chars().next().is_some_and(part)
    })
}

/// Reports each of `dirs` that no step of `workflows` documents: a step that runs `cargo doc`, names the directory,
/// and denies rustdoc's warnings (`-D warnings`) in the step itself or its `RUSTDOCFLAGS`. The workflows are
/// `(file, text)` pairs.
pub fn uncovered(dirs: &[String], workflows: &[(String, String)]) -> Vec<Finding> {
    let all: Vec<String> = workflows.iter().flat_map(|(_, text)| steps(text)).collect();
    dirs.iter()
        .filter(|dir| {
            !all.iter().any(|step| {
                step.contains("cargo doc") && step.contains("-D warnings") && names(step, dir)
            })
        })
        .map(|dir| Finding {
            rule: "doc-lint-gap",
            location: dir.clone(),
            message: format!(
                "{dir} is a workspace of its own and no workflow step runs `cargo doc` over it with `-D warnings`"
            ),
        })
        .collect()
}

/// The check over a repository: its sub-workspaces against `.github/workflows/*.yml`.
pub fn check(root: &Path) -> std::io::Result<Vec<Finding>> {
    let mut workflows = Vec::new();
    for entry in std::fs::read_dir(root.join(".github/workflows"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "yml" || e == "yaml") {
            workflows.push((path.display().to_string(), std::fs::read_to_string(&path)?));
        }
    }
    Ok(uncovered(&sub_workspaces(root)?, &workflows))
}
