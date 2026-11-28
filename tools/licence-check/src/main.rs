//! Runs the licence record check on every `Cargo.lock` in the repository, `licences/RECORD` and the workspace members.

use rutent_licence_check::{
    ArtifactInput, ArtifactViolation, LockFile, Member, Violation, check_artifacts_with,
    check_locks, find_all_locks, member_paths, package_identity, workspace_member_paths,
};
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    match run(&root) {
        Err(e) => {
            eprintln!("licence-check: {e}");
            ExitCode::from(2)
        }
        Ok((found, artifacts)) if found.is_empty() && artifacts.is_empty() => {
            println!(
                "licence-check: every locked dependency is recorded and decided, and every artifact is listed and clear"
            );
            ExitCode::SUCCESS
        }
        Ok((found, artifacts)) => {
            for v in &found {
                eprintln!("licence-check: {v}");
            }
            for v in &artifacts {
                eprintln!("licence-check: {v}");
            }
            eprintln!(
                "licence-check: {} dependencies and {} artifacts fail the check",
                found.len(),
                artifacts.len()
            );
            ExitCode::FAILURE
        }
    }
}

/// Reads the repository's files and runs the check on them, then reads each lock's artifacts.
fn run(root: &Path) -> Result<(Vec<Violation>, Vec<ArtifactViolation>), String> {
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
    let found = check_locks(&locks, &record)?;
    let mut inputs = Vec::new();
    for lock in &locks {
        inputs.push(ArtifactInput {
            lockfile: lock.path.clone(),
            metadata: cargo_metadata(root, &lock.path)?,
        });
    }
    let read = |path: &str| fs::read_to_string(path).ok();
    Ok((found, check_artifacts_with(&inputs, &record, &read)?))
}

/// `cargo metadata` for the workspace beside a lock, with all features so that an optional dependency is read too.
/// `--locked` so that it never changes the lock. It runs with the cargo that runs this tool, not through `rustup`,
/// so a workspace's own toolchain file installs nothing.
fn cargo_metadata(root: &Path, lockfile: &str) -> Result<String, String> {
    let manifest = root
        .join(lockfile.strip_suffix("Cargo.lock").unwrap_or_default())
        .join("Cargo.toml");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--all-features",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("{lockfile}: cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{lockfile}: cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| format!("{lockfile}: cargo metadata is not UTF-8"))
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
