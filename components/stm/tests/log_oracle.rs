//! The differential oracle between core-internal's log and the state machine (`T-020-01`): every history the log
//! admits holds only state-machine inputs the reference state machine takes, in order. A recorded input the machine
//! refuses would put the log and the order's state apart.
//!
//! This file replays core-internal's admission fixtures: every case's history, and the appended entry of every case
//! the log admits, through [`Stm`]. The property half, over generated runs, is in `log_oracle_property.rs`.

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/core-internal/fixtures/admission")
}

/// What the state machine does with a log's inputs, in order: `Applied`, `Late` or `Repeat` is a taking; anything
/// else a refusal, named with its record id.
pub fn replay(inputs: &[RecordedInput]) -> Result<(), String> {
    let mut orders: BTreeMap<String, Order> = BTreeMap::new();
    for input in inputs {
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

/// The state-machine inputs among `entries`, in order.
pub fn inputs_of<'a>(entries: impl Iterator<Item = &'a Value>) -> Vec<RecordedInput> {
    entries
        .filter(|e| e["kind"] == "verified" && e["what"]["call"] == "input")
        .map(|e| serde_json::from_value(e["what"]["input"].clone()).expect("a recorded input"))
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
        if REFUSED_BY_THE_MACHINE_ON_PURPOSE.iter().any(|(n, _)| *n == name) {
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
