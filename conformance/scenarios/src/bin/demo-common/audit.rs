//! The outside check: what the operator hands a checker, written to a folder as files, and `audit-check`, the
//! checker's own tool from `rutent-core-audit`, run over them as a separate process.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rutent_contracts::core_internal::KeptStatement;
use rutent_core_audit::receipts::store::Shelved;
use rutent_scenarios::assembly::checking::{self, CHECKPOINT_EVERY};
use rutent_scenarios::assembly::{LOG_NAME, context};
use rutent_scenarios::kit;
use serde_json::Value;

/// What the checker is handed, as files under the output folder.
pub struct Handed {
    /// The log, a record to a line.
    pub history: PathBuf,
    /// The checkpoints, as the checker received each when it was published, one to a line.
    pub checkpoints: PathBuf,
    /// The openings the operator hands over, one to a line.
    pub openings: PathBuf,
    /// The evidence store's directory: `kept/<commitment>.json` and `erased/`.
    pub evidence: PathBuf,
    /// The last tick a checkpoint was published for.
    pub through: u64,
}

fn lines(path: &Path, records: &[Vec<u8>]) -> std::io::Result<()> {
    let mut text = Vec::new();
    for record in records {
        text.extend_from_slice(record);
        text.push(b'\n');
    }
    fs::write(path, text)
}

/// Writes the history, the checkpoints, the openings and the evidence store under `out/log` and `out/evidence`.
pub fn write(
    out: &Path,
    history: &[Vec<u8>],
    checkpoints: &[Vec<u8>],
    openings: &[Vec<u8>],
    kept: &[KeptStatement],
    through: u64,
) -> std::io::Result<Handed> {
    let log = out.join("log");
    let evidence = out.join("evidence");
    fs::create_dir_all(&log)?;
    fs::create_dir_all(evidence.join("kept"))?;
    fs::create_dir_all(evidence.join("erased"))?;
    let handed = Handed {
        history: log.join("history.jsonl"),
        checkpoints: log.join("checkpoints.jsonl"),
        openings: log.join("openings.jsonl"),
        evidence,
        through,
    };
    lines(&handed.history, history)?;
    lines(&handed.checkpoints, checkpoints)?;
    lines(&handed.openings, openings)?;
    for (place, statement) in kept.iter().enumerate() {
        let shelved = Shelved {
            place: place as u64,
            kept: statement.clone(),
        };
        let name = format!("{}.json", statement.commitment.value.as_str());
        fs::write(
            handed.evidence.join("kept").join(name),
            serde_json::to_vec_pretty(&shelved).expect("a kept statement is JSON"),
        )?;
    }
    Ok(handed)
}

/// A copy of the history at `to` with one figure changed in one record: what someone holding the file and the
/// operator's full access could write. The record changed is the first that carries an `amount` of `amount`, which
/// becomes ten times it; its index is handed back.
pub fn tamper(history: &[Vec<u8>], to: &Path, amount: &str) -> std::io::Result<Option<usize>> {
    let mut out = history.to_vec();
    let mut changed = None;
    for (n, record) in out.iter_mut().enumerate() {
        let Ok(mut value) = serde_json::from_slice::<Value>(record) else {
            continue;
        };
        if rewrite_amount(&mut value, amount) {
            *record = serde_json::to_vec(&value).expect("JSON");
            changed = Some(n);
            break;
        }
    }
    lines(to, &out)?;
    Ok(changed)
}

/// Changes the first `amount` of `amount` in `value` to ten times it.
fn rewrite_amount(value: &mut Value, amount: &str) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("amount").and_then(Value::as_str) == Some(amount) {
                map.insert("amount".to_owned(), Value::from(format!("{amount}0")));
                return true;
            }
            map.values_mut().any(|v| rewrite_amount(v, amount))
        }
        Value::Array(items) => items.iter_mut().any(|v| rewrite_amount(v, amount)),
        _ => false,
    }
}

/// The arguments `audit-check` takes for `history`: the log's name and public key, the schedule, the evidence
/// directory and the deployment its statements name. Nothing secret.
pub fn arguments(handed: &Handed, history: &Path, evidence: bool) -> Vec<String> {
    let first = rutent_prov::catalogue::Instant(kit::START)
        .rfc3339()
        .expect("the scenario's start is a time");
    let deployment = context().deployment;
    let mut args: Vec<String> = vec![
        "--history".into(),
        history.display().to_string(),
        "--checkpoints".into(),
        handed.checkpoints.display().to_string(),
        "--openings".into(),
        handed.openings.display().to_string(),
        "--key".into(),
        checking::log_public_key().as_str().to_owned(),
        "--log".into(),
        LOG_NAME.into(),
        "--first".into(),
        first,
        "--every".into(),
        CHECKPOINT_EVERY.to_string(),
        "--through".into(),
        handed.through.to_string(),
    ];
    if evidence {
        args.extend([
            "--evidence".into(),
            handed.evidence.display().to_string(),
            "--deployment".into(),
            deployment.name,
            "--operator-key".into(),
            deployment.operator_key.as_str().to_owned(),
        ]);
    }
    args
}

/// The `audit-check` binary: beside this one, built by cargo first if it isn't there yet.
pub fn binary() -> Result<PathBuf, String> {
    let here = std::env::current_exe().map_err(|e| format!("this binary's path: {e}"))?;
    let mut dir = here.parent().ok_or("this binary has no directory")?;
    // Run from cargo's copy under `deps/` (`relaunch::from_deps`): the profile's directory is the one above it.
    if dir.file_name().is_some_and(|n| n == "deps") {
        dir = dir.parent().ok_or("deps/ has no parent")?;
    }
    let found = dir.join(format!("audit-check{}", std::env::consts::EXE_SUFFIX));
    if found.exists() {
        return Ok(found);
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build.args([
        "build",
        "--quiet",
        "-p",
        "rutent-core-audit",
        "--bin",
        "audit-check",
    ]);
    if !cfg!(debug_assertions) {
        build.arg("--release");
    }
    let status = build
        .status()
        .map_err(|e| format!("cargo couldn't be run to build audit-check: {e}"))?;
    if !status.success() || !found.exists() {
        return Err(format!(
            "audit-check wasn't built ({status}); build it with `cargo build -p rutent-core-audit --bin audit-check`"
        ));
    }
    Ok(found)
}

/// What one run of `audit-check` gave: its exit code and what it printed.
pub struct Checked {
    /// 0: the history holds; 1: it found something; 2: it couldn't run.
    pub code: Option<i32>,
    /// Its standard output and standard error, as printed.
    pub output: String,
}

/// Runs `audit-check` with `args`.
pub fn check(binary: &Path, args: &[String]) -> Result<Checked, String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|e| format!("audit-check couldn't be run: {e}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(Checked {
        code: output.status.code(),
        output: text,
    })
}

/// The command a presenter, or a customer's auditor, types to check the folder again, by hand.
pub fn script(binary: &Path, args: &[String]) -> String {
    let quoted: Vec<String> = args.iter().map(|a| format!("'{a}'")).collect();
    format!(
        "#!/bin/sh\n# Checks the demo's log against its checkpoints and its evidence, as an outside checker does.\n# Exit 0: it holds. Exit 1: a finding, printed. Exit 2: it couldn't run.\nexec '{}' {}\n",
        binary.display(),
        quoted.join(" ")
    )
}
