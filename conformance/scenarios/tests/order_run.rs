//! Core's order run, in `core-orch` (`T-020-61`, #143's first agreed blocker): an accepted order runs on Core's ticks with no
//! test's call, a started order resumes after a restart, a held one never starts, a failed deposit's recalculated route and a
//! linked order run on Core's ticks, and the stuck commands are Core's entry points. Every step here is the deployment's: a
//! request, a quote, an acceptance, the timer's tick, an operator's command; what carries an order out is Core's.

use rutent_contracts::core_internal::{Action, Entry, What};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::recalc::Recalculation;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE};
use rutent_scenarios::on_tick::OnTick;

fn scenario() -> Scenario {
    Scenario::new(
        "Core's order run",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
}

/// An order of `name` requested, quoted and accepted by the customer in a session of its own, and nothing else.
fn accepted(core: &mut Core, name: &str) -> Id {
    let customer = kit::customer();
    let session = core.open_session(&customer).expect("a session");
    let order = Core::order_of(&core.request(base_request(name, AMOUNT, &session)).unwrap())
        .expect("an order");
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = core.quote(&customer, &order).expect("quoted")
    else {
        panic!("quoted")
    };
    core.accept(&customer, &order, &digest).expect("accepted");
    order
}

/// Every `move_value` effect the log holds of `order`.
fn moves_of(core: &Core, order: &Id) -> usize {
    core.verified_whats()
        .iter()
        .filter(|w| {
            matches!(w, What::Effect { action: Action::MoveValue { order: o, .. }, .. } if o == order)
        })
        .count()
}

/// The state of `order` as Core follows it.
fn state(core: &mut Core, order: &Id) -> OrderState {
    core.status(&kit::customer(), order)
        .expect("a status")
        .state
}

/// The reviewer's probe: an accepted order, three ticks and no test's call of a run. Core's own run hands the reserve its
/// withdrawal on the first tick, and the order completes on Core's ticks alone, its executed price reported once.
// verifies: AC-1, REQ-ORD-40
#[test]
fn accepted_order_runs_on_ticks_with_no_test_caller() {
    let (mut core, _) = scenario().core(1, &mut Reference);
    let order = accepted(&mut core, "req-probe");
    assert!(
        core.started(&order).is_some(),
        "intake's accept path holds what it runs on"
    );
    let asked = core.operations_received(RESERVE);
    assert_eq!(moves_of(&core, &order), 0, "nothing runs before a tick");
    for _ in 0..3 {
        core.tick();
    }
    assert!(
        core.operations_received(RESERVE) >= asked + 2,
        "the withdrawal and the deposit were handed over"
    );
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    assert_eq!(moves_of(&core, &order), 2, "each segment moved once");
    assert!(
        core.price_reported(&order).is_some(),
        "its executed price is reported"
    );
}

/// An order accepted and started, Core and its platform restarted before its run: the restarted Core rebuilds what the order
/// runs on from the log (its record held to the log's digest) and its run resumes it on its ticks: the order completes, each
/// segment moved once, under a transaction of its own (`NFR-6`, `INV-8`).
// verifies: NFR-6, INV-8
#[test]
fn started_order_resumes_after_a_restart() {
    let scenario = scenario();
    let (mut core, _) = scenario.core(1, &mut Reference);
    let order = accepted(&mut core, "req-resume");
    core.tick();
    let moved = moves_of(&core, &order);
    assert!(moved >= 1, "the withdrawal moved before the restart");
    let mut core = scenario.restart(core, 1, &mut Reference);
    assert!(core.started(&order).is_some(), "rebuilt from the log");
    for _ in 0..3 {
        core.tick();
    }
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    assert_eq!(moves_of(&core, &order), 2, "no segment moved twice");
    let segments: std::collections::BTreeSet<String> = core
        .verified_whats()
        .iter()
        .filter_map(|w| match w {
            What::Effect {
                action:
                    Action::MoveValue {
                        order: o, segment, ..
                    },
                ..
            } if o == &order => Some(segment.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(segments.len(), 2, "one move of each segment");
}

/// The inputs statement an accepted order's `order_opened` names unreadable as Core reopens: the order is held, and Core's
/// own resume gives it nothing to run on, however often it ticks (`NFR-6`, `T-020-46`, `T-020-52`).
// verifies: NFR-6
#[test]
fn held_order_accepted_before_a_restart_never_starts_after_core_reopens() {
    let scenario = scenario();
    let (mut core, _) = scenario.core(1, &mut Reference);
    let order = accepted(&mut core, "req-held");
    let inputs = inputs_of(&core, &order);
    let mut core = scenario.reopen(core, 1, &mut Reference);
    core.intake_reopened_without(&inputs);
    let asked = core.operations_received(RESERVE);
    for _ in 0..3 {
        core.tick();
    }
    assert!(core.started(&order).is_none(), "held: nothing to run on");
    assert_eq!(
        core.operations_received(RESERVE),
        asked,
        "nothing handed over"
    );
    assert_eq!(moves_of(&core, &order), 0);
}

/// The same, Core and its platform restarted: the platform's releases confirmed and the run resumed, the held order still
/// never starts, while an order accepted beside it and not held runs to its end.
// verifies: NFR-6
#[test]
fn held_order_accepted_before_core_restarts_never_starts_after_the_run_resumes() {
    let scenario = scenario();
    let (mut core, _) = scenario.core(1, &mut Reference);
    let held = accepted(&mut core, "req-held");
    let runs = accepted(&mut core, "req-runs");
    let inputs = inputs_of(&core, &held);
    let mut core = scenario.restart(core, 1, &mut Reference);
    core.intake_reopened_without(&inputs);
    rutent_core_orch::schedule::resume(&mut core.ports());
    for _ in 0..4 {
        core.tick();
    }
    assert!(core.started(&held).is_none(), "held: nothing to run on");
    assert_eq!(moves_of(&core, &held), 0, "nothing of it moved");
    assert_eq!(
        state(&mut core, &runs),
        OrderState::Complete,
        "the other runs on"
    );
}

/// The commitment of the inputs statement `order`'s `order_opened` names.
fn inputs_of(core: &Core, order: &Id) -> rutent_contracts::module_interface::Digest {
    core.verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::OrderOpened {
                order: o,
                inputs: Some(inputs),
                ..
            } if &o == order => Some(inputs),
            _ => None,
        })
        .expect("the order's inputs statement")
}

/// The final deposit refused, having moved nothing: Core's next tick recalculates the part from where its value is
/// evidenced, at the user's cost, and the tick after runs the recalculated route to completion; a second tick before it ran
/// recalculates nothing again (`REQ-ORD-22`, `REQ-ORD-4`).
// verifies: REQ-ORD-22, REQ-ORD-4
#[test]
fn failed_deposits_recalculated_route_runs_on_ticks() {
    let (mut core, _) = scenario().core(1, &mut Reference);
    let order = accepted(&mut core, "req-fails");
    core.world().script_for(
        RESERVE,
        "deposit",
        rutent_conformance::sim::Behaviour::act(rutent_conformance::sim::Act::Refuse),
    );
    core.tick();
    let recalculated = core
        .recalculated_on_tick(&order)
        .expect("the tick recalculates the part");
    assert!(
        matches!(recalculated, Recalculation::Recalculated(_)),
        "{recalculated:?}"
    );
    let inputs = |core: &Core| {
        core.inputs()
            .into_iter()
            .filter(|i| {
                matches!(
                    i.observed,
                    rutent_contracts::state_machine::Observed::RouteRecalculated { .. }
                )
            })
            .count()
    };
    assert_eq!(inputs(&core), 1);
    core.world().script_for(
        RESERVE,
        "deposit",
        rutent_conformance::sim::Behaviour::act(rutent_conformance::sim::Act::Answer),
    );
    for _ in 0..3 {
        core.tick();
    }
    assert_eq!(inputs(&core), 1, "recalculated once");
    assert_eq!(state(&mut core, &order), OrderState::Complete);
}

/// A part recalculated whose new route Core can't name now (its record not held: #316's round-3 L7) is never timed on
/// another route: Core's tick declares it left with no route, on the last route the log names for it, and runs nothing of
/// it: a failed final deposit's part then waits for its customer, any other is stuck for the operator.
// verifies: REQ-ORD-28, REQ-ORD-29
#[test]
fn part_whose_current_route_core_cannot_name_is_left_no_route_on_its_tick() {
    let (mut core, _) = scenario().core(1, &mut Reference);
    let order = accepted(&mut core, "req-unnamed");
    core.world().script_for(
        RESERVE,
        "deposit",
        rutent_conformance::sim::Behaviour::act(rutent_conformance::sim::Act::Refuse),
    );
    core.tick();
    let Ok(Recalculation::Recalculated(recalculated)) = core.recalculated_on_tick(&order) else {
        panic!("recalculated")
    };
    core.forget_decision_record(recalculated.record.record_id());
    let moves = moves_of(&core, &order);
    let round = core.tick();
    assert!(
        round
            .watched
            .iter()
            .any(|w| w.order == order
                && matches!(&w.found, Err(why) if why.contains("declared stuck"))),
        "{:?}",
        round.watched
    );
    // Left no route on the last route the log names: a failed final deposit's part waits for its customer
    // (`REQ-ORD-43`), any other is stuck for the operator; neither is timed or run on another route.
    let held = core.followed(&order).expect("followed").parts[0].state;
    assert!(
        matches!(held, OrderState::Stuck | OrderState::WaitingForCustomer),
        "{held:?}"
    );
    assert_eq!(moves_of(&core, &order), moves, "nothing of it ran");
}

/// The log's entries of `what` kind about `order`, for the linked order's tests.
#[allow(dead_code)]
fn entries_about(core: &Core, order: &Id) -> Vec<Entry> {
    core.log()
        .entries()
        .iter()
        .map(|(_, e)| e.clone())
        .filter(|e| match e {
            Entry::Stated(s) => s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order),
            Entry::Verified(_) => true,
        })
        .collect()
}
