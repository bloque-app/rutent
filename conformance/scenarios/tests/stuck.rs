//! Stuck orders on the running platform with simulated providers (`T-020-11`): a part a recalculation left no
//! route recorded and stuck with nothing rolled back, a segment's funds timed against its tuple's p99 or its
//! advertised time, a material change in a p99 raised, the stuck state seen by the customer, the provider and the
//! operator, and a resolution decided by one party and moved by another. Over `T-020-08`'s kit, with policy's engine
//! deciding each move and the log replayed as the log itself would admit it; Core's clock is the kit's, moved by the
//! tests, so the timing ones are simulation tests.

mod segments_support;

use std::rc::Rc;

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{
    Action, Answer, Decided, Destination, Difference, Outcome, Receipt, Resolution, StuckBasis,
    What,
};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Id, PositiveAmount};
use rutent_contracts::state_machine::{Observed, OrderState, SegmentState};
use rutent_core_authz::engine::Unbuilt;
use rutent_core_orch::recalc::{NotRecalculable, Recalculation};
use rutent_core_orch::segments::{
    Accounted, NoProxy, Ran, SegmentOutcome, SegmentsError, StatusError,
};
use rutent_core_orch::stuck::{
    Confirming, ConfirmingSegment, Deciding, HISTORY, Moved, Moving, NoRouteLeft, Reporting,
    ResolveError, Resolved, Stuck, StuckError, StuckValue, Watch, seen_by_provider,
};
use segments_support::rerouting::Rerouting;
use segments_support::resolving::{
    Moves, PermitsEveryStep, Raises, destination, follow, on_parts, origin, recorded, staff, watch,
};
use segments_support::*;

fn segments(ran: &Ran) -> &[Accounted] {
    match ran {
        Ran::Segments(done) => done,
        Ran::Refused { refused, .. } => panic!("the part was refused: {refused:?}"),
    }
}

fn cop_unit() -> Denomination {
    Denomination {
        id: "COP".into(),
        decimals: 2,
    }
}

/// Runs the current order's part on the platform, Core's clock moved on `seconds` while each operation runs.
fn run_taking(core: &mut Core, world: &mut World, seconds: u64) -> Ran {
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.takes = Some((Rc::clone(&core.clock), seconds));
    core.run_over(&world.catalogue, &mut watched)
}

/// Runs the current order's part on the platform, with why it couldn't run.
fn try_run(core: &mut Core, world: &mut World) -> Result<Ran, SegmentsError> {
    let session = world.context().session;
    core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut Recorded,
        &session,
    )
}

/// When the current order's `segment` was carried out, by Core's clock.
fn carried_since(core: &Core, segment: &str) -> u64 {
    core.segments
        .carried_since(&id(&core.named.order), &id(segment))
        .expect("the segment was carried out and has no final outcome")
        .seconds
}

fn count(core: &Core, pick: impl Fn(&What) -> bool) -> usize {
    core.log.borrow().count(pick)
}

fn is_stuck_declared(what: &What) -> bool {
    matches!(what, What::Input { input } if matches!(input.observed, Observed::StuckDeclared { .. }))
}

fn is_no_route(what: &What) -> bool {
    matches!(what, What::Input { input } if matches!(input.observed, Observed::NoRoute { .. }))
}

fn is_reversal(what: &What) -> bool {
    matches!(what, What::Input { input } if matches!(input.observed, Observed::ReversalCompleted { .. }))
}

fn is_resolve_stuck(what: &What) -> bool {
    matches!(
        what,
        What::Effect {
            action: Action::ResolveStuck { .. },
            ..
        }
    )
}

/// The kit's order and more settled at the reserve, the n-th order's operations each taking `seconds[n]`, so each of
/// the reserve's tuples has a history; the last order is current.
fn settled_history(seconds: &[u64]) -> (World, Core) {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for (n, s) in seconds.iter().enumerate() {
        if n > 0 {
            core.take_up(Named::another(n as u8 + 1));
        }
        let ran = run_taking(&mut core, &mut world, *s);
        assert!(
            segments(&ran)
                .iter()
                .all(|a| matches!(a.outcome, SegmentOutcome::Settled { .. })),
            "{ran:?}"
        );
    }
    (world, core)
}

/// Runs the current order's part with policy refusing every move from the withdrawal's on: the withdrawal settles,
/// and its deposit is refused, with no effect recorded and nothing handed over. Policy permits again after.
fn run_with_the_deposit_refused(core: &mut Core, world: &mut World) -> Ran {
    let refuser = core.policy.refuser();
    let during = Rc::clone(&refuser);
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.during = Some(Box::new(move |_, _| {
        during.set(Some(
            rutent_contracts::core_internal::RefusalClass::NotPermitted,
        ));
    }));
    let ran = core.run_over(&world.catalogue, &mut watched);
    drop(watched);
    refuser.set(None);
    ran
}

/// The kit's order with its withdrawal settled and its deposit never begun (the platform refused to begin it, so
/// nothing passed out and its effect's result is `failed`, its value not in motion), then recalculated from the
/// reserve's ledger where its value sits, and offered no route: the reserve refuses every dry run of the deposit.
fn left_no_route() -> (World, Core, Recalculation) {
    let mut world = World::of(&[RESERVE, OTHER_RESERVE]);
    let mut core = Core::over(&mut world);
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_begin = Some(1);
    let ran = core.run_over(&world.catalogue, &mut watched);
    drop(watched);
    let done = segments(&ran);
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert!(
        matches!(done[1].outcome, SegmentOutcome::NotHandedOver(_)),
        "{done:?}"
    );
    world
        .fleet
        .world()
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let mut rerouting = Rerouting::of(&core);
    let outcome = rerouting
        .recalculate(&mut core, &mut world)
        .expect("the recalculation runs");
    assert!(
        matches!(outcome, Recalculation::NoRoute { .. }),
        "{outcome:?}"
    );
    (world, core, outcome)
}

/// Records the no-route outcome of the kit's current order's part.
fn record_no_route(
    core: &mut Core,
    stuck: &mut Stuck,
    outcome: &Recalculation,
) -> Result<rutent_core_orch::stuck::NoRouteRecorded, StuckError> {
    let (order, part, last) = (
        id(&core.named.order),
        id(PART),
        core.record.record_id().clone(),
    );
    let actor = segments_support::core();
    on_parts(core, |parts| {
        stuck.no_route(
            parts,
            NoRouteLeft {
                order: &order,
                part: &part,
                outcome,
                last_record: &last,
                core: &actor,
            },
        )
    })
}

/// The kit's order stuck by no route, its value resting at the reserve's ledger.
fn stuck_by_no_route() -> (World, Core, Stuck) {
    let (world, mut core, outcome) = left_no_route();
    let mut stuck = Stuck::new();
    let recorded = record_no_route(&mut core, &mut stuck, &outcome).expect("recorded");
    assert_eq!(recorded.state, OrderState::Stuck);
    follow(&core, &mut stuck).expect("its ends are Core's");
    (world, core, stuck)
}

/// The resolution that returns the stuck value to its origin, through the reserve that holds it.
fn back_to_origin(amount: &str) -> Resolution {
    Resolution::Deliver {
        by: id(RESERVE),
        to: origin(),
        asset: cop_unit(),
        amount: PositiveAmount::new(amount).unwrap(),
    }
}

fn evidence() -> Vec<rutent_contracts::module_interface::Digest> {
    vec![digest(&"ab".repeat(32))]
}

/// Asks the decide step of the kit's stuck order of `policy`, by `decider`, for `resolution`.
fn decide(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut dyn rutent_contracts::core_internal::calls::StuckOrders,
    decider: &rutent_contracts::core_internal::Actor,
    resolution: &Resolution,
    evidence: &[rutent_contracts::module_interface::Digest],
) -> Result<rutent_contracts::core_internal::Decision, ResolveError> {
    let (order, part) = (id(&core.named.order), id(PART));
    on_parts(core, |parts| {
        stuck.decide(
            parts,
            policy,
            Deciding {
                order: &order,
                part: &part,
                decider,
                resolution,
                evidence,
            },
        )
    })
}

/// Asks the move step of `order` of `policy`, by `mover`, carrying out `carries_out`, moved by `moves`.
fn carry_out(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut dyn rutent_contracts::core_internal::calls::StuckOrders,
    moves: &mut Moves,
    order: &str,
    carries_out: &Decided,
    mover: &rutent_contracts::core_internal::Actor,
) -> Result<Moved, ResolveError> {
    let order = id(order);
    let actor = segments_support::core();
    on_parts(core, |parts| {
        stuck.carry_out(
            parts,
            policy,
            moves,
            Moving {
                order: &order,
                carries_out,
                mover,
                core: &actor,
            },
        )
    })
}

fn decided(decision: &rutent_contracts::core_internal::Decision) -> Decided {
    Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    }
}

// The tests -----------------------------------------------------------------------------------------------------

/// The withdrawal settles; the deposit is never handed over, and the recalculation from the reserve's ledger, where
/// the value sits, offers no route. Recorded, the part is stuck: its `no_route` cites the routing's decision record,
/// the order is marked stuck, and nothing is rolled back. The withdrawal stays settled, no reversal is asked, no
/// effect is recorded and nothing is held or released; the value rests at the ledger, where the operator sees it in
/// full. The part hands nothing more over and isn't recalculated, asking no one. An answer that leaves a route
/// records nothing.
// verifies: REQ-ORD-28, INV-4
#[test]
fn no_route_left_makes_the_order_stuck_and_nothing_is_rolled_back() {
    let (mut world, mut core, outcome) = left_no_route();
    let mut stuck = Stuck::new();
    let entries = core.log.borrow().entries.len();
    assert_eq!(
        record_no_route(
            &mut core,
            &mut stuck,
            &Recalculation::NotNow(NotRecalculable::Unknown)
        ),
        Err(StuckError::RouteLeft)
    );
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
    let (effects, held) = (count(&core, is_effect), core.exposures.held().count());

    let recorded = record_no_route(&mut core, &mut stuck, &outcome).expect("recorded");
    assert_eq!(recorded.state, OrderState::Stuck);
    let Recalculation::NoRoute { record } = &outcome else {
        unreachable!()
    };
    let log = core.log.borrow();
    let inputs = log.inputs();
    let no_route = inputs.last().expect("an input");
    assert_eq!(
        no_route.observed,
        Observed::NoRoute {
            decision_record: rutent_contracts::state_machine::Id::new(
                record.record.record_id().as_str()
            )
            .unwrap()
        }
    );
    drop(log);
    let order = id(ORDER);
    let part = core.segments.order(&order).unwrap().parts[0].clone();
    assert_eq!(part.state, OrderState::Stuck);
    assert_eq!(part.segments[0].state, SegmentState::Settled);
    assert_eq!(part.segments[1].state, SegmentState::Pending);
    assert_eq!(count(&core, is_reversal), 0, "no reversal is asked");
    assert_eq!(count(&core, is_effect), effects, "no effect is recorded");
    assert_eq!(
        core.exposures.held().count(),
        held + 1,
        "nothing released; the value at rest is held where it sits"
    );
    assert_eq!(
        core.exposures.in_flight(&id(RESERVE), &cop_unit()),
        Amount::new(AMOUNT).unwrap(),
        "the ceilings in flight count the value at rest"
    );
    assert_eq!(
        stuck.stuck_values(&core.exposures, &*core.clock),
        vec![StuckValue {
            order: order.clone(),
            asset: cop_unit(),
            amount: Amount::new(AMOUNT).unwrap(),
            age_seconds: 0,
        }],
        "the value resting at the ledger is seen in full"
    );

    let handled = world.fleet.platform.operations_received(RESERVE);
    assert_eq!(
        try_run(&mut core, &mut world),
        Err(SegmentsError::Stuck(id(PART)))
    );
    assert_eq!(
        world.fleet.platform.operations_received(RESERVE),
        handled,
        "nothing more is handed over"
    );
    let mut rerouting = Rerouting::of(&core);
    let entries = core.log.borrow().entries.len();
    let again = rerouting
        .recalculate(&mut core, &mut world)
        .expect("the recalculation runs");
    assert_eq!(
        again,
        Recalculation::NotNow(NotRecalculable::Held(OrderState::Stuck))
    );
    assert_eq!(rerouting.asked, 0, "no one is asked");
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
    assert_eq!(
        record_no_route(&mut core, &mut stuck, &outcome),
        Err(StuckError::AlreadyStuck)
    );
    assert_eq!(count(&core, is_no_route), 1, "recorded once");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A failed final deposit leaves the value at the node where the destination reserve lives: with no route from
/// there, the state machine has the part wait for the customer, not stuck (`REQ-ORD-43`). It isn't on the
/// operator's stuck view and can't be resolved as stuck.
// verifies: REQ-ORD-28, REQ-ORD-43
#[test]
fn no_route_at_a_failed_final_deposit_waits_for_the_customer() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let ran = core.run(&mut world);
    assert!(
        matches!(segments(&ran)[1].outcome, SegmentOutcome::Refused(_)),
        "{ran:?}"
    );
    world
        .fleet
        .world()
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let mut rerouting = Rerouting::of(&core);
    let outcome = rerouting.recalculate(&mut core, &mut world).unwrap();
    assert!(
        matches!(outcome, Recalculation::NoRoute { .. }),
        "{outcome:?}"
    );
    let mut stuck = Stuck::new();
    let recorded = record_no_route(&mut core, &mut stuck, &outcome).expect("recorded");
    assert_eq!(recorded.state, OrderState::WaitingForCustomer);
    assert!(!core.segments.stuck(&id(ORDER), &id(PART)));
    assert!(stuck.stuck_values(&core.exposures, &*core.clock).is_empty());
    assert!(core.exposures.stuck().is_empty());
    let mut policy = PermitsEveryStep::new(&core.log);
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &staff("desk-a", "alice"),
            &back_to_origin(AMOUNT),
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::NotStuck)
    );
    assert_eq!(policy.asked, 0);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Twenty orders settle at the reserve, each operation taking 30 s, so each tuple has its history and its p99 is
/// 30 s; formed, it is raised, half the advertised 60 s. The next order's withdrawal is carried out and its outcome
/// is unknown, its funds held. At exactly twice the p99 it isn't stuck; a second later it is: the figures are
/// recorded, the part declared stuck over time, the order marked stuck, and the declaration handed back as the
/// operator's warning, with the order on the operator's stuck view at what it holds. The advertised 60 s would have
/// given it until 120 s. Run again, the part asks nothing of its provider and hands nothing more over.
// verifies: REQ-ORD-29
#[test]
fn segment_past_twice_the_p99_makes_the_order_stuck_and_warns_the_operator() {
    let (mut world, mut core) = settled_history(&[30; HISTORY]);
    core.take_up(Named::another(21));
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let ran = run_taking(&mut core, &mut world, 30);
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Unknown(_)),
        "{ran:?}"
    );
    let since = carried_since(&core, "p1-s1-of-21");
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());

    core.clock.set(since + 2 * 30);
    assert_eq!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Running),
        "exactly twice the p99 is not over it"
    );
    assert_eq!(count(&core, is_stuck_declared), 0);

    core.clock.set(since + 2 * 30 + 1);
    let Ok(Watch::Stuck(called)) = watch(&mut core, &mut stuck, &mut raises) else {
        panic!("stuck past twice the p99");
    };
    assert_eq!(
        (
            called.order.as_str(),
            called.segment.as_str(),
            called.provider.as_str(),
            called.basis,
            called.figure_seconds,
            called.held_seconds,
        ),
        (
            "ord-21",
            "p1-s1-of-21",
            RESERVE,
            StuckBasis::TwiceP99,
            30,
            61
        )
    );
    let order = id("ord-21");
    assert!(core.segments.stuck(&order, &id(PART)));
    assert_eq!(
        stuck.stuck_values(&core.exposures, &*core.clock),
        vec![StuckValue {
            order: order.clone(),
            asset: cop_unit(),
            amount: Amount::new(AMOUNT).unwrap(),
            age_seconds: 0,
        }],
        "the operator sees it at what it holds"
    );
    assert_eq!(
        raises.raised.len(),
        2,
        "each tuple's first p99, against 60 s"
    );
    assert_eq!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::AlreadyStuck)
    );
    assert_eq!(count(&core, is_stuck_declared), 1);
    let handled = world.fleet.platform.operations_received(RESERVE);
    let ran = try_run(&mut core, &mut world).expect("what it gave back is recorded");
    assert_eq!(segments(&ran).len(), 1, "the deposit isn't handed over");
    assert!(matches!(
        segments(&ran)[0].outcome,
        SegmentOutcome::Unknown(_)
    ));
    assert_eq!(
        world.fleet.platform.operations_received(RESERVE),
        handled,
        "nothing is asked of the provider again"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The withdrawal settles in 30 s, so that tuple has history; the deposit's tuple has none, and its outcome is
/// unknown. Its clock runs on twice its advertised 60 s, not on the withdrawal's p99: still running at 120 s, stuck
/// at 121 s, on the advertised time.
// verifies: REQ-ORD-29
#[test]
fn provider_with_no_history_is_timed_on_twice_its_advertised_time() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Unknown));
    let ran = run_taking(&mut core, &mut world, 30);
    assert!(
        matches!(segments(&ran)[1].outcome, SegmentOutcome::Unknown(_)),
        "{ran:?}"
    );
    let since = carried_since(&core, DEPOSIT);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    core.clock.set(since + 2 * DECLARED);
    assert_eq!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Running)
    );
    core.clock.set(since + 2 * DECLARED + 1);
    let Ok(Watch::Stuck(called)) = watch(&mut core, &mut stuck, &mut raises) else {
        panic!("stuck past twice the advertised time");
    };
    assert_eq!(
        (called.segment.as_str(), called.basis, called.figure_seconds),
        (DEPOSIT, StuckBasis::TwiceAdvertised, DECLARED)
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The figures the call ran on are recorded before the declaration that rests on them: the order, the part, the
/// provider holding the funds, the basis, the figure and how long they had been held. A declaration the log can't
/// take now is appended as built on the next call, its figures recorded once.
// verifies: REQ-ORD-6, INV-8
#[test]
fn figures_behind_a_stuck_call_are_on_record() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let ran = core.run(&mut world);
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Unknown(_)),
        "{ran:?}"
    );
    let since = carried_since(&core, WITHDRAWAL);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    core.clock.set(since + 2 * DECLARED + 5);
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Stuck, Fault::Unavailable));
    }
    assert_eq!(
        watch(&mut core, &mut stuck, &mut raises),
        Err(StuckError::Append(
            rutent_contracts::core_internal::calls::AppendFailure::Unavailable
        ))
    );
    assert!(!core.segments.stuck(&id(ORDER), &id(PART)));
    core.clock.advance(7);
    let Ok(Watch::Stuck(called)) = watch(&mut core, &mut stuck, &mut raises) else {
        panic!("declared on the next call");
    };
    assert_eq!(
        called.held_seconds,
        2 * DECLARED + 5,
        "the figures as first built"
    );
    let log = core.log.borrow();
    let records = log.records();
    let calls: Vec<_> = records
        .iter()
        .filter(|(_, w)| matches!(w, What::StuckCall { .. }))
        .collect();
    assert_eq!(calls.len(), 1, "the figures are recorded once");
    let (
        at,
        What::StuckCall {
            order,
            part,
            provider,
            basis,
            figure_seconds,
            held_seconds,
        },
    ) = calls[0]
    else {
        unreachable!()
    };
    assert_eq!(
        (
            order.as_str(),
            part.as_str(),
            provider.as_str(),
            *basis,
            u64::from(*figure_seconds),
            u64::from(*held_seconds),
        ),
        (
            ORDER,
            PART,
            RESERVE,
            StuckBasis::TwiceAdvertised,
            DECLARED,
            2 * DECLARED + 5
        )
    );
    let declared: Vec<usize> = records
        .iter()
        .filter(|(_, w)| is_stuck_declared(w))
        .map(|(i, _)| *i)
        .collect();
    assert_eq!(declared.len(), 1, "declared once");
    assert!(*at < declared[0], "the figures before the declaration");
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The settlement time differences raised, as (before, now, cited entries), and how each was recorded.
fn settlement_changes(raised: &[rutent_core_orch::stuck::Raised]) -> Vec<(u64, u64, usize, bool)> {
    raised
        .iter()
        .map(|r| {
            let Difference::SettlementTime {
                provider,
                before_seconds,
                now_seconds,
            } = &r.raise.difference
            else {
                panic!("a settlement time difference");
            };
            assert_eq!(provider.as_str(), RESERVE);
            (
                u64::from(*before_seconds),
                u64::from(*now_seconds),
                r.raise.cites.len(),
                r.delivered,
            )
        })
        .collect()
}

fn raises_recorded(core: &Core, outcome: Outcome) -> usize {
    core.log
        .borrow()
        .verified()
        .iter()
        .filter(|v| matches!(v.what, What::Raise { .. }) && v.outcome == outcome)
        .count()
}

/// Twenty orders settle at the reserve taking 30 s: each tuple's first p99, half the advertised 60 s, is raised,
/// citing the settlement that formed it. One more at 31 s moves it under half, and nothing is raised. One at 60 s
/// doubles it: each tuple's change is raised, citing the baseline's settlement and its own, and recorded as
/// delivered. Learnt again, nothing is raised twice. A raise the operator's channel doesn't deliver is recorded as
/// failed. The p99 falling after a long slow history is the unit test's (`p99_falls_after_a_long_slow_history`).
// verifies: REQ-ORD-6
#[test]
fn material_change_in_a_providers_p99_is_raised() {
    let (mut world, mut core) = settled_history(&[30; HISTORY]);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    let learn = |core: &mut Core, stuck: &mut Stuck, raises: &mut Raises| {
        let actor = segments_support::core();
        on_parts(core, |parts| stuck.learn(parts, raises, &actor)).expect("learnt")
    };
    assert_eq!(
        settlement_changes(&learn(&mut core, &mut stuck, &mut raises)),
        vec![(DECLARED, 30, 1, true); 2],
        "each tuple's first p99, against the advertised time"
    );

    core.take_up(Named::another(21));
    let _ = run_taking(&mut core, &mut world, 31);
    assert!(
        learn(&mut core, &mut stuck, &mut raises).is_empty(),
        "31 s is no material change from 30 s"
    );

    core.take_up(Named::another(22));
    let _ = run_taking(&mut core, &mut world, 60);
    assert_eq!(
        settlement_changes(&learn(&mut core, &mut stuck, &mut raises)),
        vec![(30, 60, 2, true); 2]
    );
    assert_eq!(raises.raised.len(), 4);
    assert_eq!(raises_recorded(&core, Outcome::Done {}), 4);
    assert!(
        learn(&mut core, &mut stuck, &mut raises).is_empty(),
        "nothing raised twice"
    );
    assert_eq!(raises.raised.len(), 4);

    raises.delivers = false;
    core.take_up(Named::another(23));
    let _ = run_taking(&mut core, &mut world, 120);
    assert_eq!(
        settlement_changes(&learn(&mut core, &mut stuck, &mut raises)),
        vec![(60, 120, 2, false); 2]
    );
    assert_eq!(raises_recorded(&core, Outcome::Failed {}), 2);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Each tuple's first p99 is raised, and the log refuses the raise's entries: the raise is dropped, never retried,
/// and the stuck clock still runs. A part over twice the p99 is declared stuck all the same, since detection never
/// holds up the primary path.
// verifies: REQ-ORD-29, REQ-ORD-9
#[test]
fn raise_the_log_refuses_holds_up_no_stuck_call() {
    let (mut world, mut core) = settled_history(&[30; HISTORY]);
    core.take_up(Named::another(21));
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let _ = run_taking(&mut core, &mut world, 30);
    let since = carried_since(&core, "p1-s1-of-21");
    for _ in 0..2 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Raise, Fault::Refuse));
    }
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    core.clock.set(since + 2 * 30 + 1);
    let Ok(Watch::Stuck(called)) = watch(&mut core, &mut stuck, &mut raises) else {
        panic!("stuck though the raises were refused");
    };
    assert_eq!(
        (called.basis, called.figure_seconds),
        (StuckBasis::TwiceP99, 30)
    );
    assert_eq!(raises.raised.len(), 2, "each tuple's change raised once");
    assert_eq!(count(&core, |w| matches!(w, What::Raise { .. })), 0);
    assert_eq!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::AlreadyStuck)
    );
    assert_eq!(raises.raised.len(), 2, "a refused raise isn't raised again");
    assert_eq!(
        count(&core, |w| matches!(w, What::Raise { .. })),
        0,
        "nor appended again"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Stuck over time, the order's state is the customer's to read, and its segment the provider's that ran it, each on
/// a read policy allows and records; another provider's read of it is refused, and recorded.
// verifies: REQ-ORD-6
#[test]
fn stuck_state_is_visible_to_the_customer_and_the_provider() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Unknown));
    let _ = core.run(&mut world);
    let since = carried_since(&core, DEPOSIT);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    core.clock.set(since + 2 * DECLARED + 1);
    assert!(matches!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Stuck(_))
    ));
    let order = id(ORDER);
    let reads = |core: &Core| {
        core.log
            .borrow()
            .records()
            .iter()
            .filter(|(_, w)| matches!(w, What::CheckRead { .. }))
            .count()
    };
    let before = reads(&core);
    let status = core
        .segments
        .status(
            &mut core.policy,
            &*core.clock,
            &mut core.ids,
            &customer(),
            None,
            &order,
        )
        .expect("the customer reads its order");
    assert_eq!(status.state, OrderState::Stuck);
    let seen = seen_by_provider(
        &core.segments,
        &mut core.policy,
        &*core.clock,
        &mut core.ids,
        &provider_desk(),
        &order,
        &id(DEPOSIT),
    )
    .expect("the provider reads its segment");
    assert_eq!(
        (seen.part_state, seen.segment_state),
        (OrderState::Stuck, SegmentState::OutcomeUnknown)
    );
    let refused = seen_by_provider(
        &core.segments,
        &mut core.policy,
        &*core.clock,
        &mut core.ids,
        &other_provider_desk(),
        &order,
        &id(DEPOSIT),
    );
    assert!(
        matches!(refused, Err(StatusError::Refused(_))),
        "{refused:?}"
    );
    assert_eq!(reads(&core), before + 3, "each read recorded");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// With policy's stuck rules as they stand, unbuilt, the decide step is not known: it is recorded, kept for nothing,
/// and a move naming it is refused before policy is asked. Where policy permits every step, Core still refuses a move
/// by the decider's own party, through another login, or by the decider's own login under another stated party, and
/// a move naming a decision Core doesn't hold, a forged one, one of another order, or one policy answered as if to
/// another request. Nothing moves and no effect is recorded.
// verifies: REQ-AUZ-4
#[test]
fn move_step_without_another_actors_decision_is_refused() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut moves = Moves::default();
    let resolution = back_to_origin(AMOUNT);
    let (alice, alice_again, bob) = (
        staff("desk-a", "alice"),
        staff("desk-a", "alice-second-login"),
        staff("desk-b", "bob"),
    );

    let mut unbuilt = Unbuilt::new(PolicyPath(Rc::clone(&core.log)));
    let not_known = decide(
        &mut core,
        &mut stuck,
        &mut unbuilt,
        &alice,
        &resolution,
        &evidence(),
    )
    .expect("asked");
    assert!(matches!(not_known.outcome, Answer::NotKnown { .. }));
    assert_eq!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut unbuilt,
            &mut moves,
            ORDER,
            &decided(&not_known),
            &bob
        ),
        Err(ResolveError::NoDecision)
    );

    let mut policy = PermitsEveryStep::new(&core.log);
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &resolution,
        &evidence(),
    )
    .expect("asked");
    assert!(matches!(permitted.outcome, Answer::Permit {}));
    let asked = policy.asked;
    assert_eq!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            ORDER,
            &decided(&permitted),
            &alice_again
        ),
        Err(ResolveError::SameParty)
    );
    assert_eq!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            ORDER,
            &decided(&permitted),
            &staff("desk-b", "alice")
        ),
        Err(ResolveError::SameLogin),
        "one login under another stated party is one actor"
    );
    let forged = Decided {
        request: permitted.request.clone(),
        decision: id("forged-decision"),
    };
    assert_eq!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            ORDER,
            &forged,
            &bob
        ),
        Err(ResolveError::NoDecision)
    );
    assert_eq!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            "ord-9",
            &decided(&permitted),
            &bob
        ),
        Err(ResolveError::NoDecision)
    );
    assert_eq!(policy.asked, asked, "policy isn't asked for a refused move");
    let mut answering_another = PermitsEveryStep::new(&core.log);
    answering_another.answers_another = true;
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut answering_another,
            &alice,
            &resolution,
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::NotItsAnswer)
    );
    assert!(moves.0.is_empty(), "nothing moves");
    assert_eq!(count(&core, is_resolve_stuck), 0, "no effect is recorded");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Decided by one party naming its evidence and moved by another, the resolution is recorded as a `resolve_stuck`
/// effect on the move's permit, carrying out exactly the decided outcome, and moved once: an effect whose append the
/// log couldn't take is appended as built and carried out once, asking policy nothing again, and only by its own
/// mover for its own order; a result the log couldn't take is appended on the next call with nothing carried out
/// again. The part is resolved:
/// the operator's stuck view drops it, the order's stuck mark goes, and no second decision or move is taken.
// verifies: REQ-AUZ-4, INV-8
#[test]
fn resolution_is_moved_once_by_another_party_on_its_decision() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut moves = Moves::default();
    let mut policy = PermitsEveryStep::new(&core.log);
    let resolution = back_to_origin(AMOUNT);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &resolution,
        &evidence(),
    )
    .expect("asked");
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Effect, Fault::Unavailable));
    }
    assert!(matches!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            ORDER,
            &decided(&permitted),
            &bob
        ),
        Err(ResolveError::Append(_))
    ));
    assert!(
        moves.0.is_empty(),
        "nothing moves before its effect is recorded"
    );
    let asked = policy.asked;
    assert_eq!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            ORDER,
            &decided(&permitted),
            &alice
        ),
        Err(ResolveError::NotTheMover),
        "the decider can't finish another's move"
    );
    assert_eq!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            "ord-9",
            &decided(&permitted),
            &bob
        ),
        Err(ResolveError::NoDecision),
        "nor can it be finished for another order"
    );
    assert!(moves.0.is_empty());
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::LaterResult, Fault::Unavailable));
    }
    assert!(matches!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            ORDER,
            &decided(&permitted),
            &bob
        ),
        Err(ResolveError::Append(_))
    ));
    assert_eq!(moves.0.len(), 1, "carried out once its effect is recorded");
    let moved = carry_out(
        &mut core,
        &mut stuck,
        &mut policy,
        &mut moves,
        ORDER,
        &decided(&permitted),
        &bob,
    )
    .expect("moved");
    assert!(
        matches!(
            moved,
            Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            }
        ),
        "{moved:?}"
    );
    assert_eq!(policy.asked, asked, "policy isn't asked again");
    assert_eq!(moves.0.len(), 1, "moved once");
    // The move goes under the transaction its decision named (`stuck-effect-moves-the-decided-resolution`).
    let decided_under = decided_transaction(&core);
    assert_eq!(
        moves.0[0].action,
        Action::ResolveStuck {
            order: id(ORDER),
            carries_out: decided(&permitted),
            resolution: resolution.clone(),
            transaction: decided_under,
        }
    );
    assert_eq!(count(&core, is_resolve_stuck), 1);
    let resolved: Vec<_> = core
        .log
        .borrow()
        .inputs()
        .iter()
        .filter_map(|i| match &i.observed {
            Observed::StuckResolved { to, .. } => Some(*to),
            _ => None,
        })
        .collect();
    assert_eq!(
        resolved,
        vec![rutent_contracts::state_machine::ResolvedTo::Origin],
        "the part's resolution recorded once, back at its origin"
    );
    assert!(stuck.stuck_values(&core.exposures, &*core.clock).is_empty());
    assert!(!stuck.is_stuck(&id(ORDER), &id(PART)));
    assert!(core.exposures.stuck().is_empty());
    assert_eq!(
        carry_out(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            ORDER,
            &decided(&permitted),
            &staff("desk-c", "carol")
        ),
        Err(ResolveError::NoDecision)
    );
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &alice,
            &resolution,
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::Resolved)
    );
    assert_eq!(moves.0.len(), 1);
    // The order Core follows took the `stuck_resolved`: the part ended where its resolution sent it, and running it
    // again hands nothing of it over (`REQ-STM-6`).
    let followed = core.segments.order(&id(ORDER)).expect("followed");
    assert_eq!(followed.parts[0].state, OrderState::EndedWithoutCompleting);
    let mut world = world;
    let handled = world.fleet.platform.operations_received(RESERVE);
    assert_eq!(
        try_run(&mut core, &mut world).map(|_| ()),
        Err(SegmentsError::Stuck(id(PART)))
    );
    assert_eq!(world.fleet.platform.operations_received(RESERVE), handled);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A decision that names no evidence, delivers to an account that is neither the order's destination nor its
/// origin, moves more than the part holds or another asset, or moves it through a provider the part never ran at,
/// is refused before policy is asked (`THR-77`).
// verifies: REQ-AUZ-4
#[test]
fn resolution_elsewhere_or_beyond_the_stuck_value_is_refused() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut policy = PermitsEveryStep::new(&core.log);
    let alice = staff("desk-a", "alice");
    let entries = core.log.borrow().entries.len();
    let refused =
        |core: &mut Core, stuck: &mut Stuck, policy: &mut PermitsEveryStep, r, e: &[_]| {
            decide(core, stuck, policy, &alice, &r, e).map(|_| ())
        };
    assert_eq!(
        refused(
            &mut core,
            &mut stuck,
            &mut policy,
            back_to_origin(AMOUNT),
            &[]
        ),
        Err(ResolveError::NoEvidence)
    );
    let to_a_friend = Resolution::Deliver {
        by: id(RESERVE),
        to: Destination {
            account: id("acct-of-a-friend"),
            ..origin()
        },
        asset: cop_unit(),
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    assert_eq!(
        refused(&mut core, &mut stuck, &mut policy, to_a_friend, &evidence()),
        Err(ResolveError::ElsewhereThanItsEnds)
    );
    assert_eq!(
        refused(
            &mut core,
            &mut stuck,
            &mut policy,
            back_to_origin("1000001"),
            &evidence()
        ),
        Err(ResolveError::NotItsValue)
    );
    let another_asset = Resolution::Deliver {
        by: id(RESERVE),
        to: origin(),
        asset: Denomination {
            id: "USD".into(),
            decimals: 2,
        },
        amount: PositiveAmount::new("1").unwrap(),
    };
    assert_eq!(
        refused(
            &mut core,
            &mut stuck,
            &mut policy,
            another_asset,
            &evidence()
        ),
        Err(ResolveError::NotItsValue)
    );
    let elsewhere: Id = id("sim-reserve-b");
    let another_provider = Resolution::Deliver {
        by: elsewhere,
        to: origin(),
        asset: cop_unit(),
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    assert_eq!(
        refused(
            &mut core,
            &mut stuck,
            &mut policy,
            another_provider,
            &evidence()
        ),
        Err(ResolveError::NotItsValue)
    );
    assert_eq!(policy.asked, 0, "policy isn't asked");
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

fn moves_once(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut PermitsEveryStep,
    moves: &mut Moves,
    decision: &rutent_contracts::core_internal::Decision,
    mover: &rutent_contracts::core_internal::Actor,
) -> Result<Moved, ResolveError> {
    carry_out(core, stuck, policy, moves, ORDER, &decided(decision), mover)
}

fn to_destination(core: &Core, amount: &str) -> Resolution {
    Resolution::Deliver {
        by: id(RESERVE),
        to: destination(core),
        asset: cop_unit(),
        amount: PositiveAmount::new(amount).unwrap(),
    }
}

/// The reviewer's sequence: two decisions for the part's whole value; the first's move stops before the log takes
/// its effect. While it is in progress, nothing more is decided for the part and no other decision moves; the move is
/// finished by its mover, the value moves once, and every other decision is spent. Moved once, never twice.
// verifies: REQ-AUZ-4, INV-8
#[test]
fn resumed_move_never_pays_a_part_twice() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let (mut policy, mut moves) = (PermitsEveryStep::new(&core.log), Moves::default());
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let earlier = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .expect("asked");
    let first = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .expect("asked");
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Effect, Fault::Unavailable));
    }
    assert!(matches!(
        moves_once(&mut core, &mut stuck, &mut policy, &mut moves, &first, &bob),
        Err(ResolveError::Append(_))
    ));
    let to = to_destination(&core, AMOUNT);
    assert_eq!(
        decide(&mut core, &mut stuck, &mut policy, &alice, &to, &evidence()).map(|_| ()),
        Err(ResolveError::MoveInProgress),
        "nothing more is decided while a move is in progress"
    );
    assert_eq!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &earlier,
            &bob
        ),
        Err(ResolveError::MoveInProgress),
        "no other decision moves meanwhile"
    );
    assert!(matches!(
        moves_once(&mut core, &mut stuck, &mut policy, &mut moves, &first, &bob),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    for decision in [&first, &earlier] {
        assert_eq!(
            moves_once(
                &mut core,
                &mut stuck,
                &mut policy,
                &mut moves,
                decision,
                &bob
            ),
            Err(ResolveError::NoDecision)
        );
    }
    assert_eq!(moves.0.len(), 1, "moved once");
    assert_eq!(count(&core, is_resolve_stuck), 1);
    assert!(stuck.stuck_values(&core.exposures, &*core.clock).is_empty());
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A part is resolved once, on its resolution's move done, so a resolution moves all of what the part holds: one
/// for 1 unit of a 1 000 000 part, which would leave the rest stranded, is refused, and so is one for more. One for
/// the whole is decided and moved, and the part is resolved, its value released.
// verifies: REQ-AUZ-4, REQ-OPS-2
#[test]
fn resolution_moves_the_whole_of_what_the_part_holds() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let (mut policy, mut moves) = (PermitsEveryStep::new(&core.log), Moves::default());
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    for amount in ["1", "999999", "1000001"] {
        assert_eq!(
            decide(
                &mut core,
                &mut stuck,
                &mut policy,
                &alice,
                &back_to_origin(amount),
                &evidence()
            )
            .map(|_| ()),
            Err(ResolveError::NotItsValue),
            "{amount}"
        );
    }
    assert_eq!(policy.asked, 0);
    let whole = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    assert!(matches!(
        moves_once(&mut core, &mut stuck, &mut policy, &mut moves, &whole, &bob),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    assert!(!stuck.is_stuck(&id(ORDER), &id(PART)));
    assert!(stuck.stuck_values(&core.exposures, &*core.clock).is_empty());
    assert!(core.exposures.stuck().is_empty());
    assert_eq!(moves.0.len(), 1);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The log answers the effect's append with the withdrawal effect's receipt, every try: Core takes no receipt that
/// isn't its entry's, so nothing moves and no effect rests on a record the log didn't take. Tried again, the effect
/// is recorded and moved once, on its own receipt, held in the one register the segments keep.
// verifies: INV-8, REQ-AUZ-4
#[test]
fn replayed_receipt_moves_nothing_without_its_record() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let (mut policy, mut moves) = (PermitsEveryStep::new(&core.log), Moves::default());
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let decision = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Effect, Fault::Replay));
    }
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &decision,
            &bob
        ),
        Err(ResolveError::Append(_))
    ));
    assert!(
        moves.0.is_empty(),
        "nothing moves on another entry's receipt"
    );
    assert_eq!(count(&core, is_resolve_stuck), 0);
    core.log
        .borrow_mut()
        .faults
        .push_back((On::Effect, Fault::Replay));
    let Ok(Moved::CarriedOut {
        effect,
        resolved: Resolved::Done,
        ..
    }) = moves_once(
        &mut core,
        &mut stuck,
        &mut policy,
        &mut moves,
        &decision,
        &bob,
    )
    else {
        panic!("moved on its own receipt");
    };
    assert_eq!(moves.0.len(), 1);
    assert_eq!(moves.0[0].receipt, effect);
    assert_eq!(count(&core, is_resolve_stuck), 1);
    let log = core.log.borrow();
    let at = usize::try_from(u64::from(effect.index)).unwrap();
    assert!(
        matches!(&log.entries[at].0, rutent_contracts::core_internal::Entry::Verified(v) if is_resolve_stuck(&v.what))
    );
    drop(log);
    assert!(
        core.segments.acted_on(&effect),
        "one register of effects acted on"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A move that failed spends its decision: carol can't take bob's decision on again, nor bob, and a new move needs
/// a new decision. A move whose outcome is unknown spends its decision too and holds the part: nothing more is
/// decided or moved for it, the value stays on the operator's view in full, and taking the move on again carries
/// nothing out.
// verifies: REQ-AUZ-4, REQ-ORD-1
#[test]
fn failed_or_unknown_move_spends_its_decision() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut policy = PermitsEveryStep::new(&core.log);
    let mut moves = Moves(Vec::new(), [Resolved::Failed, Resolved::Unknown].into());
    let (alice, bob, carol) = (
        staff("desk-a", "alice"),
        staff("desk-b", "bob"),
        staff("desk-c", "carol"),
    );
    let failed = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    let Ok(Moved::CarriedOut {
        resolved: Resolved::Failed,
        result: Some(_),
        ..
    }) = moves_once(
        &mut core,
        &mut stuck,
        &mut policy,
        &mut moves,
        &failed,
        &bob,
    )
    else {
        panic!("failed, its result recorded");
    };
    for mover in [&carol, &bob] {
        assert_eq!(
            moves_once(
                &mut core,
                &mut stuck,
                &mut policy,
                &mut moves,
                &failed,
                mover
            ),
            Err(ResolveError::NoDecision),
            "a failed move's decision is spent"
        );
    }
    let unknown = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    let Ok(Moved::CarriedOut {
        resolved: Resolved::Unknown,
        result: Some(_),
        ..
    }) = moves_once(
        &mut core,
        &mut stuck,
        &mut policy,
        &mut moves,
        &unknown,
        &bob,
    )
    else {
        panic!("unknown, recorded unanswered");
    };
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &alice,
            &back_to_origin(AMOUNT),
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::OutcomeUnknown)
    );
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &unknown,
            &bob
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Unknown,
            result: Some(_),
            ..
        })
    ));
    assert_eq!(moves.0.len(), 2, "nothing carried out again");
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    assert_eq!(
        stuck.stuck_values(&core.exposures, &*core.clock)[0].amount,
        Amount::new(AMOUNT).unwrap()
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The order's destination and origin come from Core's records, never from the decision's caller: with none
/// followed, nothing is decided. The destination, its account included, is the log's own record of the order's
/// permitted authorisation, never one policy refused; an order the log holds no such record of has none. The origin is the reserve and outer
/// node the order's withdrawal took the value from, and an account the withdrawal Core signed named: a friend's
/// account at the destination or at the origin is refused.
// verifies: REQ-AUZ-4, REQ-AUZ-3
#[test]
fn resolution_ends_come_from_cores_records() {
    let (_world, mut core, outcome) = left_no_route();
    let mut stuck = Stuck::new();
    record_no_route(&mut core, &mut stuck, &outcome).expect("recorded");
    let mut policy = PermitsEveryStep::new(&core.log);
    let alice = staff("desk-a", "alice");
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &alice,
            &back_to_origin(AMOUNT),
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::NoEnds)
    );
    assert_eq!(
        stuck.follow(
            &core.segments,
            &recorded(&core),
            &id("ord-without-a-record")
        ),
        Err(StuckError::Unknown)
    );
    let unauthorised: Vec<_> = recorded(&core)
        .into_iter()
        .filter(|e| {
            !matches!(e, rutent_contracts::core_internal::Entry::Verified(v)
            if matches!(v.what, What::Authorise { .. }))
        })
        .collect();
    assert_eq!(
        stuck.follow(&core.segments, &unauthorised, &id(ORDER)),
        Err(StuckError::NotItsAuthorisation),
        "no authorisation recorded, no ends"
    );
    assert_eq!(policy.asked, 0);
    // A later authorisation policy refused, naming a friend's account, is no record of the order's destination.
    let mut with_refused = recorded(&core);
    let refused = with_refused
        .iter()
        .find_map(|e| match e {
            rutent_contracts::core_internal::Entry::Verified(v)
                if matches!(v.what, What::Authorise { .. }) =>
            {
                let mut v = v.clone();
                if let What::Authorise { authorisation, .. } = &mut v.what {
                    authorisation.destination.account = id("acct-of-a-friend");
                }
                v.outcome = Outcome::Refused {
                    class: rutent_contracts::core_internal::RefusalClass::NotPermitted,
                };
                Some(rutent_contracts::core_internal::Entry::Verified(v))
            }
            _ => None,
        })
        .unwrap();
    with_refused.push(refused);
    stuck
        .follow(&core.segments, &with_refused, &id(ORDER))
        .expect("its ends are the log's permitted authorisation");
    for resolution in [back_to_origin(AMOUNT), to_destination(&core, AMOUNT)] {
        assert!(
            decide(
                &mut core,
                &mut stuck,
                &mut policy,
                &alice,
                &resolution,
                &evidence()
            )
            .is_ok()
        );
    }
    for to in [
        Destination {
            account: id("acct-of-a-friend"),
            ..destination(&core)
        },
        Destination {
            account: id("acct-of-a-friend"),
            ..origin()
        },
    ] {
        let to_a_friend = Resolution::Deliver {
            by: id(RESERVE),
            to,
            asset: cop_unit(),
            amount: PositiveAmount::new(AMOUNT).unwrap(),
        };
        assert_eq!(
            decide(
                &mut core,
                &mut stuck,
                &mut policy,
                &alice,
                &to_a_friend,
                &evidence()
            )
            .map(|_| ()),
            Err(ResolveError::ElsewhereThanItsEnds)
        );
    }
}

/// The withdrawal's outcome is unknown, and the log can't take its result's stated entry: the segment was carried
/// out with something still to record. Declared stuck over time, the part hands nothing over, but running it again
/// records what the segment gave back, each step once, asking nothing of the provider again. A late outcome from
/// the provider itself has no path into Core yet (`T-020-12`, `T-024-06`).
// verifies: REQ-ORD-28, REQ-ORD-1
#[test]
fn stuck_part_records_what_its_carried_segment_gave_back() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::StatedResult, Fault::Unavailable));
    }
    let first = try_run(&mut core, &mut world);
    assert!(first.is_err());
    let since = carried_since(&core, WITHDRAWAL);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    core.clock.set(since + 2 * DECLARED + 1);
    assert!(matches!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Stuck(_))
    ));
    let handled = world.fleet.platform.operations_received(RESERVE);
    let results = |core: &Core| {
        core.log
            .borrow()
            .entries
            .iter()
            .filter(|(e, _)| {
                matches!(e,
            rutent_contracts::core_internal::Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE)
            })
            .count()
    };
    let before = results(&core);
    let ran = try_run(&mut core, &mut world).expect("recorded");
    assert_eq!(segments(&ran).len(), 1);
    assert!(matches!(
        segments(&ran)[0].outcome,
        SegmentOutcome::Unknown(_)
    ));
    assert_eq!(results(&core), before + 1, "what it gave back is recorded");
    assert_eq!(
        world.fleet.platform.operations_received(RESERVE),
        handled,
        "nothing asked again"
    );
    let _ = try_run(&mut core, &mut world).expect("recorded");
    assert_eq!(results(&core), before + 1, "recorded once");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A `no_route` the log can't take now is kept: the next timing of the part appends it as built rather than
/// overwriting it, and the part is stuck, recorded once.
// verifies: REQ-ORD-28, INV-8
#[test]
fn no_route_the_log_couldnt_take_is_appended_by_the_next_watch() {
    let (world, mut core, outcome) = left_no_route();
    let mut stuck = Stuck::new();
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Stuck, Fault::Unavailable));
    }
    assert!(matches!(
        record_no_route(&mut core, &mut stuck, &outcome),
        Err(StuckError::Append(_))
    ));
    assert!(!core.segments.stuck(&id(ORDER), &id(PART)));
    let mut raises = Raises::default();
    assert_eq!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::AlreadyStuck)
    );
    assert!(core.segments.stuck(&id(ORDER), &id(PART)));
    assert_eq!(count(&core, is_no_route), 1);
    assert_eq!(count(&core, is_stuck_declared), 0, "nothing overwrote it");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// After a restart, the order is rebuilt from the log's inputs: it reads stuck, so it hands nothing over, and a
/// fresh stuck record, which holds no declaration, decides nothing for it. Neither place alone lets its value move.
// verifies: REQ-ORD-28, REQ-AUZ-4
#[test]
fn restart_keeps_a_stuck_part_from_moving() {
    let (mut world, mut core, _) = stuck_by_no_route();
    let inputs: Vec<_> = core.log.borrow().inputs().into_iter().cloned().collect();
    let mut rebuilt = rutent_core_orch::segments::Segments::new();
    assert!(rebuilt.follow_recorded(&*core.clock, &inputs));
    assert!(rebuilt.stuck(&id(ORDER), &id(PART)));
    core.segments = rebuilt;
    assert_eq!(
        try_run(&mut core, &mut world),
        Err(SegmentsError::Stuck(id(PART)))
    );
    let mut fresh = Stuck::new();
    follow(&core, &mut fresh).expect("its ends are Core's");
    let mut policy = PermitsEveryStep::new(&core.log);
    assert_eq!(
        decide(
            &mut core,
            &mut fresh,
            &mut policy,
            &staff("desk-a", "alice"),
            &back_to_origin(AMOUNT),
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::NotStuck)
    );
    assert_eq!(policy.asked, 0);
}

/// Stuck over time, its withdrawal's outcome unknown, the part's value may still be in motion: nothing is decided
/// for it until that outcome is settled (`AC-6`), and running it again carries nothing out and keeps its hold.
// verifies: REQ-ORD-8, REQ-AUZ-4
#[test]
fn unknown_withdrawal_holds_the_stuck_parts_decision_back() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let _ = core.run(&mut world);
    let since = carried_since(&core, WITHDRAWAL);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    follow(&core, &mut stuck).expect("its ends are Core's");
    core.clock.set(since + 2 * DECLARED + 1);
    assert!(matches!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Stuck(_))
    ));
    let mut policy = PermitsEveryStep::new(&core.log);
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &staff("desk-a", "alice"),
            &back_to_origin(AMOUNT),
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::ValueInMotion(id(WITHDRAWAL)))
    );
    let handled = world.fleet.platform.operations_received(RESERVE);
    let _ = try_run(&mut core, &mut world);
    assert_eq!(world.fleet.platform.operations_received(RESERVE), handled);
    assert_eq!(
        core.exposures.in_flight(&id(RESERVE), &cop_unit()),
        Amount::new(AMOUNT).unwrap(),
        "its value stays held"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The order's ends are followed when it is taken up. Its deposit is never handed over (policy refuses its move), the
/// route is recalculated from the ledger and its deposit isn't handed over either, and the next routing offers nothing: the part is stuck
/// on a recalculated route, and its ends, followed before, still decide where its value may go. Following again
/// changes nothing, and another authorisation can't replace them.
// verifies: REQ-AUZ-4, REQ-ORD-28
#[test]
fn ends_followed_at_take_up_survive_a_recalculation() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut stuck = Stuck::new();
    follow(&core, &mut stuck).expect("followed at take-up");
    let _ = run_with_the_deposit_refused(&mut core, &mut world);
    let mut rerouting = Rerouting::of(&core);
    let Ok(Recalculation::Recalculated(done)) = rerouting.recalculate(&mut core, &mut world) else {
        panic!("recalculated");
    };
    // Policy refuses the recalculated route's deposit too, so nothing of it is handed over.
    let refuser = core.policy.refuser();
    refuser.set(Some(
        rutent_contracts::core_internal::RefusalClass::NotPermitted,
    ));
    let ran = Rerouting::run(
        &mut core,
        &world.catalogue,
        &mut world.fleet.platform,
        &done,
    )
    .expect("ran");
    refuser.set(None);
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::NotPermitted(_)),
        "{ran:?}"
    );
    world
        .fleet
        .world()
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let outcome = rerouting
        .recalculate(&mut core, &mut world)
        .expect("routed");
    assert!(
        matches!(outcome, Recalculation::NoRoute { .. }),
        "{outcome:?}"
    );
    let no_route = record_no_route(&mut core, &mut stuck, &outcome).expect("recorded");
    assert_eq!(no_route.state, OrderState::Stuck);
    follow(&core, &mut stuck).expect("following again keeps what was followed");
    let mut other = recorded(&core);
    for entry in &mut other {
        if let rutent_contracts::core_internal::Entry::Verified(v) = entry
            && let What::Authorise { authorisation, .. } = &mut v.what
        {
            authorisation.destination.account = id("acct-of-a-friend");
        }
    }
    assert_eq!(
        stuck.follow(&core.segments, &other, &id(ORDER)),
        Err(StuckError::NotItsAuthorisation),
        "the ends first followed stand"
    );
    let mut policy = PermitsEveryStep::new(&core.log);
    for resolution in [back_to_origin(AMOUNT), to_destination(&core, AMOUNT)] {
        assert!(
            decide(
                &mut core,
                &mut stuck,
                &mut policy,
                &staff("desk-a", "alice"),
                &resolution,
                &evidence()
            )
            .is_ok_and(|d| matches!(d.outcome, Answer::Permit {})),
        );
    }
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The log refuses the resolution's effect: nothing moved, the permit backs no effect, and the move leaves the part
/// free. Its decision is spent; a new decision and move go through.
// verifies: REQ-AUZ-4, INV-7
#[test]
fn effect_the_log_refuses_frees_the_part_and_spends_its_decision() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let (mut policy, mut moves) = (PermitsEveryStep::new(&core.log), Moves::default());
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let refused = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    core.log
        .borrow_mut()
        .faults
        .push_back((On::Effect, Fault::Refuse));
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &refused,
            &bob
        ),
        Err(ResolveError::Append(
            rutent_contracts::core_internal::calls::AppendFailure::Refused(_)
        ))
    ));
    assert!(moves.0.is_empty());
    assert_eq!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &refused,
            &bob
        ),
        Err(ResolveError::NoDecision),
        "its decision is spent"
    );
    let again = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .expect("the part is free to decide");
    assert!(matches!(
        moves_once(&mut core, &mut stuck, &mut policy, &mut moves, &again, &bob),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    assert_eq!(moves.0.len(), 1);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The receipt rule Core holds a stuck entry's receipt to is the log's own: an entry `CMP-CORE-AUDIT`'s log takes
/// is answered with a receipt that passes it, and one of another entry doesn't.
// verifies: INV-8, REQ-AUD-2
#[test]
fn real_logs_receipt_passes_the_receipt_rule() {
    use rutent_contracts::core_internal::calls::{GeneralEntry, Log};
    use rutent_contracts::core_internal::{Entry, TimeSource, VerifiedEntry, VerifiedKind, When};
    use rutent_core_audit::log::{AuditLog, KeptInMemory, MemoryStore, Settings};
    use rutent_core_orch::snapshot::Clock as _;
    let context = rutent_conformance::role::SuiteContext::default();
    let settings = Settings::new(
        id("log-1"),
        context.deployment.clone(),
        operator().acts,
        operator().party,
    );
    let mut log =
        AuditLog::open(settings, MemoryStore::new(), KeptInMemory::new()).expect("opened");
    let at = rutent_core_orch::snapshot::SystemClock
        .now()
        .timestamp()
        .unwrap();
    let call = |held: u64| {
        GeneralEntry::try_from(Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: segments_support::core(),
            what: What::StuckCall {
                order: id(ORDER),
                part: id(PART),
                provider: id(RESERVE),
                basis: StuckBasis::TwiceAdvertised,
                figure_seconds: rutent_contracts::provider_role::SafeInteger::try_from(60u64)
                    .unwrap(),
                held_seconds: rutent_contracts::provider_role::SafeInteger::try_from(held).unwrap(),
            },
            when: When {
                at: at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Observed {},
        }))
        .unwrap()
    };
    let (first, second) = (call(121), call(122));
    let receipt = log.append(&first).expect("taken");
    assert!(rutent_core_orch::stuck::is_receipt_of(&receipt, &first));
    assert!(!rutent_core_orch::stuck::is_receipt_of(&receipt, &second));
}

/// The deposit is carried out and its provider answers it settled, but the log can't take its result; the part is
/// declared stuck over time. Its value may still be in motion, so nothing is decided for it: a resolution around a
/// move that may still deliver would pay twice (`part-waits-for-value-in-motion`). Run again, the part records the
/// deposit's result and completes, and then it is no stuck part to decide for. (The stalled move of the re-review's
/// sequence can't arise: no decision is taken while the deposit is in flight.)
// verifies: REQ-AUZ-4, INV-4, INV-8
#[test]
fn nothing_is_decided_while_a_parts_value_is_in_motion() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut stuck = Stuck::new();
    follow(&core, &mut stuck).expect("followed at take-up");
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::StatedResultOf(DEPOSIT), Fault::Unavailable));
    }
    assert!(try_run(&mut core, &mut world).is_err());
    let since = carried_since(&core, DEPOSIT);
    let mut raises = Raises::default();
    core.clock.set(since + 2 * DECLARED + 1);
    assert!(matches!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Stuck(_))
    ));
    let mut policy = PermitsEveryStep::new(&core.log);
    let alice = staff("desk-a", "alice");
    let to = to_destination(&core, AMOUNT);
    assert_eq!(
        decide(&mut core, &mut stuck, &mut policy, &alice, &to, &evidence()).map(|_| ()),
        Err(ResolveError::ValueInMotion(id(DEPOSIT)))
    );
    assert_eq!(policy.asked, 0, "policy isn't asked");
    let ran = try_run(&mut core, &mut world).expect("the deposit's result recorded");
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Settled { .. }),
        "{ran:?}"
    );
    assert_eq!(
        core.segments.order(&id(ORDER)).unwrap().parts[0].state,
        OrderState::Complete
    );
    assert_eq!(
        decide(&mut core, &mut stuck, &mut policy, &alice, &to, &evidence()).map(|_| ()),
        Err(ResolveError::NotStuck)
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The re-review's sequence: the withdrawal settles at its provider but the log can't take its result, and the part
/// is declared stuck holding 1 000 000 in flight. Run again, the withdrawal's settlement is recorded and its hold
/// released; the part is still stuck, its value now resting at the ledger, held there, on the operator's view in
/// full, and decidable in full.
// verifies: REQ-ORD-8, REQ-OPS-2, REQ-AUZ-4
#[test]
fn settlement_recorded_late_keeps_a_stuck_parts_value_held() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut stuck = Stuck::new();
    follow(&core, &mut stuck).expect("followed at take-up");
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::StatedResultOf(WITHDRAWAL), Fault::Unavailable));
    }
    assert!(try_run(&mut core, &mut world).is_err());
    let since = carried_since(&core, WITHDRAWAL);
    let mut raises = Raises::default();
    core.clock.set(since + 2 * DECLARED + 1);
    assert!(matches!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Stuck(_))
    ));
    let ran = try_run(&mut core, &mut world).expect("the withdrawal's result recorded");
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Settled { .. }),
        "{ran:?}"
    );
    assert!(core.segments.stuck(&id(ORDER), &id(PART)));
    assert_eq!(
        core.exposures.in_flight(&id(RESERVE), &cop_unit()),
        Amount::new(AMOUNT).unwrap(),
        "the value resting at the ledger is held"
    );
    assert_eq!(
        stuck.stuck_values(&core.exposures, &*core.clock)[0].amount,
        Amount::new(AMOUNT).unwrap()
    );
    let mut policy = PermitsEveryStep::new(&core.log);
    assert!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &staff("desk-a", "alice"),
            &back_to_origin(AMOUNT),
            &evidence()
        )
        .is_ok_and(|d| matches!(d.outcome, Answer::Permit {}))
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Reports, as `mover`, through `policy`, the known outcome of `decision`'s move, which ended unknown, naming
/// `evidence`.
#[allow(clippy::too_many_arguments)]
fn known_outcome(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut PermitsEveryStep,
    order: &str,
    decision: &rutent_contracts::core_internal::Decision,
    mover: &rutent_contracts::core_internal::Actor,
    outcome: Resolved,
    evidence: &Receipt,
) -> Result<Moved, ResolveError> {
    let (order, carries_out, actor) = (id(order), decided(decision), segments_support::core());
    on_parts(core, |parts| {
        stuck.known_outcome(
            parts,
            policy,
            Moving {
                order: &order,
                carries_out: &carries_out,
                mover,
                core: &actor,
            },
            outcome,
            evidence,
        )
    })
}

/// Confirms, as `confirmer`, through `policy`, naming `evidence`, the report `report` of `decision`'s move: its
/// outcome, its version and its evidence, as the confirmer read them.
#[allow(clippy::too_many_arguments)]
fn confirm_outcome(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut PermitsEveryStep,
    order: &str,
    decision: &rutent_contracts::core_internal::Decision,
    confirmer: &rutent_contracts::core_internal::Actor,
    report: (Resolved, u64, &Receipt),
    evidence: &Receipt,
) -> Result<Moved, ResolveError> {
    let (order, carries_out, actor) = (id(order), decided(decision), segments_support::core());
    on_parts(core, |parts| {
        stuck.confirm_outcome(
            parts,
            policy,
            Confirming {
                order: &order,
                carries_out: &carries_out,
                confirmer,
                outcome: report.0,
                report: report.1,
                reported_with: report.2,
                evidence,
                core: &actor,
            },
        )
    })
}

/// How many of the log's decisions are reports, and how many confirmations, of a move's outcome.
fn outcome_decisions(core: &Core) -> (usize, usize) {
    (
        count(core, |w| matches!(w, What::ReportOutcome { .. })),
        count(core, |w| matches!(w, What::ConfirmOutcome { .. })),
    )
}

/// The outcomes of the results the log holds for `effect`, in order.
fn results_of(core: &Core, effect: &Receipt) -> Vec<Outcome> {
    core.log
        .borrow()
        .verified()
        .iter()
        .filter(|v| matches!(&v.what, What::Result { effect: e } if e == effect))
        .map(|v| v.outcome)
        .collect()
}

/// A resolution whose outcome was unknown is recorded `unanswered`, and its known outcome comes later on two actors'
/// word, each decided by policy: its own mover, for its own order, reports it naming a statement about the part, and
/// settles nothing; an actor of another party and another login confirms it, citing the report and naming another
/// statement, by another maker; only then is the final result recorded, once. The log records the report and the
/// confirmation, each with its actor and its evidence. Done counts what moved, and the part resolves once all of it
/// moved. A move's outcome is reported once: a second report is refused. Nothing is carried out again.
// verifies: REQ-AUZ-4, REQ-ORD-1, INV-8
#[test]
fn unknown_move_takes_its_known_outcome_once_on_two_actors_word() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut policy = PermitsEveryStep::new(&core.log);
    let mut moves = Moves(Vec::new(), [Resolved::Unknown].into());
    let (alice, bob, carol) = (
        staff("desk-a", "alice"),
        staff("desk-b", "bob"),
        staff("desk-c", "carol"),
    );
    let first = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    let Ok(Moved::CarriedOut {
        effect,
        resolved: Resolved::Unknown,
        result: Some(_),
    }) = moves_once(&mut core, &mut stuck, &mut policy, &mut moves, &first, &bob)
    else {
        panic!("unknown, recorded unanswered");
    };
    let (result, other) = results_after_the_resolution(&mut core);
    assert_eq!(results_of(&core, &effect), vec![Outcome::Unanswered {}]);
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &carol,
            Resolved::Done,
            &result
        ),
        Err(ResolveError::NotTheMover)
    );
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            "ord-9",
            &first,
            &bob,
            Resolved::Done,
            &result
        ),
        Err(ResolveError::NoDecision)
    );
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &bob,
            Resolved::Unknown,
            &result
        ),
        Err(ResolveError::OutcomeUnknown)
    );
    assert_eq!(
        confirm_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &alice,
            (Resolved::Done, 1, &result),
            &other
        ),
        Err(ResolveError::NoDecision),
        "nothing to confirm before a report"
    );
    // A statement older than the move can't say what it came to.
    let older = segments_support::resolving::statements_about(&core, WITHDRAWAL)[0]
        .0
        .clone();
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &bob,
            Resolved::Done,
            &older
        ),
        Err(ResolveError::NoEvidence)
    );
    // The mover reports done, once: a second report is refused, and done alone settles nothing.
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &bob,
            Resolved::Done,
            &result
        ),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &bob,
            Resolved::Failed,
            &result
        ),
        Err(ResolveError::AlreadyReported),
        "a move's outcome is reported once"
    );
    assert_eq!(
        outcome_decisions(&core),
        (1, 0),
        "the report decided by policy"
    );
    assert!(
        stuck.is_stuck(&id(ORDER), &id(PART)),
        "the mover's done alone resolves nothing"
    );
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &alice,
            &back_to_origin(AMOUNT),
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::OutcomeUnknown)
    );
    for (confirmer, refused) in [
        (bob.clone(), ResolveError::SameParty),
        (staff("desk-b", "bob-second-login"), ResolveError::SameParty),
        (staff("desk-c", "bob"), ResolveError::SameLogin),
    ] {
        assert_eq!(
            confirm_outcome(
                &mut core,
                &mut stuck,
                &mut policy,
                ORDER,
                &first,
                &confirmer,
                (Resolved::Done, 1, &result),
                &other
            ),
            Err(refused)
        );
    }
    assert_eq!(
        confirm_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &alice,
            (Resolved::Done, 1, &result),
            &result
        ),
        Err(ResolveError::NoEvidence),
        "the confirmation rests on another statement than the report's"
    );
    assert!(matches!(
        confirm_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &alice,
            (Resolved::Done, 1, &result),
            &other
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            result: Some(_),
            ..
        })
    ));
    assert_eq!(
        results_of(&core, &effect),
        vec![Outcome::Unanswered {}, Outcome::Done {}]
    );
    assert_eq!(outcome_decisions(&core), (1, 1));
    assert!(
        !stuck.is_stuck(&id(ORDER), &id(PART)),
        "a confirmed done resolves the part"
    );
    assert_eq!(
        confirm_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &alice,
            (Resolved::Done, 1, &result),
            &other
        ),
        Err(ResolveError::NoDecision),
        "recorded once"
    );
    assert_eq!(moves.0.len(), 1, "nothing carried out again");
    assert_eq!(open_effects(&core), 0, "its effect has its final result");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A resolution of unknown outcome its mover reports failed is recorded so, and its part freed for a new decision,
/// only once another actor confirms it.
// verifies: REQ-AUZ-4, REQ-ORD-1
#[test]
fn confirmed_failure_frees_the_part_for_a_new_decision() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut policy = PermitsEveryStep::new(&core.log);
    let mut moves = Moves(Vec::new(), [Resolved::Unknown].into());
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let first = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    let _ = moves_once(&mut core, &mut stuck, &mut policy, &mut moves, &first, &bob);
    let (result, other) = results_after_the_resolution(&mut core);
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &bob,
            Resolved::Failed,
            &result
        ),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    assert!(matches!(
        confirm_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &alice,
            (Resolved::Failed, 1, &result),
            &other
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Failed,
            result: Some(_),
            ..
        })
    ));
    let second = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .expect("free for a new decision");
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &second,
            &bob
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    assert!(!stuck.is_stuck(&id(ORDER), &id(PART)));
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The grading that refuses every settled segment's evidence, as `CMP-EVIDENCE` refusing it would.
struct RefusesEvidence;

impl rutent_core_orch::segments::Grading for RefusesEvidence {
    fn grade(
        &mut self,
        _settled: &rutent_core_orch::segments::ToGrade<'_>,
    ) -> rutent_core_orch::segments::Graded {
        rutent_core_orch::segments::Graded::Waits(rutent_core_orch::segments::Short::Refused(
            rutent_evidence::Refusal::NoEvidence,
        ))
    }
}

/// The withdrawal's provider answers it settled, and `CMP-EVIDENCE` refuses its evidence: the segment awaits
/// evidence (`AwaitsEvidence(Refused)`, `T-020-09`), carried out with no final outcome and its value held. Its stuck
/// clock runs: past twice its advertised time the part is declared stuck, at what it holds.
// verifies: REQ-ORD-29, REQ-ORD-28
#[test]
fn segment_awaiting_refused_evidence_is_declared_stuck_past_its_limit() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let session = world.context().session;
    let ran = core
        .try_run_with(
            &world.catalogue,
            &mut world.fleet.platform,
            &mut NoProxy,
            &mut RefusesEvidence,
            &session,
        )
        .expect("the part runs");
    assert!(
        matches!(
            segments(&ran)[0].outcome,
            SegmentOutcome::AwaitsEvidence(rutent_core_orch::segments::Short::Refused(_))
        ),
        "the evidence is refused: {ran:?}"
    );
    let since = carried_since(&core, WITHDRAWAL);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    core.clock.set(since + 2 * DECLARED);
    assert_eq!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Running)
    );
    core.clock.set(since + 2 * DECLARED + 1);
    let Ok(Watch::Stuck(called)) = watch(&mut core, &mut stuck, &mut raises) else {
        panic!("stuck past its limit");
    };
    assert_eq!(called.segment.as_str(), WITHDRAWAL);
    assert_eq!(
        stuck.stuck_values(&core.exposures, &*core.clock)[0].amount,
        Amount::new(AMOUNT).unwrap()
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// How many `resolve_stuck` effects the log holds with no result.
fn open_effects(core: &Core) -> usize {
    let log = core.log.borrow();
    let receipts: Vec<_> = (0..log.entries.len())
        .filter(|&i| {
            matches!(&log.entries[i].0,
            rutent_contracts::core_internal::Entry::Verified(v) if is_resolve_stuck(&v.what))
        })
        .map(|i| log.receipt_at(i))
        .collect();
    receipts
        .iter()
        .filter(|r| {
            !log.verified()
                .iter()
                .any(|v| matches!(&v.what, What::Result { effect } if effect == *r))
        })
        .count()
}

/// The withdrawal settles at its provider but the log can't take its result; the part is declared stuck. Nothing is
/// decided while the withdrawal's value may be in motion. Run again, the withdrawal's settlement is recorded and its
/// value rests held at the ledger; then its whole value is returned to its origin, resolving the part, and the rest
/// hold goes with it: a resolved part holds nothing, so nothing of it counts against the ceilings for ever.
// verifies: REQ-ORD-8, REQ-AUZ-4
#[test]
fn resolved_part_takes_no_rest_hold_when_its_settlement_is_recorded_late() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut stuck = Stuck::new();
    follow(&core, &mut stuck).expect("followed at take-up");
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::StatedResultOf(WITHDRAWAL), Fault::Unavailable));
    }
    assert!(try_run(&mut core, &mut world).is_err());
    let since = carried_since(&core, WITHDRAWAL);
    let mut raises = Raises::default();
    core.clock.set(since + 2 * DECLARED + 1);
    assert!(matches!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Stuck(_))
    ));
    let (mut policy, mut moves) = (PermitsEveryStep::new(&core.log), Moves::default());
    let alice = staff("desk-a", "alice");
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &alice,
            &back_to_origin(AMOUNT),
            &evidence()
        )
        .map(|_| ()),
        Err(ResolveError::ValueInMotion(id(WITHDRAWAL)))
    );
    let ran = try_run(&mut core, &mut world).expect("the withdrawal's result recorded");
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Settled { .. }),
        "{ran:?}"
    );
    assert_eq!(
        core.exposures.in_flight(&id(RESERVE), &cop_unit()),
        Amount::new(AMOUNT).unwrap()
    );
    let decision = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &decision,
            &staff("desk-b", "bob")
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    assert_eq!(
        core.exposures.in_flight(&id(RESERVE), &cop_unit()),
        Amount::zero(),
        "a resolved part holds nothing"
    );
    let _ = try_run(&mut core, &mut world);
    assert_eq!(
        core.exposures.in_flight(&id(RESERVE), &cop_unit()),
        Amount::zero()
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A confirmation names the report it confirms: its outcome, its version and its evidence. A confirmation naming
/// another outcome, another version or another evidence than the report's is refused and records nothing, so the
/// confirmer confirms exactly what it read. The report stands: the mover can't replace it. Only a confirmation of
/// the report records it.
// verifies: REQ-AUZ-4, ADR-0011
#[test]
fn confirmation_names_the_report_it_confirms() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut policy = PermitsEveryStep::new(&core.log);
    let mut moves = Moves(Vec::new(), [Resolved::Unknown].into());
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let first = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    let _ = moves_once(&mut core, &mut stuck, &mut policy, &mut moves, &first, &bob);
    let (result, other) = results_after_the_resolution(&mut core);
    let entries = |core: &Core| core.log.borrow().entries.len();
    let refused = |core: &mut Core,
                   stuck: &mut Stuck,
                   policy: &mut PermitsEveryStep,
                   report: (Resolved, u64, &Receipt)| {
        let before = entries(core);
        let answer = confirm_outcome(core, stuck, policy, ORDER, &first, &alice, report, &other);
        assert_eq!(answer, Err(ResolveError::NotTheReport), "{report:?}");
        assert_eq!(entries(core), before, "nothing recorded for {report:?}");
        assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    };
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &bob,
            Resolved::Failed,
            &result
        ),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    refused(
        &mut core,
        &mut stuck,
        &mut policy,
        (Resolved::Done, 1, &result),
    );
    refused(
        &mut core,
        &mut stuck,
        &mut policy,
        (Resolved::Failed, 2, &result),
    );
    refused(
        &mut core,
        &mut stuck,
        &mut policy,
        (Resolved::Failed, 1, &other),
    );
    assert_eq!(
        known_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &bob,
            Resolved::Done,
            &result
        ),
        Err(ResolveError::AlreadyReported),
        "the mover can't replace its report after the confirmer read it"
    );
    assert!(matches!(
        confirm_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &alice,
            (Resolved::Failed, 1, &result),
            &other
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Failed,
            result: Some(_),
            ..
        })
    ));
    assert_eq!(outcome_decisions(&core), (1, 1));
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// With policy's side of reporting and confirming as it stands, unbuilt (`T-021-06`), a mover's report of a
/// resolution's unknown outcome is not known: nothing is pending, nothing settles, and the part stays held. Core
/// fails closed.
// verifies: REQ-AUZ-4, REQ-AUZ-5
#[test]
fn outcome_report_under_policys_unbuilt_rules_settles_nothing() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut policy = PermitsEveryStep::new(&core.log);
    let mut moves = Moves(Vec::new(), [Resolved::Unknown].into());
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let first = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .unwrap();
    let _ = moves_once(&mut core, &mut stuck, &mut policy, &mut moves, &first, &bob);
    let (result, other) = results_after_the_resolution(&mut core);
    let mut unbuilt = Unbuilt::new(PolicyPath(Rc::clone(&core.log)));
    let (order, carries_out, actor) = (id(ORDER), decided(&first), segments_support::core());
    let reported = on_parts(&mut core, |parts| {
        stuck.known_outcome(
            parts,
            &mut unbuilt,
            Moving {
                order: &order,
                carries_out: &carries_out,
                mover: &bob,
                core: &actor,
            },
            Resolved::Done,
            &result,
        )
    });
    assert!(
        matches!(reported, Ok(Moved::NotPermitted(Answer::NotKnown { .. }))),
        "{reported:?}"
    );
    assert_eq!(
        confirm_outcome(
            &mut core,
            &mut stuck,
            &mut policy,
            ORDER,
            &first,
            &alice,
            (Resolved::Done, 1, &result),
            &other
        ),
        Err(ResolveError::NoDecision),
        "nothing pending to confirm"
    );
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Reports, as `reporter`, through `policy`, the outcome of the current order's `segment`'s move, unknown until now,
/// naming `evidence`.
fn report_segment(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut PermitsEveryStep,
    segment: &str,
    reporter: &rutent_contracts::core_internal::Actor,
    outcome: Resolved,
    evidence: &Receipt,
) -> Result<Moved, ResolveError> {
    let (order, part, segment) = (id(ORDER), id(PART), id(segment));
    on_parts(core, |parts| {
        stuck.report_segment_outcome(
            parts,
            policy,
            Reporting {
                order: &order,
                part: &part,
                segment: &segment,
                reporter,
            },
            outcome,
            evidence,
        )
    })
}

/// Confirms, as `confirmer`, through `policy`, naming `evidence`, the report of `segment`'s move, its outcome and
/// evidence as the confirmer read them.
#[allow(clippy::too_many_arguments)]
fn confirm_segment(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut PermitsEveryStep,
    segment: &str,
    confirmer: &rutent_contracts::core_internal::Actor,
    report: (Resolved, &Receipt),
    evidence: &Receipt,
) -> Result<Moved, ResolveError> {
    let (order, part, segment, actor) =
        (id(ORDER), id(PART), id(segment), segments_support::core());
    on_parts(core, |parts| {
        stuck.confirm_segment_outcome(
            parts,
            policy,
            ConfirmingSegment {
                order: &order,
                part: &part,
                segment: &segment,
                confirmer,
                outcome: report.0,
                reported_with: report.1,
                evidence,
                core: &actor,
            },
        )
    })
}

/// `AC-6`: a silent `move_value` on a stuck part. The withdrawal's outcome is unknown, so its result is `unanswered`
/// and the part, declared stuck over time, is decided nothing while its value may still move. Its mover, and only
/// its mover, reports it failed, naming one provider's result about the segment's transaction made after the move;
/// another actor, of another party and login, confirms it on another provider's result. Each is decided by policy,
/// and a report settles nothing alone. Once confirmed, the segments record the move's final result, `failed`, and
/// the segment's `segment_failed`; the segment is no longer in motion, its value still held, and the part is decided
/// and resolved, ending where its resolution sent it. A move reported done is refused, failing closed, since nothing
/// graded would settle it; a second report is refused.
// verifies: REQ-AUZ-4, REQ-ORD-1, REQ-ORD-2, INV-4
#[test]
fn silent_move_on_a_stuck_part_settles_on_two_actors_word_and_frees_the_part() {
    let mut world = World::of(&[RESERVE, OTHER_RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let _ = core.run(&mut world);
    let since = carried_since(&core, WITHDRAWAL);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    follow(&core, &mut stuck).expect("its ends are Core's");
    core.clock.set(since + 2 * DECLARED + 1);
    assert!(matches!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::Stuck(_))
    ));
    let unknown = core
        .segments
        .unknown_move(&id(ORDER), &id(WITHDRAWAL))
        .expect("the withdrawal's move is of unknown outcome");
    assert_eq!(
        results_of(&core, &unknown.effect),
        vec![Outcome::Unanswered {}]
    );
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let in_motion = |core: &mut Core, stuck: &mut Stuck, policy: &mut PermitsEveryStep| {
        decide(
            core,
            stuck,
            policy,
            &alice,
            &back_to_origin(AMOUNT),
            &evidence(),
        )
        .map(|_| ())
    };
    assert_eq!(
        in_motion(&mut core, &mut stuck, &mut policy),
        Err(ResolveError::ValueInMotion(id(WITHDRAWAL)))
    );
    let (reported, confirmed) =
        results_after_the_move(&mut core, Some(WITHDRAWAL), unknown.transaction.as_str());
    let mover = unknown.mover.clone();
    let report = |core: &mut Core,
                  stuck: &mut Stuck,
                  policy: &mut PermitsEveryStep,
                  who: &rutent_contracts::core_internal::Actor,
                  outcome: Resolved| {
        report_segment(core, stuck, policy, WITHDRAWAL, who, outcome, &reported)
    };
    assert_eq!(
        report(&mut core, &mut stuck, &mut policy, &alice, Resolved::Failed),
        Err(ResolveError::NotTheMover),
        "only its mover reports it"
    );
    assert_eq!(
        report(&mut core, &mut stuck, &mut policy, &mover, Resolved::Done),
        Err(ResolveError::DoneNeedsEvidence),
        "done would settle it on nothing graded"
    );
    assert_eq!(
        report(&mut core, &mut stuck, &mut policy, &mover, Resolved::Failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    assert_eq!(
        report(&mut core, &mut stuck, &mut policy, &mover, Resolved::Failed),
        Err(ResolveError::AlreadyReported)
    );
    assert_eq!(
        in_motion(&mut core, &mut stuck, &mut policy),
        Err(ResolveError::ValueInMotion(id(WITHDRAWAL))),
        "a report alone settles nothing"
    );
    assert_eq!(
        confirm_segment(
            &mut core,
            &mut stuck,
            &mut policy,
            WITHDRAWAL,
            &mover,
            (Resolved::Failed, &reported),
            &confirmed
        ),
        Err(ResolveError::SameParty)
    );
    assert_eq!(
        confirm_segment(
            &mut core,
            &mut stuck,
            &mut policy,
            WITHDRAWAL,
            &alice,
            (Resolved::Failed, &reported),
            &reported
        ),
        Err(ResolveError::NoEvidence),
        "the confirmation rests on its own statement"
    );
    assert!(matches!(
        confirm_segment(
            &mut core,
            &mut stuck,
            &mut policy,
            WITHDRAWAL,
            &alice,
            (Resolved::Failed, &reported),
            &confirmed
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Failed,
            result: Some(_),
            ..
        })
    ));
    assert_eq!(
        results_of(&core, &unknown.effect),
        vec![Outcome::Unanswered {}, Outcome::Failed {}]
    );
    assert_eq!(outcome_decisions(&core), (1, 1));
    assert!(!core.segments.handed_over(&id(ORDER), &id(WITHDRAWAL)));
    assert_eq!(
        core.exposures.in_flight(&id(RESERVE), &cop_unit()),
        Amount::new(AMOUNT).unwrap(),
        "its value stays held, for the part's resolution"
    );
    let decision = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .expect("decided once nothing of the part is in motion");
    let mut moves = Moves::default();
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &decision,
            &bob
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    let followed = core.segments.order(&id(ORDER)).expect("followed");
    assert_eq!(followed.parts[0].state, OrderState::EndedWithoutCompleting);
    assert!(matches!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::AlreadyStuck)
    ));
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The transaction the log's first stuck decision named for its resolution.
fn decided_transaction(core: &Core) -> Id {
    core.log
        .borrow()
        .verified()
        .iter()
        .find_map(|v| match &v.what {
            What::StuckDecide { request, .. } => Some(request.transaction.clone()),
            _ => None,
        })
        .expect("a stuck decision")
}

/// Two results about the transaction the current order's stuck decision named, by two providers, made and appended
/// now: [`RESERVE`]'s, then [`OTHER_RESERVE`]'s. A resolution's outcome is reported and confirmed on them
/// (`outcome-confirmation-cites-verified-evidence`).
fn results_after_the_resolution(core: &mut Core) -> (Receipt, Receipt) {
    let transaction = decided_transaction(core);
    results_after_the_move(core, None, transaction.as_str())
}

/// The order's first permitted authorisation, as the log holds it, given anew: under `authorisation`, for `parts`
/// (none: the whole order), by `given_by`, delivering to `account` at the order's destination and drawing from
/// `origin_account` at its origin.
fn authorised_anew(
    core: &Core,
    authorisation: &str,
    parts: Option<Vec<Id>>,
    given_by: rutent_contracts::core_internal::Actor,
    account: &str,
    origin_account: &str,
) -> rutent_contracts::core_internal::Entry {
    let mut first = recorded(core)
        .into_iter()
        .find_map(|e| match e {
            rutent_contracts::core_internal::Entry::Verified(v)
                if matches!(v.what, What::Authorise { .. })
                    && matches!(v.outcome, Outcome::Permitted {}) =>
            {
                Some(v)
            }
            _ => None,
        })
        .expect("the order's authorisation");
    if let What::Authorise {
        authorisation: a, ..
    } = &mut first.what
    {
        a.authorisation = id(authorisation);
        a.parts = parts;
        a.destination.account = id(account);
        a.origin_account = Some(id(origin_account));
        a.given_by = given_by.clone();
    }
    first.who = given_by;
    rutent_contracts::core_internal::Entry::Verified(first)
}

/// `T-021-04`'s per-part authorisations: a split order whose first part's destination changes, the customer
/// authorising `p1` alone to another account. Following the order again takes `p1`'s new ends, as the log reads the
/// authorisation in force for it, and keeps `p2`'s. A difference no later authorisation of the customer's explains is
/// still refused and changes nothing: the first authorisation read otherwise. A later one by anyone else, or by the
/// customer's own party in another role, is in force for no part, as the engine reads it, and changes nothing either.
/// Where a later one of the customer's differs only in its origin account, its origin rules, as the log's rule takes
/// it for the part's origin (`delivery-goes-to-the-parts-ends`).
// verifies: REQ-AUZ-4, REQ-AUZ-3
#[test]
fn a_parts_new_destination_replaces_its_ends_and_keeps_the_others() {
    let mut world = World::of(&[RESERVE]);
    let core = Core::split(&mut world);
    let mut stuck = Stuck::new();
    follow(&core, &mut stuck).expect("its ends are Core's");
    let (order, p1, p2) = (id(ORDER), id(PART), id(SECOND_PART));
    let at = |account: &str| Destination {
        account: id(account),
        ..destination(&core)
    };
    let origin_at = |account: &str| Destination {
        account: id(account),
        ..origin()
    };
    let first = (destination(&core), Some(origin()));
    assert_eq!(stuck.ends_of(&order, &p1), Some(first.clone()));
    assert_eq!(stuck.ends_of(&order, &p2), Some(first.clone()));

    // Someone else's, and the customer's party in another role: in force for no part.
    let customers_party_as_operator = rutent_contracts::core_internal::Actor {
        role: rutent_contracts::core_internal::Role::Operator,
        ..rutent_scenarios::kit::customer()
    };
    for given_by in [staff("desk-a", "alice"), customers_party_as_operator] {
        let mut others = recorded(&core);
        others.push(authorised_anew(
            &core,
            "auth-p1-by-another",
            Some(vec![p1.clone()]),
            given_by,
            "acct-of-a-friend",
            "acct-1",
        ));
        stuck
            .follow(&core.segments, &others, &order)
            .expect("another's authorisation sets nothing");
        assert_eq!(stuck.ends_of(&order, &p1), Some(first.clone()));
    }

    // The first authorisation read otherwise, with no later one of the customer's: refused, nothing changed.
    let tampered: Vec<_> = recorded(&core)
        .into_iter()
        .map(|e| match e {
            rutent_contracts::core_internal::Entry::Verified(mut v)
                if matches!(v.what, What::Authorise { .. }) =>
            {
                if let What::Authorise { authorisation, .. } = &mut v.what {
                    authorisation.destination.account = id("acct-of-a-friend");
                }
                rutent_contracts::core_internal::Entry::Verified(v)
            }
            other => other,
        })
        .collect();
    assert_eq!(
        stuck.follow(&core.segments, &tampered, &order),
        Err(StuckError::NotItsAuthorisation)
    );
    assert_eq!(stuck.ends_of(&order, &p1), Some(first.clone()));

    // The customer's new destination for p1, appended to the log: p1 takes it, p2 keeps its own.
    authorise_in_the_log(
        &core,
        authorised_anew(
            &core,
            "auth-p1-new",
            Some(vec![p1.clone()]),
            rutent_scenarios::kit::customer(),
            "acct-new",
            "acct-1",
        ),
    );
    follow(&core, &mut stuck).expect("a part's new destination replaces its ends");
    assert_eq!(
        stuck.ends_of(&order, &p1),
        Some((at("acct-new"), Some(origin())))
    );
    assert_eq!(stuck.ends_of(&order, &p2), Some(first.clone()));
    follow(&core, &mut stuck).expect("following again with the same is nothing");
    assert_eq!(
        stuck.ends_of(&order, &p1),
        Some((at("acct-new"), Some(origin())))
    );

    // The customer's, differing only in its origin account: the log's rule takes it for p2's origin.
    authorise_in_the_log(
        &core,
        authorised_anew(
            &core,
            "auth-p2-origin",
            Some(vec![p2.clone()]),
            rutent_scenarios::kit::customer(),
            destination(&core).account.as_str(),
            "acct-9",
        ),
    );
    follow(&core, &mut stuck).expect("the customer's origin account rules");
    assert_eq!(
        stuck.ends_of(&order, &p2),
        Some((destination(&core), Some(origin_at("acct-9"))))
    );
    assert_eq!(
        stuck.ends_of(&order, &p1),
        Some((at("acct-new"), Some(origin())))
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A decision made before its part's terms were replaced is carried out no more: the part's destination changes after
/// the decision to deliver there, the customer authorising the part alone to another account, and following the order
/// again takes the new ends. Moving the old decision is refused before policy is asked, with no effect recorded and
/// nothing moved: it would deliver where the authorisation in force no longer names (`REQ-AUZ-3`, `THR-77`). A
/// decision to the part's origin, which the new authorisation leaves as it was, still moves.
// verifies: REQ-AUZ-4, REQ-AUZ-3
#[test]
fn decision_made_before_its_parts_ends_were_replaced_moves_nothing() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut policy = PermitsEveryStep::new(&core.log);
    let mut moves = Moves::default();
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let delivery = to_destination(&core, AMOUNT);
    let to_the_old_destination = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &delivery,
        &evidence(),
    )
    .expect("decided while the destination stood");
    replace_the_parts_destination(&mut core, &mut stuck);
    let asked = policy.asked;
    assert_eq!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &to_the_old_destination,
            &bob
        ),
        Err(ResolveError::ElsewhereThanItsEnds)
    );
    assert_eq!(policy.asked, asked, "policy isn't asked");
    assert!(moves.0.is_empty(), "nothing moves");
    assert_eq!(count(&core, is_resolve_stuck), 0, "no effect is recorded");
    let to_its_origin = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .expect("its origin stands");
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &to_its_origin,
            &bob
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Appends `entry`, a permitted `authorise`, to the kit's log on policy's handle, as the engine records it.
fn authorise_in_the_log(core: &Core, entry: rutent_contracts::core_internal::Entry) {
    use rutent_contracts::core_internal::calls::{DecisionEntry, PolicyLog};
    let rutent_contracts::core_internal::Entry::Verified(entry) = entry else {
        panic!("an authorisation is a verified entry");
    };
    PolicyPath(Rc::clone(&core.log))
        .append_decision(&DecisionEntry::try_from(entry).expect("a decision"))
        .expect("the log takes the customer's authorisation");
}

/// The customer authorises the kit's part alone to another account, recorded in the log, and Core follows the order
/// again, taking the part's new ends.
fn replace_the_parts_destination(core: &mut Core, stuck: &mut Stuck) {
    authorise_in_the_log(
        core,
        authorised_anew(
            core,
            "auth-p1-new",
            Some(vec![id(PART)]),
            rutent_scenarios::kit::customer(),
            "acct-new",
            "acct-1",
        ),
    );
    follow(core, stuck).expect("the part's new destination replaces its ends");
}

/// The reviewer's sequence (`T-020-11b`, U1): a decision to deliver to the part's destination; its move stalls, the
/// log unable to take its effect; the customer then gives the part another destination, which the log records and
/// Core follows. Taken on again by its mover, the stalled move reads the part's ends again before its effect is
/// appended: it would deliver where the authorisation in force no longer names, so it is dropped, with no effect
/// recorded and nothing moved, and its decision spent. A decision to the part's origin still moves.
// verifies: REQ-AUZ-4, REQ-AUZ-3, INV-4
#[test]
fn stalled_move_taken_on_after_its_parts_ends_were_replaced_moves_nothing() {
    let (world, mut core, mut stuck) = stuck_by_no_route();
    let mut policy = PermitsEveryStep::new(&core.log);
    let mut moves = Moves::default();
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let delivery = to_destination(&core, AMOUNT);
    let decision = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &delivery,
        &evidence(),
    )
    .expect("decided while the destination stood");
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Effect, Fault::Unavailable));
    }
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &decision,
            &bob
        ),
        Err(ResolveError::Append(_))
    ));
    replace_the_parts_destination(&mut core, &mut stuck);
    assert_eq!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &decision,
            &bob
        ),
        Err(ResolveError::ElsewhereThanItsEnds)
    );
    assert!(moves.0.is_empty(), "nothing moves");
    assert_eq!(count(&core, is_resolve_stuck), 0, "no effect is recorded");
    assert_eq!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &decision,
            &bob
        ),
        Err(ResolveError::NoDecision),
        "its decision is spent"
    );
    let to_its_origin = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &back_to_origin(AMOUNT),
        &evidence(),
    )
    .expect("the part is free for a new decision");
    assert!(matches!(
        moves_once(
            &mut core,
            &mut stuck,
            &mut policy,
            &mut moves,
            &to_its_origin,
            &bob
        ),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}
