//! The archive of an ended order (`T-010-07`, `G-33`): the pure part the VOS program uses to keep its live journal
//! bounded. An archived order must replay to the very states its journal recorded (`NFR-4`, `REQ-STM-3`).

mod common;

use common::generator::{Rng, Trace, traces};
use rutent_contracts::state_machine::{Order, OrderState, RecordedInput, StateMachine};
use rutent_stm::Stm;
use rutent_stm::archive::{
    CHUNK_BYTES, Entry, Held, INDEX_PUT, MAX_ENTRY, STEP_BUDGET, chunk_of, entry_fits, entry_in,
    entry_len, import_cost, is_archivable, pack, replay, seal_due, split, unpack,
};

const SEEDS: u64 = 600;

/// A trace's journal as the program stores it: canonical JSON input and state, one entry a record.
fn entries(trace: &Trace) -> Vec<Entry> {
    trace
        .journal
        .iter()
        .map(|record| Entry {
            input: serde_json::to_vec(&record.input).unwrap(),
            state: record
                .state_after
                .as_ref()
                .map(|s| serde_json::to_vec(s).unwrap()),
        })
        .collect()
}

fn states_of(entries: &[Entry]) -> Vec<Option<Order>> {
    entries
        .iter()
        .map(|e| e.state.as_ref().map(|s| serde_json::from_slice(s).unwrap()))
        .collect()
}

fn inputs_of(entries: &[Entry]) -> Vec<RecordedInput> {
    entries
        .iter()
        .map(|e| serde_json::from_slice(&e.input).unwrap())
        .collect()
}

// verifies: NFR-4, REQ-STM-3
#[test]
fn an_archived_order_replays_to_the_states_its_journal_recorded() {
    let mut ended = 0;
    for trace in traces(SEEDS) {
        let archived = unpack(&pack(&entries(trace))).expect("an archive unpacks");
        assert_eq!(archived, entries(trace), "seed {}", trace.seed);
        assert_eq!(
            replay(&inputs_of(&archived)),
            states_of(&archived),
            "seed {}: the archive replays to its recorded states",
            trace.seed
        );
        ended += trace.last().is_some_and(is_archivable) as u32;
    }
    assert!(ended > 20, "the seeds end enough orders to mean something");
}

// verifies: NFR-4, REQ-STM-3
#[test]
fn an_order_that_takes_inputs_after_it_ended_still_replays_from_its_archive() {
    // Inputs after the ending are recorded too (a repeat, a late input, the undo of a complete part), so the
    // archive holds them and replays them.
    let mut after_ending = 0;
    for trace in traces(SEEDS) {
        let journal = entries(trace);
        let states = states_of(&journal);
        let Some(first_end) = states
            .iter()
            .position(|s| s.as_ref().is_some_and(is_archivable))
        else {
            continue;
        };
        if first_end + 1 < journal.len() {
            after_ending += 1;
        }
        // Archived at the ending, appended to afterwards: the archive grows by each later record.
        let mut archive = pack(&journal[..=first_end]);
        for entry in &journal[first_end + 1..] {
            let mut so_far = unpack(&archive).unwrap();
            so_far.push(entry.clone());
            archive = pack(&so_far);
        }
        let all = unpack(&archive).unwrap();
        assert_eq!(all, journal, "seed {}", trace.seed);
        assert_eq!(replay(&inputs_of(&all)), states, "seed {}", trace.seed);
    }
    assert!(after_ending > 10, "some orders take inputs after ending");
}

// verifies: NFR-4
#[test]
fn a_damaged_archive_is_refused_rather_than_misread() {
    let trace = &traces(SEEDS)[0];
    let packed = pack(&entries(trace));
    for cut in 0..packed.len() {
        assert!(unpack(&packed[..cut]).is_err(), "cut at {cut}");
    }
    let mut longer = packed.clone();
    longer.push(0);
    assert!(unpack(&longer).is_err(), "bytes after the end");
    let mut wrong = packed;
    wrong[0] ^= 0xff;
    assert!(unpack(&wrong).is_err(), "a wrong header");
}

// verifies: REQ-OPS-3
#[test]
fn an_order_is_archivable_exactly_when_it_has_ended() {
    for trace in traces(SEEDS) {
        for step in &trace.steps {
            let Some(order) = &step.next else { continue };
            assert_eq!(
                is_archivable(order),
                matches!(
                    order.state,
                    OrderState::Complete
                        | OrderState::Failed
                        | OrderState::HandedOn
                        | OrderState::EndedWithoutCompleting
                ),
                "seed {}",
                trace.seed
            );
        }
    }
}

// verifies: NFR-4, REQ-STM-3
#[test]
fn replay_reaches_the_states_the_state_machine_reaches_when_a_host_folds_the_inputs() {
    assert!(replay(&[]).is_empty());
    for trace in traces(SEEDS) {
        let inputs = inputs_of(&entries(trace));
        // The state machine's own fold, written out step by step against its contract: `open` until an input
        // opens the order, `transition` after.
        let mut expected: Vec<Option<Order>> = Vec::new();
        for input in &inputs {
            let next = match expected.last().cloned().flatten() {
                Some(order) => Some(Stm::transition(&order, input)),
                None => Stm::open(input),
            };
            expected.push(next);
        }
        assert_eq!(replay(&inputs), expected, "seed {}", trace.seed);
        // And it is the states the reference host recorded, which `Stm` computed there.
        assert_eq!(
            replay(&inputs),
            states_of(&entries(trace)),
            "seed {}",
            trace.seed
        );
    }
}

// verifies: REQ-OPS-3
#[test]
fn an_archive_splits_into_rows_of_bounded_size_that_join_back() {
    for trace in traces(SEEDS) {
        let all = entries(trace);
        for limit in [1usize, 4 << 10, 12 << 10, 32 << 10] {
            let rows = split(&all, limit);
            let joined: Vec<Entry> = rows.iter().flatten().cloned().collect();
            assert_eq!(joined, all, "seed {} limit {limit}", trace.seed);
            for row in &rows {
                assert!(!row.is_empty(), "no row is empty");
                assert!(
                    pack(row).len() <= limit || row.len() == 1,
                    "seed {} limit {limit}: a row over the limit holds more than one entry",
                    trace.seed
                );
            }
        }
    }
    assert!(split(&[], 100).is_empty());
}

// verifies: REQ-OPS-3
#[test]
fn a_journal_larger_than_a_stored_row_is_split_into_several_rows() {
    // A state is a few KB, so a journal of this length is more than the 64 KiB a stored row holds.
    let trace = traces(SEEDS)
        .iter()
        .max_by_key(|t| entries(t).iter().map(entry_len).sum::<usize>())
        .unwrap();
    let mut all = entries(trace);
    while all.iter().map(entry_len).sum::<usize>() < 200 << 10 {
        all.extend(entries(trace));
    }
    let rows = split(&all, CHUNK_BYTES);
    assert!(rows.len() >= 4, "{} rows", rows.len());
    assert!(rows.iter().all(|r| pack(r).len() <= 64 << 10));
}

// verifies: NFR-4
#[test]
fn any_entries_come_back_from_their_archive_byte_for_byte() {
    for seed in 1..=400 {
        let mut rng = Rng::new(seed);
        let entries: Vec<Entry> = (0..rng.below(12))
            .map(|_| Entry {
                input: (0..rng.below(300)).map(|_| rng.next() as u8).collect(),
                state: rng
                    .pct(70)
                    .then(|| (0..rng.below(300)).map(|_| rng.next() as u8).collect()),
            })
            .collect();
        assert_eq!(unpack(&pack(&entries)), Ok(entries), "seed {seed}");
    }
}

// verifies: NFR-4, REQ-STM-3
#[test]
fn a_record_is_found_in_the_row_that_holds_it_and_a_damaged_archive_is_an_error() {
    let trace = traces(SEEDS)
        .iter()
        .max_by_key(|t| t.journal.len())
        .unwrap();
    let all = entries(trace);
    let rows = split(&all, 1);
    let mut starts = Vec::new();
    let mut next = 1;
    for row in &rows {
        starts.push(next);
        next += row.len() as u64;
    }
    for (at, entry) in all.iter().enumerate() {
        let seq = at as u64 + 1;
        let row = chunk_of(&starts, seq).expect("a record has a row");
        assert_eq!(entry_in(&rows[row], starts[row], seq), Ok(entry), "{seq}");
    }
    // A head that does not begin at record 1, or a row shorter than its head says, is an error: never a record
    // that is simply not there.
    assert!(chunk_of(&[], 1).is_err());
    assert!(chunk_of(&[2, 3], 1).is_err());
    let row = &rows[0];
    assert!(entry_in(row, starts[0], starts[0] + row.len() as u64).is_err());
    assert!(
        entry_in(row, 5, 4).is_err(),
        "a record before the row begins"
    );
}

/// A host's packing of one order as its records arrive: live bytes are counted, and when a record fills the live
/// rows (or ends the order) they are sealed into archive rows. Returns the rows each step wrote.
fn stream(all: &[Entry], ending: Option<usize>) -> (Vec<usize>, Vec<Entry>) {
    let mut live: Vec<Entry> = Vec::new();
    let mut live_bytes = 0;
    let mut sealed: Vec<Entry> = Vec::new();
    let mut written = Vec::new();
    for (at, entry) in all.iter().enumerate() {
        assert!(entry_fits(entry));
        live.push(entry.clone());
        live_bytes += entry_len(entry);
        let ends = ending == Some(at);
        let mut step = 0;
        if seal_due(live_bytes, ends) {
            for row in split(&live, CHUNK_BYTES) {
                step += pack(&row).len();
                sealed.extend(row);
            }
            live.clear();
            live_bytes = 0;
        }
        written.push(step);
    }
    sealed.extend(live);
    (written, sealed)
}

// verifies: REQ-OPS-3
#[test]
fn sealing_records_as_they_arrive_keeps_every_step_far_under_a_steps_write_cap() {
    // VOS caps a step's writes at 256 KiB. A journal several times that, ended at its last record, must never
    // write more than a few rows in one step, whatever the order and size of its records.
    let trace = traces(SEEDS)
        .iter()
        .max_by_key(|t| t.journal.len())
        .unwrap();
    let base = entries(trace);
    let mut all = Vec::new();
    while all.iter().map(entry_len).sum::<usize>() < 1 << 20 {
        all.extend(base.iter().cloned());
    }
    let (written, sealed) = stream(&all, Some(all.len() - 1));
    assert_eq!(sealed, all, "nothing is lost or reordered");
    assert!(written.iter().sum::<usize>() >= 1 << 20);
    let most = *written.iter().max().unwrap();
    assert!(
        most <= CHUNK_BYTES + MAX_ENTRY + 64,
        "a step wrote {most} bytes"
    );
    assert!(most < 128 << 10);
    // Ending a journal of a megabyte writes no more than ending a short one.
    let (short, _) = stream(&base, Some(base.len() - 1));
    assert!(short.iter().max().unwrap() <= &(CHUNK_BYTES + MAX_ENTRY + 64));
}

// verifies: REQ-OPS-3
#[test]
fn a_record_too_large_for_an_archive_row_does_not_fit() {
    let small = Entry {
        input: vec![0; 100],
        state: Some(vec![0; 100]),
    };
    assert!(entry_fits(&small));
    let edge = Entry {
        input: vec![0; MAX_ENTRY - 9 - 8 - 5],
        state: Some(vec![0; 5]),
    };
    assert!(entry_fits(&edge));
    assert!(pack(&[edge]).len() < 64 << 10);
    let big = Entry {
        input: vec![0; MAX_ENTRY],
        state: None,
    };
    assert!(!entry_fits(&big));
    assert!(!seal_due(0, false), "an empty live journal is not sealed");
    assert!(seal_due(CHUNK_BYTES, false));
    assert!(seal_due(1, true));
}

/// An import step, as the program takes it: entries in the stream's order, each charged `import_cost` against the
/// order's live bytes, stopping before the budget is passed (but always taking one). Returns each step's total.
fn import_steps(records: &[(usize, usize, bool)]) -> Vec<usize> {
    let mut live = std::collections::BTreeMap::<usize, usize>::new();
    let mut steps = Vec::new();
    let mut spent = 0;
    let mut taken = 0;
    for &(order, weight, ends) in records {
        let held = live.get(&order).copied().unwrap_or(0);
        let cost = import_cost(
            Held::Live {
                bytes: held,
                count: held / weight,
            },
            weight,
            ends,
        );
        if taken > 0 && spent + cost > STEP_BUDGET {
            steps.push(spent);
            spent = 0;
            taken = 0;
        }
        spent += cost;
        taken += 1;
        if seal_due(held + weight, ends) {
            live.insert(order, 0);
        } else {
            live.insert(order, held + weight);
        }
    }
    steps.push(spent);
    steps
}

// verifies: REQ-OPS-3, NFR-6
#[test]
fn an_import_step_stops_before_its_writes_pass_the_budget() {
    // Orders just under a row's worth of live records, each sealed by its next record: eight of them in a row, the
    // stream a busy export gives. Taken eight at a time they write over VOS's 256 KiB step cap.
    let weight = 4 << 10;
    let near = CHUNK_BYTES - weight + 1;
    let orders = 40;
    let mut records = Vec::new();
    for order in 0..orders {
        for _ in 0..(near / weight) {
            records.push((order, weight, false));
        }
    }
    let first_seals = records.len();
    for order in 0..orders {
        records.push((order, weight, false)); // fills the row: sealed
    }
    let eight_at_a_time: usize = (first_seals..first_seals + 8)
        .map(|at| {
            let (_, weight, ends) = records[at];
            import_cost(
                Held::Live {
                    bytes: near / 4096 * 4096,
                    count: near / 4096,
                },
                weight,
                ends,
            )
        })
        .sum();
    assert!(
        eight_at_a_time > 256 << 10,
        "eight seals in one step are {eight_at_a_time} bytes"
    );
    let steps = import_steps(&records);
    let most = *steps.iter().max().unwrap();
    assert!(
        most <= STEP_BUDGET + CHUNK_BYTES + MAX_ENTRY,
        "a step wrote {most}"
    );
    assert!(most < 256 << 10);
    assert!(steps.len() >= 2 + orders / 6, "{} steps", steps.len());
    // A single entry is always taken, however it weighs.
    assert_eq!(import_steps(&[(0, MAX_ENTRY, true)]).len(), 1);
    assert_eq!(import_steps(&[]).len(), 1);
}

// verifies: REQ-OPS-3
#[test]
fn an_entry_that_seals_costs_its_live_records_and_one_that_does_not_costs_itself() {
    let live = |bytes, count| Held::Live { bytes, count };
    assert!(import_cost(live(0, 0), 1000, false) < 2000);
    assert!(import_cost(live(CHUNK_BYTES - 500, 7), 1000, false) > CHUNK_BYTES);
    assert!(import_cost(live(5000, 2), 1000, true) > 6000);
    // Each record the seal deletes and each index row put is charged its key.
    let few = import_cost(live(5000, 1), 1000, true);
    let many = import_cost(live(5000, 11), 1000, true);
    assert_eq!(many - few, 10 * INDEX_PUT);
}

// verifies: REQ-OPS-3, NFR-6
#[test]
fn a_late_record_to_an_ended_order_is_charged_for_rewriting_its_last_row() {
    // The last row of an ended order is up to 32 KiB, and a late record rewrites it whole.
    let last_row = CHUNK_BYTES - 200;
    let cost = import_cost(Held::Ended { last_row }, 1000, false);
    assert!(cost >= last_row + 1000, "{cost}");
    // Eight late records to eight ended orders, one chunk of an import: they write over 256 KiB if taken together.
    let together: usize = (0..8)
        .map(|_| import_cost(Held::Ended { last_row }, 1000, false))
        .sum();
    assert!(together > 256 << 10, "{together}");
    // The budget therefore splits them over several steps.
    let mut steps = 0;
    let mut spent = 0;
    for _ in 0..8 {
        if spent > 0 && spent + cost > STEP_BUDGET {
            steps += 1;
            spent = 0;
        }
        spent += cost;
    }
    steps += 1;
    assert!(steps >= 2, "{steps} steps");
    assert!(
        (0..8).map(|_| cost).sum::<usize>() / steps < 256 << 10,
        "each step is under the cap"
    );
    // A row that is full already starts a new one: its cost is its own record's.
    assert!(import_cost(Held::Ended { last_row: 0 }, 1000, false) < 3000);
}
