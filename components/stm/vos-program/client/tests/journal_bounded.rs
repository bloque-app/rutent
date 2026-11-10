//! The VOS program keeps its live journal bounded (`T-010-07`, `G-33`): an order that ended is packed into one
//! archive row, and stays readable and replayable from it. These need the VOS node, the agent and the program,
//! built by `../build.sh`, and the paths of `../test.sh`; without the `vos` feature this file is empty. Both tests
//! run on three nodes, as the task's `Done when` has it.

#![cfg(feature = "vos")]

use std::path::PathBuf;
use std::time::Duration;

use rutent_conformance::host::probe;
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_contracts::state_machine::host::{Delivery, Host};
use rutent_contracts::state_machine::{Id, Order, RecordId, RecordedInput};
use rutent_stm::Stm;
use rutent_stm::archive::{self, unpack};
use rutent_stm_vos::cluster::{ClusterConfig, NodeBuild};
use rutent_stm_vos::host::{VosHarness, VosHost};
use rutent_stm_vos::platform::{Backend, Facts, backend_proof};
use vos_wire::Encode;

fn var(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(name).unwrap_or_else(|_| panic!("{name} names a binary: see test.sh")),
    )
}

fn development() -> bool {
    std::env::var("VOS_DEV").is_ok_and(|v| v == "1")
}

/// Three nodes running the program that ships, after the platform assertion (fail closed).
fn config() -> ClusterConfig {
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
        program_elf: var("STM_PROGRAM_ELF"),
        replicas: 3,
        spares: 0,
        executors: (facts.vcpus / 3).max(1),
        backend,
        settings: String::new(),
        wait: Duration::from_secs(60),
        strace: None,
    }
}

/// A directory for a test's cluster, removed with it.
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

fn start(name: &str) -> (Base, VosHost) {
    let base = Base::new(name);
    let mut harness = VosHarness::<Stm>::new(config(), base.0.clone(), "j");
    let store = harness.fresh_store().unwrap();
    let host = <VosHarness<Stm> as Harness<Stm>>::start(&mut harness, store).unwrap();
    (base, host)
}

fn prove_backend() {
    let backend = if development() {
        Backend::Interpreter
    } else {
        Backend::Jit
    };
    backend_proof(backend, &var("STM_PROGRAM_ELF"), &[])
        .unwrap_or_else(|e| panic!("the backend proof: {e}"));
}

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn submit_all(host: &mut VosHost, inputs: &[RecordedInput]) {
    for input in inputs {
        let delivered = Host::<Stm>::submit(host, input.clone());
        assert!(
            matches!(delivered, Ok(Delivery::Recorded(_))),
            "{:?}: {delivered:?}",
            input.record_id
        );
    }
}

/// The orders of the stream: each takes the seven inputs from its route to complete.
const COMPLETED: u32 = 30;
const INPUTS_OF_AN_ORDER: u64 = 7;

// verifies: REQ-OPS-3
#[test]
fn live_journal_stays_bounded_under_a_stream_of_completed_orders() {
    let (_base, mut host) = start("bounded");
    // One order that stays open: its records stay live however many others complete.
    let open = probe::order_inputs("ord-open", 50_000);
    submit_all(&mut host, &open[..2]);
    let mut most_live = 0;
    for n in 0..COMPLETED {
        let inputs = probe::order_inputs(&format!("ord-done-{n}"), 1_000 * (n + 1));
        submit_all(&mut host, &inputs);
        let status = host.status().unwrap();
        most_live = most_live.max(status.live_records);
        assert_eq!(
            status.live_records, 2,
            "after {} orders completed only the open order's records are live",
            n + 1
        );
        assert_eq!(status.archived_orders, u64::from(n) + 1);
        assert_eq!(
            status.records,
            2 + (u64::from(n) + 1) * INPUTS_OF_AN_ORDER,
            "no record is lost to archiving"
        );
    }
    // The bound does not move with the stream: it is the open orders' records, not the completed orders'.
    assert!(most_live <= 2 + INPUTS_OF_AN_ORDER);
    // An order that is still open keeps taking inputs, and completes into the archive too.
    submit_all(&mut host, &open[2..]);
    let status = host.status().unwrap();
    assert_eq!(status.live_records, 0);
    assert_eq!(status.archived_orders, u64::from(COMPLETED) + 1);
    prove_backend();
}

// verifies: NFR-4, REQ-STM-3
#[test]
fn archived_order_replays_to_the_same_states() {
    let (_base, mut host) = start("replays");
    let inputs = probe::order_inputs("ord-a", 0);
    submit_all(&mut host, &inputs);
    let order = id("ord-a");
    let before = Host::<Stm>::journal(&host, &order).unwrap();
    assert_eq!(before.len(), inputs.len());
    assert_eq!(host.status().unwrap().archived_orders, 1, "it was archived");
    assert_eq!(host.status().unwrap().live_records, 0);

    // A late input to the archived order is recorded, into its archive.
    let mut late = probe::order_inputs("ord-a", 100)[1].clone();
    late.record_id = RecordId::new("late-1").unwrap();
    let delivered = Host::<Stm>::submit(&mut host, late.clone());
    assert!(
        matches!(delivered, Ok(Delivery::Recorded(_))),
        "{delivered:?}"
    );
    let again = Host::<Stm>::submit(&mut host, late.clone());
    assert!(
        matches!(again, Ok(Delivery::AlreadyRecorded(_))),
        "{again:?}"
    );

    // The journal is whole, read through the host's own reads.
    let journal = Host::<Stm>::journal(&host, &order).unwrap();
    assert_eq!(journal.len(), inputs.len() + 1);
    assert_eq!(&journal[..before.len()], &before[..], "no record moved");
    assert_eq!(host.status().unwrap().live_records, 0);

    // The archive itself replays to the states the journal recorded.
    let packed: Option<Vec<u8>> = host
        .query(
            host.methods.archived,
            &"ord-a".as_bytes().to_vec().encode(),
        )
        .unwrap();
    let entries = unpack(&packed.expect("the order is archived")).expect("the archive unpacks");
    assert_eq!(entries.len(), journal.len());
    let replayed_inputs: Vec<RecordedInput> = entries
        .iter()
        .map(|e| serde_json::from_slice(&e.input).unwrap())
        .collect();
    let recorded: Vec<Option<Order>> = journal.iter().map(|r| r.state_after.clone()).collect();
    assert_eq!(archive::replay(&replayed_inputs), recorded);
    let archived_states: Vec<Option<Order>> = entries
        .iter()
        .map(|e| e.state.as_ref().map(|s| serde_json::from_slice(s).unwrap()))
        .collect();
    assert_eq!(archived_states, recorded);
    assert_eq!(
        Host::<Stm>::state(&host, &order).unwrap(),
        recorded.last().cloned().flatten()
    );
    prove_backend();
}
