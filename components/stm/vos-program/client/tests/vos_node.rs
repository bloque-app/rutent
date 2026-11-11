//! The VOS program on real nodes (`T-010-05`): the host-contract suite on one node and on three, the program's
//! states against the library's, and the leader killed mid-submit. These need the VOS node, the agent and the
//! program, built by `../build.sh`, and the paths of `../test.sh`; without the `vos` feature this file is empty.
//!
//! The suite is typed on its probe and compares states with it, so it scores the host's mechanics on the program
//! built with the probe (`PROBE_PROGRAM_ELF`). The program that ships runs `Stm` (`STM_PROGRAM_ELF`), and the other
//! tests run it: its states are `Stm`'s, and no acknowledged record is lost to a kill. The nodes are the faults
//! build, since the suite's stop after the program computed holds every node's commits.
//!
//! The nodes run the JIT on a Linux x86_64 machine that gives it, and the tests fail closed elsewhere. `VOS_DEV=1`
//! runs the interpreter instead, a development run whose result is not the task's.

#![cfg(feature = "vos")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rutent_conformance::host::probe::{self, Probe};
use rutent_conformance::host::{CellOutcome, Report, RunConfig, Verdict, run_with_fault_seam};
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_contracts::state_machine::host::{Delivery, Host, HostError};
use rutent_contracts::state_machine::{Id, Order, RecordedInput, StateMachine};
use rutent_stm::Stm;
use rutent_stm::reference_host::ReferenceHarness;
use rutent_stm_vos::cluster::{ClusterConfig, INSTANCE, NodeBuild};
use rutent_stm_vos::host::{self, VosHarness, VosHost};
use rutent_stm_vos::platform::{Backend, Facts, backend_proof};
use rutent_stm_vos_program::Submitted;

fn var(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(name).unwrap_or_else(|_| panic!("{name} names a binary: see test.sh")),
    )
}

fn development() -> bool {
    std::env::var("VOS_DEV").is_ok_and(|v| v == "1")
}

/// The configuration of a cluster of `replicas` nodes, after the platform assertion (fail closed).
fn config(replicas: usize, program: &str) -> ClusterConfig {
    let facts = Facts::read();
    let backend = if development() {
        Backend::Interpreter
    } else {
        facts.assert_jit().unwrap_or_else(|e| {
            panic!("the platform assertion failed, so the nodes did not run: {e}")
        });
        Backend::Jit
    };
    ClusterConfig {
        build: NodeBuild {
            vos: var("VOS_BIN"),
            faults: true,
        },
        agent_elf: var("VOS_AGENT_ELF"),
        program_elf: var(program),
        replicas,
        spares: 0,
        // As the re-score ran them: on three nodes floor(vCPUs / 3), at least 1; on one VOS's own default.
        executors: if replicas > 1 {
            (facts.vcpus / 3).max(1)
        } else {
            facts.vcpus.saturating_sub(1).max(1)
        },
        backend,
        settings: String::new(),
        wait: Duration::from_secs(60),
        strace: None,
    }
}

/// A directory for a test's clusters, removed with it.
struct Base(PathBuf);

impl Base {
    fn new(name: &str) -> Base {
        let dir = std::env::temp_dir().join(format!("rsv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Base(dir)
    }
}

impl Drop for Base {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assert_passes(report: &Report) {
    assert_passes_but(report, &[]);
    assert_eq!(report.verdict(), Verdict::Pass);
}

/// Every cell passes but the `excused` ones, which must be cells of the suite.
fn assert_passes_but(report: &Report, excused: &[&str]) {
    println!("{}", report.to_markdown());
    for id in excused {
        assert!(
            report.cells.iter().any(|(c, _)| c.id == *id),
            "{id} is not a cell of the suite"
        );
    }
    let failed: Vec<String> = report
        .cells
        .iter()
        .filter(|(c, o)| !matches!(o, CellOutcome::Pass) && !excused.contains(&c.id))
        .map(|(c, o)| format!("{}: {o:?}", c.id))
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The nodes this test started ran the backend asked for: the JIT's perf map names the program, or the interpreter's
/// is empty.
fn prove_backend(program: &str) {
    let backend = if development() {
        Backend::Interpreter
    } else {
        Backend::Jit
    };
    backend_proof(backend, &var(program), &[]).unwrap_or_else(|e| panic!("the backend proof: {e}"));
}

/// The two cells that compare the states a host recorded with the probe's, over shuffled inputs. `Stm` rightly
/// differs from the probe there (forward-only segments), so a host running `Stm` is excused them; the states test
/// scores it against `Stm` itself.
const STATES_OF_THE_PROBE: [&str; 2] = [
    "reordered-delivery-is-applied-in-submit-order",
    "replay-reaches-the-same-states",
];

fn suite_on(replicas: usize, name: &str, program: &str, excused: &[&str]) {
    let base = Base::new(name);
    let report = run_with_fault_seam(
        RunConfig {
            run_id: std::env::var("RUN_ID").unwrap_or_else(|_| format!("T-010-05-{name}")),
            candidate: format!("vos-stm-{replicas}"),
            target: "reference-host".into(),
            seed: 20_261_106,
        },
        &mut VosHarness::<Probe>::new(config(replicas, program), base.0.clone(), "v"),
        &mut ReferenceHarness::<Probe>::new(),
    );
    if excused.is_empty() {
        assert_passes(&report);
    } else {
        assert_passes_but(&report, excused);
    }
    prove_backend(program);
}

// verifies: REQ-OPS-3, NFR-6, DEC-26
#[test]
fn the_host_contract_suite_passes_on_one_vos_node() {
    suite_on(1, "one", "PROBE_PROGRAM_ELF", &[]);
}

// verifies: REQ-OPS-3, NFR-6, DEC-26
#[test]
fn the_host_contract_suite_passes_on_three_vos_nodes() {
    suite_on(3, "three", "PROBE_PROGRAM_ELF", &[]);
}

// verifies: REQ-OPS-3, NFR-6, DEC-26
#[test]
fn the_host_contract_suite_passes_on_one_vos_node_running_the_state_machine_library() {
    suite_on(1, "o-stm", "STM_PROGRAM_ELF", &STATES_OF_THE_PROBE);
}

// verifies: REQ-OPS-3, NFR-6, DEC-26
#[test]
fn the_host_contract_suite_passes_on_three_vos_nodes_running_the_state_machine_library() {
    suite_on(3, "t-stm", "STM_PROGRAM_ELF", &STATES_OF_THE_PROBE);
}

/// The states `Stm` computes over `inputs`, one per input, the way a host folds them over one order.
fn fold_stm(inputs: &[RecordedInput]) -> Vec<Option<Order>> {
    let mut state: Option<Order> = None;
    inputs
        .iter()
        .map(|input| {
            state = match &state {
                Some(order) => Some(Stm::transition(order, input)),
                None => Stm::open(input),
            };
            state.clone()
        })
        .collect()
}

// verifies: REQ-OPS-3, REQ-STM-4, DEC-26
#[test]
fn the_program_records_the_states_the_state_machine_library_computes() {
    states_on(1);
}

// verifies: REQ-OPS-3, REQ-STM-4, DEC-26
#[test]
fn three_vos_nodes_record_the_states_the_state_machine_library_computes() {
    states_on(3);
}

fn states_on(replicas: usize) {
    let base = Base::new(&format!("states-{replicas}"));
    let mut harness =
        VosHarness::<Stm>::new(config(replicas, "STM_PROGRAM_ELF"), base.0.clone(), "s");
    let store = harness.fresh_store().unwrap();
    let mut host: VosHost = <VosHarness<Stm> as Harness<Stm>>::start(&mut harness, store).unwrap();
    // An order to completion, one whose final deposit fails, and the first order's inputs out of their order, where
    // `Stm` refuses what the probe takes.
    let mut shuffled = probe::order_inputs("ord-c", 300);
    probe::shuffle(&mut shuffled, 20_261_106);
    for (order, inputs) in [
        ("ord-a", probe::order_inputs("ord-a", 0)),
        (
            "ord-b",
            probe::order_inputs_with_a_failed_final_deposit("ord-b", 100),
        ),
        ("ord-c", shuffled),
    ] {
        for input in &inputs {
            let delivered = Host::<Stm>::submit(&mut host, input.clone());
            assert!(
                matches!(delivered, Ok(Delivery::Recorded(_))),
                "{order}: {delivered:?}"
            );
        }
        let journal = Host::<Stm>::journal(&host, &Id::new(order).unwrap()).unwrap();
        let recorded: Vec<Option<Order>> = journal.iter().map(|r| r.state_after.clone()).collect();
        assert_eq!(
            recorded,
            fold_stm(&inputs),
            "{order}: the program's states are not Stm's"
        );
        let last = Host::<Stm>::state(&host, &Id::new(order).unwrap()).unwrap();
        assert_eq!(
            last,
            recorded.last().cloned().flatten(),
            "{order}: the state it serves"
        );
    }
    prove_backend("STM_PROGRAM_ELF");
}

/// What was acknowledged: record id to its order and sequence number.
type Acknowledged = BTreeMap<String, (Id, u64)>;

fn acknowledge(acknowledged: &mut Acknowledged, delivered: &Result<Delivery, HostError>) {
    if let Ok(Delivery::Recorded(r) | Delivery::AlreadyRecorded(r)) = delivered {
        acknowledged.insert(
            r.input.record_id.as_str().into(),
            (r.order_id.clone(), r.seq),
        );
    }
}

/// One submit through `client`, as the host's submit does it.
fn submit_via(
    client: &mut vos_node::client::Client,
    host: &VosHost,
    input: &RecordedInput,
) -> Result<Delivery, HostError> {
    let arguments = host::submit_arguments(input)?;
    let answer: Submitted =
        host::answer(client.call(host.agent, INSTANCE, host.methods.submit, &arguments))?;
    host::delivery(input.clone(), answer)
}

// verifies: NFR-6, INV-7
#[test]
fn killing_the_leader_mid_submit_loses_no_acknowledged_record() {
    let base = Base::new("leader");
    let mut harness = VosHarness::<Stm>::new(config(3, "STM_PROGRAM_ELF"), base.0.clone(), "k");
    let store = harness.fresh_store().unwrap();
    let mut host: VosHost = <VosHarness<Stm> as Harness<Stm>>::start(&mut harness, store).unwrap();
    let mut acknowledged = Acknowledged::new();

    // Some records first, so there is something durable before any kill.
    for input in probe::order_inputs("ord-warm", 0) {
        let delivered = Host::<Stm>::submit(&mut host, input);
        acknowledge(&mut acknowledged, &delivered);
    }
    assert!(!acknowledged.is_empty(), "the warm-up recorded nothing");

    let mut killed_mid_submit = 0;
    for round in 0u32..3 {
        let leader = host.cluster.leader().expect("a leader");
        let inputs: Vec<RecordedInput> =
            probe::order_inputs(&format!("ord-r{round}"), 1_000 * (round + 1));
        // The load: submits in order from a thread, recording what was acknowledged, until one fails.
        let sent = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let acked = Arc::new(Mutex::new(Acknowledged::new()));
        // The input being submitted: set before the call and cleared after it, so the kill can be seen to land in
        // one.
        let in_flight: Arc<Mutex<Option<RecordedInput>>> = Arc::new(Mutex::new(None));
        let mut client = host.cluster.client_with([21 + round as u8; 32], leader);
        let load = {
            let (sent, stop, acked, in_flight) =
                (sent.clone(), stop.clone(), acked.clone(), in_flight.clone());
            let inputs = inputs.clone();
            let (agent, submit) = (host.agent, host.methods.submit);
            std::thread::spawn(move || {
                let mut left = Vec::new();
                for input in inputs {
                    if !left.is_empty() || stop.load(Ordering::SeqCst) {
                        left.push(input);
                        continue;
                    }
                    *in_flight.lock().unwrap() = Some(input.clone());
                    sent.fetch_add(1, Ordering::SeqCst);
                    let delivered = host::submit_arguments(&input).and_then(|arguments| {
                        let answer: Submitted =
                            host::answer(client.call(agent, INSTANCE, submit, &arguments))?;
                        host::delivery(input.clone(), answer)
                    });
                    *in_flight.lock().unwrap() = None;
                    match delivered {
                        Ok(delivered) => acknowledge(&mut acked.lock().unwrap(), &Ok(delivered)),
                        // Unknown outcome: it goes again after the kill, and the host's dedup makes that safe.
                        Err(_) => left.push(input),
                    }
                }
                left
            })
        };
        // The kill lands once the load is part-way through its order, and in a submit that has not been answered.
        let until = Instant::now() + Duration::from_secs(30);
        while !(sent.load(Ordering::SeqCst) >= 2 + u64::from(round)
            && in_flight.lock().unwrap().is_some())
            && Instant::now() < until
        {
            std::thread::sleep(Duration::from_micros(100));
        }
        let mid_submit = in_flight.lock().unwrap().is_some();
        host.cluster.kill(leader);
        killed_mid_submit += u32::from(mid_submit);
        stop.store(true, Ordering::SeqCst);
        let left = load.join().expect("the load");
        acknowledged.extend(acked.lock().unwrap().clone());

        // The two nodes left elect a leader; what the load did not get acknowledged goes again, in order.
        let survivor = (1..=3).find(|n| *n != leader).unwrap();
        host.cluster
            .leader()
            .expect("a new leader among the survivors");
        let mut resubmitting = host.cluster.client_with([61; 32], survivor);
        for input in left {
            let delivered = submit_via(&mut resubmitting, &host, &input);
            assert!(delivered.is_ok(), "{input:?} after the kill: {delivered:?}");
            acknowledge(&mut acknowledged, &delivered);
        }
        host.cluster
            .start(leader)
            .expect("the killed node starts again");

        // Every acknowledged record is in the journal a surviving node serves, once.
        host.client = host.cluster.client_with([71; 32], survivor);
        for (record_id, (order, seq)) in &acknowledged {
            let journal = Host::<Stm>::journal(&host, order).unwrap();
            let held: Vec<_> = journal
                .iter()
                .filter(|r| r.input.record_id.as_str() == record_id)
                .collect();
            assert_eq!(
                held.len(),
                1,
                "round {round}: {record_id} is held {} times",
                held.len()
            );
            assert_eq!(held[0].seq, *seq, "round {round}: {record_id} moved");
        }
    }
    // The kill is meant to land in a submit that was not answered; the interpreter is slow enough that it does, and
    // the JIT may answer one now and then before the kill, so one round is what is required.
    println!("killed mid-submit in {killed_mid_submit} of 3 rounds");
    assert!(
        killed_mid_submit >= 1,
        "no kill landed in a submit that was in flight"
    );
    prove_backend("STM_PROGRAM_ELF");
}

// verifies: REQ-OPS-3
#[test]
fn a_vos_node_on_the_jit_names_the_program_in_its_perf_map() {
    // What `../host-check.sh` runs on a host: one node, one record, and the backend's proof (`T-010-08`, `G-37`).
    let base = Base::new("perf");
    let mut harness = VosHarness::<Stm>::new(config(1, "STM_PROGRAM_ELF"), base.0.clone(), "p");
    let store = harness.fresh_store().unwrap();
    let mut host: VosHost = <VosHarness<Stm> as Harness<Stm>>::start(&mut harness, store).unwrap();
    for input in probe::order_inputs("ord-perf", 0).into_iter().take(1) {
        let delivered = Host::<Stm>::submit(&mut host, input);
        assert!(
            matches!(delivered, Ok(Delivery::Recorded(_))),
            "{delivered:?}"
        );
    }
    Host::<Stm>::journal(&host, &Id::new("ord-perf").unwrap()).unwrap();
    prove_backend("STM_PROGRAM_ELF");
}

// The recovery drill for total loss (`T-010-08`, `R-08`): the scheduled halted export, imported onto nodes that have
// none of the lost cluster's data.
// verifies: NFR-6, INV-7
#[test]
fn halted_export_imported_onto_fresh_nodes_keeps_every_record_acknowledged_before_it() {
    let base = Base::new("drill");
    let mut harness = VosHarness::<Stm>::new(config(3, "STM_PROGRAM_ELF"), base.0.clone(), "d");
    let store = harness.fresh_store().unwrap();
    let mut host: VosHost = <VosHarness<Stm> as Harness<Stm>>::start(&mut harness, store).unwrap();

    // Records acknowledged before the export: an order to completion and one whose final deposit fails.
    let mut acknowledged = Acknowledged::new();
    let orders = [
        ("ord-a", probe::order_inputs("ord-a", 0)),
        (
            "ord-b",
            probe::order_inputs_with_a_failed_final_deposit("ord-b", 100),
        ),
    ];
    for (_, inputs) in &orders {
        for input in inputs {
            let delivered = Host::<Stm>::submit(&mut host, input.clone());
            assert!(
                matches!(delivered, Ok(Delivery::Recorded(_))),
                "{delivered:?}"
            );
            acknowledge(&mut acknowledged, &delivered);
        }
    }
    assert!(!acknowledged.is_empty());
    let before: Vec<_> = orders
        .iter()
        .map(|(order, _)| Host::<Stm>::journal(&host, &Id::new(*order).unwrap()).unwrap())
        .collect();

    // The scheduled export: halt, read the stream out whole.
    Host::<Stm>::halt(&mut host).unwrap();
    let entries: Vec<_> = Host::<Stm>::export(&host)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    // Total loss: every node of the cluster is gone, and its data with it.
    let lost = <VosHarness<Stm> as Harness<Stm>>::kill(&mut harness, host).unwrap();
    std::fs::remove_dir_all(&lost.dir).unwrap();
    assert!(!lost.dir.exists());

    // Fresh nodes, empty, take the import.
    let fresh = harness.fresh_store().unwrap();
    assert_ne!(fresh.dir, lost.dir);
    let imported: VosHost =
        <VosHost as Host<Stm>>::import(fresh, Box::new(entries.into_iter().map(Ok))).unwrap();
    for (record_id, (order, seq)) in &acknowledged {
        let journal = Host::<Stm>::journal(&imported, order).unwrap();
        let held: Vec<_> = journal
            .iter()
            .filter(|r| r.input.record_id.as_str() == record_id)
            .collect();
        assert_eq!(held.len(), 1, "{record_id} is held {} times", held.len());
        assert_eq!(held[0].seq, *seq, "{record_id} moved");
    }
    for ((order, _), journal) in orders.iter().zip(&before) {
        let after = Host::<Stm>::journal(&imported, &Id::new(*order).unwrap()).unwrap();
        assert_eq!(&after, journal, "{order}: the journal is not what it was");
    }
    prove_backend("STM_PROGRAM_ELF");
}
