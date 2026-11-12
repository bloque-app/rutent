//! A failed final deposit and a route handed on, on the running platform with simulated providers (`T-020-13`):
//! the deposit refused, recalculated to the same destination first, then, with no route to it, the order waiting for
//! the customer, whose choice resolves it; the value handed on to a linked order that starts where it sits, its
//! start proven by the holding account's own record, the original customer kept as requester. Over `T-020-08`'s kit,
//! with policy's engine deciding each authorisation and the log replayed as the log itself would admit it. The
//! customer's new destination is held by Core's rules in `components/core-orch/tests/hand_on.rs`: the simulated
//! providers route to no other destination.

mod segments_support;

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{Authorisation, Destination, What};
use rutent_contracts::provider_role::OuterNode;
use rutent_contracts::state_machine::{Observed, OrderState, SegmentState};
use rutent_core_orch::hand_on::{
    self, Basis, HandOn, HandOnError, HandedOn, Handing, Refused, Unproven,
};
use rutent_core_orch::recalc::{NotRecalculable, Recalculation};
use rutent_core_orch::segments::{Accounted, NoProxy, Ran, SegmentOutcome, SegmentsError};
use rutent_core_orch::stuck::{NoRouteLeft, Stuck};
use rutent_prov::holding::wallet::WithdrawalRecord;
use rutent_stm::HandOnRefusal;
use segments_support::handing::{
    from_the_ledger, hand_parts, linked_plan, linked_record, linked_route, recorded_with_receipts,
};
use segments_support::rerouting::Rerouting;
use segments_support::resolving::{PermitsEveryStep, on_parts, recorded, staff};
use segments_support::*;

fn segments(ran: &Ran) -> &[Accounted] {
    match ran {
        Ran::Segments(done) => done,
        Ran::Refused { refused, .. } => panic!("the part was refused: {refused:?}"),
    }
}

fn count(core: &Core, pick: impl Fn(&What) -> bool) -> usize {
    core.log.borrow().count(pick)
}

fn is_reversal(what: &What) -> bool {
    matches!(what, What::Input { input } if matches!(input.observed, Observed::ReversalCompleted { .. }))
}

fn is_authorise(what: &What) -> bool {
    matches!(what, What::Authorise { .. })
}

fn entries(core: &Core) -> usize {
    core.log.borrow().entries.len()
}

fn part_of(core: &Core, order: &str) -> rutent_contracts::state_machine::Part {
    core.segments.order(&id(order)).unwrap().parts[0].clone()
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

/// The kit's order with its withdrawal settled and its final deposit refused by the reserve: the value rests at the
/// reserve's ledger in full.
fn deposit_refused() -> (World, Core) {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert!(
        matches!(done[1].outcome, SegmentOutcome::Refused(_)),
        "{done:?}"
    );
    (world, core)
}

/// [`deposit_refused`], then recalculated with the reserve refusing the deposit's dry run too, so no route to the
/// destination is left: `T-020-11` records `no_route`, and the state machine has the part wait for the customer at
/// the ledger (`REQ-ORD-43`).
fn waiting_for_the_customer() -> (World, Core) {
    let (mut world, mut core) = deposit_refused();
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
    let (order, part, last) = (id(ORDER), id(PART), core.record.record_id().clone());
    let actor = segments_support::core();
    let recorded = on_parts(&mut core, |parts| {
        stuck.no_route(
            parts,
            NoRouteLeft {
                order: &order,
                part: &part,
                outcome: &outcome,
                last_record: &last,
                core: &actor,
            },
        )
    })
    .expect("no route recorded");
    assert_eq!(recorded.state, OrderState::WaitingForCustomer);
    (world, core)
}

/// The customer's new authorisation for the kit's order's part: to the other reserve's bank.
fn new_destination(given_by: rutent_contracts::core_internal::Actor) -> Authorisation {
    let mut new = authorisation_of(&Named::first(), None);
    new.authorisation = id("auth-ord-1-new-destination");
    new.destination = Destination {
        reserve: id(OTHER_RESERVE),
        node: OuterNode {
            network: format!("{OTHER_RESERVE}-bank"),
        },
        account: id("acct-9"),
    };
    new.parts = Some(vec![id(PART)]);
    new.given_by = given_by;
    new
}

/// Hands the kit's order's part on to `linked`, whose part is `plan`, on the customer's acceptance `acceptance`.
fn hand_on_to(
    core: &mut Core,
    hand_on: &mut HandOn,
    linked: &str,
    plan: &[rutent_contracts::state_machine::PartPlan],
    acceptance: &str,
) -> Result<HandedOn, HandOnError> {
    let recorded = recorded_with_receipts(core);
    let (order, part, linked, record, acceptance) = (
        id(ORDER),
        id(PART),
        id(linked),
        linked_record(linked),
        id(acceptance),
    );
    let (customer, actor) = (customer(), segments_support::core());
    hand_parts(core, |parts| {
        hand_on.hand_on(
            parts,
            Handing {
                order: &order,
                part: &part,
                linked: &linked,
                decision_record: &record,
                plan,
                acceptance: &acceptance,
                basis: Basis::Customer,
                recorded: &recorded,
                by: &customer,
                core: &actor,
            },
        )
    })
}

/// The holding account's record of its withdrawal for the kit's linked order: the reserve's, at its ledger, of the
/// kit's order's whole value. The kit's ledger is a private node, so the record is the reserve's word as its
/// publisher (`REQ-ORD-40`).
fn withdrawal_record() -> WithdrawalRecord {
    WithdrawalRecord {
        by: RESERVE.into(),
        node: format!("{RESERVE}-ledger"),
        asset: "COP".into(),
        order: ORDER.into(),
        amount: AMOUNT.parse().unwrap(),
        reference: format!("{RESERVE}-withdrawal-tx-handed-on"),
    }
}

/// The final deposit is refused at the reserve. Nothing is rolled back: no reversal is asked, the withdrawal stays
/// settled, and the value rests at the ledger where the deposit left it. Nothing the customer may choose applies
/// yet: the part doesn't wait for it, so neither new terms nor a hand-on is taken, and nothing is asked or recorded.
/// Recalculation to the same destination comes first: the next deposit goes to the authorisation's own destination,
/// in its to asset, and settles.
// verifies: REQ-ORD-43
#[test]
fn failed_final_deposit_is_recalculated_to_the_same_destination_first() {
    let (mut world, mut core) = deposit_refused();
    let part = part_of(&core, ORDER);
    assert_eq!(part.state, OrderState::FirstSegmentSettled, "not rolled back");
    assert_eq!(part.segments[0].state, SegmentState::Settled);
    assert!(hand_on::after_failed_final_deposit(&part));
    assert_eq!(count(&core, is_reversal), 0, "no reversal is asked");

    // The customer's choice isn't open yet: the part doesn't wait for it.
    let (before, authorised) = (entries(&core), count(&core, is_authorise));
    let current = authorisation_of(&Named::first(), None);
    assert_eq!(
        hand_on::authorise(
            &core.segments,
            &mut core.policy,
            &current,
            &new_destination(customer()),
            &customer().party,
        ),
        Err(HandOnError::Refused(Refused::CoversAPartThatGoesOn(id(PART))))
    );
    seed_linked(
        &mut core,
        &Named::another(2),
        &customer().party,
        &linked_record("ord-2"),
    );
    let seeded = entries(&core);
    let mut hand_on = HandOn::new();
    let plan_p1_s1_of_2 = from_the_ledger(&core, "p1-s1-of-2");
    assert_eq!(
        hand_on_to(
            &mut core,
            &mut hand_on,
            "ord-2",
            &plan_p1_s1_of_2,
            "acc-ord-2"
        ),
        Err(HandOnError::Refused(Refused::NotWaiting(
            OrderState::FirstSegmentSettled
        )))
    );
    assert_eq!(count(&core, is_authorise), authorised + 1, "policy asked only for the linked order's");
    assert_eq!(entries(&core), seeded, "nothing recorded");
    assert!(seeded > before);

    // Recalculation to the same destination, first.
    let mut rerouting = Rerouting::of(&core);
    let Recalculation::Recalculated(recalculated) =
        rerouting.recalculate(&mut core, &mut world).unwrap()
    else {
        panic!("recalculated");
    };
    assert_eq!(recalculated.route.destination, current.destination);
    assert_eq!(recalculated.route.to_asset, current.to_asset);
    let part = part_of(&core, ORDER);
    assert_eq!(part.destination.provider.as_str(), RESERVE);
    let ran = Rerouting::run(
        &mut core,
        &world.catalogue,
        &mut world.fleet.platform,
        &recalculated,
    )
    .expect("the recalculated part runs");
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Settled { .. }),
        "{ran:?}"
    );
    assert_eq!(part_of(&core, ORDER).state, OrderState::Complete);
    assert_eq!(count(&core, is_reversal), 0, "nothing rolled back");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// With no route left to the destination, the order waits for the customer where the value sits, visibly, with
/// its age; nothing is rolled back and nothing more is recalculated. Only now does the customer's choice open: Core
/// asks policy for the customer's new terms for the waiting part, and never for anyone else's.
// verifies: REQ-ORD-43
#[test]
fn with_no_route_to_it_the_order_waits_for_the_customer() {
    let (mut world, mut core) = waiting_for_the_customer();
    let part = part_of(&core, ORDER);
    assert_eq!(part.state, OrderState::WaitingForCustomer);
    assert!(part.waiting_since.is_some(), "visible with its age");
    assert_eq!(part.segments[0].state, SegmentState::Settled, "not rolled back");
    assert_eq!(
        part.destination.provider.as_str(),
        RESERVE,
        "the destination is unchanged"
    );
    assert_eq!(count(&core, is_reversal), 0);
    let mut rerouting = Rerouting::of(&core);
    assert_eq!(
        rerouting.recalculate(&mut core, &mut world).unwrap(),
        Recalculation::NotNow(NotRecalculable::Held(OrderState::WaitingForCustomer))
    );

    // Anyone else's new destination is never asked of policy.
    let current = authorisation_of(&Named::first(), None);
    let authorised = count(&core, is_authorise);
    assert_eq!(
        hand_on::authorise(
            &core.segments,
            &mut core.policy,
            &current,
            &new_destination(operator()),
            &customer().party,
        ),
        Err(HandOnError::Refused(Refused::NotTheCustomer))
    );
    let mut whole_order = new_destination(customer());
    whole_order.parts = Some(vec![id(PART), id("p9")]);
    assert_eq!(
        hand_on::authorise(
            &core.segments,
            &mut core.policy,
            &current,
            &whole_order,
            &customer().party,
        ),
        Err(HandOnError::Refused(Refused::CoversAPartThatGoesOn(id("p9"))))
    );
    assert_eq!(count(&core, is_authorise), authorised, "policy not asked");

    // The customer's own, for the waiting part, is.
    let decision = hand_on::authorise(
        &core.segments,
        &mut core.policy,
        &current,
        &new_destination(customer()),
        &customer().party,
    )
    .expect("asked");
    assert!(decision.permit().is_some(), "{decision:?}");
    assert_eq!(count(&core, is_authorise), authorised + 1);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A route handed on must start where the first order's value sits. A linked route from the other reserve's ledger,
/// or one starting with a withdrawal, is refused before anything is recorded, the first order still waiting and the
/// linked order not opened. From the ledger where the value rests, it is taken.
// verifies: REQ-ORD-43, INV-4
#[test]
fn handed_on_route_starting_away_from_the_value_is_refused() {
    let (world, mut core) = waiting_for_the_customer();
    seed_linked(
        &mut core,
        &Named::another(2),
        &customer().party,
        &linked_record("ord-2"),
    );
    let before = entries(&core);
    let mut hand_on = HandOn::new();
    let away = linked_plan(&core, ledger(OTHER_RESERVE), "p1-s1-of-2");
    assert_eq!(
        hand_on_to(&mut core, &mut hand_on, "ord-2", &away, "acc-ord-2"),
        Err(HandOnError::StartRule(HandOnRefusal::StartsAwayFromTheValue))
    );
    let withdrawing = vec![plan_of(&core.record, &Named::another(2))];
    assert_eq!(
        hand_on_to(&mut core, &mut hand_on, "ord-2", &withdrawing, "acc-ord-2"),
        Err(HandOnError::NotAHandedOnRoute)
    );
    assert_eq!(entries(&core), before, "nothing recorded");
    assert_eq!(part_of(&core, ORDER).state, OrderState::WaitingForCustomer);
    assert!(core.segments.order(&id("ord-2")).is_none(), "not opened");

    let plan_p1_s1_of_2 = from_the_ledger(&core, "p1-s1-of-2");
    let handed = hand_on_to(
        &mut core,
        &mut hand_on,
        "ord-2",
        &plan_p1_s1_of_2,
        "acc-ord-2",
    )
    .expect("handed on from where the value sits");
    assert_eq!(handed.at, serde_json::from_value(ledger(RESERVE)).unwrap());
    assert_eq!(part_of(&core, ORDER).state, OrderState::HandedOn);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// No segment of the linked order is handed over before the holding account's own record proves the value left
/// it for that order. A record by another provider, of another node, asset or order, or of less than the part held,
/// proves nothing; the reserve's own record of the whole value does.
// verifies: REQ-ORD-40, INV-5
#[test]
fn handed_on_routes_withdrawal_is_proven_by_the_holding_accounts_record() {
    let (mut world, mut core) = waiting_for_the_customer();
    seed_linked(
        &mut core,
        &Named::another(2),
        &customer().party,
        &linked_record("ord-2"),
    );
    let mut hand_on = HandOn::new();
    let plan_p1_s1_of_2 = from_the_ledger(&core, "p1-s1-of-2");
    hand_on_to(
        &mut core,
        &mut hand_on,
        "ord-2",
        &plan_p1_s1_of_2,
        "acc-ord-2",
    )
    .expect("handed on");
    let record = linked_route(&mut world, &core.clock, "ord-2");
    let handled = world.fleet.platform.operations_received(RESERVE);
    assert_eq!(
        run_linked(&mut core, &mut world, &record).map(|_| ()),
        Err(SegmentsError::StartUnproven(id("ord-2")))
    );
    assert_eq!(
        world.fleet.platform.operations_received(RESERVE),
        handled,
        "nothing handed over"
    );

    let (linked, reserve) = (id("ord-2"), id(RESERVE));
    let wrong: [(fn(&mut WithdrawalRecord), Unproven); 5] = [
        (|r| r.by = OTHER_RESERVE.into(), Unproven::NotTheStartingReserves),
        (|r| r.node = format!("{OTHER_RESERVE}-ledger"), Unproven::AnotherNode),
        (|r| r.asset = "USD".into(), Unproven::AnotherAsset),
        (|r| r.order = "ord-7".into(), Unproven::AnotherOrdersValue),
        (|r| r.amount -= 1, Unproven::AnotherAmount),
    ];
    for (change, why) in wrong {
        let mut proof = withdrawal_record();
        change(&mut proof);
        assert_eq!(
            hand_on.prove_start(&mut core.segments, &linked, &reserve, &proof),
            Err(HandOnError::Unproven(why))
        );
        assert_eq!(
            run_linked(&mut core, &mut world, &record).map(|_| ()),
            Err(SegmentsError::StartUnproven(id("ord-2")))
        );
    }
    hand_on
        .prove_start(&mut core.segments, &linked, &reserve, &withdrawal_record())
        .expect("the holding account's own record proves the start");
    // Proven, the linked order's route runs from the ledger: its deposit settles at the destination, under its own
    // authorisation, and the log admits it.
    let ran = run_linked(&mut core, &mut world, &record).expect("the linked route runs");
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Settled { .. }),
        "{ran:?}"
    );
    assert_eq!(
        world.fleet.platform.operations_received(RESERVE),
        handled + 1,
        "the deposit alone: no withdrawal from an outer node"
    );
    assert_eq!(part_of(&core, "ord-2").state, OrderState::Complete);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Runs the kit's linked order, `ord-2`, over `record`, its route handed on.
fn run_linked(
    core: &mut Core,
    world: &mut World,
    record: &rutent_contracts::router_api::DecisionRecord,
) -> Result<Ran, SegmentsError> {
    let session = id(&world.context().session);
    let named = Named::another(2);
    let (order, part, auth, origin, end) = (
        id(&named.order),
        id(PART),
        id(&named.authorisation()),
        id(RESERVE),
        route_end(),
    );
    let segments = [id("p1-s1-of-2")];
    let (caller, core_actor, operator) = (
        segments_support::core(),
        segments_support::core(),
        core.operator.clone(),
    );
    let live = rutent_core_orch::segments::Live {
        order: &order,
        part: &part,
        segments: &segments,
        record,
        at: 0,
        authorisation: &auth,
        origin_reserve: &origin,
        route: &end,
        caller: &caller,
        core: &core_actor,
        operator: &operator,
        session: Some(&session),
        inputs: &core.inputs,
        floor: None,
    };
    let mut general = General(std::rc::Rc::clone(&core.log));
    let mut parts = rutent_core_orch::segments::Parts {
        clock: &*core.clock,
        catalogue: &world.catalogue,
        log: &mut general,
        evidence: &mut core.store,
        decisions: &mut core.policy,
        modules: &mut world.fleet.platform,
        uses: &mut NoProxy,
        grading: &mut Recorded,
        operator_key: &mut core.key,
        fresh: &mut core.ids,
        issued: &core.issued,
        exposures: &mut core.exposures,
    };
    core.segments.run_part(&mut parts, &live)
}

/// The linked order's acceptance must keep the original customer as requester: one naming anyone else is refused
/// before anything is recorded. On the customer's own, the first order's `handed_on` names the linked order and its
/// acceptance, and the linked order's `route_chosen` names the first order back; the first order's part is handed
/// on and takes no more, and the log admits both records.
// verifies: REQ-ORD-13
#[test]
fn handed_on_route_keeps_the_original_customer_as_requester() {
    let (mut world, mut core) = waiting_for_the_customer();
    seed_linked(
        &mut core,
        &Named::another(3),
        &id("someone-else"),
        &linked_record("ord-3"),
    );
    seed_linked(
        &mut core,
        &Named::another(2),
        &customer().party,
        &linked_record("ord-2"),
    );
    let before = entries(&core);
    let mut hand_on = HandOn::new();
    let plan_p1_s1_of_3 = from_the_ledger(&core, "p1-s1-of-3");
    assert_eq!(
        hand_on_to(
            &mut core,
            &mut hand_on,
            "ord-3",
            &plan_p1_s1_of_3,
            "acc-ord-3"
        ),
        Err(HandOnError::NotTheRequester)
    );
    let plan_p1_s1_of_2 = from_the_ledger(&core, "p1-s1-of-2");
    assert_eq!(
        hand_on_to(
            &mut core,
            &mut hand_on,
            "ord-2",
            &plan_p1_s1_of_2,
            "acc-ord-3"
        ),
        Err(HandOnError::NotItsAcceptance)
    );
    assert_eq!(entries(&core), before, "nothing recorded");

    let plan_p1_s1_of_2 = from_the_ledger(&core, "p1-s1-of-2");
    let handed = hand_on_to(
        &mut core,
        &mut hand_on,
        "ord-2",
        &plan_p1_s1_of_2,
        "acc-ord-2",
    )
    .expect("handed on");
    assert_eq!(
        handed.handed_on.observed,
        Observed::HandedOn {
            linked_order: rutent_contracts::state_machine::Id::new("ord-2").unwrap(),
            acceptance: rutent_contracts::state_machine::Id::new("acc-ord-2").unwrap(),
        }
    );
    let Observed::RouteChosen { handed_on_from, .. } = &handed.route_chosen.observed else {
        panic!("a route chosen");
    };
    assert_eq!(handed_on_from.as_ref().map(|f| f.as_str()), Some(ORDER));
    assert!(
        u64::from(handed.handed_on_receipt.index) < u64::from(handed.route_chosen_receipt.index),
        "the first order's record first: the linked order's points back to it"
    );
    let first = part_of(&core, ORDER);
    assert_eq!(first.state, OrderState::HandedOn);
    assert_eq!(first.linked_order.as_ref().map(|l| l.as_str()), Some("ord-2"));
    let linked = core.segments.order(&id("ord-2")).expect("the linked order is followed");
    assert_eq!(linked.handed_on_from.as_ref().map(|f| f.as_str()), Some(ORDER));
    // The requester the log holds for both orders is the original customer.
    let entries_now = recorded(&core);
    assert_eq!(
        hand_on::requester_of(&entries_now, &id("ord-2")),
        hand_on::requester_of(&entries_now, &id(ORDER))
    );
    // The first order's part takes no more.
    let handled = world.fleet.platform.operations_received(RESERVE);
    let _ = try_run(&mut core, &mut world);
    assert_eq!(world.fleet.platform.operations_received(RESERVE), handled);
    // Asked again, nothing more is recorded.
    let again = entries(&core);
    let plan_p1_s1_of_2 = from_the_ledger(&core, "p1-s1-of-2");
    assert!(matches!(
        hand_on_to(
            &mut core,
            &mut hand_on,
            "ord-2",
            &plan_p1_s1_of_2,
            "acc-ord-2"
        ),
        Err(HandOnError::Refused(Refused::NotWaiting(OrderState::HandedOn)))
    ));
    assert_eq!(entries(&core), again);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The customer's new route is held to the new authorisation, never to the router's record, and to the state
/// machine's rules, before anything is recorded: a route the router chose from the ledger back to the kit's own
/// destination doesn't reach the new one the customer authorised, and under the terms in force it keeps the
/// destination, which is a recalculation and not the customer's choice. The simulated providers route to no other
/// destination, so a change taken is held by Core's rules in `components/core-orch/tests/hand_on.rs`.
// verifies: REQ-ORD-43, INV-10
#[test]
fn customers_new_route_is_held_to_its_new_authorisation() {
    let (mut world, mut core) = waiting_for_the_customer();
    let record = linked_route(&mut world, &core.clock, "ord-1");
    let before = entries(&core);
    let entries_now = recorded(&core);
    let mut hand_on = HandOn::new();
    let (order, part, acceptance) = (id(ORDER), id(PART), id("acc-ord-1"));
    let (customer, actor) = (customer(), segments_support::core());
    let mut change = |core: &mut Core, authorisation: &Authorisation| {
        hand_parts(core, |parts| {
            hand_on.change_destination(
                parts,
                hand_on::Change {
                    order: &order,
                    part: &part,
                    record: &record,
                    acceptance: &acceptance,
                    authorisation,
                    recorded: &entries_now,
                    customer: &customer,
                    core: &actor,
                },
            )
        })
    };
    assert_eq!(
        change(&mut core, &new_destination(segments_support::customer())),
        Err(HandOnError::Rejected(
            rutent_core_orch::recalc::Rejected::ChangesDestination
        ))
    );
    assert_eq!(
        change(&mut core, &authorisation_of(&Named::first(), None)),
        Err(HandOnError::Refused(Refused::KeepsDestination))
    );
    assert_eq!(entries(&core), before, "nothing recorded");
    assert_eq!(part_of(&core, ORDER).state, OrderState::WaitingForCustomer);
}

/// The operator hands a stuck order's value on only as a stuck resolution under `REQ-AUZ-4`: decided by one party,
/// moved by another, its `resolve_stuck` effect's result done. Until that is in the log, Core records no hand-on,
/// and the customer's path doesn't reach a stuck part; after it, the part's `handed_on` and the linked order's
/// route are recorded, and the log admits them on the done resolution.
// verifies: REQ-ORD-43, REQ-AUZ-4
#[test]
fn operators_hand_on_follows_its_done_resolution() {
    let mut world = World::of(&[RESERVE, OTHER_RESERVE]);
    let mut core = Core::over(&mut world);
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_begin = Some(1);
    let ran = core.run_over(&world.catalogue, &mut watched);
    drop(watched);
    assert!(
        matches!(segments(&ran)[1].outcome, SegmentOutcome::NotHandedOver(_)),
        "{ran:?}"
    );
    world
        .fleet
        .world()
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let outcome = Rerouting::of(&core)
        .recalculate(&mut core, &mut world)
        .unwrap();
    let mut stuck = Stuck::new();
    let (order, part, last) = (id(ORDER), id(PART), core.record.record_id().clone());
    let actor = segments_support::core();
    let declared = on_parts(&mut core, |parts| {
        stuck.no_route(
            parts,
            NoRouteLeft {
                order: &order,
                part: &part,
                outcome: &outcome,
                last_record: &last,
                core: &actor,
            },
        )
    })
    .expect("no route recorded");
    assert_eq!(declared.state, OrderState::Stuck);
    segments_support::resolving::follow(&core, &mut stuck).expect("its ends are Core's");
    seed_linked(
        &mut core,
        &Named::another(2),
        &customer().party,
        &linked_record("ord-2"),
    );
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));

    let mut hand_on = HandOn::new();
    let plan = from_the_ledger(&core, "p1-s1-of-2");
    let before = entries(&core);
    assert_eq!(
        hand_on_to(&mut core, &mut hand_on, "ord-2", &plan, "acc-ord-2"),
        Err(HandOnError::Refused(Refused::NotWaiting(OrderState::Stuck))),
        "the customer's path doesn't reach a stuck part"
    );
    let unresolved = core.log.borrow().receipt_at(before - 1);
    assert_eq!(
        operator_hand_on(&mut core, &mut hand_on, &plan, &unresolved, &alice),
        Err(HandOnError::NoDoneResolution)
    );
    assert_eq!(entries(&core), before, "nothing recorded");

    let mut policy = PermitsEveryStep::new(&core.log);
    let resolution = rutent_contracts::core_internal::Resolution::HandOn {
        by: id(RESERVE),
        linked_order: id("ord-2"),
        asset: rutent_contracts::provider_role::Denomination {
            id: "COP".into(),
            decimals: 2,
        },
        amount: rutent_contracts::router_api::PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = vec![digest(&"ab".repeat(32))];
    let decision = on_parts(&mut core, |parts| {
        stuck.decide(
            parts,
            &mut policy,
            rutent_core_orch::stuck::Deciding {
                order: &order,
                part: &part,
                decider: &alice,
                resolution: &resolution,
                evidence: &evidence,
            },
        )
    })
    .expect("decided");
    let carries_out = rutent_contracts::core_internal::Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    let mut moves = segments_support::resolving::Moves::default();
    let moved = on_parts(&mut core, |parts| {
        stuck.carry_out(
            parts,
            &mut policy,
            &mut moves,
            rutent_core_orch::stuck::Moving {
                order: &order,
                carries_out: &carries_out,
                mover: &bob,
                core: &actor,
            },
        )
    })
    .expect("moved");
    let rutent_core_orch::stuck::Moved::CarriedOut {
        effect,
        resolved: rutent_core_orch::stuck::Resolved::Done,
        ..
    } = moved
    else {
        panic!("carried out: {moved:?}");
    };
    let handed = operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob)
        .expect("handed on after its done resolution");
    assert_eq!(handed.handed_on.source.kind, rutent_contracts::state_machine::SourceKind::Operator);
    assert_eq!(part_of(&core, ORDER).state, OrderState::HandedOn);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Hands the kit's stuck order's part on to `ord-2` on the operator's resolution `effect`.
fn operator_hand_on(
    core: &mut Core,
    hand_on: &mut HandOn,
    plan: &[rutent_contracts::state_machine::PartPlan],
    effect: &rutent_contracts::core_internal::Receipt,
    by: &rutent_contracts::core_internal::Actor,
) -> Result<HandedOn, HandOnError> {
    let recorded = recorded_with_receipts(core);
    let (order, part, linked, record, acceptance) = (
        id(ORDER),
        id(PART),
        id("ord-2"),
        linked_record("ord-2"),
        id("acc-ord-2"),
    );
    let actor = segments_support::core();
    hand_parts(core, |parts| {
        hand_on.hand_on(
            parts,
            Handing {
                order: &order,
                part: &part,
                linked: &linked,
                decision_record: &record,
                plan,
                acceptance: &acceptance,
                basis: Basis::Resolution { effect },
                recorded: &recorded,
                by,
                core: &actor,
            },
        )
    })
}

/// A part waiting at its floor (`T-020-10b`) is resumed by the customer's own choice: new terms with a lower floor,
/// asked of policy only because the part waits, then a route from where the value sits to the same destination,
/// held to those terms and recorded as a `route_recalculated`. The part waits no more, its route runs under the new
/// authorisation, and the log admits it.
// verifies: REQ-ORD-27, REQ-AUZ-3
#[test]
fn customers_new_floor_resumes_a_part_waiting_at_it() {
    // The withdrawal delivers 1 % short, 990 000, below the floor of 995 000 the deposit's quote would leave.
    let floor = rutent_contracts::router_api::PositiveAmount::new("995000").unwrap();
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::under(&mut world, Some(floor.clone()));
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::delivering(rutent_conformance::sim::Delivery::Short { bps: 100 }),
    );
    let _ = core.run(&mut world);
    let part = part_of(&core, ORDER);
    assert_eq!(part.state, OrderState::WaitingForCustomer, "waits at its floor");

    let current = authorisation_of(&Named::first(), Some(floor));
    let mut lower = current.clone();
    lower.authorisation = id("auth-ord-1-lower-floor");
    lower.floor = rutent_contracts::router_api::PositiveAmount::new("900000");
    lower.parts = Some(vec![id(PART)]);
    lower.given_by = customer();
    let mut elsewhere = new_destination(customer());
    elsewhere.floor = current.floor.clone();
    assert_eq!(
        hand_on::authorise(
            &core.segments,
            &mut core.policy,
            &current,
            &elsewhere,
            &customer().party
        ),
        Err(HandOnError::Refused(Refused::NoFailedDeposit)),
        "no new destination from a floor wait"
    );
    let decision = hand_on::authorise(
        &core.segments,
        &mut core.policy,
        &current,
        &lower,
        &customer().party,
    )
    .expect("asked");
    assert!(decision.permit().is_some(), "{decision:?}");

    let record_id = id("dr-resume-ord-1");
    let record = segments_support::handing::route_from_the_ledger(
        &mut world,
        &core.clock,
        &record_id,
        Some(core.record.request.request_id.as_str()),
        "990000",
    );
    segments_support::handing::seed_record(&core, ORDER, &record_id);
    let mut hand_on = HandOn::new();
    let (order, part_id, actor) = (id(ORDER), id(PART), segments_support::core());
    let mut resume = |core: &mut Core, acceptance: &str| {
        let (recorded, acceptance) = (recorded(core), id(acceptance));
        hand_parts(core, |parts| {
            hand_on.resume(
                parts,
                hand_on::Resume {
                    order: &order,
                    part: &part_id,
                    record: &record,
                    authorisation: &lower,
                    acceptance: &acceptance,
                    recorded: &recorded,
                    core: &actor,
                },
            )
        })
    };
    // Not on the acceptance of the terms the part waits under: the customer accepts the new ones.
    let before = entries(&core);
    assert_eq!(
        resume(&mut core, "acc-ord-1"),
        Err(HandOnError::NotItsAcceptance)
    );
    assert_eq!(entries(&core), before, "nothing recorded");
    segments_support::handing::seed_acceptance(
        &core,
        ORDER,
        "acc-ord-1-lower-floor",
        &lower.authorisation,
    );
    let resumed = resume(&mut core, "acc-ord-1-lower-floor").expect("resumed");
    assert!(matches!(resumed.input.observed, Observed::RouteRecalculated { .. }));
    let part = part_of(&core, ORDER);
    assert_eq!(part.state, OrderState::FirstSegmentSettled, "it waits no more");
    assert!(part.waiting_since.is_none());

    let session = id(&world.context().session);
    let (auth, origin) = (id("auth-ord-1-lower-floor"), id(RESERVE));
    let (caller, core_actor, operator) = (
        segments_support::core(),
        segments_support::core(),
        core.operator.clone(),
    );
    let new_floor = lower.floor.clone();
    let live = rutent_core_orch::segments::Live {
        order: &order,
        part: &part_id,
        segments: &resumed.segments,
        record: &record,
        at: 0,
        authorisation: &auth,
        origin_reserve: &origin,
        route: &resumed.route,
        caller: &caller,
        core: &core_actor,
        operator: &operator,
        session: Some(&session),
        inputs: &core.inputs,
        floor: new_floor.as_ref(),
    };
    let mut general = General(std::rc::Rc::clone(&core.log));
    let mut parts = rutent_core_orch::segments::Parts {
        clock: &*core.clock,
        catalogue: &world.catalogue,
        log: &mut general,
        evidence: &mut core.store,
        decisions: &mut core.policy,
        modules: &mut world.fleet.platform,
        uses: &mut NoProxy,
        grading: &mut Recorded,
        operator_key: &mut core.key,
        fresh: &mut core.ids,
        issued: &core.issued,
        exposures: &mut core.exposures,
    };
    let ran = core.segments.run_part(&mut parts, &live).expect("the resumed part runs");
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Settled { .. }),
        "{ran:?}"
    );
    assert_eq!(part_of(&core, ORDER).state, OrderState::Complete);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}
