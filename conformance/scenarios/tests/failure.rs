//! Failure and waiting scenarios over the assembled Core against the simulated providers (`T-024-06`): an order that
//! waits for the customer at its floor, after a settled segment (`AC-42`) and after a recalculation (`AC-12`), a final
//! deposit the destination refuses (`AC-40`), and a provider that stops answering (`AC-5`). Each drives Core's own
//! `request`, `quote`, `accept` and `run`, and its recalculation, watch and notices, over [`execution_support`]'s world.

mod execution_support;
mod failure_support;

use execution_support::*;
use failure_support::*;
use rutent_conformance::sim::{Act, Behaviour, Delivery};
use rutent_contracts::core_internal::{Action, What};
use rutent_contracts::router_api::DropReason;
use rutent_contracts::state_machine::{Observed, OrderState, SegmentState};
use rutent_core_orch::recalc::{NotRecalculable, Recalculation};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{self, AMOUNT, RESERVE};

/// Whether `observed` is a wait at the customer's floor.
fn is_floor_wait(observed: &Observed) -> bool {
    matches!(observed, Observed::WaitingAtFloor {})
}

/// How many operations the reserve was handed.
fn handled(core: &Core) -> usize {
    core.fleet().platform.operations_received(RESERVE)
}

/// How many moves of value the log holds: what Core handed to a provider.
fn effects(core: &Core) -> usize {
    entries(core)
        .iter()
        .filter(|v| {
            matches!(
                v.what,
                What::Effect {
                    action: Action::MoveValue { .. },
                    ..
                }
            )
        })
        .count()
}

/// What the customer is told, in order, of `order`, as `(change, state after it)`.
fn told_to_the_user(
    core: &mut Core,
    order: &rutent_contracts::router_api::Id,
) -> Vec<(String, OrderState)> {
    core.notify().expect("the notices read the log");
    changes(&core.notices_to(&Core::user_receiver(), order))
}

/// What the operator is told, in order, of `order`.
fn told_to_the_operator(
    core: &mut Core,
    order: &rutent_contracts::router_api::Id,
) -> Vec<(String, OrderState)> {
    core.notify().expect("the notices read the log");
    changes(&core.notices_to(&core.operator_receiver(), order))
}

// verifies: AC-42
#[test]
fn settled_segment_below_the_floor_makes_the_order_wait_there() {
    // The withdrawal delivers 1 % short: 990 000 at the reserve's ledger, and the deposit still ahead, quoted at no
    // charge, would leave 990 000. Under a floor of 995 000 the order waits for the customer after the withdrawal,
    // before the deposit is handed over: at the settled segment, with no recalculation in between.
    let (mut core, session) = core_with("AC-42", &[RESERVE]);
    core.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::delivering(Delivery::Short { bps: 100 }),
    );
    let order = accepted(
        &mut core,
        floored(
            within("req-below", &session, AMOUNT, RESERVE, OTHER),
            "995000",
        ),
    );
    let ran = core.run(&order).expect("the order ran");
    let segments = segments_of(&ran);
    assert_eq!(segments.len(), 1, "only the withdrawal is handed over");
    assert!(matches!(
        segments[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert_eq!(
        core.world().journal(RESERVE).len(),
        1,
        "the reserve was handed the withdrawal and nothing after it"
    );
    assert_eq!(state(&mut core, &order), OrderState::WaitingForCustomer);
    assert_eq!(
        count_inputs(&core, &order, is_floor_wait),
        1,
        "one wait, recorded on the value the settled segment evidenced"
    );
    assert_eq!(
        count_inputs(&core, &order, |o| matches!(
            o,
            Observed::RouteRecalculated { .. } | Observed::NoRoute { .. }
        )),
        0,
        "no recalculation was tried: the wait is at the settled segment"
    );

    // It is visible to the customer, with its age; and to the operator, who is told as the customer is.
    core.clock.advance(3_600);
    let status = core
        .status(&kit::customer(), &order)
        .expect("the customer reads its order");
    assert_eq!(status.state, OrderState::WaitingForCustomer);
    assert!((3_600..3_602).contains(&status.age_seconds), "{status:?}");
    let to_user = told_to_the_user(&mut core, &order);
    assert_eq!(
        to_user.last(),
        Some(&(
            "waiting_at_floor".to_owned(),
            OrderState::WaitingForCustomer
        ))
    );
    assert_eq!(
        told_to_the_operator(&mut core, &order).last(),
        to_user.last(),
        "the operator is told what the customer is"
    );

    // Nothing more moves while it waits: run again, and the deposit is not handed over.
    let before = (handled(&core), effects(&core));
    let _ = core.run(&order);
    assert_eq!((handled(&core), effects(&core)), before);

    // Above the floor, the same run goes on to the deposit, and the order completes.
    let (mut core, session) = core_with("AC-42", &[RESERVE]);
    core.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::delivering(Delivery::Short { bps: 100 }),
    );
    let (order, ran) = run(
        &mut core,
        floored(
            within("req-above", &session, AMOUNT, RESERVE, OTHER),
            "985000",
        ),
    );
    assert_eq!(segments_of(&ran).len(), 2);
    assert_eq!(count_inputs(&core, &order, is_floor_wait), 0);
    assert_eq!(state(&mut core, &order), OrderState::Complete);
}

// verifies: AC-12
#[test]
fn recalculation_past_the_floor_makes_the_order_wait_visibly_with_its_age() {
    // The deposit is refused finally and the withdrawal's value rests at the ledger in full. The reserve's price has
    // moved since the quote: a deposit now costs 10 000, so the only route from the ledger would leave the customer
    // 990 000, below its floor of 995 000.
    let (mut core, session) = core_with("AC-12", &[RESERVE]);
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let order = accepted(
        &mut core,
        floored(
            within("req-floor", &session, AMOUNT, RESERVE, OTHER),
            "995000",
        ),
    );
    let ran = core.run(&order).expect("the order ran");
    let segments = segments_of(&ran);
    assert!(matches!(
        segments[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert!(matches!(segments[1].outcome, SegmentOutcome::Refused(_)));
    assert_eq!(state(&mut core, &order), OrderState::FirstSegmentSettled);

    core.reprice_deposits(RESERVE, flat_fee("10000"));
    let done = core.recalculate(&order).expect("the recalculation ran");
    let Recalculation::WaitsForCustomer { record } = &done else {
        panic!("a route past the floor makes the order wait: {done:?}");
    };
    assert!(
        record
            .record
            .candidates()
            .iter()
            .all(|c| c.dropped == Some(DropReason::BelowFloor {})),
        "every candidate was dropped as below the floor: {:?}",
        record.record.candidates()
    );
    assert!(
        !record.record.candidates().is_empty(),
        "a route was weighed"
    );
    assert_eq!(state(&mut core, &order), OrderState::WaitingForCustomer);
    assert_eq!(
        count_inputs(&core, &order, is_floor_wait),
        1,
        "the wait is recorded"
    );
    assert_eq!(
        count_inputs(&core, &order, |o| matches!(
            o,
            Observed::RouteRecalculated { .. }
        )),
        0,
        "no route was taken past the floor"
    );

    // Visible to the customer with its age, and told to it and to the operator.
    core.clock.advance(3_600);
    let status = core
        .status(&kit::customer(), &order)
        .expect("the customer reads its order");
    assert_eq!(status.state, OrderState::WaitingForCustomer);
    assert!((3_600..3_602).contains(&status.age_seconds), "{status:?}");
    let to_user = told_to_the_user(&mut core, &order);
    assert_eq!(
        to_user.last(),
        Some(&(
            "waiting_at_floor".to_owned(),
            OrderState::WaitingForCustomer
        ))
    );
    assert_eq!(
        told_to_the_operator(&mut core, &order).last(),
        to_user.last()
    );

    // While it waits, nothing is asked of anyone, nothing is recorded and no route starts.
    let before = (handled(&core), core.log().entries().len());
    assert_eq!(
        core.recalculate(&order).expect("tried"),
        Recalculation::NotNow(NotRecalculable::Held(OrderState::WaitingForCustomer))
    );
    assert_eq!((handled(&core), core.log().entries().len()), before);

    // With the price as quoted, the same failure is recalculated and goes on: the floor is what made it wait.
    let (mut core, session) = core_with("AC-12", &[RESERVE]);
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let order = accepted(
        &mut core,
        floored(
            within("req-above", &session, AMOUNT, RESERVE, OTHER),
            "995000",
        ),
    );
    core.run(&order).expect("the order ran");
    assert!(matches!(
        core.recalculate(&order).expect("the recalculation ran"),
        Recalculation::Recalculated(_)
    ));
    assert_eq!(count_inputs(&core, &order, is_floor_wait), 0);
}

// verifies: AC-40
#[test]
fn refused_final_deposit_is_recalculated_first_and_then_waits_for_the_customer() {
    // The destination refuses the final deposit. The order isn't rolled back: the withdrawal stays settled, no reversal
    // is asked, and the value rests at the ledger.
    let (mut core, session) = core_with("AC-40", &[RESERVE]);
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let (order, ran) = run(
        &mut core,
        within("req-refused", &session, AMOUNT, RESERVE, OTHER),
    );
    let segments = segments_of(&ran);
    assert!(matches!(
        segments[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert!(matches!(segments[1].outcome, SegmentOutcome::Refused(_)));
    let followed = core.followed(&order).expect("Core follows the order");
    assert_eq!(followed.state, OrderState::FirstSegmentSettled);
    assert_eq!(followed.parts[0].segments[0].state, SegmentState::Settled);
    assert_eq!(
        count_inputs(&core, &order, |o| matches!(
            o,
            Observed::ReversalCompleted { .. } | Observed::WithdrawalCancelled {}
        )),
        0,
        "nothing is rolled back"
    );

    // Recalculation to the same destination comes first: the next deposit goes to the authorisation's own
    // destination, in its to asset, and settles.
    let Recalculation::Recalculated(recalculated) = core.recalculate(&order).expect("tried") else {
        panic!("a route to the same destination was found");
    };
    assert_eq!(
        Some(recalculated.route.destination.clone()),
        core.requested_destination(&order),
        "to the same destination: the same reserve, node and account"
    );
    let ran = core
        .run_recalculated(&order, &recalculated)
        .expect("the recalculated part runs");
    assert!(matches!(
        segments_of(&[ran])[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    assert_eq!(
        count_inputs(&core, &order, |o| matches!(
            o,
            Observed::DestinationChanged { .. }
        )),
        0,
        "no other destination was taken"
    );

    // With no route left to it, the order waits for the customer where the value sits: visibly, with its age, never
    // stuck, and nothing more moves without the customer.
    let (mut core, session) = core_with("AC-40", &[RESERVE]);
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let (order, _) = run(
        &mut core,
        within("req-nowhere", &session, AMOUNT, RESERVE, OTHER),
    );
    // The reserve refuses the dry run of any deposit from now on: no route to the destination is left.
    core.world()
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let outcome = core.recalculate(&order).expect("tried");
    assert!(
        matches!(outcome, Recalculation::NoRoute { .. }),
        "no route to the destination: {outcome:?}"
    );
    let recorded = core
        .record_no_route(&order, &outcome)
        .expect("no route recorded");
    assert_eq!(recorded.state, OrderState::WaitingForCustomer);
    assert!(!core.is_stuck(&order), "it is the customer's, not stuck");
    assert!(core.stuck_values().is_empty(), "nor on the operator's view");
    core.clock.advance(600);
    let status = core
        .status(&kit::customer(), &order)
        .expect("the customer reads its order");
    assert_eq!(status.state, OrderState::WaitingForCustomer);
    assert!((600..602).contains(&status.age_seconds), "{status:?}");
    let followed = core.followed(&order).expect("Core follows the order");
    assert_eq!(followed.parts[0].segments[0].state, SegmentState::Settled);
    assert_eq!(
        count_inputs(&core, &order, |o| matches!(
            o,
            Observed::ReversalCompleted { .. } | Observed::WithdrawalCancelled {}
        )),
        0,
        "nothing is rolled back"
    );
    let to_user = told_to_the_user(&mut core, &order);
    assert_eq!(
        to_user.last(),
        Some(&("no_route".to_owned(), OrderState::WaitingForCustomer))
    );

    let before = (handled(&core), core.log().entries().len());
    // Held where it waits, and, with no route found, routed no more until the customer chooses.
    let again = core.recalculate(&order).expect("tried");
    assert!(
        matches!(
            again,
            Recalculation::NotNow(NotRecalculable::Held(OrderState::WaitingForCustomer))
                | Recalculation::NoRouteLeft { .. }
        ),
        "no route starts on its own: {again:?}"
    );
    let _ = core.run(&order);
    assert_eq!(
        (handled(&core), effects(&core)),
        (before.0, 2),
        "nothing more is handed over: the withdrawal's effect and the refused deposit's, and no third"
    );
}

/// The seconds each operation takes in the p99 scenario, and what twice it is.
const TAKES: u64 = 10;

/// The reserve settles twenty orders; the next one's deposit is taken by the provider, which stops answering; past
/// twice its p99 the order is stuck. Core, the order and the deposit's segment.
fn stuck_by_silence() -> (
    Core,
    rutent_contracts::router_api::Id,
    rutent_contracts::router_api::Id,
) {
    use rutent_contracts::core_internal::StuckBasis;
    use rutent_core_orch::stuck::{HISTORY, Watch};

    // The reserve settles twenty orders, each operation taking 10 s, so each of its tuples has a history and its p99 is
    // 10 s (the customer's session outlasts the twenty).
    let (mut core, session) = core_with("AC-5", &[RESERVE]);
    core.set_check_bound(1_000);
    core.faults.operation_takes = TAKES;
    for n in 0..HISTORY {
        let request = within(&format!("req-h-{n}"), &session, AMOUNT, RESERVE, OTHER);
        let told = core.request(request.clone()).expect("the request is taken");
        assert!(Core::order_of(&told).is_some(), "order {n}: {told:?}");
        let (_, ran) = run(&mut core, request);
        assert!(
            segments_of(&ran)
                .iter()
                .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
            "{ran:?}"
        );
    }
    // The next order's deposit is taken by the provider, which then stops answering.
    core.faults.operation_takes = 0;
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::late());
    let (order, ran) = run(
        &mut core,
        within("req-silent", &session, AMOUNT, RESERVE, OTHER),
    );
    let segments = segments_of(&ran);
    assert!(matches!(
        segments[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert!(
        matches!(segments[1].outcome, SegmentOutcome::Unknown(_)),
        "the deposit's outcome is not known: {segments:?}"
    );
    let deposit = segments[1].segment.clone();
    let carried = core
        .followed(&order)
        .and_then(|o| {
            o.parts[0]
                .segments
                .iter()
                .find(|s| s.segment_id.as_str() == deposit.as_str())
                .map(|s| s.state)
        })
        .expect("the segment is followed");
    assert_eq!(carried, SegmentState::OutcomeUnknown);

    // Exactly twice its p99 is not over it; a second more is. The clock is Core's: nothing else says the time passed.
    core.clock.advance(2 * TAKES - 1);
    assert_eq!(
        core.watch(&order).expect("watched"),
        Watch::Running,
        "not yet twice its p99"
    );
    assert_eq!(state(&mut core, &order), OrderState::FirstSegmentSettled);
    core.clock.advance(2 * TAKES + 5);
    let Watch::Stuck(called) = core.watch(&order).expect("watched") else {
        panic!("stuck past twice the p99");
    };
    assert_eq!(
        (
            called.provider.as_str(),
            called.segment.as_str(),
            called.basis
        ),
        (RESERVE, deposit.as_str(), StuckBasis::TwiceP99)
    );
    assert!(
        (TAKES..=TAKES + 2).contains(&called.figure_seconds),
        "the p99 is the history's: {called:?}"
    );
    assert!(
        called.held_seconds > 2 * called.figure_seconds,
        "{called:?}"
    );

    // The order is stuck; the operator is warned and the customer told, and both read it so.
    assert_eq!(state(&mut core, &order), OrderState::Stuck);
    assert!(core.is_stuck(&order));
    let to_operator = told_to_the_operator(&mut core, &order);
    assert_eq!(
        to_operator.last(),
        Some(&("stuck_declared".to_owned(), OrderState::Stuck)),
        "the operator is warned"
    );
    assert_eq!(
        told_to_the_user(&mut core, &order).last(),
        Some(&("stuck_declared".to_owned(), OrderState::Stuck)),
        "the customer is told"
    );
    let values = core.stuck_values();
    assert_eq!(values.len(), 1, "the operator sees what it holds");
    assert_eq!(values[0].order, order);
    assert_eq!(values[0].amount.as_str(), AMOUNT);

    // The provider reads it too, on its own segment, and a provider that ran none of the order's is refused.
    let reserve_desk = rutent_contracts::core_internal::Actor {
        party: kit::id(RESERVE),
        acts: kit::id("sim-reserve-a-desk"),
        role: rutent_contracts::core_internal::Role::Provider,
        kind: rutent_contracts::core_internal::ActorKind::Person,
    };
    core.registers().list(&reserve_desk);
    let seen = core
        .seen_by_provider(&reserve_desk, &order, &deposit)
        .expect("the provider reads its segment");
    assert_eq!(
        (seen.part_state, seen.segment_state),
        (OrderState::Stuck, SegmentState::OutcomeUnknown)
    );

    // No new route starts: nothing is recalculated, nothing is handed over, nothing is asked of the provider.
    let before = (handled(&core), core.log().entries().len());
    assert_eq!(
        core.recalculate(&order).expect("tried"),
        Recalculation::NotNow(NotRecalculable::Held(OrderState::Stuck))
    );
    let _ = core.run(&order);
    assert_eq!(handled(&core), before.0, "nothing is handed over");
    (core, order, deposit)
}

// verifies: AC-5
#[test]
fn provider_silent_past_twice_its_p99_makes_the_order_stuck_and_a_late_delivery_moves_nothing_twice()
 {
    let (core, order, _) = stuck_by_silence();
    assert!(core.is_stuck(&order));
}

// verifies: AC-5
#[test]
#[ignore = "blocked: components/core-orch/src/reversal/journal.rs rejects the audit log's receipt for a verified stated entry; see F-024 notes, T-024-06"]
fn a_late_delivery_moves_nothing_twice() {
    use rutent_core_orch::stuck::HISTORY;

    let (mut core, order, deposit) = stuck_by_silence();
    // The provider's answer comes late: it is kept as its maker's statement and moves nothing, once. What is
    // recorded is the statement alone: no input, no effect, no change of state, no value moved.
    let late = core
        .provider_again(&order, &deposit)
        .expect("the provider answers again, as it did");
    let (log_before, inputs_before, effects_before) = (
        core.log().entries().len(),
        core.inputs().len(),
        effects(&core),
    );
    let held = core.stuck_values();
    let kept = core
        .late_delivery(&order, &deposit, &late)
        .expect("a late delivery is kept");
    let twice = core
        .late_delivery(&order, &deposit, &late)
        .expect("kept once");
    assert_eq!(kept, twice, "the same entry the second time");
    assert_eq!(
        core.log().entries().len(),
        log_before + 1,
        "one statement, once"
    );
    assert_eq!(
        core.inputs().len(),
        inputs_before,
        "no input: the state machine takes nothing"
    );
    assert_eq!(effects(&core), effects_before, "no value moves");
    assert_eq!(
        state(&mut core, &order),
        OrderState::Stuck,
        "the order is as it was"
    );
    assert_eq!(
        core.stuck_values(),
        held,
        "what the operator holds is as it was"
    );
    assert_eq!(
        core.world().journal(RESERVE).len(),
        2 * (HISTORY + 1),
        "the provider's books grew only by its own deposit"
    );
}
