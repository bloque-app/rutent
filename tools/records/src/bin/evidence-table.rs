//! `evidence-table [--strict | --ci] [--root DIR] [--results FILE] [--base DIR]`: the requirement-to-evidence
//! table (`T-024-09`). It prints each row of `conformance/evidence/v0-scope` with the tests and checks behind it.
//!
//! - Report mode (the default) prints the table and every finding, and exits 0.
//! - `--strict` exits 1 on any finding, whatever the scope file's switch says.
//! - `--ci` is the CI step that reads the switch: with `strict: on` it is strict; with `strict: off` the table's
//!   own findings are reported and do not fail. A change against the base that loosens the scope fails either way.
//! - `--results FILE` is what `cargo test` printed; without it no test counts as passed.
//! - `--base DIR` is a checkout of `develop`: its scope file and `PLAN.md` are what the head is compared with.
//!   A base with no scope file yet is skipped, and said so; a base whose `SPEC.md` or `PLAN.md` can't be read
//!   is an error (exit 2), not a skip.

use rutent_records::Finding;
use rutent_records::evidence::{
    Inputs, Report, Results, Scope, build, compare, parse_scope, universe,
};
use rutent_records::{ids::load_defined, tags};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SCOPE: &str = "conformance/evidence/v0-scope";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Report,
    Strict,
    Ci,
}

fn main() -> ExitCode {
    let mut mode = Mode::Report;
    let mut root = PathBuf::from(".");
    let mut results = None;
    let mut base = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--strict" => mode = Mode::Strict,
            "--ci" if mode != Mode::Strict => mode = Mode::Ci,
            "--ci" => {}
            "--root" | "--results" | "--base" => {
                let Some(value) = args.next() else {
                    return usage();
                };
                match arg.as_str() {
                    "--root" => root = PathBuf::from(value),
                    "--results" => results = Some(PathBuf::from(value)),
                    _ => base = Some(PathBuf::from(value)),
                }
            }
            _ => return usage(),
        }
    }
    match run(mode, &root, results.as_deref(), base.as_deref()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("evidence-table: {e}");
            ExitCode::from(2)
        }
    }
}

/// A results file holds whatever the tests printed, so bytes that are not UTF-8 are replaced, not refused.
fn read_lossy(path: &Path) -> Result<String, String> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn run(
    mode: Mode,
    root: &Path,
    results: Option<&Path>,
    base: Option<&Path>,
) -> Result<ExitCode, String> {
    let spec = read(&root.join("SPEC.md"))?;
    let plan = read(&root.join("PLAN.md"))?;
    let (scope, problems) = parse_scope(&read(&root.join(SCOPE))?);
    let defined = load_defined(root)
        .map_err(|e| format!("cannot read the records under {}: {e}", root.display()))?;
    let universe = universe(&spec);
    let mut tests = Vec::new();
    for path in
        tags::find_rust_files(root).map_err(|e| format!("cannot walk {}: {e}", root.display()))?
    {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let shown = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        tests.extend(tags::scan_tests(&shown, &text));
    }
    let results = match results {
        Some(path) => Results::parse(&read_lossy(path)?),
        None => Results::default(),
    };
    let features = root.join("features");
    let review_exists = |feature: &str| {
        std::fs::read_dir(&features)
            .map(|dir| {
                dir.flatten().any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with(&format!("{feature}-"))
                        && e.path().join("notes.md").is_file()
                })
            })
            .unwrap_or(false)
    };
    let mut report: Report = build(
        &scope,
        &Inputs {
            universe: &universe,
            defined: &defined,
            tests: &tests,
            results: &results,
            review_exists: &review_exists,
        },
    );
    report.table.splice(0..0, problems);
    if results.is_empty() {
        report.table.insert(
            0,
            Finding {
                rule: "no-results",
                location: "results".into(),
                message: "no test results were read, so no test counts as passed".into(),
            },
        );
    }
    let mut notes = Vec::new();
    if let Some(base) = base {
        notes.push(compare_with_base(&mut report, &scope, &plan, base)?);
    }

    for line in &report.lines {
        println!(
            "{:<11} {:<8} {:<12} {}",
            line.id,
            line.class.word(),
            line.status,
            line.detail
        );
    }
    for note in &notes {
        println!("{note}");
    }
    let count = |status: &str| report.lines.iter().filter(|l| l.status == status).count();
    println!(
        "evidence-table: {} rows: {} verified, {} inspected, {} partly met, {} unverified, {} M4's, {} deferred",
        report.lines.len(),
        count("verified"),
        count("inspected"),
        count("partly met"),
        count("unverified"),
        count("M4's"),
        count("deferred")
    );
    for finding in report.ratchet.iter().chain(&report.table) {
        println!("{finding}");
    }
    let (label, fails) = match mode {
        Mode::Report => ("report mode", false),
        Mode::Strict => (
            "strict mode",
            !report.table.is_empty() || !report.ratchet.is_empty(),
        ),
        Mode::Ci if scope.strict_on => (
            "strict mode, the switch on",
            !report.table.is_empty() || !report.ratchet.is_empty(),
        ),
        Mode::Ci => ("report mode, the switch off", !report.ratchet.is_empty()),
    };
    println!(
        "evidence-table: {} table findings, {} against the base ({label})",
        report.table.len(),
        report.ratchet.len()
    );
    Ok(if fails {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Compares the head with `develop`'s checkout and returns a line to print.
fn compare_with_base(
    report: &mut Report,
    head: &Scope,
    head_plan: &str,
    base: &Path,
) -> Result<String, String> {
    read(&base.join("SPEC.md"))?;
    let base_plan = read(&base.join("PLAN.md"))?;
    let base_scope = base.join(SCOPE);
    if !base_scope.is_file() {
        return Ok(format!(
            "evidence-table: the scope file is not on the base yet ({}), so nothing is compared",
            base.display()
        ));
    }
    let (before, _) = parse_scope(&read(&base_scope)?);
    report
        .ratchet
        .extend(compare(head, &before, head_plan, &base_plan));
    Ok(format!(
        "evidence-table: compared with the base at {}",
        base.display()
    ))
}

fn usage() -> ExitCode {
    eprintln!("usage: evidence-table [--strict | --ci] [--root DIR] [--results FILE] [--base DIR]");
    ExitCode::from(2)
}
