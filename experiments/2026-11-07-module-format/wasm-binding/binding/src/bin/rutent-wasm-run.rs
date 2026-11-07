//! Runs the module-boundary suite against the Wasm binding, once per provider category's stand-ins, in Rust and C,
//! and prints the report with its `EXP-2` scoring rows: `rutent-wasm-run <run id> [report.json]`. The stand-ins
//! are read from `RUTENT_WASM_STANDINS`, which `run.sh` sets.
//!
//! A row `M-01` to `M-15` is `pass` in a language when no finding is under its check there and none of the fixtures
//! and controls the scoring sheet lists for it is uncovered there, in every category's run; `M-16` is the whole
//! report's `passed()`, in every category's run.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use rutent_conformance::boundary::{Check, Exercised, Report, RunConfig, run_boundary_suite};
use rutent_wasm_binding::children::{Paths, compile};
use rutent_wasm_binding::engine;
use rutent_wasm_binding::platform::WasmBinding;
use rutent_wasm_binding::wire::sha256;
use serde_json::json;

const CATEGORIES: [&str; 4] = ["reserve", "exchange", "ramp", "verifier"];
const LANGUAGES: [&str; 2] = ["rs", "c"];

/// The scoring sheet's rows, `M-01` to `M-15`, with the check and the fixtures and controls each counts.
fn rows() -> Vec<(&'static str, Check, Vec<Exercised>)> {
    use Exercised as E;
    vec![
        (
            "M-01",
            Check::DeclaredReachWorks,
            vec![
                E::OwnSecretResolved,
                E::OwnStateRead,
                E::DeclaredCallCompleted,
                E::DeclaredRedirectFollowed,
                E::DeclaredCallDelivered,
                E::HonestOutputPassed,
            ],
        ),
        (
            "M-02",
            Check::OtherProvidersSecrets,
            vec![
                E::ForeignSecretRefusedToOutside,
                E::ForeignSecretRefusedToSandboxed,
            ],
        ),
        (
            "M-03",
            Check::OtherProvidersState,
            vec![
                E::ForeignStateRefusedToOutside,
                E::ForeignStateRefusedToSandboxed,
            ],
        ),
        (
            "M-04",
            Check::SandboxedPartHoldsNoSecret,
            vec![E::SandboxedLookupRefused],
        ),
        (
            "M-05",
            Check::UndeclaredSecretRefused,
            vec![E::UndeclaredSecretNameRefused],
        ),
        (
            "M-06",
            Check::UndeclaredDestinationRefused,
            vec![
                E::UndeclaredDestinationRefused,
                E::OtherProvidersEndpointRefused,
            ],
        ),
        (
            "M-07",
            Check::RedirectChecked,
            vec![E::UndeclaredRedirectRefused, E::RedirectChainRefused],
        ),
        (
            "M-08",
            Check::ConnectedAddressChecked,
            vec![
                E::AddressRefused,
                E::RedirectToPrivateAddressRefused,
                E::Ipv6LoopbackRefused,
                E::Ipv6UniqueLocalRefused,
                E::Ipv4MappedRefused,
            ],
        ),
        ("M-09", Check::RecordsTellTheTruth, vec![]),
        (
            "M-10",
            Check::UndeclaredCallRefused,
            vec![E::UndeclaredCallRefused, E::UndeclaredKindRefused],
        ),
        (
            "M-11",
            Check::OutputShapeChecked,
            vec![
                E::UnparseableOutputRefused,
                E::OutputOfAnotherKindRefused,
                E::OutputForAnotherTransactionRefused,
                E::OutputWithAnUnknownFieldRefused,
            ],
        ),
        (
            "M-12",
            Check::OutputSizeBounded,
            vec![E::OutputAtTheLimitPassed, E::OversizedOutputRefused],
        ),
        (
            "M-13",
            Check::AdmissionRecordsWhatItAdmits,
            vec![E::FirstReleaseAdmitted, E::AlteredReleaseRefused],
        ),
        (
            "M-14",
            Check::WideningNeedsFreshAcceptance,
            vec![
                E::WideningReleaseHeld,
                E::PendingReleaseUnreachable,
                E::NarrowingReleaseRan,
                E::NarrowedReachRefused,
                E::WideningBySecretHeld,
                E::WideningByCalleeHeld,
                E::WideningByAddressHeld,
            ],
        ),
        (
            "M-15",
            Check::ProvidersAreIsolated,
            vec![
                E::OtherProviderUndisturbed,
                E::FailedUpgradeKeptThePrevious,
                E::RestartedProviderAnswered,
            ],
        ),
    ]
}

fn row_passes(report: &Report, language: &str, check: Check, fixtures: &[Exercised]) -> bool {
    let found = report
        .findings
        .iter()
        .any(|f| f.check == check && f.language == language);
    let uncovered = report
        .uncovered
        .iter()
        .any(|u| u.language == language && fixtures.contains(&u.answer));
    !found && !uncovered
}

fn summary(durations: &[Duration]) -> serde_json::Value {
    if durations.is_empty() {
        return json!(null);
    }
    let mut ms: Vec<f64> = durations.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    ms.sort_by(|a, b| a.total_cmp(b));
    json!({
        "count": ms.len(),
        "min_ms": (ms[0] * 10.0).round() / 10.0,
        "median_ms": (ms[ms.len() / 2] * 10.0).round() / 10.0,
        "max_ms": (ms[ms.len() - 1] * 10.0).round() / 10.0,
    })
}

/// The resident memory of each process, in KiB, as `ps` reports it.
fn resident_kib(processes: &[(String, u32)]) -> Vec<u64> {
    if processes.is_empty() {
        return Vec::new();
    }
    let pids: Vec<String> = processes.iter().map(|(_, pid)| pid.to_string()).collect();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pids.join(",")])
        .output();
    output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn summary_kib(values: &[u64]) -> serde_json::Value {
    if values.is_empty() {
        return json!(null);
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    json!({"count": v.len(), "min_kib": v[0], "median_kib": v[v.len() / 2], "max_kib": v[v.len() - 1]})
}

/// Each stand-in part as built, before any program is written into it: its digest and size, its imports and
/// exports as the compiler read them, and its machine code from two compilations in two fresh compiler
/// processes, with the time each took.
fn standins_section(paths: &Paths) -> Result<Vec<serde_json::Value>, String> {
    let workdir = std::env::temp_dir().join(format!("rutent-wasm-run-{}", std::process::id()));
    std::fs::create_dir_all(&workdir).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    println!();
    println!(
        "| Stand-in | Part | Portable bytes | SHA-256 | Imports | Machine-code bytes | Same twice | Compile ms |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for language in LANGUAGES {
        for category in CATEGORIES {
            let read = |part: &str| {
                std::fs::read(
                    paths
                        .standins
                        .join(format!("{language}-{category}-{part}.wasm")),
                )
                .map_err(|e| format!("{language}-{category}-{part}: {e}"))
            };
            let (sandboxed, outside) = (read("sandboxed")?, read("outside")?);
            let first = compile(paths, &workdir, &sandboxed, &outside)?;
            let second = compile(paths, &workdir, &sandboxed, &outside)?;
            for (index, (part, bytes)) in [("sandboxed", &sandboxed), ("outside", &outside)]
                .into_iter()
                .enumerate()
            {
                let (code_1, code_2) = if index == 0 {
                    (&first.sandboxed, &second.sandboxed)
                } else {
                    (&first.outside, &second.outside)
                };
                let verdict = &first.verdicts[index];
                let same = sha256(code_1) == sha256(code_2);
                println!(
                    "| {language}-{category} | {part} | {} | `{}` | {} | {} | {} | {:.1} / {:.1} |",
                    bytes.len(),
                    sha256(bytes),
                    verdict.imports.join(", "),
                    code_1.len(),
                    if same { "yes" } else { "no" },
                    first.took.as_secs_f64() * 1000.0,
                    second.took.as_secs_f64() * 1000.0,
                );
                out.push(json!({
                    "standin": format!("{language}-{category}"),
                    "language": language,
                    "category": category,
                    "part": part,
                    "portable_bytes": bytes.len(),
                    "portable_sha256": sha256(bytes),
                    "imports": verdict.imports,
                    "exports": verdict.exports,
                    "machine_code_bytes": code_1.len(),
                    "machine_code_sha256": [sha256(code_1), sha256(code_2)],
                    "same_machine_code_twice": same,
                    "compile_ms_both_parts": [first.took.as_secs_f64() * 1000.0, second.took.as_secs_f64() * 1000.0],
                }));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&workdir);
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(run_id) = args.first().cloned() else {
        eprintln!("usage: rutent-wasm-run <run id> [report.json]");
        return ExitCode::FAILURE;
    };
    let Some(standins) = std::env::var_os("RUTENT_WASM_STANDINS").map(PathBuf::from) else {
        eprintln!("RUTENT_WASM_STANDINS names no directory: run run.sh");
        return ExitCode::FAILURE;
    };
    let executables = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_default();
    let paths = Paths::new(&executables, &standins);
    let mut reports = Vec::new();
    let mut all_passed = true;
    for category in CATEGORIES {
        let mut binding = match WasmBinding::new(paths.clone(), &LANGUAGES, category) {
            Ok(binding) => binding,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        let config = RunConfig {
            run_id: run_id.clone(),
            ..RunConfig::default()
        };
        eprintln!("==> {category}: running the suite");
        let report = run_boundary_suite(&mut binding, &config);
        all_passed &= report.passed();
        let measurements = binding.measurements.clone();
        let resident = resident_kib(&binding.processes());
        reports.push((category, report, measurements, resident));
    }

    println!("run id: {run_id}");
    println!("platform version: {}", engine::platform_version());
    let first = &reports[0].1;
    println!(
        "format: {}; languages: {}; output limit: {} bytes",
        first.format,
        first.languages.join(", "),
        first.output_limit
    );
    let mut json_reports = Vec::new();
    for (category, report, measurements, resident) in &reports {
        println!();
        println!(
            "## {category}: passed {}; findings {}; uncovered {}; missing_languages {}; exercised {}",
            report.passed(),
            report.findings.len(),
            report.uncovered.len(),
            report.missing_languages,
            report.exercised.len()
        );
        for finding in &report.findings {
            println!(
                "  finding [{}] {:?}: {}",
                finding.language, finding.check, finding.detail
            );
        }
        for uncovered in &report.uncovered {
            println!(
                "  uncovered [{}] {:?}",
                uncovered.language, uncovered.answer
            );
        }
        let mut by_check: BTreeMap<String, usize> = BTreeMap::new();
        for finding in &report.findings {
            *by_check
                .entry(format!("{}/{:?}", finding.language, finding.check))
                .or_default() += 1;
        }
        let compiles: Vec<Duration> = measurements.compiles.iter().map(|c| c.0).collect();
        json_reports.push(json!({
            "category": category,
            "run_id": report.run_id,
            "format": report.format,
            "languages": report.languages,
            "output_limit": report.output_limit,
            "passed": report.passed(),
            "findings": report.findings.iter().map(|f| json!({"check": format!("{:?}", f.check), "language": f.language, "detail": f.detail})).collect::<Vec<_>>(),
            "findings_by_check": by_check,
            "uncovered": report.uncovered.iter().map(|u| json!({"language": u.language, "answer": format!("{:?}", u.answer)})).collect::<Vec<_>>(),
            "missing_languages": report.missing_languages,
            "exercised": report.exercised.len(),
            "compile": summary(&compiles),
            "process_start": summary(&measurements.starts),
            "portable_bytes": measurements.compiles.first().map(|c| c.1),
            "machine_code_bytes": measurements.compiles.first().map(|c| c.2),
            "provider_processes_alive_at_the_end": resident.len(),
            "resident_kib": summary_kib(resident),
        }));
    }
    println!();
    println!("| Row | Check | rs | c |");
    println!("|---|---|---|---|");
    let mut rows_json = Vec::new();
    for (id, check, fixtures) in rows() {
        let cells: Vec<bool> = LANGUAGES
            .iter()
            .map(|l| {
                reports
                    .iter()
                    .all(|(_, r, _, _)| row_passes(r, l, check, &fixtures))
            })
            .collect();
        let word = |b: bool| if b { "pass" } else { "fail" };
        println!(
            "| {id} | {check:?} | {} | {} |",
            word(cells[0]),
            word(cells[1])
        );
        rows_json.push(json!({"row": id, "check": format!("{check:?}"), "rs": word(cells[0]), "c": word(cells[1])}));
    }
    let whole = if all_passed { "pass" } else { "fail" };
    println!("| M-16 | Report::passed() | {whole} | {whole} |");
    rows_json.push(json!({"row": "M-16", "check": "Report::passed()", "rs": whole, "c": whole}));
    let standins = match standins_section(&paths) {
        Ok(section) => section,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let out = json!({
        "run_id": run_id,
        "standins": standins,
        "platform_version": engine::platform_version(),
        "settings": engine::settings().into_iter().map(|(k, v)| json!({"setting": k, "value": v})).collect::<Vec<_>>(),
        "isolation": rutent_wasm_sandbox::layers(),
        "reports": json_reports,
        "rows": rows_json,
        "passed": all_passed,
    });
    if let Some(path) = args.get(1) {
        let text = serde_json::to_string_pretty(&out).unwrap_or_default();
        if let Err(e) = std::fs::write(path, text) {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    }
    if all_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
