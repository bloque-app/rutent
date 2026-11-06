//! Runs the licence record check on the repository's `Cargo.lock`, `licences/RECORD` and workspace members.

use rutent_licence_check::{Violation, check, member_paths, package_identity};
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
    let lock = read("Cargo.lock")?;
    let record = read("licences/RECORD")?;
    let manifest = read("Cargo.toml")?;
    let mut members = Vec::new();
    for path in member_paths(&manifest)? {
        let text = read(&format!("{path}/Cargo.toml"))?;
        members.push(package_identity(&text, &manifest).map_err(|e| format!("{path}: {e}"))?);
    }
    check(&lock, &record, &members)
}
