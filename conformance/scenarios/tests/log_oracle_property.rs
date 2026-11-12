//! The differential oracle's property half (`T-020-01`, `DEC-18`): over the seeded generator's runs, the log's own
//! reading of an order's inputs ([`state_machine_takes`]) agrees with the reference state machine on every input,
//! both ways. Where the log takes an input the machine refuses, the log is lenient and holds an input the order never
//! took; where it refuses one the machine takes, Core's append would fail on an order the machine moved.
//!
//! Each run's inputs go to the log's history only where the machine took them, as Core records them, with noise
//! between them: another order's inputs and entries that aren't inputs.
//!
//! It lives here, not in `CMP-STM`'s tests, since an outer library never names core-internal (`DEC-2`, the boundary
//! check); it drives `CMP-STM`'s own seeded generator, by path. The fixture replay, which reads the log's fixtures as
//! JSON, is `CMP-STM`'s `tests/log_oracle.rs`.

#[path = "../../../components/stm/tests/common/mod.rs"]
mod common;

use common::generator::{Answer, traces};
use rutent_contracts::core_internal::{Entry, Receipt, state_machine_takes};
use rutent_contracts::state_machine::{Observed, Outcome, RecordedInput, StateMachine};
use rutent_stm::Stm;
use serde_json::{Value, json};

const SEEDS: u64 = 600;

fn fixture(name: &str) -> Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/core-internal/fixtures/valid")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn receipt(index: usize) -> Receipt {
    serde_json::from_value(json!({
        "log": "log-main",
        "index": index,
        "entry": {"algorithm": "sha-256", "value": format!("{:064x}", index + 1)},
    }))
    .unwrap()
}

/// The verified entry Core appends for `input`.
fn entry_of(input: &RecordedInput) -> Entry {
    let mut entry = fixture("verified-entry-of-a-recorded-input-route-chosen-three-parts");
    entry["what"]["input"] = serde_json::to_value(input).unwrap();
    serde_json::from_value(entry).unwrap()
}

/// The machine's verdict on a step: it applied the input, or recorded it late; a repeat is the host's to answer.
fn machine_takes(
    prev: &Option<rutent_contracts::state_machine::Order>,
    input: &RecordedInput,
) -> bool {
    match prev {
        None => {
            !matches!(input.observed, Observed::RouteChosen { .. }) || Stm::open(input).is_some()
        }
        Some(state) => matches!(
            Stm::transition(state, input).last_input.outcome,
            Outcome::Applied {} | Outcome::Late {}
        ),
    }
}

// verifies: INV-4, REQ-STM-3, DEC-18
#[test]
fn the_log_takes_exactly_the_inputs_the_state_machine_takes() {
    let noise = fixture("verified-entry-of-a-permission-granted");
    let mut compared = 0;
    let mut kinds = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for trace in traces(SEEDS) {
        let mut history: Vec<(Receipt, Entry)> = Vec::new();
        for (n, step) in trace.steps.iter().enumerate() {
            if step.answer == Answer::AlreadyRecorded {
                continue;
            }
            let input = &step.input;
            if n % 3 == 0 {
                history.push((
                    receipt(history.len()),
                    serde_json::from_value(noise.clone()).unwrap(),
                ));
            }
            if input.subject.order.as_str() != trace.order_id {
                // Another order's input, which has no route here: recorded with no state.
                assert!(state_machine_takes(&history, input), "seed {}", trace.seed);
                history.push((receipt(history.len()), entry_of(input)));
                continue;
            }
            if let Some(state) = &step.prev
                && Stm::transition(state, input).last_input.outcome == (Outcome::Repeat {})
            {
                continue;
            }
            // The host's exactly-once: a record id the log holds names one content.
            let reused = history.iter().any(|(_, e)| {
                matches!(e, Entry::Verified(v) if matches!(&v.what,
                    rutent_contracts::core_internal::What::Input { input: earlier }
                        if earlier.record_id == input.record_id && earlier != input))
            });
            let machine = !reused && machine_takes(&step.prev, input);
            let log = state_machine_takes(&history, input);
            compared += 1;
            if machine {
                kinds.insert(format!("{:?}", std::mem::discriminant(&input.observed)));
            }
            if machine != log {
                failures.push(format!(
                    "seed {} step {n}: machine {machine}, log {log}: {:?} on {:?}",
                    trace.seed, input.observed, input.subject
                ));
            }
            if machine {
                history.push((receipt(history.len()), entry_of(input)));
            }
        }
    }
    assert!(compared > 1000, "only {compared} inputs compared");
    assert!(
        kinds.len() >= 12,
        "only {} kinds taken: {kinds:?}",
        kinds.len()
    );
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// verifies: REQ-STM-4, INV-8
#[test]
fn the_log_refuses_another_input_under_a_record_id_it_holds_as_the_host_does() {
    let trace = &traces(SEEDS)[0];
    let opening = &trace.steps[0].input;
    let history = vec![(receipt(0), entry_of(opening))];
    assert!(
        state_machine_takes(&history, opening),
        "the same input again"
    );
    let mut other = opening.clone();
    other.recorded_at =
        rutent_contracts::state_machine::Timestamp::new("2026-11-06T10:00:09Z").unwrap();
    assert!(
        !state_machine_takes(&history, &other),
        "another input under its record id"
    );
}
