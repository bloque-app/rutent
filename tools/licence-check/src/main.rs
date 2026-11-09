//! Runs the licence record check on every `Cargo.lock` in the repository, `licences/RECORD` and the workspace members.

use rutent_licence_check::{
    LockFile, Member, Violation, check_locks, find_all_locks, member_paths, package_identity,
    workspace_member_paths,
};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    match run(&root) {
        Err(e) => {
            eprintln!("licence-check: {e}");
            ExitCode::from(2)
        }
        Ok(found) if found.is_empty() => {
            println!("licence-check: every locked dependency is recorded and decided");
            ExitCode::SUCCESS
        }
        Ok(found) => {
            for v in &found {
                eprintln!("licence-check: {v}");
            }
            eprintln!("licence-check: {} dependencies fail the check", found.len());
            ExitCode::FAILURE
        }
    }
}

/// Reads the repository's files and runs the check on them.
fn run(root: &Path) -> Result<Vec<Violation>, String> {
    let read = |rel: &str| {
        fs::read_to_string(root.join(rel)).map_err(|e| format!("cannot read {rel}: {e}"))
    };
    let record = read("licences/RECORD")?;
    let manifest = read("Cargo.toml")?;
    let members = members_of(root, "", &manifest, &member_paths(&manifest)?)?;
    let mut locks = vec![LockFile {
        path: "Cargo.lock".to_string(),
        text: read("Cargo.lock")?,
        members: members.clone(),
    }];
    for path in find_all_locks(root)? {
        let text = read(&path)?;
        let dir = path.strip_suffix("Cargo.lock").unwrap_or_default();
        let ws_manifest = read(&format!("{dir}Cargo.toml")).map_err(|e| format!("{path}: {e}"))?;
        let own =
            workspace_member_paths(&ws_manifest).map_err(|e| format!("{dir}Cargo.toml: {e}"))?;
        let mut all = members_of(root, dir, &ws_manifest, &own)?;
        all.extend(members.iter().cloned());
        locks.push(LockFile {
            path,
            text,
            members: all,
        });
    }
    check_locks(&locks, &record)
}

/// The identities of the packages at `paths`, relative to the workspace in `dir` (relative to `root`, ending in
/// `/` or empty) whose manifest text is `ws_manifest`.
fn members_of(
    root: &Path,
    dir: &str,
    ws_manifest: &str,
    paths: &[String],
) -> Result<Vec<Member>, String> {
    let mut members = Vec::new();
    for path in paths {
        let rel = format!("{dir}{path}/Cargo.toml");
        let text =
            fs::read_to_string(root.join(&rel)).map_err(|e| format!("cannot read {rel}: {e}"))?;
        members.push(package_identity(&text, ws_manifest).map_err(|e| format!("{rel}: {e}"))?);
    }
    Ok(members)
}
