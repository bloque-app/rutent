//! A segment that fails mid-route, over the assembled Core against the simulated providers that join one another's nodes
//! (`T-024-06`): its reversal owed, asked and confirmed, the route recalculated from where the value is shown back
//! (`AC-4`), and, with no route left, the order stuck and resolved by the operator in two steps by two actors (`AC-6`).
//! Each drives Core's own `request`, `quote`, `accept` and `run`, and its reversals, recalculation and stuck order,
//! over [`execution_support`]'s world.
//!
//! The stuck resolution's move is made by Core's own mover (`T-020-16`).

mod execution_support;
mod failure_support;

use execution_support::*;
use failure_support::*;
use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Decided, Destination, Entry, Resolution, Role, What,
};
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::provider_role::OuterNode;
use rutent_contracts::router_api::{Id, PositiveAmount};
use rutent_contracts::state_machine::{Observed, OrderState, SegmentState};
use rutent_core_authz::roles::Permission;
use rutent_core_orch::intake::OrderRequest;
use rutent_core_orch::recalc::{NotRecalculable, Recalculation};
use rutent_core_orch::reversal::{Confirmation, NotCounted, Word};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_core_orch::stuck::{Moved, ResolveError, Resolved};
use rutent_scenarios::assembly::{Core, ORIGIN_ACCOUNT};
use rutent_scenarios::kit::{self, AMOUNT, id};
use rutent_scenarios::on_tick::OnTick;

const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const QUICK: &str = "sim-join-ramp-quick";
const OUT: &str = "sim-join-out";

/// The order that carries `AMOUNT` of COP from the reserve `sim-join-in`'s bank to USD at the bank of `sim-join-out`.
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

/// The order across the joined providers, run with its exchange failing: the withdrawal settled and the value
/// converted nowhere, the exchange's reversal owed. Core, the order, the withdrawal and the exchange's segments.
fn exchange_failed(accepts: &'static str) -> (Core, Id, Id, Id) {
    let (mut core, session) = core_with(accepts, &[IN, EXCHANGE, QUICK, OUT]);
    core.world()
        .script_for(EXCHANGE, "conversion", Behaviour::act(Act::Fail));
    let (order, ran) = run(&mut core, across("req-fails", &session));
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[0].outcome, SegmentOutcome::Settled { .. }),
        "{segments:?}"
    );
    assert!(
        matches!(segments[1].outcome, SegmentOutcome::Failed),
        "the exchange failed: {segments:?}"
    );
    assert_eq!(segments.len(), 2, "nothing is handed over after it");
    (
        core,
        order,
        segments[0].segment.clone(),
        segments[1].segment.clone(),
    )
}

/// Every entry of Core's log is admitted on what came before it.
fn admitted_log(core: &Core) {
    use rutent_contracts::core_internal::{DeploymentOperator, Through, admit_entry, calls};
    let history: Vec<_> = core.log().entries().to_vec();
    let operator = DeploymentOperator {
        maker: kit::operator().acts,
        party: kit::operator().party,
    };
    for (i, (_, entry)) in history.iter().enumerate() {
        let through = match entry {
            Entry::Verified(v) if calls::is_decision(&v.what) => Through::Policy,
            _ => Through::General,
        };
        if let Err(violation) = admit_entry(&history[..i], entry, through, &operator) {
            panic!("entry {i} isn't admitted: {violation:?}");
        }
    }
}

/// The state of `segment` as Core follows it.
fn segment_state(core: &Core, order: &Id, segment: &Id) -> SegmentState {
    core.followed(order).expect("Core follows the order").parts[0]
        .segments
        .iter()
        .find(|s| s.segment_id.as_str() == segment.as_str())
        .expect("the segment is followed")
        .state
}

/// A failed middle segment's reversal is Core's run's, on ticks alone (`T-020-61`, item 10; the review of #355, H1): on
/// the tick after the exchange failed, Core records it owed and refuses to recalculate the part, its value in flight; on
/// the next it asks the exchange, which refuses, moving nothing; on the next it asks the origin reserve, whose word
/// about its own publisher's provider completes nothing either; once the retry cadence has passed it asks the exchange
/// again, which it may since the first ask moved nothing, and the answer inside its publisher's node completes it; then
/// the part is recalculated from the node the value is shown back at, and the recalculated route runs, and the order
/// completes. No test calls a step: each is a tick of the deployment's timer.
// verifies: REQ-ORD-3, REQ-ORD-30
#[test]
fn failed_segments_reversal_is_owed_asked_and_confirmed_by_cores_run() {
    use rutent_contracts::core_internal::Outcome;
    let (mut core, order, _, exchange) = exchange_failed("AC-4");
    core.world()
        .script_for(EXCHANGE, "reversal", Behaviour::act(Act::Refuse));
    core.world().script(IN, Behaviour::crediting_a_reversal());

    // Owed: its own step, and the recalculation refused while the reversal holds the value in flight (R12).
    let round = core.tick();
    let reversed = round.runs.reversed(&order);
    assert_eq!(reversed.len(), 1, "{reversed:?}");
    let owing = reversed[0].owed.clone().expect("owed");
    assert_eq!(
        (owing.provider.as_str(), owing.asset.as_str()),
        (EXCHANGE, "COP")
    );
    assert!(reversed[0].asked.is_none() && reversed[0].confirmed.is_none());
    assert!(
        matches!(
            round.runs.recalculated(&order),
            Some(Ok(Recalculation::NotNow(NotRecalculable::InFlight {
                state: SegmentState::FailedReversalOwed,
                ..
            })))
        ),
        "no route starts from a reversal owed: {:?}",
        round.runs.recalculated(&order)
    );

    // Asked of the failed exchange, which refuses: nothing moved.
    let round = core.tick();
    let reversed = round.runs.reversed(&order);
    let asked = reversed[0].asked.clone().expect("asked").expect("asked");
    assert_eq!(asked.why, Some(NotCounted::Refused), "{asked:?}");
    assert!(reversed[0].confirmed.is_none());

    // Confirmed with the origin reserve, whose word about its own publisher's provider completes nothing; the exchange
    // isn't asked again before the retry cadence has passed.
    let round = core.tick();
    let reversed = round.runs.reversed(&order);
    assert!(reversed[0].asked.is_none());
    assert!(
        matches!(
            reversed[0].confirmed,
            Some(Ok(Confirmation::NotCompleted {
                why: NotCounted::SamePublisher,
                ..
            }))
        ),
        "{reversed:?}"
    );
    assert_eq!(
        segment_state(&core, &order, &exchange),
        SegmentState::FailedReversalOwed
    );
    let round = core.tick();
    assert!(
        round.runs.reversed(&order)[0].asked.is_none()
            && round.runs.reversed(&order)[0].confirmed.is_none(),
        "nothing is pressed on every tick: {:?}",
        round.runs.reversed(&order)
    );

    // Past the cadence, asked again, since the first ask moved nothing: the exchange answers inside its publisher's
    // node, complete, and the part waits for the next tick.
    core.world()
        .script_for(EXCHANGE, "reversal", Behaviour::default());
    core.clock
        .advance(rutent_core_orch::reversal::DEFAULT_REVERSAL_RETRY_SECONDS);
    let round = core.tick();
    let reversed = round.runs.reversed(&order);
    assert!(reversed[0].completed(), "{reversed:?}");
    assert!(round.runs.recalculated(&order).is_none());
    assert_eq!(
        segment_state(&core, &order, &exchange),
        SegmentState::ReversalComplete
    );
    let asks: Vec<_> = core
        .log()
        .entries()
        .iter()
        .filter_map(|(receipt, e)| match e {
            Entry::Verified(v) if matches!(&v.what, What::Effect { action: Action::AskReversal { order: o, .. }, .. } if o == &order) => {
                Some(receipt.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(asks.len(), 2, "asked twice, the first refused");
    assert!(
        entries(&core).iter().any(
            |v| matches!(&v.what, What::Result { effect } if effect == &asks[0])
                && v.outcome == Outcome::Failed {}
        ),
        "the first ask failed, moving nothing"
    );

    // Recalculated from where the value is shown back, then run: the order completes on ticks alone.
    let round = core.tick();
    assert!(
        matches!(
            round.runs.recalculated(&order),
            Some(Ok(Recalculation::Recalculated(_)))
        ),
        "{:?}",
        round.runs.recalculated(&order)
    );
    core.tick();
    assert_eq!(state(&mut core, &order), OrderState::Complete);
}

/// A reversal whose outcome is unknown is never asked again (the review of #355's round 2, H1): the exchange is asked
/// once and answers that it can't tell, which may mean it paid the value back. Core carries that ask on once, under
/// its own transaction (the exchange declares neither a status query nor a safe repeat, so the answer the platform
/// recorded is read back), and asks the origin reserve to confirm on the retry cadence, up to its cap; however many
/// ticks pass, and across a restart of Core, the exchange is handed one reversal operation of the conversion and Core's
/// log holds one ask. A second ask, the log refuses (`reversal-is-asked-again-only-after-a-refusal`).
// verifies: REQ-ORD-3, INV-8, NFR-6
#[test]
fn reversal_whose_outcome_is_unknown_is_never_asked_again_under_a_new_transaction() {
    use rutent_contracts::provider_role::Operation;
    use rutent_core_orch::reversal::{DEFAULT_REVERSAL_RETRY_SECONDS, MAX_REVERSAL_TRIES};
    let (mut core, order, _, exchange) = exchange_failed("AC-4");
    core.world()
        .script_for(EXCHANGE, "reversal", Behaviour::act(Act::Unknown));
    let asks = |core: &Core| {
        entries(core)
            .iter()
            .filter(|v| {
                matches!(&v.what, What::Effect { action: Action::AskReversal { order: o, .. }, .. } if o == &order)
            })
            .count()
    };
    let read_back = |core: &Core| {
        entries(core)
            .iter()
            .filter(|v| {
                matches!(&v.what, What::Effect { action: Action::ReadRecordedAnswer { order: o, .. }, .. } if o == &order)
            })
            .count()
    };
    let handed = |core: &mut Core| {
        core.world()
            .journal(EXCHANGE)
            .iter()
            .filter(|e| matches!(e.operation.statement, Operation::Reversal(_)))
            .count()
    };
    // Owed, then asked: the exchange can't tell.
    core.tick();
    let round = core.tick();
    let asked = round.runs.reversed(&order)[0]
        .asked
        .clone()
        .expect("asked")
        .expect("asked");
    assert!(asked.completed.is_none(), "{asked:?}");
    assert_eq!(handed(&mut core), 1);
    // Carried on once under its own transaction: the recorded answer read back, moving nothing.
    let round = core.tick();
    let carried = round.runs.reversed(&order)[0]
        .carried_on
        .clone()
        .expect("carried on")
        .expect("carried on");
    assert!(carried.completed.is_none(), "{carried:?}");
    assert_eq!(read_back(&core), 1);
    // Then only confirmed, on the cadence, however many ticks pass.
    for _ in 0..6 {
        core.clock.advance(DEFAULT_REVERSAL_RETRY_SECONDS);
        let round = core.tick();
        let reversed = round.runs.reversed(&order);
        assert!(
            reversed
                .iter()
                .all(|r| r.asked.is_none() && r.carried_on.is_none()),
            "{reversed:?}"
        );
    }
    assert_eq!(asks(&core), 1, "asked once");
    assert_eq!(handed(&mut core), 1, "one reversal operation handed over");
    let confirmations = core.confirmations_asked(&order, &exchange);
    assert!(
        (1..=MAX_REVERSAL_TRIES).contains(&confirmations),
        "{confirmations}"
    );
    // Across a restart: the reopened Core rebuilds the ask, its carrying-on and the confirmations from the log, and
    // asks nothing again.
    let given = execution_support::given(&[IN, EXCHANGE, QUICK, OUT]);
    let mut core = rutent_scenarios::harness::Scenario::new("execution", "AC-4", given).reopen(
        core,
        1,
        &mut rutent_scenarios::assembly::machine::Reference,
    );
    assert_eq!(core.confirmations_asked(&order, &exchange), confirmations);
    for _ in 0..4 {
        core.clock.advance(DEFAULT_REVERSAL_RETRY_SECONDS);
        let round = core.tick();
        let reversed = round.runs.reversed(&order);
        assert!(
            reversed
                .iter()
                .all(|r| r.asked.is_none() && r.carried_on.is_none()),
            "{reversed:?}"
        );
    }
    assert_eq!(asks(&core), 1, "asked once, a restart included");
    assert_eq!(read_back(&core), 1, "carried on once, a restart included");
    assert_eq!(
        handed(&mut core),
        1,
        "one reversal operation, a restart included"
    );
    // Past the cap Core confirms no more: the stuck check times the part.
    for _ in 0..(2 * MAX_REVERSAL_TRIES) {
        core.clock.advance(DEFAULT_REVERSAL_RETRY_SECONDS);
        core.tick();
    }
    assert_eq!(
        core.confirmations_asked(&order, &exchange),
        MAX_REVERSAL_TRIES
    );
    assert_eq!(asks(&core), 1);
    assert_eq!(state(&mut core, &order), OrderState::Stuck);
    admitted_log(&core);
}

// verifies: AC-4
#[test]
fn true_undo_is_shown_back_at_the_origin_by_another_party() {
    let (mut core, order, withdrawal, exchange) = exchange_failed("AC-4");
    assert_eq!(
        segment_state(&core, &order, &exchange),
        SegmentState::FailedReversalOwed
    );
    // The failed exchange's value is in motion, not evidenced anywhere: no route is recalculated from it.
    assert_eq!(
        core.recalculated_on_tick(&order).expect("tried"),
        Recalculation::NotNow(NotRecalculable::InFlight {
            segment: rutent_contracts::state_machine::Id::new(exchange.as_str()).expect("an id"),
            state: SegmentState::FailedReversalOwed
        })
    );
    // The reversal is owed by the exchange, in the asset it took in and in full.
    let owing = core.owed_reversal(&order, &exchange).expect("owed");
    assert_eq!(owing.provider.as_str(), EXCHANGE);
    assert_eq!(owing.asset, "COP");

    // Core's run asks the failed exchange to reverse on the tick after it recorded it owed: its own word completes
    // nothing, except inside the node its own publisher holds, where it is that publisher's, said so: the reserve's at
    // its own node. (The reserve's own word about its publisher's provider completes nothing: `publishers_binding`.)
    let asked = core.asked_on_tick(&order, &exchange).expect("asked");
    let completed = asked.completed.expect("the node is its publisher's own");
    assert_eq!(completed.word, Word::Publishers(id("sim-join-publisher")));
    assert_eq!(
        segment_state(&core, &order, &exchange),
        SegmentState::ReversalComplete
    );
    assert_eq!(
        core.completed_reversal(&order, &exchange).map(|c| c.word),
        Some(completed.word.clone())
    );
    assert_eq!(
        count_inputs(&core, &order, |o| matches!(
            o,
            Observed::ReversalCompleted { .. }
        )),
        1,
        "the log took the undo once"
    );

    // The value is shown back at the node the exchange took it from, and the route is recalculated from there,
    // automatically: to the same destination and in the same to asset, and with no yes asked of the customer again.
    let acceptances = |core: &Core| {
        entries(core)
            .iter()
            .filter(|v| matches!(&v.what, What::Acceptance { order: o, .. } if o == &order))
            .count()
    };
    assert_eq!(acceptances(&core), 1);
    let Recalculation::Recalculated(recalculated) =
        core.recalculated_on_tick(&order).expect("tried")
    else {
        panic!("a route from where the value is shown back");
    };
    assert_eq!(
        Some(recalculated.route.destination.clone()),
        core.requested_destination(&order)
    );
    assert_eq!(recalculated.route.to_asset.id, "USD");
    assert_eq!(
        recalculated.from.node,
        NodeRef::Inner("sim-join-ledger".to_owned())
    );
    assert_eq!(
        count_inputs(&core, &order, |o| matches!(
            o,
            Observed::DestinationChanged { .. }
        )),
        0
    );
    assert_eq!(acceptances(&core), 1, "no yes was asked again");
    let ran = core
        .ran_once_on_tick(&order)
        .expect("the recalculated part runs");
    assert!(
        segments_of(&[ran])
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. }))
    );
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    let _ = withdrawal;
}

/// The order, its exchange failed and reversed by its publisher inside their node, and recalculated with the exchange
/// refusing every dry run: no route is left from where the value sits, and the order is stuck there.
fn stuck_with_no_route(accepts: &'static str) -> (Core, Id, Id) {
    let (mut core, order, _, exchange) = exchange_failed(accepts);
    let asked = core.asked_on_tick(&order, &exchange).expect("asked");
    assert!(
        asked.completed.is_some(),
        "the reversal completed: {asked:?}"
    );
    core.world()
        .script_dry_run(EXCHANGE, "conversion", Behaviour::act(Act::Refuse));
    let outcome = core.recalculated_on_tick(&order).expect("tried");
    assert!(
        matches!(outcome, Recalculation::NoRoute { .. }),
        "no route is left: {outcome:?}"
    );
    // Core's run recorded it on the same tick (`REQ-ORD-28`, `T-020-61`).
    assert_eq!(
        core.followed(&order).expect("followed").parts[0].state,
        OrderState::Stuck
    );
    (core, order, exchange)
}

/// The operator's staff: two persons on two desks, each acting for the operator, each holding one permission.
fn staff_of_the_operator(core: &Core) -> (Actor, Actor) {
    let (alice, bob) = (
        staff("operator-desk-a", "alice"),
        staff("operator-desk-b", "bob"),
    );
    for desk in ["operator-desk-a", "operator-desk-b"] {
        core.registers().list(&Actor {
            party: id("operator"),
            acts: id(desk),
            role: Role::Operator,
            kind: ActorKind::Component,
        });
    }
    core.registers().list(&alice);
    core.registers().list(&bob);
    // Each holds both permissions, so only the two-actor rule can keep the one that decided from moving.
    for person in [&alice, &bob] {
        core.registers().permit(person, Permission::DecideStuck);
        core.registers().permit(person, Permission::MoveStuck);
    }
    (alice, bob)
}

/// The commitment of a provider's result Core kept about `order`: what a decision names as its evidence, a party's
/// word (`T-020-40`).
fn kept_about(core: &Core, order: &Id) -> rutent_contracts::module_interface::Digest {
    core.log()
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order) =>
            {
                Some(s.commitment.clone())
            }
            _ => None,
        })
        .expect("Core kept a statement about the order")
}

// verifies: AC-6
#[test]
fn failed_segment_with_no_route_left_is_resolved_under_policy_by_two_actors() {
    let (mut core, order, exchange) = stuck_with_no_route("AC-6");
    // Stuck, with nothing rolled back: the withdrawal is settled and the value rests at the reserve's ledger.
    assert_eq!(state(&mut core, &order), OrderState::Stuck);
    assert!(core.is_stuck(&order, &core.part_of(&order).expect("a part")));
    let values = core.stuck_values();
    assert_eq!(values.len(), 1);
    let held = values[0].clone();
    assert_eq!(held.order, order);
    let (alice, bob) = staff_of_the_operator(&core);

    // The operator is told, and so is the customer.
    let told = ("no_route".to_owned(), OrderState::Stuck);
    core.notify().expect("notices");
    assert_eq!(
        changes(&core.notices_to(&core.operator_receiver(), &order)).last(),
        Some(&told)
    );
    assert_eq!(
        changes(&core.notices_to(&Core::user_receiver(), &order)).last(),
        Some(&told)
    );

    // Back to the order's origin, through the reserve that holds the value.
    let origin = Destination {
        reserve: id(IN),
        node: OuterNode {
            network: "sim-join-in-bank".to_owned(),
        },
        account: id(ORIGIN_ACCOUNT),
    };
    let resolution = Resolution::Deliver {
        by: id(IN),
        to: origin,
        asset: held.asset.clone(),
        amount: PositiveAmount::new(held.amount.as_str()).expect("an amount"),
    };
    let evidence = vec![kept_about(&core, &order)];

    // Core is its own party, not the operator's: it neither decides nor moves. Nor does the customer, nor the provider
    // holding the funds. Each is refused, and nothing is moved.
    let effects = |core: &Core| {
        core.log()
            .entries()
            .iter()
            .filter(|(_, e)| matches!(e, Entry::Verified(v) if matches!(v.what, What::Effect { action: Action::ResolveStuck { .. }, .. })))
            .count()
    };
    let holder = staff(IN, "sim-join-in-desk");
    core.registers().list(&holder);
    core.registers().permit(&holder, Permission::DecideStuck);
    for decider in [kit::core(), kit::customer(), holder.clone()] {
        let decision = core.decide_stuck_in_core(&order, &decider, &resolution, &evidence);
        assert!(
            !matches!(&decision, Ok(d) if d.permit().is_some()),
            "{decider:?}: {decision:?}"
        );
    }
    // A decision names its evidence: none is refused.
    assert!(matches!(
        core.decide_stuck_in_core(&order, &alice, &resolution, &[]),
        Err(ResolveError::NoEvidence)
    ));
    assert_eq!(effects(&core), 0);

    // One actor decides.
    let decision = core
        .decide_stuck_in_core(&order, &alice, &resolution, &evidence)
        .expect("decided");
    assert!(decision.permit().is_some(), "{decision:?}");
    let decided = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };

    // It is not moved by the one that decided, though alice holds the permission to move: Core's own rule refuses her as
    // the decider's party. Core isn't the operator's, and policy refuses it the move. Nothing moves.
    assert!(
        matches!(
            core.move_stuck_in_core(&order, &decided, &alice),
            Err(ResolveError::SameParty)
        ),
        "the decider's own party"
    );
    assert!(
        matches!(
            core.move_stuck_in_core(&order, &decided, &kit::core()),
            Ok(Moved::NotPermitted(_))
        ),
        "Core is its own party"
    );
    assert_eq!(effects(&core), 0, "no value moved");
    assert_eq!(state(&mut core, &order), OrderState::Stuck);

    // Another moves it, once, and the part ends: its value back at the origin.
    let moved = core
        .move_stuck_in_core(&order, &decided, &bob)
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
    assert_eq!(effects(&core), 1, "one move");
    assert_eq!(state(&mut core, &order), OrderState::EndedWithoutCompleting);
    assert!(core.stuck_values().is_empty());
    let _ = (exchange, held);
}
