//! The archive of an ended order (`T-010-07`, `G-33`): the pure part the VOS program uses to keep its live journal
//! bounded. An archived order must replay to the very states its journal recorded (`NFR-4`, `REQ-STM-3`).

mod common;

use common::generator::{Rng, Trace, traces};
use rutent_contracts::state_machine::{Order, OrderState, RecordedInput, StateMachine};
use rutent_stm::Stm;
use rutent_stm::archive::{Entry, is_archivable, pack, replay, unpack};

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
fn replaying_nothing_gives_nothing_and_replay_is_the_hosts_own_fold() {
    assert!(replay(&[]).is_empty());
    for trace in traces(SEEDS) {
        // The generator re-sends recorded inputs, which a host answers from its record without a new entry, so
        // only the journal's inputs are folded here.
        let journal_inputs = inputs_of(&entries(trace));
        let mut state: Option<Order> = None;
        let folded: Vec<Option<Order>> = journal_inputs
            .iter()
            .map(|input| {
                state = match &state {
                    Some(order) => Some(Stm::transition(order, input)),
                    None => Stm::open(input),
                };
                state.clone()
            })
            .collect();
        assert_eq!(replay(&journal_inputs), folded);
    }
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
