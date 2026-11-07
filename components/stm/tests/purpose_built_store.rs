//! The purpose-built store against the host-contract suite (`T-007-02`, `EXP-1` candidate `pb`), and what the
//! in-memory suite can't show: a real process killed mid-write, and a log torn mid-record. Built only with the
//! `purpose-built-store` feature.

#![cfg(feature = "purpose-built-store")]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{RwLock, RwLockReadGuard};

use rutent_conformance::host::probe::{self, Probe};
use rutent_conformance::host::{CellOutcome, Report, RunConfig, Verdict, run_with_fault_seam};
use rutent_contracts::state_machine::RecordedInput;
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_contracts::state_machine::host::{Delivery, Host, HostError, TransitionRecord};
use rutent_stm::purpose_built_store::{
    DiskStore, FRAME_HEADER, PurposeBuiltHarness, PurposeBuiltHost,
};
use rutent_stm::reference_host::ReferenceHarness;

type Pb = PurposeBuiltHost<Probe>;

/// Keeps a process spawn from overlapping a store being closed and opened again in another test.
///
/// A spawned process briefly holds copies of every descriptor its parent has open, until it executes (they are all
/// close-on-exec). An `flock` belongs to the open file description, not the descriptor, so while a copy exists a
/// dropped host's lock is still held, and the reopen in another test thread fails with "the store is open in
/// another host". Every test that opens a store holds the gate shared for its whole body; the one place that spawns
/// holds it exclusive for the spawn.
static SPAWN_GATE: RwLock<()> = RwLock::new(());

fn stores_in_use() -> RwLockReadGuard<'static, ()> {
    SPAWN_GATE.read().unwrap_or_else(|e| e.into_inner())
}

fn spawn_writer(command: &mut Command) -> std::io::Result<std::process::Child> {
    let _exclusive = SPAWN_GATE.write().unwrap_or_else(|e| e.into_inner());
    command.spawn()
}

/// The run these tests report under: the scored `EXP-1` run after the contract freeze `C1`
/// (`experiments/2026-11-06-runtime-host/README.md`, Runs).
const RUN_ID: &str = "EXP-1-pb-20261107-1";

fn config(candidate: &str, target: &str) -> RunConfig {
    RunConfig {
        run_id: RUN_ID.into(),
        candidate: candidate.into(),
        target: target.into(),
        seed: 20_261_106,
    }
}

fn failures(report: &Report) -> Vec<String> {
    report
        .cells
        .iter()
        .filter(|(_, o)| !matches!(o, CellOutcome::Pass))
        .map(|(c, o)| format!("{}: {o:?}", c.id))
        .collect()
}

fn assert_passes(report: &Report) {
    // The report goes to the test's output, so a development run can be pasted into the scoring sheet's notes.
    println!("{}", report.to_markdown());
    let failed = failures(report);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(report.verdict(), Verdict::Pass);
}

// verifies: REQ-OPS-3, NFR-6, DEC-6, DEC-5
#[test]
fn the_purpose_built_store_passes_the_host_contract_suite_swapping_into_the_reference_host() {
    let _gate = stores_in_use();
    assert_passes(&run_with_fault_seam(
        config("purpose-built-store", "reference-host"),
        &mut PurposeBuiltHarness::<Probe>::new(),
        &mut ReferenceHarness::<Probe>::new(),
    ));
}

// verifies: REQ-OPS-3, NFR-6, DEC-6, DEC-5
#[test]
fn the_purpose_built_store_passes_the_host_contract_suite_swapping_into_itself() {
    let _gate = stores_in_use();
    assert_passes(&run_with_fault_seam(
        config("purpose-built-store", "purpose-built-store"),
        &mut PurposeBuiltHarness::<Probe>::new(),
        &mut PurposeBuiltHarness::<Probe>::new(),
    ));
}

// verifies: REQ-OPS-3, NFR-6, DEC-6, DEC-5
#[test]
fn the_reference_host_swaps_into_the_purpose_built_store_under_the_host_contract_suite() {
    let _gate = stores_in_use();
    assert_passes(&run_with_fault_seam(
        config("reference-host", "purpose-built-store"),
        &mut ReferenceHarness::<Probe>::new(),
        &mut PurposeBuiltHarness::<Probe>::new(),
    ));
}

/// Every record of every order, by record id.
fn records_by_id(host: &Pb) -> BTreeMap<String, Vec<TransitionRecord>> {
    let mut all: BTreeMap<String, Vec<TransitionRecord>> = BTreeMap::new();
    for order in host.orders().unwrap() {
        for r in host.journal(&order).unwrap() {
            all.entry(r.input.record_id.as_str().to_owned())
                .or_default()
                .push(r);
        }
    }
    all
}

/// Every journal is gapless and every recorded state is the probe's fold of the journal's inputs.
fn assert_journals_follow_the_probe(host: &Pb, after: &str) {
    for order in host.orders().unwrap() {
        let journal = host.journal(&order).unwrap();
        let expected = probe::fold(journal.iter().map(|r| &r.input));
        for (n, (r, state)) in journal.iter().zip(&expected).enumerate() {
            assert_eq!(r.seq, n as u64 + 1, "{} has a gap {after}", order.as_str());
            assert_eq!(
                &r.state_after,
                state,
                "seq {} of {} doesn't follow from its journal {after}",
                r.seq,
                order.as_str()
            );
        }
        let latest = expected.into_iter().rev().find_map(|s| s);
        assert_eq!(host.state(&order).unwrap(), latest, "{after}");
    }
}

fn log_len(store: &DiskStore) -> u64 {
    fs::metadata(store.log_path()).unwrap().len()
}

/// A store in a fresh directory of the harness holding these bytes as its log.
fn store_with_log(harness: &mut PurposeBuiltHarness<Probe>, log: &[u8]) -> DiskStore {
    let store = harness.fresh_store().unwrap();
    fs::write(store.log_path(), log).unwrap();
    store
}

/// The inputs the kill test's writer submits, in order: many orders, one after another, more than any kill
/// lets it reach.
fn writer_inputs() -> Vec<RecordedInput> {
    (0..400u32)
        .flat_map(|k| probe::order_inputs(&format!("ord-{k}"), 10 * k))
        .collect()
}

/// The environment variable that turns the kill test into its own writer process, naming the store's directory.
const WRITER_STORE: &str = "RUTENT_PB_KILL_TEST_WRITER_STORE";
const KILL_TEST: &str =
    "crash_between_record_and_effect_leaves_no_effect_when_the_process_is_killed_mid_write";

/// The writer: opens the store, submits every input in order and prints each acknowledged record id as soon as
/// `submit` returns it. It is killed long before it runs out.
fn run_writer(dir: &Path) -> ! {
    use std::io::Write;
    let mut host = Pb::open(DiskStore::at(dir)).expect("the writer opens the store");
    let mut out = std::io::stdout().lock();
    for input in writer_inputs() {
        match host.submit(input.clone()) {
            Ok(Delivery::Recorded(_) | Delivery::AlreadyRecorded(_)) => {
                writeln!(out, "ACK {}", input.record_id.as_str()).unwrap();
                out.flush().unwrap();
            }
            Err(e) => {
                writeln!(out, "ERR {e:?}").unwrap();
                std::process::exit(2);
            }
        }
    }
    writeln!(out, "DONE").unwrap();
    std::process::exit(0);
}

/// The record id a writer's line acknowledges. The test harness may print its own text before the writer's first
/// line, on the same line, so the mark is looked for anywhere in it.
fn acked(line: &str) -> Option<&str> {
    line.split_once("ACK ").map(|(_, id)| id.trim())
}

// verifies: INV-7, NFR-5, REQ-STM-4, INV-8, REQ-OPS-3
#[test]
fn crash_between_record_and_effect_leaves_no_effect_when_the_process_is_killed_mid_write() {
    if let Some(dir) = std::env::var_os(WRITER_STORE) {
        run_writer(Path::new(&dir));
    }
    let mut gate = stores_in_use();
    let inputs = writer_inputs();
    let by_id: BTreeMap<&str, &RecordedInput> =
        inputs.iter().map(|i| (i.record_id.as_str(), i)).collect();
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let store = harness.fresh_store().unwrap();
    let mut acknowledged = BTreeSet::<String>::new();
    let mut in_flight_present = 0;
    // Each round starts a writer on the same store, waits for this many new acknowledgements past what earlier
    // rounds saw, lets it run on for this many microseconds, and kills it with SIGKILL while it goes on
    // computing, writing and syncing. The delays vary where in a submit each kill lands; a kill doesn't tear a
    // single write on a local file system, so the torn-write test below cuts the log by hand.
    let rounds = [
        (1usize, 0u64),
        (5, 300),
        (13, 1_100),
        (2, 2_500),
        (21, 50),
        (8, 4_000),
        (1, 700),
        (34, 6_500),
        (3, 1_800),
        (11, 150),
        (2, 9_000),
        (17, 3_300),
    ];
    for (round, (new, delay)) in rounds.into_iter().enumerate() {
        // The spawn takes the gate exclusive, so this test lets go of its shared hold for it.
        drop(gate);
        let spawned = spawn_writer(
            Command::new(std::env::current_exe().unwrap())
                .args([KILL_TEST, "--exact", "--nocapture", "--test-threads=1"])
                .env(WRITER_STORE, store.dir())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
        )
        .expect("the writer starts");
        gate = stores_in_use();
        let mut child = spawned;
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut fresh = 0;
        while fresh < new {
            let line = lines
                .next()
                .unwrap_or_else(|| panic!("round {round}: the writer ended before the kill"))
                .unwrap();
            let Some(id) = acked(&line) else {
                assert!(
                    !line.contains("ERR ") && !line.contains("DONE"),
                    "round {round}: {line}"
                );
                continue;
            };
            if acknowledged.insert(id.to_owned()) {
                fresh += 1;
            }
        }
        std::thread::sleep(std::time::Duration::from_micros(delay));
        child.kill().expect("SIGKILL reaches the writer");
        child.wait().unwrap();
        // Acknowledgements printed after the last one read and before the kill count too.
        for line in lines.map_while(Result::ok) {
            if let Some(id) = acked(&line) {
                acknowledged.insert(id.to_owned());
            }
        }

        let host = Pb::open(store.clone()).expect("the store reopens after a kill");
        let found = records_by_id(&host);
        for id in &acknowledged {
            let records = found
                .get(id.as_str())
                .unwrap_or_else(|| panic!("round {round}: acknowledged {id} is gone"));
            assert_eq!(records.len(), 1, "round {round}: {id} recorded twice");
            assert_eq!(
                &records[0].input,
                by_id[id.as_str()],
                "round {round}: {id} isn't whole"
            );
        }
        assert!(
            found.values().all(|r| r.len() == 1),
            "round {round}: a record id recorded twice"
        );
        // The writer submits one input at a time, so at most the one in flight is durable and unacknowledged.
        let extra: Vec<&String> = found
            .keys()
            .filter(|id| !acknowledged.contains(*id))
            .collect();
        assert!(
            extra.len() <= 1,
            "round {round}: unacknowledged records {extra:?}"
        );
        in_flight_present += extra.len();
        assert_journals_follow_the_probe(&host, &format!("after kill {round}"));
        drop(host);
    }

    // The input after the last acknowledged one is recorded once, whether or not it was in flight at the kill.
    let mut host = Pb::open(store.clone()).unwrap();
    let next = inputs
        .iter()
        .find(|i| !acknowledged.contains(i.record_id.as_str()))
        .unwrap();
    let before = records_by_id(&host).len();
    match host.submit(next.clone()).unwrap() {
        Delivery::Recorded(r) | Delivery::AlreadyRecorded(r) => assert_eq!(&r.input, next),
    }
    let after = records_by_id(&host).len();
    assert!(after == before || after == before + 1);
    drop(host);
    let host = Pb::open(store).unwrap();
    assert_journals_follow_the_probe(&host, "after the last reopen");
    println!(
        "real-kill: {} kills, {} records acknowledged, {in_flight_present} kills landed after the write of the \
         record in flight and before its acknowledgement",
        rounds.len(),
        acknowledged.len()
    );
}

/// A store holding two orders' inputs, killed after the last: the inputs, the log, and where each input's frame
/// starts in it.
fn written_store_frames(
    harness: &mut PurposeBuiltHarness<Probe>,
) -> (Vec<RecordedInput>, Vec<u8>, Vec<usize>) {
    let mut inputs = probe::order_inputs("ord-1", 0);
    inputs.extend(probe::order_inputs("ord-2", 100));
    let store = harness.fresh_store().unwrap();
    let mut host = Pb::open(store.clone()).unwrap();
    let mut starts = Vec::new();
    for input in &inputs {
        // A fresh log's format entry is its own frame, before the first input's.
        let before = log_len(&store) as usize;
        host.submit(input.clone()).unwrap();
        let after = log_len(&store) as usize;
        let own = probe_frame_len(
            &fs::read(store.log_path()).unwrap()[before..after],
            before == 0,
        );
        starts.push(after - own);
    }
    harness.kill(host).unwrap();
    (inputs, fs::read(store.log_path()).unwrap(), starts)
}

/// The length of the last frame in bytes that hold one or, for a fresh log, two whole frames.
fn probe_frame_len(appended: &[u8], with_format: bool) -> usize {
    if !with_format {
        return appended.len();
    }
    let format = u32::from_le_bytes(appended[..4].try_into().unwrap()) as usize + FRAME_HEADER;
    appended.len() - format
}

/// A store holding two orders' inputs, killed after the last; the log's length before and after the last frame.
fn written_store(harness: &mut PurposeBuiltHarness<Probe>) -> (Vec<RecordedInput>, Vec<u8>, usize) {
    let (inputs, log, starts) = written_store_frames(harness);
    (inputs, log, *starts.last().unwrap())
}

// verifies: NFR-5, INV-7, REQ-STM-4, REQ-OPS-3
#[test]
fn a_log_torn_mid_record_reopens_at_its_last_whole_record() {
    let _gate = stores_in_use();
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let (inputs, log, start) = written_store(&mut harness);
    let last = inputs.last().unwrap();
    let whole = log.len();
    let mut flipped = log.clone();
    flipped[whole - 3] ^= 0x40;
    let torn: Vec<(String, Vec<u8>)> = [
        1,
        4,
        FRAME_HEADER,
        FRAME_HEADER + 1,
        (whole - start) / 2,
        whole - start - 1,
    ]
    .into_iter()
    .map(|keep| {
        (
            format!("cut {keep} bytes into the last frame"),
            log[..start + keep].to_vec(),
        )
    })
    .chain([("a byte of the last frame flipped".to_owned(), flipped)])
    .collect();

    for (what, bytes) in torn {
        let store = store_with_log(&mut harness, &bytes);
        let mut host =
            Pb::open(store.clone()).unwrap_or_else(|e| panic!("{what}: open gave {e:?}"));
        assert_eq!(
            log_len(&store) as usize,
            start,
            "{what}: the tail wasn't cut"
        );
        let found = records_by_id(&host);
        assert_eq!(found.len(), inputs.len() - 1, "{what}");
        assert!(!found.contains_key(last.record_id.as_str()), "{what}");
        for input in &inputs[..inputs.len() - 1] {
            assert_eq!(&found[input.record_id.as_str()][0].input, input, "{what}");
        }
        assert_journals_follow_the_probe(&host, &what);
        match host.submit(last.clone()) {
            Ok(Delivery::Recorded(r)) => assert_eq!(r.seq, 7, "{what}"),
            other => panic!("{what}: submitting the torn input again gave {other:?}"),
        }
        let store = harness.kill(host).unwrap();
        let host = Pb::open(store).unwrap();
        assert_eq!(records_by_id(&host).len(), inputs.len(), "{what}");
        assert_journals_follow_the_probe(&host, &format!("{what}, reopened"));
    }
}

// verifies: NFR-5, INV-7, REQ-AUD-1
#[test]
fn damage_to_more_than_the_last_append_refuses_the_store_and_cuts_nothing() {
    let _gate = stores_in_use();
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let (_, log, starts) = written_store_frames(&mut harness);
    let last_three = starts[starts.len() - 3];
    let last = *starts.last().unwrap();
    let mut three_zeroed = log.clone();
    three_zeroed[last_three..].fill(0);
    let mut three_flipped = log.clone();
    for start in &starts[starts.len() - 3..] {
        three_flipped[start + FRAME_HEADER + 5] ^= 0x01;
    }
    let mut one_zeroed = log.clone();
    one_zeroed[last..].fill(0);
    let mut past_its_length = log[..last].to_vec();
    past_its_length.extend_from_slice(&log[last..last + FRAME_HEADER]);
    past_its_length.resize(log.len() + 16, b'x');
    let damaged = [
        ("the last three frames zero-filled", three_zeroed),
        (
            "a byte flipped in each of the last three frames",
            three_flipped,
        ),
        (
            "the last frame zero-filled, its length unreadable",
            one_zeroed,
        ),
        (
            "bytes past the length the last header declares",
            past_its_length,
        ),
    ];
    for (what, bytes) in damaged {
        let store = store_with_log(&mut harness, &bytes);
        match Pb::open(store.clone()) {
            Err(HostError::Unavailable { reason }) => {
                assert!(reason.contains("more than one append"), "{what}: {reason}")
            }
            other => panic!("{what}: the store opened: {:?}", other.map(|_| ())),
        }
        assert_eq!(
            fs::read(store.log_path()).unwrap(),
            bytes,
            "{what}: the store was changed"
        );
    }
}

// verifies: NFR-5, INV-7, REQ-AUD-1
#[test]
fn a_corrupt_entry_inside_synced_history_refuses_the_store_and_cuts_nothing() {
    let _gate = stores_in_use();
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let (_, mut log, start) = written_store(&mut harness);
    log[start / 2] ^= 0x01;
    let store = store_with_log(&mut harness, &log);
    match Pb::open(store.clone()) {
        Err(HostError::Unavailable { reason }) => assert!(reason.contains("corrupt"), "{reason}"),
        other => panic!("a corrupt store opened: {:?}", other.map(|_| ())),
    }
    assert_eq!(
        fs::read(store.log_path()).unwrap(),
        log,
        "the store was changed"
    );
}

// verifies: NFR-5, INV-7, REQ-OPS-3, NFR-6
#[test]
fn damage_inside_a_finished_import_is_corruption_and_inside_an_unfinished_one_is_an_incomplete_import()
 {
    let _gate = stores_in_use();
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let mut inputs = probe::order_inputs("ord-1", 0);
    inputs.extend(probe::order_inputs("ord-2", 100));
    let (before, after) = inputs.split_at(9);
    let mut source = Pb::open(harness.fresh_store().unwrap()).unwrap();
    for input in before {
        source.submit(input.clone()).unwrap();
    }
    source.halt().unwrap();
    let entries: Vec<_> = source.export().unwrap().collect();
    let stream = || -> rutent_contracts::state_machine::host::SnapshotStream<'static> {
        Box::new(entries.clone().into_iter())
    };

    // A finished import, resumed and used: damage among the imported records is corruption of synced history.
    let fresh = harness.fresh_store().unwrap();
    let mut host = harness.import(fresh.clone(), stream()).unwrap();
    let imported = log_len(&fresh) as usize;
    host.resume().unwrap();
    for input in after {
        host.submit(input.clone()).unwrap();
    }
    let store = harness.kill(host).unwrap();
    let mut log = fs::read(store.log_path()).unwrap();
    log[imported / 2] ^= 0x01;
    let damaged = store_with_log(&mut harness, &log);
    match Pb::open(damaged.clone()) {
        Err(HostError::Unavailable { reason }) => assert!(reason.contains("corrupt"), "{reason}"),
        Err(HostError::ImportIncomplete) => {
            panic!("damage inside a finished import was reported as an unfinished import")
        }
        other => panic!("a damaged store opened: {:?}", other.map(|_| ())),
    }
    assert_eq!(
        fs::read(damaged.log_path()).unwrap(),
        log,
        "the store was changed"
    );

    // An import stopped part-way, with damage inside what it wrote: still an unfinished import, redone afresh.
    let fresh = harness.fresh_store().unwrap();
    let stopped = harness
        .import_crashing_after(fresh, stream(), entries.len() as u64 - 2)
        .unwrap();
    let mut log = fs::read(stopped.log_path()).unwrap();
    let at = log.len() / 2;
    log[at] ^= 0x01;
    let damaged = store_with_log(&mut harness, &log);
    assert!(matches!(
        Pb::open(damaged.clone()),
        Err(HostError::ImportIncomplete)
    ));
    assert_eq!(
        fs::read(damaged.log_path()).unwrap(),
        log,
        "the store was changed"
    );
}

// verifies: REQ-OPS-3, NFR-6, NFR-5
#[test]
fn an_import_stopped_right_after_its_first_frame_is_incomplete_and_a_torn_first_frame_is_refused() {
    let _gate = stores_in_use();
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let mut source = Pb::open(harness.fresh_store().unwrap()).unwrap();
    for input in probe::order_inputs("ord-1", 0) {
        source.submit(input).unwrap();
    }
    source.halt().unwrap();
    let entries: Vec<_> = source.export().unwrap().collect();

    // The harness's seam stops the import after its first frame is synced, before any of the stream: with
    // nothing but that frame, the store is an unfinished import, never an empty store that opens.
    for stop in [0, 1] {
        let fresh = harness.fresh_store().unwrap();
        let stopped = harness
            .import_crashing_after(fresh, Box::new(entries.clone().into_iter()), stop)
            .unwrap();
        assert!(
            matches!(Pb::open(stopped.clone()), Err(HostError::ImportIncomplete)),
            "an import stopped after {stop} entries opened"
        );
    }

    // The first frame itself torn: refused, not cut to an empty store, and left as it was.
    let fresh = harness.fresh_store().unwrap();
    let stopped = harness
        .import_crashing_after(fresh, Box::new(entries.clone().into_iter()), 0)
        .unwrap();
    let log = fs::read(stopped.log_path()).unwrap();
    for keep in [FRAME_HEADER - 2, FRAME_HEADER + 3, log.len() - 1] {
        let store = store_with_log(&mut harness, &log[..keep]);
        match Pb::open(store.clone()) {
            Err(HostError::Unavailable { reason }) => {
                assert!(reason.contains("first frame"), "{reason}")
            }
            other => panic!(
                "a torn first frame of {keep} bytes gave {:?}",
                other.map(|_| ())
            ),
        }
        assert_eq!(fs::read(store.log_path()).unwrap(), &log[..keep]);
    }
}

// verifies: NFR-5, REQ-STM-4
#[test]
fn a_store_opens_in_one_host_at_a_time() {
    let _gate = stores_in_use();
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let store = harness.fresh_store().unwrap();
    let first = Pb::open(store.clone()).unwrap();
    assert!(matches!(
        Pb::open(store.clone()),
        Err(HostError::Unavailable { .. })
    ));
    drop(first);
    Pb::open(store).unwrap();
}

/// A regression test for the gate: process spawns running beside a store that is closed and opened again must
/// never make the reopen fail. With `Command::spawn` called directly instead of `spawn_writer` it failed within
/// a few hundred iterations, with the "the store is open in another host" the suite's tests flaked with.
#[test]
fn a_process_spawned_beside_a_closing_store_never_keeps_its_lock() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let store = harness.fresh_store().unwrap();
    let done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                spawn_writer(Command::new("true").stdout(Stdio::null()))
                    .and_then(|mut child| child.wait())
                    .unwrap();
            }
        });
        let mut failure = None;
        for i in 0..2_000 {
            // Shared for one close and reopen, so the spawner gets its turn between iterations.
            let _gate = stores_in_use();
            match Pb::open(store.clone()) {
                Ok(host) => drop(host),
                Err(e) => {
                    failure = Some(format!("iteration {i}: {e:?}"));
                    break;
                }
            }
        }
        done.store(true, Ordering::Relaxed);
        assert_eq!(failure, None);
    });
}
