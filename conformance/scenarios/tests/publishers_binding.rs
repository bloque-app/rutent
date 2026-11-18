//! The simulated world binds each private node to its publisher in the log (`T-020-32`, `AC-4`, `AC-6`): Core appends
//! the publisher's signed `inner-segments` record as a stated entry about the order and the node, and cites its receipt
//! where it binds the node (an evidence naming the node, or the replay's `rests_on`), so a failed provider's own
//! answer inside its publisher's node completes its reversal as the publisher's word, and the reserve's confirmation of
//! its own publisher's provider stays refused (`REQ-ORD-3`).

mod execution_support;
mod failure_support;

use execution_support::*;
use failure_support::*;
use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{Attribution, Entry, Receipt, What};
use rutent_contracts::provider_role::INNER_SEGMENTS_SHAPE;
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::intake::OrderRequest;
use rutent_core_orch::reversal::{Confirmation, NotCounted, Word};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{AMOUNT, id};

const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const RAMP: &str = "sim-join-ramp";
const QUICK: &str = "sim-join-ramp-quick";
const OUT: &str = "sim-join-out";
const NODE: &str = "sim-join-ledger";
const PUBLISHER: &str = "sim-join-publisher";

fn across(name: &str, session: &Id) -> OrderRequest {
    request(
        name,
        session,
        AMOUNT,
        IN,
        outer("sim-join-in-bank", "COP"),
        outer("sim-join-out-bank", "USD"),
        OUT,
        OTHER,
        "lowest_total_cost",
    )
}

/// The order across the joined providers with its exchange failing: the withdrawal settled, the exchange's reversal
/// owed. Core, the order and the exchange's segment.
fn exchange_failed(accepts: &'static str) -> (Core, Id, Id) {
    let (mut core, session) = core_with(accepts, &[IN, EXCHANGE, QUICK, OUT]);
    core.world()
        .script_for(EXCHANGE, "conversion", Behaviour::act(Act::Fail));
    let (order, ran) = run(&mut core, across("req-binds", &session));
    let segments = segments_of(&ran);
    assert!(matches!(
        segments[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert!(matches!(segments[1].outcome, SegmentOutcome::Failed));
    (core, order.clone(), segments[1].segment.clone())
}

/// The `inner-segments` records the log holds about `order`: each with its receipt, whether it is verified, and the
/// party it is the word of, the node it is about and the transactions it lists.
fn records(core: &Core, order: &Id) -> Vec<(Receipt, Option<Id>, Option<Id>, bool)> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(receipt, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str() == INNER_SEGMENTS_SHAPE
                    && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order) =>
            {
                Some((
                    receipt.clone(),
                    s.about.as_ref().and_then(|a| a.node.clone()),
                    match &s.attribution {
                        Attribution::Verified { who } => Some(who.party.clone()),
                        Attribution::Unverified { .. } => None,
                    },
                    s.about
                        .as_ref()
                        .is_some_and(|a| a.segment.is_none() && a.transaction.is_none()),
                ))
            }
            _ => None,
        })
        .collect()
}

// verifies: REQ-ORD-3, REQ-ORD-41
#[test]
fn publishers_inner_segments_record_is_appended_as_a_stated_entry_and_cited_by_what_binds_its_node()
{
    let (core, order, _) = exchange_failed("AC-4");
    let held = records(&core, &order);
    assert_eq!(held.len(), 1, "the reserve's record of the node: {held:?}");
    let (receipt, node, party, about_nothing_else) = held[0].clone();
    assert_eq!(node, Some(id(NODE)), "about the node it records");
    assert_eq!(party, Some(id(PUBLISHER)), "verified, the publisher's");
    assert!(
        about_nothing_else,
        "no segment and no transaction of its own"
    );
    // Cited where it binds the node: an evidence of the order naming the node, resting on it.
    let binding: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Evidence {
                order: o,
                movement: Some(m),
                rests_on,
                ..
            } if o == order && m.node.is_some() => Some((m.node, rests_on)),
            _ => None,
        })
        .collect();
    assert_eq!(binding.len(), 1, "{binding:?}");
    assert_eq!(binding[0].0, Some(id(NODE)));
    assert!(binding[0].1.contains(&receipt), "{binding:?}");
}

// verifies: REQ-AUD-9, REQ-ORD-41
#[test]
fn replay_cites_the_receipt_of_each_inner_segments_record_it_read() {
    let (mut core, session) = core_with("AC-35", &[IN, EXCHANGE, RAMP, OUT]);
    let (order, ran) = run(&mut core, across("req-replay", &session));
    assert!(
        segments_of(&ran)
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. }))
    );
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    let replays: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Replay {
                order: o,
                nodes,
                rests_on,
                ..
            } if o == order => Some((nodes, rests_on)),
            _ => None,
        })
        .collect();
    assert_eq!(replays.len(), 1, "{replays:?}");
    let (nodes, rests_on) = &replays[0];
    assert_eq!(nodes.len(), 1);
    // The exit's result first, then one verified record of the node it replayed.
    assert_eq!(rests_on.len(), 2, "{rests_on:?}");
    let held = records(&core, &order);
    let cited = held
        .iter()
        .find(|(r, ..)| *r == rests_on[1])
        .expect("the log holds the record the replay cites");
    assert_eq!(cited.1.as_ref(), Some(&nodes[0]));
    assert_eq!(
        cited.2,
        Some(id(PUBLISHER)),
        "the node's publisher's, verified"
    );
}

// verifies: REQ-ORD-3
#[test]
fn failed_providers_own_answer_inside_a_node_the_log_binds_completes_its_reversal_as_its_publishers_word()
 {
    let (mut core, order, exchange) = exchange_failed("AC-4");
    let asked = core.ask_reversal(&order, &exchange).expect("asked");
    let completed = asked.completed.expect("the node is bound to its publisher");
    assert_eq!(completed.word, Word::Publishers(id(PUBLISHER)));
    let labelled: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Evidence {
                order: o,
                publisher: Some(p),
                grade,
                ..
            } if o == order => Some((p, grade)),
            _ => None,
        })
        .collect();
    assert_eq!(
        labelled,
        [(
            id(PUBLISHER),
            rutent_contracts::provider_role::EvidenceGrade::Statement
        )]
    );
}

// verifies: REQ-ORD-3
#[test]
fn reserves_confirmation_of_its_own_publishers_providers_reversal_stays_refused() {
    let (mut core, order, exchange) = exchange_failed("AC-4");
    core.world().script(IN, Behaviour::crediting_a_reversal());
    let done = core.confirm_reversal(&order, &exchange).expect("asked");
    assert!(
        matches!(
            done,
            Confirmation::NotCompleted {
                why: NotCounted::SamePublisher,
                ..
            }
        ),
        "{done:?}"
    );
    assert!(core.completed_reversal(&order, &exchange).is_none());
}

// verifies: REQ-ORD-41
#[test]
fn a_book_spanning_two_orders_binds_nothing_for_the_second_and_stops_nothing() {
    // The reserve's book holds the first order's withdrawal when the second enters the node, so its record lists a
    // segment the second order didn't run: the log would not verify it about the second order, so it isn't appended
    // or cited (fail closed), and the second order still runs.
    let (mut core, session) = core_with("AC-35", &[IN, EXCHANGE, RAMP, OUT]);
    let (first, _) = run(&mut core, across("req-first", &session));
    assert_eq!(state(&mut core, &first), OrderState::Complete);
    let (second, ran) = run(&mut core, across("req-second", &session));
    assert!(
        segments_of(&ran)
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{ran:?}"
    );
    assert_eq!(records(&core, &first).len(), 2, "the first order's");
    assert!(
        records(&core, &second).is_empty(),
        "{:?}",
        records(&core, &second)
    );
}
