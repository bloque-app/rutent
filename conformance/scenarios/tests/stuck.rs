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
    Action, Answer, Decided, Destination, Difference, Outcome, Resolution, StuckBasis, What,
};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Id, PositiveAmount};
use rutent_contracts::state_machine::{Observed, OrderState, SegmentState};
use rutent_core_authz::engine::Unbuilt;
use rutent_core_orch::recalc::{NotRecalculable, Recalculation};
use rutent_core_orch::segments::{
    Accounted, NoProxy, ProvidersWord, Ran, SegmentOutcome, SegmentsError, StatusError,
};
use rutent_core_orch::stuck::{
    Deciding, Ends, Moved, Moving, NoRouteLeft, ResolveError, Resolved, Stuck, StuckError,
    StuckValue, Watch, seen_by_provider,
};
use segments_support::rerouting::Rerouting;
use segments_support::resolving::{
    Moves, PermitsEveryStep, Raises, destination, on_parts, origin, staff, watch,
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
        &mut ProvidersWord,
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

/// The kit's order and two more settled at the reserve, each operation taking `seconds`, so each of its tuples has
/// a history; the third order is current.
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

/// The kit's order with its withdrawal settled and its deposit never handed over (the platform wouldn't begin it),
/// then recalculated from the reserve's ledger where its value sits, and offered no route: the reserve refuses every
/// dry run of the deposit.
fn left_no_route() -> (World, Core, Recalculation) {
    let mut world = World::of(&[RESERVE]);
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
    let (to, from) = (destination(core), origin());
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
                ends: Ends {
                    destination: &to,
                    origin: &from,
                },
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
        held,
        "nothing held or released"
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

/// Two orders settle at the reserve, each operation taking 30 s, so each tuple's p99 is 30 s. A third order's
/// withdrawal is carried out and its outcome is unknown, its funds held. At exactly twice the p99 it isn't stuck; a
/// second later it is: the figures are recorded, the part declared stuck over time, the order marked stuck, and the
/// declaration handed back as the operator's warning, with the order on the operator's stuck view at what it holds.
/// The advertised 60 s would have given it until 120 s.
// verifies: REQ-ORD-29
#[test]
fn segment_past_twice_the_p99_makes_the_order_stuck_and_warns_the_operator() {
    let (mut world, mut core) = settled_history(&[30, 30]);
    core.take_up(Named::another(3));
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let ran = run_taking(&mut core, &mut world, 30);
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Unknown(_)),
        "{ran:?}"
    );
    let since = carried_since(&core, "p1-s1-of-3");
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
        ("ord-3", "p1-s1-of-3", RESERVE, StuckBasis::TwiceP99, 30, 61)
    );
    let order = id("ord-3");
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
    assert!(raises.raised.is_empty(), "the p99 didn't move");
    assert_eq!(
        watch(&mut core, &mut stuck, &mut raises),
        Ok(Watch::AlreadyStuck)
    );
    assert_eq!(count(&core, is_stuck_declared), 1);
    assert_eq!(
        try_run(&mut core, &mut world),
        Err(SegmentsError::Stuck(id(PART)))
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

/// Orders settle at the reserve taking 30 s, then 31 s: the p99 moves under half its baseline, and nothing is
/// raised. One taking 60 s doubles it: each tuple's change is raised to the operator, citing the settlements it rests
/// on, and recorded as delivered. Learnt again, nothing is raised twice. A raise the operator's channel doesn't
/// deliver is recorded as failed.
// verifies: REQ-ORD-6
#[test]
fn material_change_in_a_providers_p99_is_raised() {
    let (mut world, mut core) = settled_history(&[30, 31]);
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    let learn = |core: &mut Core, stuck: &mut Stuck, raises: &mut Raises| {
        let actor = segments_support::core();
        on_parts(core, |parts| stuck.learn(parts, raises, &actor)).expect("learnt")
    };
    assert!(learn(&mut core, &mut stuck, &mut raises).is_empty());
    assert!(
        raises.raised.is_empty(),
        "31 s is no material change from 30 s"
    );

    core.take_up(Named::another(3));
    let _ = run_taking(&mut core, &mut world, 60);
    let raised = learn(&mut core, &mut stuck, &mut raises);
    assert_eq!(
        raised.len(),
        2,
        "each of the reserve's two tuples: {raised:?}"
    );
    for r in &raised {
        assert!(r.delivered);
        let Difference::SettlementTime {
            provider,
            before_seconds,
            now_seconds,
        } = &r.raise.difference
        else {
            panic!("a settlement time difference");
        };
        assert_eq!(
            (
                provider.as_str(),
                u64::from(*before_seconds),
                u64::from(*now_seconds)
            ),
            (RESERVE, 30, 60)
        );
        assert_eq!(r.raise.cites.len(), 2);
    }
    assert_eq!(raises.raised.len(), 2);
    let done = core
        .log
        .borrow()
        .verified()
        .iter()
        .filter(|v| matches!(v.what, What::Raise { .. }) && v.outcome == (Outcome::Done {}))
        .count();
    assert_eq!(done, 2);
    assert!(
        learn(&mut core, &mut stuck, &mut raises).is_empty(),
        "nothing raised twice"
    );
    assert_eq!(raises.raised.len(), 2);

    raises.delivers = false;
    core.take_up(Named::another(4));
    let _ = run_taking(&mut core, &mut world, 120);
    let raised = learn(&mut core, &mut stuck, &mut raises);
    assert_eq!(raised.len(), 2);
    assert!(raised.iter().all(|r| !r.delivered));
    let failed = core
        .log
        .borrow()
        .verified()
        .iter()
        .filter(|v| matches!(v.what, What::Raise { .. }) && v.outcome == (Outcome::Failed {}))
        .count();
    assert_eq!(failed, 2);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A material change in the p99 is raised, and the log refuses the raise's entries: the raise is dropped, never
/// retried, and the stuck clock still runs. A part over twice the new p99 is declared stuck all the same, since
/// detection never holds up the primary path.
// verifies: REQ-ORD-29, REQ-ORD-9
#[test]
fn raise_the_log_refuses_holds_up_no_stuck_call() {
    let (mut world, mut core) = settled_history(&[30, 60]);
    core.take_up(Named::another(3));
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let _ = run_taking(&mut core, &mut world, 30);
    let since = carried_since(&core, "p1-s1-of-3");
    for _ in 0..2 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Raise, Fault::Refuse));
    }
    let (mut stuck, mut raises) = (Stuck::new(), Raises::default());
    core.clock.set(since + 2 * 60 + 1);
    let Ok(Watch::Stuck(called)) = watch(&mut core, &mut stuck, &mut raises) else {
        panic!("stuck though the raises were refused");
    };
    assert_eq!(
        (called.basis, called.figure_seconds),
        (StuckBasis::TwiceP99, 60)
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
/// by the decider's own party, through another login, and a move naming a decision Core doesn't hold, a forged one
/// or one of another order. Nothing moves and no effect is recorded.
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
    assert_eq!(
        moves.0[0].action,
        Action::ResolveStuck {
            order: id(ORDER),
            carries_out: decided(&permitted),
            resolution: resolution.clone(),
        }
    );
    assert_eq!(count(&core, is_resolve_stuck), 1);
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
