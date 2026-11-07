//! The passes `exp1-rt` runs, their environment, and the checks every VOS pass ends with.

use std::path::PathBuf;
use std::time::Duration;

use rutent_conformance::host::probe::Probe;
use rutent_conformance::host::{RunConfig, Verdict, run_with_fault_seam};
use rutent_contracts::state_machine::host::Host;
use rutent_stm::reference_host::ReferenceHarness;
use rutent_vos_rt::cluster::{ClusterConfig, NodeBuild, STARTED};
use rutent_vos_rt::host::{IMPORT_BYTES, IMPORT_ENTRIES, RtHarness, RtHost};
use rutent_vos_rt::platform::{self, Backend, Facts};

use crate::{pb, rows};

/// Every pass, in the order `../run-rt.sh` runs them.
pub const ALL: &[&str] = &[
    "platform",
    "smoke",
    "corpus",
    "suite-1",
    "suite-1-faults",
    "swap-in-1",
    "suite-3",
    "suite-3-faults",
    "swap-in-3",
    "r01",
    "r02",
    "r03",
    "r04a-1",
    "r04a-3",
    "r04b",
    "r05-1",
    "r05-1-fsyncs",
    "r05-3",
    "r05-3-fsyncs",
    "r06a-1",
    "r06a-3",
    "r06b-1",
    "r06b-3",
    "r07a",
    "r07b",
    "r08-1",
    "r08-3",
    "r09-1",
    "r09-3",
    "pb-r01",
    "pb-r03",
    "pb-r04a",
    "pb-r05",
    "pb-r05-fsyncs",
    "pb-r06a",
    "pb-r06b",
    "pb-r08",
    "pb-r09",
];

/// Where a pass finds its binaries, and how it runs.
pub struct Env {
    pub run_id: String,
    pub scored: PathBuf,
    pub faults: PathBuf,
    pub agent_elf: PathBuf,
    pub program_elf: PathBuf,
    pub next_elf: PathBuf,
    pub next_format_elf: PathBuf,
    /// A development run off the scoring platform: the interpreter, and nothing scored.
    pub dev: bool,
    pub facts: Facts,
}

fn var(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_default())
}

impl Env {
    pub fn read(run_id: &str) -> Env {
        Env {
            run_id: run_id.into(),
            scored: var("VOS_BIN"),
            faults: var("VOS_FAULTS_BIN"),
            agent_elf: var("VOS_AGENT_ELF"),
            program_elf: var("PROGRAM_ELF"),
            next_elf: var("PROGRAM_NEXT_ELF"),
            next_format_elf: var("PROGRAM_NEXT_FORMAT_ELF"),
            dev: std::env::var("EXP1_DEV").is_ok_and(|v| v == "1"),
            facts: Facts::read(),
        }
    }

    /// Fails closed unless this machine can run the JIT; a development run says so instead.
    pub fn check_platform(&self) -> Result<(), String> {
        if self.dev {
            return Ok(());
        }
        if self.run_id.contains("-dev-") {
            return Err("a development run id outside a development run".into());
        }
        self.facts.assert_jit()
    }

    /// The backend the nodes run: the JIT, unless this is a development run.
    pub fn backend(&self) -> Backend {
        if self.dev {
            Backend::Interpreter
        } else {
            Backend::Jit
        }
    }

    /// Each node's executors for a cluster of `replicas`: floor(vCPUs / 3), at least 1, on three nodes; VOS's own
    /// default on one (`node/host/src/node/config.rs:189`: the parallelism less one, at least 1).
    pub fn executors(&self, replicas: usize) -> usize {
        if replicas > 1 {
            (self.facts.vcpus / 3).max(1)
        } else {
            self.facts.vcpus.saturating_sub(1).max(1)
        }
    }

    /// A cluster's configuration: `replicas` nodes and `spares` more, on the scored build or the faults build.
    pub fn cluster(
        &self,
        replicas: usize,
        spares: usize,
        faults: bool,
        backend: Backend,
    ) -> ClusterConfig {
        ClusterConfig {
            build: NodeBuild {
                vos: if faults {
                    self.faults.clone()
                } else {
                    self.scored.clone()
                },
                faults,
            },
            agent_elf: self.agent_elf.clone(),
            program_elf: self.program_elf.clone(),
            replicas,
            spares,
            executors: self.executors(replicas),
            backend,
            settings: String::new(),
            wait: Duration::from_secs(60),
            strace: None,
        }
    }

    /// The run note's header for a pass: the platform facts and how its nodes ran.
    pub fn header(&self, pass: &str, config: Option<&ClusterConfig>) {
        println!("#### Pass `{pass}`, run {}\n", self.run_id);
        if self.dev {
            println!(
                "**A development run off the scoring platform: the interpreter, nothing scored.**\n"
            );
        }
        println!("| What | Value |\n|---|---|");
        print!("{}", self.facts.markdown());
        if let Some(c) = config {
            println!(
                "| Nodes | {} replicas, {} spares, each its own OS process on the runner host |",
                c.replicas, c.spares
            );
            println!(
                "| Node build | {} (`{}`) |",
                c.build.name(),
                c.build
                    .vos
                    .file_name()
                    .map_or(String::new(), |n| n.to_string_lossy().into_owned())
            );
            println!(
                "| Backend | `VOS_PVM_BACKEND={}`{} |",
                c.backend.env(),
                if c.backend == Backend::Jit {
                    ", `VOS_PVM_PERF_MAP=1`"
                } else {
                    ""
                }
            );
            println!("| Executors per node | {} |", c.executors);
            println!(
                "| Import page | {IMPORT_ENTRIES} entries or {IMPORT_BYTES} bytes, whichever comes first |"
            );
        }
        println!();
    }
}

/// Every node a pass started ran on the backend it was given: on the JIT each node's perf map is non-empty and no
/// log says the map is off; the nodes named must name the program. On the interpreter every map is absent or empty.
pub fn backend_proof(env: &Env, backend: Backend, named_by: &[u32]) -> bool {
    let started = std::mem::take(&mut *STARTED.lock().unwrap());
    let program = std::fs::read(&env.program_elf)
        .map_err(|e| e.to_string())
        .and_then(|elf| platform::perf_map_name(&elf));
    let program = match program {
        Ok(name) => name,
        Err(e) => {
            println!("The program's perf-map name could not be computed: {e}");
            return false;
        }
    };
    let mut ok = true;
    let mut empty = 0;
    let mut naming = 0;
    for (pid, log) in &started {
        let (lines, named) = platform::perf_map_lines(*pid, &program);
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if text.contains("the PVM's perf map is off") {
            println!("Node pid {pid}'s log says its perf map is off.");
            ok = false;
        }
        match backend {
            Backend::Jit => {
                if lines == 0 {
                    empty += 1;
                }
                if named > 0 {
                    naming += 1;
                }
                if named_by.contains(pid) && named == 0 {
                    println!(
                        "Node pid {pid} ran the program, and its perf map does not name `{program}`."
                    );
                    ok = false;
                }
            }
            Backend::Interpreter => {
                if lines > 0 {
                    println!(
                        "Node pid {pid} ran on the interpreter, and its perf map has {lines} lines."
                    );
                    ok = false;
                }
            }
        }
    }
    if backend == Backend::Jit && empty > 0 {
        println!(
            "{empty} of {} node processes left an empty or absent perf map.",
            started.len()
        );
        ok = false;
    }
    println!("\n| Backend proof | Value |\n|---|---|");
    println!("| Node processes started | {} |", started.len());
    match backend {
        Backend::Jit => {
            println!("| The program's name in a perf map | `{program}` |");
            println!(
                "| Processes whose perf map is non-empty | {} |",
                started.len() - empty
            );
            println!("| Processes whose perf map names the program | {naming} |");
            println!(
                "| Processes checked to name it (they ran it at the pass's end) | {} |",
                named_by.len()
            );
        }
        Backend::Interpreter => println!(
            "| Processes whose perf map is absent or empty | {} |",
            started.len()
        ),
    }
    println!(
        "| Verdict | {} |",
        if ok {
            "the backend is proved"
        } else {
            "**not proved: the pass fails**"
        }
    );
    ok
}

/// The suite on the VOS program, with every cell and the fault seam; swaps into the reference host.
fn suite(
    env: &Env,
    pass: &str,
    base: PathBuf,
    replicas: usize,
    faults: bool,
    into_vos: bool,
) -> bool {
    let config = env.cluster(replicas, 0, faults, env.backend());
    env.header(pass, Some(&config));
    let candidate = format!("vos-rt{replicas} ({} build)", config.build.name());
    let report = if into_vos {
        run_with_fault_seam(
            RunConfig {
                run_id: env.run_id.clone(),
                candidate: "reference-host".into(),
                target: candidate,
                seed: 20_261_106,
            },
            &mut ReferenceHarness::<Probe>::new(),
            &mut RtHarness::new(config.clone(), base.clone(), "t"),
        )
    } else {
        run_with_fault_seam(
            RunConfig {
                run_id: env.run_id.clone(),
                candidate,
                target: "reference-host".into(),
                seed: 20_261_106,
            },
            &mut RtHarness::new(config.clone(), base.clone(), "v"),
            &mut ReferenceHarness::<Probe>::new(),
        )
    };
    println!("{}", report.to_markdown());
    let named = rows::name_the_program_on_a_fresh_cluster(env, &config, &base.join("proof"));
    let proved = backend_proof(env, config.backend, &named);
    // A suite pass completes whatever its verdict: the verdict is the result, as the suite printed it.
    let _ = report.verdict() == Verdict::Pass;
    proved
}

/// The largest input fixture submitted on one node: does `submit` fit its gas and the 512 KiB handler heap.
fn smoke(env: &Env, base: PathBuf) -> bool {
    let config = env.cluster(1, 0, false, env.backend());
    env.header("smoke", Some(&config));
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../contracts/state-machine/fixtures/valid/recorded-input-route-chosen-three-parts.json"
    );
    let input: rutent_contracts::state_machine::RecordedInput =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture"))
            .expect("a recorded input");
    let mut harness = RtHarness::new(config.clone(), base.clone(), "s");
    let store = harness.fresh().expect("a store");
    let mut host = match RtHost::open(store) {
        Ok(host) => host,
        Err(e) => {
            println!("The host did not open: {e:?}");
            return false;
        }
    };
    let delivered = host.submit(input.clone());
    let again = host.submit(input.clone());
    let stages = host.cluster.stages(1).unwrap_or_default();
    let pid = host
        .cluster
        .pids()
        .into_iter()
        .map(|(_, p)| p)
        .collect::<Vec<_>>();
    println!("| What | Value |\n|---|---|");
    println!("| The largest input fixture | `recorded-input-route-chosen-three-parts.json` |");
    println!("| First submit | {} |", short(&delivered));
    println!("| The same input again | {} |", short(&again));
    for (name, count, total) in &stages {
        if ["refine_gas", "step_gas", "refine", "commit", "batch"].contains(&name.as_str())
            && *count > 0
        {
            println!(
                "| Stage `{name}`: values, mean | {count}, {} |",
                total / count
            );
        }
    }
    let ok = delivered.is_ok()
        && matches!(
            again,
            Ok(rutent_contracts::state_machine::host::Delivery::AlreadyRecorded(_))
        );
    drop(host);
    backend_proof(env, config.backend, &pid) && ok
}

pub fn short<T: std::fmt::Debug, E: std::fmt::Debug>(result: &Result<T, E>) -> String {
    let text = match result {
        Ok(v) => format!("{v:?}"),
        Err(e) => format!("error: {e:?}"),
    };
    let mut text: String = text.chars().take(160).collect();
    text = text.replace('|', "\\|");
    text
}

/// Runs one pass, printing its report. False when it could not complete.
pub fn run(env: &Env, pass: &str, base: PathBuf) -> bool {
    match pass {
        "platform" => {
            env.header("platform", None);
            match env.facts.assert_jit() {
                Ok(()) => println!(
                    "The platform assertion passed: Linux x86_64, kernel 6.7 or later, userfaultfd write-protect."
                ),
                Err(e) => println!("The platform assertion failed: {e}."),
            }
            true
        }
        "smoke" => smoke(env, base),
        "corpus" => rows::corpus(env, base),
        "suite-1" => suite(env, pass, base, 1, false, false),
        "suite-1-faults" => suite(env, pass, base, 1, true, false),
        "swap-in-1" => suite(env, pass, base, 1, false, true),
        "suite-3" => suite(env, pass, base, 3, false, false),
        "suite-3-faults" => suite(env, pass, base, 3, true, false),
        "swap-in-3" => suite(env, pass, base, 3, false, true),
        "r01" => rows::r01(env, base),
        "r02" => rows::r02(env, base),
        "r03" => rows::r03(env, base),
        "r04a-1" => rows::r04a(env, base, 1),
        "r04a-3" => rows::r04a(env, base, 3),
        "r04b" => rows::r04b(env, base),
        "r05-1" => rows::r05(env, base, 1, false),
        "r05-1-fsyncs" => rows::r05(env, base, 1, true),
        "r05-3" => rows::r05(env, base, 3, false),
        "r05-3-fsyncs" => rows::r05(env, base, 3, true),
        "r06a-1" => rows::r06a(env, base, 1),
        "r06a-3" => rows::r06a(env, base, 3),
        "r06b-1" => rows::r06b(env, base, 1),
        "r06b-3" => rows::r06b(env, base, 3),
        "r07a" => rows::r07a(env, base),
        "r07b" => rows::r07b(env, base),
        "r08-1" => rows::r08(env, base, 1),
        "r08-3" => rows::r08(env, base, 3),
        "r09-1" => rows::r09(env, base, 1),
        "r09-3" => rows::r09(env, base, 3),
        "pb-r01" => pb::r01(env, base),
        "pb-r03" => pb::r03(env, base),
        "pb-r04a" => pb::r04a(env, base),
        "pb-r05" => pb::r05(env, base, false),
        "pb-r05-fsyncs" => pb::r05(env, base, true),
        "pb-r06a" => pb::r06a(env, base),
        "pb-r06b" => pb::r06b(env, base),
        "pb-r08" => pb::r08(env, base),
        "pb-r09" => pb::r09(env, base),
        "pb-load" => pb::load_child(&base),
        "pb-submit-child" => pb::submit_child(&base),
        "list" => {
            println!("{}", ALL.join(" "));
            true
        }
        other => {
            println!("No pass {other}.");
            false
        }
    }
}
