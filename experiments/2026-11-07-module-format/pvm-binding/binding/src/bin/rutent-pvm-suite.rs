//! Runs the module-boundary suite on the PolkaVM binding, once per provider category, in Rust and C, and prints
//! the report the run note records, with the binding's own checks beside it.
//!
//! `rutent-pvm-suite --run-id <id> --guests <dir> [--output-limit <bytes>] [--unisolated]`
//!
//! `--unisolated` runs the worker and the admitting compiler without isolation, for debugging; `B-07` and `B-08`
//! are then not run.
//!
//! It exits non-zero if any category's report doesn't pass.

use std::collections::BTreeSet;
use std::path::PathBuf;

use rutent_conformance::boundary::{
    AdmissionRefusal, Admitted, Check, Exercised, ModuleBinding, Report, RunConfig,
    run_boundary_suite,
};
use rutent_conformance::role::{SuiteContext, placeholder_key};
use rutent_contracts::module_interface::{Declaration, PartName, Parts};
use rutent_pvm_binding::binding::{
    CATEGORIES, FORMAT, LANGUAGES, PvmBinding, Setup, Stats, assertion,
};
use rutent_pvm_binding::engine;
use rutent_pvm_binding::isolation::{self, Isolation, Role};
use rutent_pvm_binding::sha256::hex_digest;
use rutent_pvm_binding::spec;
use serde_json::json;

/// The scoring sheet's suite rows: each check, and the fixtures and controls it counts (scoring.md §1).
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
    !report
        .findings
        .iter()
        .any(|f| f.check == check && f.language == language)
        && !report
            .uncovered
            .iter()
            .any(|u| u.language == language && fixtures.contains(&u.answer))
}

fn median(values: &[u128]) -> String {
    let mut v = values.to_vec();
    v.sort_unstable();
    match v.get(v.len() / 2) {
        Some(m) => format!(
            "{m} µs (median of {}, min {}, max {})",
            v.len(),
            v[0],
            v[v.len() - 1]
        ),
        None => "none".to_owned(),
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn declaration(provider: &str) -> Declaration {
    serde_json::from_value(json!({
        "provider": provider,
        "version": "1.0.0",
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": ["withdrawal"] },
        "outside": {
            "operations": [{ "name": "api.call", "destinations": ["api"], "secrets": [] }],
            "destinations": [{ "name": "api", "scheme": "https", "host": "api.hostile.example", "port": 443, "addresses": "public" }],
            "secrets": [],
            "calls": [],
            "statement_key": placeholder_key(1),
        },
    }))
    .expect("a valid declaration")
}

/// `B-02` and part of `B-09`: hostile sandboxed parts, and blobs that aren't programs, are refused at admission,
/// and the binding still admits an honest release afterwards.
fn admission_checks(setup: &Setup) -> bool {
    println!("\n## Admission checks (B-02, B-09 in part)\n");
    let mut binding = PvmBinding::new(setup.clone(), "reserve");
    let mut all = true;
    for language in LANGUAGES {
        let Some(outside) = binding
            .template(language, PartName::Outside)
            .map(<[u8]>::to_vec)
        else {
            println!("- {language}: no stand-ins built");
            all = false;
            continue;
        };
        let honest_sandboxed = binding
            .template(language, PartName::Sandboxed)
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        let provider = format!("hostile-{language}");
        let spec_text = format!("k reserve\np {provider}\nv 1.0.0\ns 1\no api.call\nout honest\n");
        let patched = |bytes: &[u8]| spec::patch(bytes, &spec_text).unwrap_or_default();
        let mut cases: Vec<(String, Vec<u8>, bool)> = Vec::new();
        for hostile in ["hostile-secret", "hostile-clock"] {
            let path = setup
                .guests
                .join(language)
                .join(format!("{hostile}.polkavm"));
            match std::fs::read(&path) {
                Ok(bytes) => cases.push((
                    format!(
                        "a sandboxed part importing {}",
                        if hostile == "hostile-secret" {
                            "the outside part's rt_secret"
                        } else {
                            "an undeclared clock_now"
                        }
                    ),
                    patched(&bytes),
                    false,
                )),
                Err(_) => {
                    println!("- {language}: {hostile} not built");
                    all = false;
                }
            }
        }
        let honest = patched(&honest_sandboxed);
        cases.push((
            "a blob truncated to half".to_owned(),
            honest[..honest.len() / 2].to_vec(),
            false,
        ));
        cases.push((
            "bytes that aren't a program".to_owned(),
            b"not a polkavm blob".to_vec(),
            false,
        ));
        cases.push((
            "the outside part as the sandboxed part".to_owned(),
            patched(&outside),
            false,
        ));
        cases.push((
            "the honest stand-in, afterwards".to_owned(),
            honest.clone(),
            true,
        ));
        for (what, sandboxed, admissible) in cases {
            let binaries = Parts {
                sandboxed,
                outside: patched(&outside),
            };
            let declaration = declaration(&format!(
                "{provider}-{}",
                hex_digest(what.as_bytes()).get(..8).unwrap_or("x")
            ));
            let admitted = binding.admit(&assertion(&declaration, &binaries), &binaries);
            let ok = matches!(
                (&admitted, admissible),
                (Admitted::Refused(AdmissionRefusal::UnreadableBinary), false)
                    | (Admitted::Artefact(_), true)
            );
            all &= ok;
            let shown = match admitted {
                Admitted::Artefact(_) => "admitted".to_owned(),
                Admitted::Refused(r) => format!("refused as {r:?}"),
            };
            println!(
                "- {language}, {what}: {shown}: {}",
                if ok { "as expected" } else { "NOT as expected" }
            );
        }
    }
    for reason in &binding.stats.refused {
        println!("  - the admitting compiler's reason: {reason}");
    }
    all
}

/// `B-11` and `G-19`, where the compiler runs: each compiled stand-in admitted twice, in two separate admitting
/// compiler processes, and the digests compared.
fn determinism(setup: &Setup) {
    println!("\n## Same input, two compilations (B-11, G-19 in part)\n");
    if !engine::COMPILER {
        println!(
            "- Not run: this platform has no PolkaVM compiler; the interpreter makes no machine code to compare."
        );
        return;
    }
    let mut binding = PvmBinding::new(setup.clone(), "reserve");
    for category in CATEGORIES {
        for language in LANGUAGES {
            let read = |part: &str| {
                std::fs::read(
                    setup
                        .guests
                        .join(language)
                        .join(format!("{part}-{category}.polkavm")),
                )
            };
            let (Ok(sandboxed), Ok(outside)) = (read("sandboxed"), read("outside")) else {
                println!("- {category} {language}: not built");
                continue;
            };
            let binaries = Parts { sandboxed, outside };
            let (first, second) = (binding.compile(&binaries), binding.compile(&binaries));
            match (first, second) {
                (Ok(a), Ok(b)) => println!(
                    "- {category} {language}: sandboxed {} {}, outside {} {}",
                    a.0.value.as_str(),
                    if a.0 == b.0 { "same twice" } else { "DIFFERS" },
                    a.1.value.as_str(),
                    if a.1 == b.1 { "same twice" } else { "DIFFERS" },
                ),
                (a, b) => println!("- {category} {language}: not compiled: {a:?} {b:?}"),
            }
        }
    }
}

fn print_stats(category: &str, stats: &Stats, workers: &[u64]) {
    println!("\n### Measurements and processes, {category}\n");
    println!(
        "- admitting compiler, compile time of both parts: {}",
        median(&stats.compile_micros)
    );
    println!(
        "- admission round trip, process start included: {}",
        median(&stats.admission_micros)
    );
    println!(
        "- load into a worker (compile, instantiate, start both parts): {}",
        median(&stats.load_micros)
    );
    println!(
        "- workers started: {}; most alive at once: {}",
        stats.workers_started, stats.most_workers
    );
    let distinct: BTreeSet<_> = workers.iter().collect();
    let real = workers
        .iter()
        .filter(|pid| {
            std::process::Command::new("ps")
                .args(["-o", "pid=", "-p", &pid.to_string()])
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .count();
    println!(
        "- workers alive at the end: {}, distinct process ids: {}, found by ps: {real}",
        workers.len(),
        distinct.len()
    );
    println!(
        "- PolkaVM sandbox worker processes seen: {}",
        stats.guest_pids
    );
    println!(
        "- loads whose digests differed from the admitted ones: {}",
        stats.digest_mismatches
    );
    println!(
        "- host calls refused before a record: {}",
        stats.host_refusals.len()
    );
    for reason in &stats.host_refusals {
        println!("  - {reason}");
    }
    println!("- releases that didn't start: {}", stats.not_started.len());
    for reason in &stats.not_started {
        println!("  - {reason}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let run_id = arg(&args, "--run-id").unwrap_or_else(|| "EXP-2-pvm-local".to_owned());
    let guests = PathBuf::from(arg(&args, "--guests").expect("--guests <dir>"));
    let output_limit = arg(&args, "--output-limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(4096);
    let isolation = if args.iter().any(|a| a == "--unisolated") {
        Isolation::Unisolated
    } else {
        Isolation::Required
    };
    let here = std::env::current_exe().expect("the program's path");
    let dir = here.parent().expect("its directory");
    let setup = Setup {
        worker: dir.join("rutent-pvm-worker"),
        admit: dir.join("rutent-pvm-admit"),
        guests: guests.clone(),
        isolation,
    };

    println!("# Module-boundary suite on PolkaVM: {run_id}\n");
    println!(
        "- platform: {}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    println!(
        "- format: {FORMAT}; platform version: {}",
        engine::platform_version()
    );
    for (name, value) in engine::settings() {
        println!("- engine {name}: {value}");
    }
    println!(
        "- sandboxed imports allowed: {:?}",
        engine::SANDBOXED_IMPORTS
    );
    println!("- outside imports allowed: {:?}", engine::OUTSIDE_IMPORTS);
    println!(
        "- admitting compiler isolation: {}",
        isolation::describe(Role::Admit, isolation)
    );
    println!(
        "- worker isolation: {}",
        isolation::describe(Role::Worker, isolation)
    );
    if isolation == Isolation::Unisolated {
        println!("- UNISOLATED: --unisolated was given, so B-07 and B-08 are not run");
    }
    println!("- output limit: {output_limit} bytes");
    println!("\n## Stand-ins\n");
    println!("| Category | Language | Part | Bytes | SHA-256 |");
    println!("|---|---|---|---|---|");
    for category in CATEGORIES {
        for language in LANGUAGES {
            for part in ["sandboxed", "outside"] {
                let path = guests
                    .join(language)
                    .join(format!("{part}-{category}.polkavm"));
                match std::fs::read(&path) {
                    Ok(b) => println!(
                        "| {category} | {language} | {part} | {} | `{}` |",
                        b.len(),
                        hex_digest(&b)
                    ),
                    Err(_) => println!("| {category} | {language} | {part} | not built | |"),
                }
            }
        }
    }

    let mut reports = Vec::new();
    for category in CATEGORIES {
        let mut binding = PvmBinding::new(setup.clone(), category);
        let config = RunConfig {
            run_id: run_id.clone(),
            output_limit,
            context: SuiteContext::default(),
        };
        let report = run_boundary_suite(&mut binding, &config);
        println!("\n## Report, {category} stand-ins\n");
        println!("- run_id: {}", report.run_id);
        println!("- format: {}", report.format);
        println!("- languages: {:?}", report.languages);
        println!("- output_limit: {}", report.output_limit);
        println!("- missing_languages: {}", report.missing_languages);
        println!(
            "- exercised: {} of {}",
            report.exercised.len(),
            Exercised::ALL.len() * report.languages.len().max(1)
        );
        println!("- findings: {}", report.findings.len());
        for f in &report.findings {
            println!("  - {:?} [{}]: {}", f.check, f.language, f.detail);
        }
        println!("- uncovered: {}", report.uncovered.len());
        for u in &report.uncovered {
            println!("  - [{}] {:?}", u.language, u.answer);
        }
        println!("- passed: {}", report.passed());
        let workers: Vec<u64> = binding.workers().into_values().collect();
        print_stats(category, &binding.stats, &workers);
        reports.push((category, report));
    }

    println!("\n## Scoring rows, over the four categories\n");
    println!("| Row | Check | rs | c |");
    println!("|---|---|---|---|");
    for (id, check, fixtures) in rows() {
        let cell = |language: &str| {
            let ok = reports.iter().all(|(_, r)| {
                r.languages.iter().any(|l| l == language)
                    && row_passes(r, language, check, &fixtures)
            });
            if ok { "pass" } else { "fail" }
        };
        println!("| {id} | {check:?} | {} | {} |", cell("rs"), cell("c"));
    }
    let all = reports.iter().all(|(_, r)| r.passed());
    let whole = if all { "pass" } else { "fail" };
    println!("| M-16 | Report::passed() | {whole} | {whole} |");

    let admission = admission_checks(&setup);
    determinism(&setup);
    println!("\n## Result\n");
    println!("- every category's report passed: {all}");
    println!("- admission checks as expected: {admission}");
    if !(all && admission) {
        std::process::exit(1);
    }
}
