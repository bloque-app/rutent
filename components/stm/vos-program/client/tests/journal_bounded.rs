//! The VOS program keeps its live journal bounded (`T-010-07`, `G-33`): an order that ended is packed into one
//! archive row, and stays readable and replayable from it. These need the VOS node, the agent and the program,
//! built by `../build.sh`, and the paths of `../test.sh`; without the `vos` feature this file is empty. Both tests
//! run on three nodes, as the task's `Done when` has it, and again on one.

#![cfg(feature = "vos")]

use std::path::PathBuf;
use std::time::Duration;

use rutent_conformance::host::probe;
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_contracts::state_machine::host::{Delivery, Host};
use rutent_contracts::state_machine::{Id, Order, RecordId, RecordedInput};
use rutent_stm::Stm;
use rutent_stm::archive::{CHUNK_BYTES, Entry, replay, unpack};
use rutent_stm_vos::cluster::{ClusterConfig, NodeBuild};
use rutent_stm_vos::host::{VosHarness, VosHost};
use rutent_stm_vos::platform::{Backend, Facts, backend_proof};
use rutent_stm_vos_program::Rutent;
use vos_wire::{Decode, Encode};

fn var(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(name).unwrap_or_else(|_| panic!("{name} names a binary: see test.sh")),
    )
}

fn development() -> bool {
    std::env::var("VOS_DEV").is_ok_and(|v| v == "1")
}

/// `replicas` nodes running the program that ships, after the platform assertion (fail closed).
fn config(replicas: usize) -> ClusterConfig {
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
        replicas,
        spares: 0,
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

fn start(name: &str, replicas: usize) -> (Base, VosHost) {
    let base = Base::new(name);
    let mut harness = VosHarness::<Stm>::new(config(replicas), base.0.clone(), "j");
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

fn bounded_on(replicas: usize) {
    let (_base, mut host) = start(&format!("bounded-{replicas}"), replicas);
    // One order that stays open: its records stay live however many others complete.
    let open = probe::order_inputs("ord-open", 50_000);
    submit_all(&mut host, &open[..2]);
    for n in 0..COMPLETED {
        let inputs = probe::order_inputs(&format!("ord-done-{n}"), 1_000 * (n + 1));
        submit_all(&mut host, &inputs);
        let status = host.status().unwrap();
        // The bound does not move with the stream: it is the open order's records, not the completed orders'.
        assert_eq!(
            status.live_records,
            2,
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
    // An order that is still open keeps taking inputs, and completes into the archive too.
    submit_all(&mut host, &open[2..]);
    let status = host.status().unwrap();
    assert_eq!(status.live_records, 0);
    assert_eq!(status.archived_orders, u64::from(COMPLETED) + 1);
    prove_backend();
}

// verifies: REQ-OPS-3
#[test]
fn live_journal_stays_bounded_under_a_stream_of_completed_orders() {
    bounded_on(3);
}

// verifies: REQ-OPS-3
#[test]
fn live_journal_stays_bounded_under_a_stream_of_completed_orders_on_one_node() {
    bounded_on(1);
}

/// The archived rows of `order`, read one at a time as the program serves them.
fn archived_entries(host: &VosHost, order: &str) -> Vec<Vec<Entry>> {
    let mut rows = Vec::new();
    loop {
        let packed: Option<Vec<u8>> = host
            .query(
                host.methods.archived,
                &[
                    order.as_bytes().to_vec().encode(),
                    (rows.len() as u64).encode(),
                ]
                .concat(),
            )
            .unwrap();
        let Some(packed) = packed else { break };
        assert!(packed.len() <= 64 << 10, "a row is under the stored cap");
        rows.push(unpack(&packed).expect("the archive unpacks"));
    }
    rows
}

/// Checks that `order`'s archive replays to the states its journal recorded, and returns the number of rows.
fn assert_replays(host: &VosHost, order: &str, expected_len: usize) -> usize {
    let journal = Host::<Stm>::journal(host, &id(order)).unwrap();
    assert_eq!(journal.len(), expected_len);
    let rows = archived_entries(host, order);
    let entries: Vec<Entry> = rows.iter().flatten().cloned().collect();
    assert_eq!(entries.len(), journal.len());
    let inputs: Vec<RecordedInput> = entries
        .iter()
        .map(|e| serde_json::from_slice(&e.input).unwrap())
        .collect();
    let recorded: Vec<Option<Order>> = journal.iter().map(|r| r.state_after.clone()).collect();
    assert_eq!(replay(&inputs), recorded);
    let archived: Vec<Option<Order>> = entries
        .iter()
        .map(|e| e.state.as_ref().map(|s| serde_json::from_slice(s).unwrap()))
        .collect();
    assert_eq!(archived, recorded);
    assert_eq!(
        Host::<Stm>::state(host, &id(order)).unwrap(),
        recorded.last().cloned().flatten()
    );
    rows.len()
}

fn replays_on(replicas: usize) {
    let (_base, mut host) = start(&format!("replays-{replicas}"), replicas);
    let inputs = probe::order_inputs("ord-a", 0);
    let order = id("ord-a");
    // Six inputs leave the order open, so its journal is live; the seventh ends it into the archive.
    submit_all(&mut host, &inputs[..6]);
    let before = Host::<Stm>::journal(&host, &order).unwrap();
    // Some of them may already be sealed into an archive row, if they filled one.
    assert!(host.status().unwrap().live_records <= 6);
    submit_all(&mut host, &inputs[6..]);
    assert_eq!(host.status().unwrap().archived_orders, 1, "it was archived");
    assert_eq!(host.status().unwrap().live_records, 0);
    let journal = Host::<Stm>::journal(&host, &order).unwrap();
    assert_eq!(&journal[..before.len()], &before[..], "no record moved");

    // A late input to the archived order is recorded, into its archive.
    let mut late = probe::order_inputs("ord-a", 100)[1].clone();
    late.record_id = RecordId::new("late-1").unwrap();
    let delivered = Host::<Stm>::submit(&mut host, late.clone());
    assert!(
        matches!(delivered, Ok(Delivery::Recorded(_))),
        "{delivered:?}"
    );
    let again = Host::<Stm>::submit(&mut host, late);
    assert!(
        matches!(again, Ok(Delivery::AlreadyRecorded(_))),
        "{again:?}"
    );
    assert_eq!(host.status().unwrap().live_records, 0);
    assert_replays(&host, "ord-a", inputs.len() + 1);
    prove_backend();
}

// verifies: NFR-4, REQ-STM-3
#[test]
fn archived_order_replays_to_the_same_states() {
    replays_on(3);
}

// verifies: NFR-4, REQ-STM-3
#[test]
fn archived_order_replays_to_the_same_states_on_one_node() {
    replays_on(1);
}

// verifies: REQ-OPS-3, NFR-4
#[test]
fn an_order_whose_journal_passes_a_steps_write_cap_still_ends_and_replays() {
    // VOS caps a step's writes at 256 KiB. Two hundred and sixty records are over a third of a megabyte,
    // so an ending that wrote the whole journal in its step could not be recorded; sealing as records arrive does.
    let (_base, mut host) = start("steps", 3);
    let inputs = probe::order_inputs("ord-huge", 0);
    let again = |n: u32| {
        let mut input = inputs[1].clone();
        input.record_id = RecordId::new(format!("again-{n}")).unwrap();
        input
    };
    submit_all(&mut host, &inputs[..2]);
    // About 1.3 KB a record in the journal, so two hundred and sixty pass 256 KiB with room to spare.
    let filler: Vec<RecordedInput> = (0..260).map(again).collect();
    submit_all(&mut host, &filler);
    let journal = Host::<Stm>::journal(&host, &id("ord-huge")).unwrap();
    let weight: usize = journal
        .iter()
        .map(|r| {
            serde_json::to_vec(&r.input).unwrap().len()
                + r.state_after
                    .as_ref()
                    .map_or(0, |s| serde_json::to_vec(s).unwrap().len())
        })
        .sum();
    assert!(weight > 256 << 10, "the journal weighs {weight} bytes");
    assert!(
        host.status().unwrap().live_records < 20,
        "records were sealed while the order was open"
    );
    submit_all(&mut host, &inputs[2..]);
    assert_eq!(host.status().unwrap().archived_orders, 1);
    assert_eq!(host.status().unwrap().live_records, 0);
    let rows = assert_replays(&host, "ord-huge", inputs.len() + filler.len());
    assert!(rows >= 6, "{rows} archive rows");
    prove_backend();
}

/// Many inputs that change nothing but are recorded, with a state each, before and after the order ends.
fn long_order_on(replicas: usize) {
    let (_base, mut host) = start(&format!("long-{replicas}"), replicas);
    let inputs = probe::order_inputs("ord-long", 0);
    let again = |n: u32| {
        let mut input = inputs[1].clone();
        input.record_id = RecordId::new(format!("again-{n}")).unwrap();
        input
    };
    // The order is open for forty records, each with a state of a few KB, so its journal is well over the 64 KiB a
    // stored row holds when it ends: ending it must write more than one row.
    submit_all(&mut host, &inputs[..2]);
    let filler: Vec<RecordedInput> = (0..40).map(again).collect();
    submit_all(&mut host, &filler);
    submit_all(&mut host, &inputs[2..]);
    let status = host.status().unwrap();
    assert_eq!(status.archived_orders, 1, "the order ended");
    assert_eq!(status.live_records, 0);
    let rows = assert_replays(&host, "ord-long", inputs.len() + filler.len());
    assert!(
        rows >= 2,
        "{rows} archive row(s) for a journal over the stored cap (chunks of {CHUNK_BYTES} bytes)"
    );
    // And it still takes inputs once archived, into the last row or a new one.
    let late: Vec<RecordedInput> = (40..60).map(again).collect();
    submit_all(&mut host, &late);
    assert_replays(&host, "ord-long", inputs.len() + filler.len() + late.len());
    prove_backend();
}

// verifies: REQ-OPS-3, NFR-4
#[test]
fn an_order_with_a_long_journal_ends_into_several_archive_rows_and_replays() {
    long_order_on(3);
}

// verifies: REQ-OPS-3, NFR-4
#[test]
fn an_order_with_a_long_journal_ends_into_several_archive_rows_and_replays_on_one_node() {
    long_order_on(1);
}

// verifies: REQ-OPS-3, DEC-26
#[test]
fn the_struct_of_a_format_one_instance_still_decodes_so_the_format_check_can_refuse_it() {
    // Format 1's struct row, field by field, as `T-010-05` wrote it: the format, then the flags and counts. A
    // program whose struct had more fields could not decode this and would trap in `state::load`, before any
    // handler could refuse the format.
    let v1 = [
        1u32.encode(),
        false.encode(),
        false.encode(),
        0u64.encode(),
        0u64.encode(),
        0u64.encode(),
        0u64.encode(),
    ]
    .concat();
    let (state, used) = Rutent::decode(&v1).expect("a format-1 struct row decodes");
    assert_eq!(used, v1.len());
    assert_eq!(state.stored_format(), 1, "it is a format-1 instance");
    assert_ne!(state.stored_format(), rutent_stm_vos_program::FORMAT);
}

// verifies: REQ-OPS-3, NFR-6
#[test]
fn an_export_of_orders_that_each_seal_in_a_row_imports_in_steps_under_the_write_cap() {
    // Orders that sit just under a row's worth of live records, each sealed by its next record, one after another in
    // the export: taken eight to a step they would write over VOS's 256 KiB, so the program takes fewer and the
    // client resumes where it stopped.
    let base = Base::new("import-source");
    let mut harness = VosHarness::<Stm>::new(config(1), base.0.clone(), "i");
    let store = harness.fresh_store().unwrap();
    let mut host: VosHost = <VosHarness<Stm> as Harness<Stm>>::start(&mut harness, store).unwrap();
    const ORDERS: usize = 14;
    let name = |n: usize| format!("ord-{n:02}");
    let again = |order: &str, n: u32| {
        let mut input = probe::order_inputs(order, 0)[1].clone();
        input.record_id = RecordId::new(format!("again-{order}-{n}")).unwrap();
        input
    };
    for n in 0..ORDERS {
        let order = name(n);
        submit_all(&mut host, &probe::order_inputs(&order, 0)[..2]);
    }
    // Round after round, one more record to each order, until the first one's records are sealed: then every order
    // is one record short of sealing, in the same shape.
    let mut round = 0u32;
    let sealed_at = loop {
        let before = host.status().unwrap().live_records;
        submit_all(&mut host, &[again(&name(0), round)]);
        if host.status().unwrap().live_records < before {
            break round;
        }
        for n in 1..ORDERS {
            submit_all(&mut host, &[again(&name(n), round)]);
        }
        round += 1;
        assert!(round < 200, "the first order never sealed");
    };
    // The others now each take the record that seals them, one after another.
    for n in 1..ORDERS {
        submit_all(&mut host, &[again(&name(n), sealed_at)]);
    }
    let status = host.status().unwrap();
    Host::<Stm>::halt(&mut host).unwrap();
    let entries: Vec<_> = Host::<Stm>::export(&host)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(host);

    let base2 = Base::new("import-target");
    let mut harness2 = VosHarness::<Stm>::new(config(1), base2.0.clone(), "j");
    let store2 = harness2.fresh_store().unwrap();
    let imported: VosHost = <VosHarness<Stm> as Harness<Stm>>::import(
        &mut harness2,
        store2,
        Box::new(entries.into_iter().map(Ok)),
    )
    .expect("the export imports");
    let after = imported.status().unwrap();
    assert_eq!(after.records, status.records);
    assert_eq!(after.live_records, status.live_records);
    for n in 0..ORDERS {
        let journal = Host::<Stm>::journal(&imported, &id(&name(n))).unwrap();
        assert_eq!(journal.len() as u32, 2 + sealed_at + 1, "{}", name(n));
    }
    drop(imported);
    prove_backend();
}
