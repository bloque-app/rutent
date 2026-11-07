//! The contract guard's command line: `contract-guard [base-ref]`, default `origin/main`. It diffs the merge base of
//! the base ref and HEAD, reads changed paths NUL-separated, reads base file text with `git show <merge-base>:<path>`
//! (a missing file is new), and reads head text from the working tree. Exits 1 on any violation, 2 on a usage or git
//! error.

use rutent_contract_guard::{Files, check, split_nul_names};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Base text is read at the merge base, the same commit the diff starts from.
struct Git {
    root: PathBuf,
    merge_base: String,
}

impl Files for Git {
    fn base(&self, path: &str) -> Option<String> {
        git(
            &self.root,
            &["show", &format!("{}:{path}", self.merge_base)],
        )
        .ok()
    }

    fn head(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(path)).ok()
    }

    fn base_files(&self) -> Result<Vec<String>, String> {
        git(
            &self.root,
            &["ls-tree", "-r", "-z", "--name-only", &self.merge_base],
        )
        .map(|text| split_nul_names(&text))
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    String::from_utf8(output.stdout).map_err(|error| format!("git output is not UTF-8: {error}"))
}

fn run() -> Result<bool, (u8, String)> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| "origin/main".to_string());
    if args.next().is_some() {
        return Err((2, "usage: contract-guard [base-ref]".to_string()));
    }
    let root = git(Path::new("."), &["rev-parse", "--show-toplevel"]).map_err(|e| (2, e))?;
    let root = PathBuf::from(root.trim());
    git(
        &root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    )
    .map_err(|_| {
        (
            2,
            format!("base ref {base} does not exist here; fetch it first"),
        )
    })?;
    let merge_base = git(&root, &["merge-base", &base, "HEAD"])
        .map_err(|_| (2, format!("{base} and HEAD share no history")))?
        .trim()
        .to_string();

    let diff = git(
        &root,
        &[
            "diff",
            "-z",
            "--name-only",
            "--no-renames",
            &merge_base,
            "HEAD",
        ],
    )
    .map_err(|e| (2, e))?;
    let changed = split_nul_names(&diff);
    let contracts_text = std::fs::read_to_string(root.join("contracts/CONTRACTS"))
        .map_err(|e| (2, format!("cannot read contracts/CONTRACTS: {e}")))?;

    let files = Git { root, merge_base };
    let violations = check(&changed, &contracts_text, &files).map_err(|e| (2, e))?;
    if violations.is_empty() {
        println!(
            "contract guard: {} changed path(s) against {base}, no violations",
            changed.len()
        );
        return Ok(true);
    }
    for violation in &violations {
        eprintln!(
            "contract guard: {}: {}",
            violation.subject, violation.message
        );
    }
    Ok(false)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err((code, message)) => {
            eprintln!("contract guard: {message}");
            ExitCode::from(code)
        }
    }
}
