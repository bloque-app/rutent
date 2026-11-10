//! Hardening the admitting compiler (`T-013-05`): binaries a fuzzer made go through admission and never crash the
//! compiler (`B-09`, `THR-85`); the Spectre mitigations are chosen settings (`G-09`, `THR-85`); machine code derived
//! again on a CPU of another vendor than the first matches (`B-11`, `G-04`, `REQ-MDL-2`, `THR-18`, ADR-0014).

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/cpu.rs"]
mod cpu;
#[path = "module_support/fuzz.rs"]
mod fuzz;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{assertion, binding, languages, spec};
use cpu::Digests;
use rutent_conformance::boundary::{Admitted, ModuleBinding};
use rutent_contracts::module_interface::Parts;
use rutent_prov::module::children::{CompileError, compile};
use rutent_prov::module::engine;
use rutent_prov::module::wire::sha256;
use wasmtime::Engine;

fn corpus() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz-corpus");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("the checked-in fuzz corpus")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wasm"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&p).expect("a corpus file"))
        })
        .collect()
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rutent-hardening-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a work directory");
    dir
}

/// Gives both parts to the admitting compiler. A verdict, `Ok` or a refusal, is what it is for; a compiler that
/// failed or passed its deadline is a finding.
fn crashed(dir: &Path, sandboxed: &[u8], outside: &[u8], within: Duration) -> Option<String> {
    match compile(
        &common::paths(),
        dir,
        sandboxed,
        outside,
        &Default::default(),
        within,
    ) {
        Ok(_) | Err(CompileError::Refused { .. }) => None,
        Err(other) => Some(other.to_string()),
    }
}

// verifies: THR-85
#[test]
fn fuzz_corpus_never_crashes_the_admitting_compiler() {
    let corpus = corpus();
    assert!(
        corpus.len() >= 20,
        "the corpus holds {} files, too few to mean anything",
        corpus.len()
    );
    let dir = workdir("corpus");
    let mut crashes = Vec::new();
    for (name, bytes) in &corpus {
        if let Some(why) = crashed(&dir, bytes, bytes, Duration::from_secs(60)) {
            crashes.push(format!("{name}: {why}"));
        }
    }
    let _ = std::fs::remove_dir_all(dir);
    assert!(crashes.is_empty(), "the compiler failed on: {crashes:#?}");
}

// verifies: THR-85
#[test]
fn fuzzed_binaries_are_refused_or_admitted_at_admission_and_a_good_release_is_admitted_after() {
    let mut binding = binding("reserve");
    let language = languages().remove(0);
    let base = spec("fuzzed", "1.0.0", Vec::new(), true);
    let honest = binding
        .build_with(&base, &language, "reserve", &[])
        .expect("a stand-in");
    for (n, (name, bytes)) in corpus().into_iter().enumerate() {
        let provider = format!("fuzzed-{n}");
        let declaration = common::declaration(&provider, "1.0.0", &["withdrawal"]);
        for parts in [
            Parts {
                sandboxed: bytes.clone(),
                outside: honest.outside.clone(),
            },
            Parts {
                sandboxed: honest.sandboxed.clone(),
                outside: bytes.clone(),
            },
        ] {
            // Whatever the verdict, the platform gives one and goes on.
            let admitted = binding.admit(&assertion(&declaration, &parts), &parts);
            if let Admitted::Artefact(artefact) = &admitted {
                // A corpus file that is a valid component is admitted with the machine code it was made from.
                assert!(
                    !artefact.build.platform_version.as_str().is_empty(),
                    "{name}"
                );
                break;
            }
        }
    }
    let good = spec("after-the-fuzzed", "1.0.0", Vec::new(), true);
    let admitted = common::install(&mut binding, &good, &language, "reserve", &[]);
    assert!(
        matches!(admitted, Admitted::Artefact(_)),
        "a good release was refused after the fuzzed ones: {admitted:?}"
    );
}

/// Seeds for the long run: the stand-ins in the smallest language and the corpus.
fn seeds() -> Vec<Vec<u8>> {
    let mut seeds: Vec<Vec<u8>> = corpus().into_iter().map(|(_, b)| b).collect();
    for name in ["control", "clock", "file", "socket", "undeclared", "canary"] {
        if let Ok(bytes) = std::fs::read(common::standins().join(format!("hostile-{name}.wasm"))) {
            seeds.push(bytes);
        }
    }
    for language in languages() {
        for category in common::CATEGORIES {
            for part in ["sandboxed", "outside"] {
                let name = format!("{language}-{category}-{part}.wasm");
                if let Ok(bytes) = std::fs::read(common::standins().join(name))
                    && bytes.len() <= 256 * 1024
                {
                    seeds.push(bytes);
                }
            }
        }
    }
    seeds
}

// A run in CI, for a time its environment sets; locally it is bounded the same way, and 120 seconds at most.
// verifies: THR-85
#[test]
#[ignore = "runs for RUTENT_FUZZ_SECONDS; CI runs it with --ignored"]
fn fuzzing_for_a_while_never_crashes_the_admitting_compiler() {
    let seconds: u64 = std::env::var("RUTENT_FUZZ_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let seed: u64 = std::env::var("RUTENT_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_secs())
        });
    eprintln!("fuzzing for {seconds} s with seed {seed}");
    let seeds = seeds();
    let cap = 512 * 1024;
    let dir = workdir("fuzz");
    let findings = std::env::var_os("RUTENT_FUZZ_FINDINGS").map(PathBuf::from);
    let mut rng = fuzz::Rng(seed);
    let started = Instant::now();
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut crashes = Vec::new();
    let mut runs = 0usize;
    while started.elapsed()
        < Duration::from_secs(seconds.min(if findings.is_some() { 3600 } else { 120 }))
    {
        let first = {
            let at = rng.below(seeds.len());
            fuzz::mutate(&mut rng, &seeds[at], cap)
        };
        let second = {
            let at = rng.below(seeds.len());
            fuzz::mutate(&mut rng, &seeds[at], cap)
        };
        runs += 1;
        let result = compile(
            &common::paths(),
            &dir,
            &first,
            &second,
            &Default::default(),
            Duration::from_secs(60),
        );
        let class = match &result {
            Ok(_) => "compiled".to_owned(),
            Err(CompileError::Refused { why, .. }) => {
                format!("refused: {}", why.chars().take(60).collect::<String>())
            }
            Err(other) => {
                crashes.push((runs, other.to_string(), first.clone(), second.clone()));
                "CRASH".to_owned()
            }
        };
        *tally.entry(class).or_default() += 1;
    }
    let _ = std::fs::remove_dir_all(dir);
    eprintln!("{runs} runs, seed {seed}: {tally:#?}");
    for (n, why, first, second) in &crashes {
        if let Some(dir) = &findings {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(dir.join(format!("crash-{seed}-{n}-sandboxed.wasm")), first);
            let _ = std::fs::write(dir.join(format!("crash-{seed}-{n}-outside.wasm")), second);
        }
        eprintln!("run {n} (seed {seed}): {why}");
    }
    assert!(
        crashes.is_empty(),
        "{} runs crashed the compiler",
        crashes.len()
    );
}

/// Makes the corpus from the seeds (`RUTENT_FUZZ_CORPUS_OUT=<directory>`): the hand-made edge cases and, for each
/// kind of verdict, a few mutated binaries.
// verifies: THR-85
#[test]
#[ignore = "writes the corpus; run by hand"]
fn the_corpus_is_written_when_asked() {
    let Some(out) = std::env::var_os("RUTENT_FUZZ_CORPUS_OUT").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    let dir = workdir("corpus-out");
    let header = b"\0asm\x0d\0\x01\0".to_vec();
    let mut chosen: Vec<(String, Vec<u8>)> = vec![
        ("empty".into(), Vec::new()),
        ("magic-only".into(), b"\0asm".to_vec()),
        ("component-header".into(), header.clone()),
        ("module-header".into(), b"\0asm\x01\0\0\0".to_vec()),
        ("elf-header".into(), b"\x7fELF\x02\x01\x01\0".to_vec()),
        ("not-wasm".into(), b"GET / HTTP/1.1\r\n\r\n".to_vec()),
        ("zeros".into(), vec![0; 64]),
        ("high-bytes".into(), vec![0xFF; 64]),
        (
            "section-too-long".into(),
            [header.clone(), vec![0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F]].concat(),
        ),
        (
            "section-overlong-leb".into(),
            [
                header.clone(),
                vec![0x01, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00],
            ]
            .concat(),
        ),
        (
            "unknown-section".into(),
            [header.clone(), vec![0x7F, 0x01, 0x00]].concat(),
        ),
    ];
    for name in ["control", "clock", "file", "socket", "undeclared", "canary"] {
        let bytes = std::fs::read(common::standins().join(format!("hostile-{name}.wasm")))
            .expect("the hostile components");
        chosen.push((format!("seed-hostile-{name}"), bytes));
    }
    let mut rng = fuzz::Rng(13);
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let seeds: Vec<Vec<u8>> = seeds()
        .into_iter()
        .filter(|s| s.len() <= 4 * 1024)
        .collect();
    assert!(!seeds.is_empty(), "no small seeds");
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(100) {
        let input = {
            let at = rng.below(seeds.len());
            fuzz::mutate(&mut rng, &seeds[at], 8 * 1024)
        };
        let class = match compile(
            &common::paths(),
            &dir,
            &input,
            &input,
            &Default::default(),
            Duration::from_secs(60),
        ) {
            Ok(_) => "compiled".to_owned(),
            Err(CompileError::Refused { why, .. }) => why.chars().take(40).collect(),
            Err(other) => format!("CRASH {other}"),
        };
        let kept = classes.entry(class.clone()).or_default();
        if *kept < 5 && input.len() <= 8 * 1024 {
            *kept += 1;
            chosen.push((format!("mutated-{}", chosen.len()), input));
        }
        if chosen.len() >= 30 {
            break;
        }
    }
    eprintln!("{classes:#?}");
    for (name, bytes) in chosen.into_iter().take(40) {
        std::fs::write(out.join(format!("{name}.wasm")), bytes).unwrap();
    }
}

fn smallest_component() -> Vec<u8> {
    let language = languages().remove(0);
    std::fs::read(common::standins().join(format!("{language}-reserve-sandboxed.wasm")))
        .expect("a stand-in")
}

/// The machine code of `bytes` under the engine's configuration with `flag` set to `value`, or as it is.
fn machine_code(bytes: &[u8], flag: Option<(&str, &str)>) -> Vec<u8> {
    let mut config = engine::config().expect("the configuration");
    if let Some((name, value)) = flag {
        // SAFETY: a Cranelift setting that only turns a mitigation on or off, to compare the code it makes.
        unsafe { config.cranelift_flag_set(name, value) };
    }
    Engine::new(&config)
        .expect("an engine")
        .precompile_component(bytes)
        .expect("machine code")
}

// verifies: THR-85
#[test]
fn engine_is_built_with_the_chosen_spectre_settings() {
    assert_eq!(
        engine::SPECTRE,
        [
            ("enable_heap_access_spectre_mitigation", "true"),
            ("enable_table_access_spectre_mitigation", "true"),
        ]
    );
    let recorded = engine::settings();
    for (name, value) in engine::SPECTRE {
        assert!(
            recorded
                .iter()
                .any(|(_, text)| text.contains(name) && text.contains(value)),
            "{name} is not in the settings the run note lists"
        );
    }
    let bytes = smallest_component();
    let chosen = machine_code(&bytes, None);
    for (name, value) in engine::SPECTRE {
        assert_eq!(
            chosen,
            machine_code(&bytes, Some((name, value))),
            "{name}={value} changes the machine code, so the engine is not built with it"
        );
        assert_ne!(
            chosen,
            machine_code(&bytes, Some((name, "false"))),
            "{name}=false makes the same machine code, so the mitigation is not in the chosen code"
        );
    }
}

// verifies: REQ-MDL-2, THR-18
#[test]
fn a_cpu_is_named_by_its_vendor_and_amd_is_told_apart() {
    let amd =
        "processor : 0\nvendor_id : AuthenticAMD\nmodel name : AMD EPYC 7763 64-Core Processor\n";
    let intel = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel(R) Xeon(R) Platinum 8370C\n";
    assert_eq!(cpu::vendor(amd).as_deref(), Some("AuthenticAMD"));
    assert_eq!(cpu::vendor(intel).as_deref(), Some("GenuineIntel"));
    assert_eq!(
        cpu::model(intel).as_deref(),
        Some("Intel(R) Xeon(R) Platinum 8370C")
    );
    assert!(cpu::is_amd("AuthenticAMD") && cpu::is_amd("HygonGenuine"));
    assert!(!cpu::is_amd("GenuineIntel"));
    assert_eq!(cpu::vendor("processor : 0\nCPU implementer : 0x41\n"), None);
    // The pairs a re-derivation may be made on: this machine is not AMD's, and the reference was made elsewhere.
    assert_eq!(cpu::crosses_vendors("GenuineIntel", "AuthenticAMD"), Ok(()));
    assert!(cpu::crosses_vendors("AuthenticAMD", "GenuineIntel").is_err());
    assert!(cpu::crosses_vendors("GenuineIntel", "GenuineIntel").is_err());
    assert!(cpu::crosses_vendors("GenuineIntel", "unknown").is_err());
}

/// The machine code of every stand-in part, compiled in a fresh compiler process, with this machine's CPU.
fn derive() -> Digests {
    let dir = workdir("derive");
    let info = cpu::cpuinfo().unwrap_or_default();
    let mut parts = BTreeMap::new();
    for language in languages() {
        for category in common::CATEGORIES {
            let read = |part: &str| {
                std::fs::read(common::standins().join(format!("{language}-{category}-{part}.wasm")))
                    .expect("a stand-in")
            };
            let machine = compile(
                &common::paths(),
                &dir,
                &read("sandboxed"),
                &read("outside"),
                &Default::default(),
                Duration::from_secs(120),
            )
            .expect("it compiles");
            parts.insert(
                format!("{language}-{category}-sandboxed"),
                sha256(&machine.sandboxed),
            );
            parts.insert(
                format!("{language}-{category}-outside"),
                sha256(&machine.outside),
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
    Digests {
        vendor: cpu::vendor(&info).unwrap_or_else(|| "unknown".to_owned()),
        model: cpu::model(&info).unwrap_or_else(|| "unknown".to_owned()),
        platform: engine::platform_version(),
        parts,
    }
}

// verifies: REQ-MDL-2, THR-18
#[test]
fn a_derivation_records_its_cpu_and_one_digest_for_each_part_and_tells_a_changed_part() {
    let derived = derive();
    assert_eq!(derived.parts.len(), languages().len() * 4 * 2);
    assert_eq!(Digests::parse(&derived.render()), Ok(derived.clone()));
    assert!(derived.differences(&derived).is_empty());
    let mut changed = derived.clone();
    let part = changed.parts.keys().next().unwrap().clone();
    changed.parts.insert(part.clone(), "0".repeat(64));
    assert_eq!(derived.differences(&changed), vec![part]);
    if let Some(out) = std::env::var_os("RUTENT_REDERIVE_OUT") {
        std::fs::write(out, derived.render()).expect("the digest file is written");
    }
}

// verifies: REQ-MDL-2, THR-18
#[test]
fn re_derived_machine_code_matches_on_a_cpu_of_another_vendor() {
    let required = std::env::var_os("RUTENT_REDERIVE_REQUIRED").is_some();
    let Some(reference) = std::env::var_os("RUTENT_REDERIVE_REFERENCE") else {
        assert!(
            !required,
            "RUTENT_REDERIVE_REQUIRED is set, but there is no reference digest file"
        );
        eprintln!("RUTENT_REDERIVE_REFERENCE is not set: no re-derivation to check here");
        return;
    };
    let reference = Digests::read(Path::new(&reference)).expect("the reference digest file");
    let info = cpu::cpuinfo().expect("/proc/cpuinfo: the CPU of this machine can't be recorded");
    let vendor = cpu::vendor(&info).expect("/proc/cpuinfo names no vendor");
    eprintln!(
        "reference made on {} ({}); re-derived on {vendor} ({})",
        reference.vendor,
        reference.model,
        cpu::model(&info).unwrap_or_default()
    );
    if let Err(why) = cpu::crosses_vendors(&vendor, &reference.vendor) {
        panic!("this is not a re-derivation on another vendor: {why}");
    }
    let derived = derive();
    assert_eq!(derived.vendor, vendor);
    let different = derived.differences(&reference);
    assert!(
        different.is_empty(),
        "machine code differs from the reference on: {different:?}"
    );
    if let Some(out) = std::env::var_os("RUTENT_REDERIVE_OUT") {
        std::fs::write(out, derived.render()).expect("the digest file is written");
    }
}
