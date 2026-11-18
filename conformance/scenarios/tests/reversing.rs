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
use rutent_scenarios::assembly::StepError;
use rutent_scenarios::assembly::{Core, ORIGIN_ACCOUNT};
use rutent_scenarios::kit::{self, AMOUNT, id};

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

/// The state of `segment` as Core follows it.
fn segment_state(core: &Core, order: &Id, segment: &Id) -> SegmentState {
    core.followed(order).expect("Core follows the order").parts[0]
        .segments
        .iter()
        .find(|s| s.segment_id.as_str() == segment.as_str())
        .expect("the segment is followed")
        .state
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
        core.recalculate(&order).expect("tried"),
        Recalculation::NotNow(NotRecalculable::InFlight {
            segment: rutent_contracts::state_machine::Id::new(exchange.as_str()).expect("an id"),
            state: SegmentState::FailedReversalOwed
        })
    );
    // The reversal is owed by the exchange, in the asset it took in and in full.
    let owing = core.owe_reversal(&order, &exchange).expect("owed");
    assert_eq!(owing.provider.as_str(), EXCHANGE);
    assert_eq!(owing.asset, "COP");

    // The exchange is of the reserve's own publisher, so the reserve's word that the credit arrived is its publisher's
    // word about its own provider's work: it completes nothing.
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
    assert_eq!(
        segment_state(&core, &order, &exchange),
        SegmentState::FailedReversalOwed
    );
    assert!(core.completed_reversal(&order, &exchange).is_none());

    // The failed exchange is asked to reverse: its own word completes nothing, except inside the node its own publisher
    // holds, where it is that publisher's, said so: the reserve's at its own node.
    let asked = core.ask_reversal(&order, &exchange).expect("asked");
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
    let Recalculation::Recalculated(recalculated) = core.recalculate(&order).expect("tried") else {
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
        .run_recalculated(&order, &recalculated)
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
    let asked = core.ask_reversal(&order, &exchange).expect("asked");
    assert!(
        asked.completed.is_some(),
        "the reversal completed: {asked:?}"
    );
    core.world()
        .script_dry_run(EXCHANGE, "conversion", Behaviour::act(Act::Refuse));
    let outcome = core.recalculate(&order).expect("tried");
    assert!(
        matches!(outcome, Recalculation::NoRoute { .. }),
        "no route is left: {outcome:?}"
    );
    let recorded = core.record_no_route(&order, &outcome).expect("recorded");
    assert_eq!(recorded.state, OrderState::Stuck);
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

/// The commitment of a statement Core kept about `order`: what a decision names as its evidence.
fn kept_about(core: &Core, order: &Id) -> rutent_contracts::module_interface::Digest {
    core.log()
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s) if s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order) => {
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
        let decision = core.decide_stuck(&order, &decider, &resolution, &evidence);
        assert!(
            !matches!(&decision, Ok(d) if d.permit().is_some()),
            "{decider:?}: {decision:?}"
        );
    }
    // A decision names its evidence: none is refused.
    assert!(matches!(
        core.decide_stuck(&order, &alice, &resolution, &[]),
        Err(StepError::Resolve(ResolveError::NoEvidence))
    ));
    assert_eq!(effects(&core), 0);

    // One actor decides.
    let decision = core
        .decide_stuck(&order, &alice, &resolution, &evidence)
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
            core.move_stuck(&order, &decided, &alice),
            Err(StepError::Resolve(ResolveError::SameParty))
        ),
        "the decider's own party"
    );
    assert!(
        matches!(
            core.move_stuck(&order, &decided, &kit::core()),
            Ok(Moved::NotPermitted(_))
        ),
        "Core is its own party"
    );
    assert_eq!(effects(&core), 0, "no value moved");
    assert_eq!(state(&mut core, &order), OrderState::Stuck);

    // Another moves it, once, and the part ends: its value back at the origin.
    let moved = core.move_stuck(&order, &decided, &bob).expect("moved");
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
