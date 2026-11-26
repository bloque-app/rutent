//! The differential oracle between core-internal's log and the state machine (`T-020-01`): every history the log
//! admits holds only state-machine inputs the reference state machine takes, in order. A recorded input the machine
//! refuses would put the log and the order's state apart.
//!
//! This file replays core-internal's admission fixtures: every case's history, and the appended entry of every case
//! the log admits, through [`Stm`]. The property half, over generated runs, is the scenarios' `tests/log_oracle_property.rs`, since an outer library
//! never names core-internal (`DEC-2`).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use rutent_contracts::state_machine::{Order, Outcome, RecordedInput, StateMachine};
use rutent_stm::Stm;
use serde_json::Value;

/// Cases whose histories hold an input the state machine refuses on purpose, each with its reason. Empty: a case
/// built to show a row's refusal still stands on a history the state machine would have produced.
const REFUSED_BY_THE_MACHINE_ON_PURPOSE: [(&str, &str); 0] = [];

fn admission_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/core-internal/fixtures/admission")
}

/// A log's input as the host recorded it: taken by the log as an `input`, or kept as an `input_not_taken` trace.
pub enum Recorded {
    /// An `input` entry's input.
    Input(RecordedInput),
    /// An `input_not_taken` entry's input: the host recorded it, so its record id is used, but no order moved.
    NotTaken(RecordedInput),
}

/// What the host and the state machine do with a log's inputs, in order: an input under a record id an earlier taken
/// one holds is that one again, or the host's refusal; under a refused one's, the host's refusal; otherwise `Applied`, `Late` or
/// `Repeat` is a taking, anything else a refusal, named with its record id.
pub fn replay(inputs: &[Recorded]) -> Result<(), String> {
    let mut orders: BTreeMap<String, Order> = BTreeMap::new();
    let mut recorded: BTreeMap<String, (&RecordedInput, bool)> = BTreeMap::new();
    for item in inputs {
        let input = match item {
            Recorded::Input(input) => input,
            Recorded::NotTaken(input) => {
                recorded
                    .entry(input.record_id.as_str().to_owned())
                    .or_insert((input, false));
                continue;
            }
        };
        // The host's exactly-once: one record id names one content (`RecordIdReused`), a refused input's too.
        match recorded.get(input.record_id.as_str()) {
            Some((earlier, _)) if *earlier != input => {
                return Err(format!(
                    "{} names two inputs, refused by the host",
                    input.record_id.as_str()
                ));
            }
            Some((_, true)) => continue,
            // The same input as one refused before: the host hands back the refusal it recorded, so it is never
            // taken.
            Some((_, false)) => {
                return Err(format!(
                    "{} was refused, and the host hands back its refusal",
                    input.record_id.as_str()
                ));
            }
            None => {
                recorded.insert(input.record_id.as_str().to_owned(), (input, true));
            }
        }
        let order = input.subject.order.as_str().to_owned();
        match orders.get(&order) {
            None => {
                // An input before its order's route is recorded with no state (the host contract's).
                if let Some(opened) = Stm::open(input) {
                    orders.insert(order, opened);
                }
            }
            Some(state) => {
                let next = Stm::transition(state, input);
                match next.last_input.outcome {
                    Outcome::Applied {} | Outcome::Late {} | Outcome::Repeat {} => {
                        orders.insert(order, next);
                    }
                    other => {
                        return Err(format!(
                            "{} ({:?}) is refused: {other:?}",
                            input.record_id.as_str(),
                            input.observed
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// The state-machine inputs among `entries`, in order, each taken or kept as refused.
pub fn inputs_of<'a>(entries: impl Iterator<Item = &'a Value>) -> Vec<Recorded> {
    entries
        .filter(|e| e["kind"] == "verified")
        .filter_map(|e| {
            let input =
                || serde_json::from_value(e["what"]["input"].clone()).expect("a recorded input");
            match e["what"]["call"].as_str() {
                Some("input") => Some(Recorded::Input(input())),
                Some("input_not_taken") => Some(Recorded::NotTaken(input())),
                _ => None,
            }
        })
        .collect()
}

// verifies: INV-4, REQ-STM-3, DEC-4
#[test]
fn every_history_the_log_admits_holds_only_inputs_the_state_machine_takes() {
    let mut failures = Vec::new();
    let mut replayed = 0;
    for path in fs::read_dir(admission_dir()).expect("the admission fixtures") {
        let path = path.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        if REFUSED_BY_THE_MACHINE_ON_PURPOSE
            .iter()
            .any(|(n, _)| *n == name)
        {
            continue;
        }
        let case: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let mut entries: Vec<&Value> = case["history"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| &item["entry"])
            .collect();
        // A refused case's appended entry is the log's to refuse; an admitted one's is held too.
        if case["breaks"].is_null() {
            entries.push(&case["entry"]);
        }
        if let Err(why) = replay(&inputs_of(entries.into_iter())) {
            failures.push(format!("{name}: {why}"));
        }
        replayed += 1;
    }
    assert!(replayed > 0, "no fixtures replayed");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
