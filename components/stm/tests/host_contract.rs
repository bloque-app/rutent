//! The host-contract suite on the reference host, with the real state machine (`T-010-04`).
//!
//! The suite (`conformance/`) is typed on its own probe state machine, so a host is scored as `Harness<Probe>`
//! (`tests/reference_host.rs` runs it on the reference host). Here the state machine is [`Stm`]: it must give
//! every input of the suite's scenarios the state the probe does, and a host that runs it must keep every
//! guarantee the suite checks, which these tests check on the reference host with the same scenarios.

use rutent_conformance::host::probe::{
    ORPHAN_ORDER, fold, order_inputs, order_inputs_with_a_failed_final_deposit, orphan, reusing,
};
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_contracts::state_machine::host::{Delivery, Host, HostError};
use rutent_contracts::state_machine::{Id, Order, RecordedInput, StateMachine};
use rutent_stm::Stm;
use rutent_stm::reference_host::{MemoryStore, ReferenceHarness, ReferenceHost};

fn fold_stm<'a>(inputs: impl IntoIterator<Item = &'a RecordedInput>) -> Vec<Option<Order>> {
    let mut state: Option<Order> = None;
    inputs
        .into_iter()
        .map(|input| {
            state = match &state {
                Some(order) => Some(Stm::transition(order, input)),
                None => Stm::open(input),
            };
            state.clone()
        })
        .collect()
}

fn scenario() -> Vec<RecordedInput> {
    let mut inputs = order_inputs("ord-a", 0);
    inputs.extend(order_inputs_with_a_failed_final_deposit("ord-b", 100));
    inputs.push(orphan(200));
    inputs
}

// verifies: REQ-OPS-3, NFR-4
#[test]
fn the_state_machine_reaches_the_states_the_host_contract_suite_expects_of_its_scenarios() {
    for (name, inputs) in [
        ("an order to completion", order_inputs("ord-a", 0)),
        (
            "an order whose final deposit fails",
            order_inputs_with_a_failed_final_deposit("ord-b", 100),
        ),
        ("an input before any route", vec![orphan(200)]),
    ] {
        assert_eq!(fold_stm(&inputs), fold(&inputs), "{name}");
    }
}

// verifies: REQ-OPS-3, REQ-STM-4
#[test]
fn a_reference_host_running_the_state_machine_keeps_one_record_per_record_id() {
    let mut host = ReferenceHost::<Stm>::open(MemoryStore::new()).unwrap();
    let inputs = scenario();
    for input in &inputs {
        assert!(matches!(
            host.submit(input.clone()).unwrap(),
            Delivery::Recorded(_)
        ));
    }
    // Each delivered again: the record, and nothing recorded.
    for input in &inputs {
        assert!(matches!(
            host.submit(input.clone()).unwrap(),
            Delivery::AlreadyRecorded(_)
        ));
    }
    // Another input under a used record id is refused, by name.
    let taken = &inputs[3];
    assert!(matches!(
        host.submit(reusing(taken)),
        Err(HostError::RecordIdReused { .. })
    ));
    for order in ["ord-a", "ord-b"] {
        let journal = host.journal(&Id::new(order).unwrap()).unwrap();
        assert_eq!(journal.len(), 7, "{order}");
        assert!(
            journal
                .iter()
                .enumerate()
                .all(|(i, r)| r.seq == i as u64 + 1)
        );
    }
    assert_eq!(
        host.journal(&Id::new(ORPHAN_ORDER).unwrap()).unwrap()[0].state_after,
        None
    );
}

// verifies: REQ-OPS-3, NFR-4, REQ-STM-3
#[test]
fn a_swap_between_two_hosts_running_the_state_machine_loses_no_order_and_replays_the_same_states() {
    let mut first = ReferenceHost::<Stm>::open(MemoryStore::new()).unwrap();
    let inputs = scenario();
    let (before, after) = inputs.split_at(5);
    for input in before {
        first.submit(input.clone()).unwrap();
    }
    first.halt().unwrap();
    assert!(matches!(
        first.submit(after[0].clone()),
        Err(HostError::Halted)
    ));
    let mut second = {
        let stream = first.export().unwrap();
        ReferenceHost::<Stm>::import(MemoryStore::new(), stream).unwrap()
    };
    second.resume().unwrap();
    // A repeat from before the swap is still recognised; the rest run to their ends on the new host.
    assert!(matches!(
        second.submit(before[2].clone()).unwrap(),
        Delivery::AlreadyRecorded(_)
    ));
    for input in after {
        second.submit(input.clone()).unwrap();
    }
    for order in second.orders().unwrap() {
        let journal = second.journal(&order).unwrap();
        let states: Vec<Option<Order>> = journal.iter().map(|r| r.state_after.clone()).collect();
        let expected = fold_stm(journal.iter().map(|r| &r.input));
        assert_eq!(states, expected, "{}", order.as_str());
    }
    let done = second.state(&Id::new("ord-a").unwrap()).unwrap().unwrap();
    assert_eq!(
        done.state,
        rutent_contracts::state_machine::OrderState::Complete
    );
}

// verifies: REQ-OPS-3
#[test]
fn a_host_killed_and_reopened_keeps_what_it_recorded_with_the_state_machine() {
    let mut harness = ReferenceHarness::<Stm>::new();
    let store = harness.fresh_store().unwrap();
    let mut host = harness.start(store).unwrap();
    for input in order_inputs("ord-a", 0).iter().take(4) {
        host.submit(input.clone()).unwrap();
    }
    let store = harness.kill(host).unwrap();
    let host = harness.reopen(store).unwrap();
    let state = host.state(&Id::new("ord-a").unwrap()).unwrap().unwrap();
    assert_eq!(
        state,
        fold_stm(order_inputs("ord-a", 0).iter().take(4))
            .pop()
            .unwrap()
            .unwrap()
    );
}
