//! The holding account's withdrawal record, written, and what the log and a restart make of it (`T-020-26`;
//! `REQ-ORD-40`, `REQ-ORD-43`, `INV-5`). A route handed on starts on the holding account's own record of its
//! withdrawal: Core appends it as the `holding_withdrawal` before the linked order's first move, which the log holds
//! to it; at a private node it is labelled its publisher's word; and a restarted Core proves the start again from the
//! record, so the linked order's segments are handed over without the holding account being asked again.
//!
//! Over `T-020-08`'s kit and the real log's rules: every history is replayed through `admit_entry`.

mod segments_support;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::sim::{Act, Behaviour};
use rutent_conformance::sim::{Role as SimRole, Spec};
use rutent_contracts::core_internal::What;
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::hand_on::{Basis, HandOn, HandOnError, HandedOn, Handing};
use rutent_core_orch::recalc::Recalculation;
use rutent_core_orch::segments::{
    Accounted, NoProxy, Ran, SegmentOutcome, Segments, SegmentsError,
};
use rutent_core_orch::stuck::{NoRouteLeft, Stuck};
use rutent_prov::holding::wallet::WithdrawalRecord;
use segments_support::handing::{
    from_the_ledger, hand_parts, holding_statement, linked_record, linked_route, prove,
    prove_labelled, recorded_with_receipts,
};
use segments_support::rerouting::Rerouting;
use segments_support::resolving::on_parts;
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

fn part_of(core: &Core, order: &str) -> rutent_contracts::state_machine::Part {
    core.segments.order(&id(order)).unwrap().parts[0].clone()
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
    let (actor, by) = (segments_support::core(), customer());
    hand_parts(core, &mut Stuck::new(), |parts| {
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
                by: &by,
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
        record_entry: None,
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
        channel: &mut rutent_core_orch::notice::Unbuilt,
    };
    core.segments.run_part(&mut parts, &live)
}

// The tests ---------------------------------------------------------------------------------------------------

/// The kit's order handed on to `ord-2`, not yet proven: the log holds the `route_chosen` of the linked order and
/// the route handed on is refused its segments until the holding account's record is recorded.
fn handed_on() -> (
    World,
    Core,
    HandOn,
    rutent_contracts::router_api::DecisionRecord,
) {
    let (mut world, mut core) = waiting_for_the_customer();
    seed_linked(
        &mut core,
        &Named::another(2),
        &customer(),
        &linked_record("ord-2"),
    );
    let mut hand_on = HandOn::new();
    let plan = from_the_ledger(&core, "p1-s1-of-2");
    hand_on_to(&mut core, &mut hand_on, "ord-2", &plan, "acc-ord-2").expect("handed on");
    let record = linked_route(&mut world, &core.clock, "ord-2");
    (world, core, hand_on, record)
}

fn is_holding_withdrawal(what: &What) -> bool {
    matches!(what, What::HoldingWithdrawal { order, .. } if order.as_str() == "ord-2")
}

/// The place of the first entry that is `pick`.
fn place(core: &Core, pick: impl Fn(&What) -> bool) -> usize {
    core.log
        .borrow()
        .records()
        .into_iter()
        .find(|(_, w)| pick(w))
        .map(|(i, _)| i)
        .expect("the entry is in the log")
}

fn is_linked_move(what: &What) -> bool {
    matches!(what, What::Effect { action: rutent_contracts::core_internal::Action::MoveValue { order, .. }, .. }
        if order.as_str() == "ord-2")
}

/// The linked order's first move, which runs only once the holding account's record is recorded (the segments refuse
/// it before), rests on that record: the log holds the `holding_withdrawal` before the move's effect, and admits it,
/// by the holding account of the reserve the route starts at.
// verifies: REQ-ORD-40
#[test]
fn handed_on_routes_first_move_rests_on_the_holding_accounts_recorded_withdrawal() {
    let (mut world, mut core, mut hand_on, record) = handed_on();
    let handled = world.fleet.platform.operations_received(RESERVE);
    assert_eq!(
        run_linked(&mut core, &mut world, &record).map(|_| ()),
        Err(SegmentsError::StartUnproven(id("ord-2")))
    );
    assert_eq!(count(&core, is_holding_withdrawal), 0, "no record yet");
    prove(&mut core, &mut hand_on, &id("ord-2"), &withdrawal_record())
        .expect("the holding account's own record proves the start");
    assert_eq!(count(&core, is_holding_withdrawal), 1);
    let ran = run_linked(&mut core, &mut world, &record).expect("the linked route runs");
    assert!(
        matches!(segments(&ran)[0].outcome, SegmentOutcome::Settled { .. }),
        "{ran:?}"
    );
    assert_eq!(
        world.fleet.platform.operations_received(RESERVE),
        handled + 1
    );
    assert!(
        place(&core, is_holding_withdrawal) < place(&core, is_linked_move),
        "the record is before the first move"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A route handed on whose first move has no recorded withdrawal behind it is refused by the log, whatever Core did:
/// the same history, with the `holding_withdrawal` left out, is refused at the linked order's first move, by the
/// log's row for it (`INV-5`: nothing moves from a holding account the host has no record of).
// verifies: REQ-ORD-40, INV-5
#[test]
fn first_move_of_a_route_handed_on_with_no_recorded_withdrawal_is_refused_by_the_log() {
    let (mut world, mut core, mut hand_on, record) = handed_on();
    prove(&mut core, &mut hand_on, &id("ord-2"), &withdrawal_record()).expect("proven");
    run_linked(&mut core, &mut world, &record).expect("the linked route runs");
    core.admitted(&world.catalogue)
        .expect("the log admits it with the record");
    let (held, moved) = (
        place(&core, is_holding_withdrawal),
        place(&core, is_linked_move),
    );
    let refused = core
        .admitted_skipping(&world.catalogue, Some(held))
        .expect_err("refused without the record");
    assert_eq!(
        refused.0, moved,
        "at the linked order's first move: {refused:?}"
    );
    assert_eq!(
        refused.1,
        "a route handed on starts on its holding account's record of the withdrawal in this log"
    );
}

/// At a private node the record is its publisher's word, labelled as such: the record's entry names the party the
/// record's maker states for, and the log holds the label to that party; a label naming another party is refused.
/// At a public node, the operator's own module's, it carries none.
// verifies: REQ-ORD-40
#[test]
fn record_at_a_private_node_is_labelled_its_publishers_word() {
    let publisher = Spec::of(SimRole::Reserve, RESERVE)
        .make(&SuiteContext::default())
        .advertisement()
        .into_statement()
        .statement
        .publisher;
    let publisher = id(&publisher);
    // Labelled by the publisher the catalogue names for the node.
    let (world, mut core, mut hand_on, _) = handed_on();
    let statement = holding_statement(&mut core, "ord-2");
    prove_labelled(
        &mut core,
        &mut hand_on,
        &id("ord-2"),
        &withdrawal_record(),
        &statement,
        Some(&publisher),
    )
    .expect("recorded");
    let labelled = core
        .log
        .borrow()
        .verified()
        .iter()
        .find_map(|v| match &v.what {
            What::HoldingWithdrawal { publisher, .. } => Some(publisher.clone()),
            _ => None,
        })
        .expect("the record");
    assert_eq!(labelled, Some(publisher.clone()));
    core.admitted(&world.catalogue)
        .expect("the log admits the labelled record");
    // A label naming another party than the one the record's maker states for is refused by the log.
    let (world, mut core, mut hand_on, _) = handed_on();
    let statement = holding_statement(&mut core, "ord-2");
    let other = id("another-publisher");
    prove_labelled(
        &mut core,
        &mut hand_on,
        &id("ord-2"),
        &withdrawal_record(),
        &statement,
        Some(&other),
    )
    .expect("Core records what it was told");
    let refused = core
        .admitted(&world.catalogue)
        .expect_err("another party's label");
    assert_eq!(
        refused.1,
        "a holding account's record labelled a publisher's word states for that publisher"
    );
    // At a public node there is no label.
    let (world, mut core, mut hand_on, _) = handed_on();
    prove(&mut core, &mut hand_on, &id("ord-2"), &withdrawal_record()).expect("recorded");
    let none = core.log.borrow().verified().iter().any(|v| {
        matches!(
            &v.what,
            What::HoldingWithdrawal {
                publisher: None,
                ..
            }
        )
    });
    assert!(none, "no label where the node is public");
    core.admitted(&world.catalogue).expect("admitted");
}

/// A restarted Core, whose segments forget the starts the holding accounts' records proved, proves them again from
/// the log: the linked order's segments are handed over, the holding account asked nothing and no second record
/// written. A log with no record proves nothing, and the segments refuse the route as before.
// verifies: REQ-ORD-43
#[test]
fn proven_start_is_rebuilt_from_the_log_after_a_restart() {
    let (mut world, mut core, mut hand_on, record) = handed_on();
    prove(&mut core, &mut hand_on, &id("ord-2"), &withdrawal_record()).expect("proven");
    let entries_before = core.log.borrow().entries.len();
    // A restart: Core's records of the starts are gone; the log holds them.
    core.segments = Segments::new();
    let inputs: Vec<_> = core
        .log
        .borrow()
        .inputs()
        .into_iter()
        .filter(|i| i.subject.order.as_str() == "ord-2")
        .cloned()
        .collect();
    assert!(core.segments.follow_recorded(&*core.clock, &inputs));
    let handled = world.fleet.platform.operations_received(RESERVE);
    assert_eq!(
        run_linked(&mut core, &mut world, &record).map(|_| ()),
        Err(SegmentsError::StartUnproven(id("ord-2"))),
        "refused until the start is rebuilt"
    );
    // From a log that holds no record, nothing is proven.
    let none: Vec<_> = core
        .log
        .borrow()
        .entries
        .iter()
        .map(|(e, _)| e.clone())
        .filter(|e| !matches!(e, rutent_contracts::core_internal::Entry::Verified(v) if is_holding_withdrawal(&v.what)))
        .collect();
    assert_eq!(HandOn::recover_starts(&mut core.segments, &none), 0);
    assert!(matches!(
        run_linked(&mut core, &mut world, &record),
        Err(SegmentsError::StartUnproven(_))
    ));
    let all: Vec<_> = core
        .log
        .borrow()
        .entries
        .iter()
        .map(|(e, _)| e.clone())
        .collect();
    // Only what the log's own rows and Core's own rules make a start: a record that isn't Core's, one of another
    // holder or node than the linked order's route starts at, or of a linked order never handed on, proves nothing.
    use rutent_contracts::core_internal::{Entry, Role};
    let altered = |change: &dyn Fn(&mut Entry)| {
        let mut entries = all.clone();
        for entry in &mut entries {
            if matches!(entry, Entry::Verified(v) if is_holding_withdrawal(&v.what)) {
                change(entry);
            }
        }
        entries
    };
    let by_another = altered(&|e| {
        if let Entry::Verified(v) = e {
            v.who.role = Role::Provider;
        }
    });
    let of_another_holder = altered(&|e| {
        if let Entry::Verified(v) = e
            && let What::HoldingWithdrawal { holder, .. } = &mut v.what
        {
            *holder = id(OTHER_RESERVE);
        }
    });
    let at_another_node = altered(&|e| {
        if let Entry::Verified(v) = e
            && let What::HoldingWithdrawal { node, .. } = &mut v.what
        {
            *node = id("elsewhere");
        }
    });
    let of_another_order = altered(&|e| {
        if let Entry::Verified(v) = e
            && let What::HoldingWithdrawal { handed_on_from, .. } = &mut v.what
        {
            *handed_on_from = id("ord-9");
        }
    });
    let never_handed_on: Vec<_> = all
        .iter()
        .filter(|e| {
            !matches!(e, Entry::Verified(v) if matches!(&v.what, What::Input { input }
                if matches!(input.observed, rutent_contracts::state_machine::Observed::HandedOn { .. })))
        })
        .cloned()
        .collect();
    for (why, entries) in [
        ("not Core's own entry", by_another),
        ("another holder", of_another_holder),
        ("another node", at_another_node),
        ("another order handed on from", of_another_order),
        ("no hand-on", never_handed_on),
    ] {
        assert_eq!(
            HandOn::recover_starts(&mut core.segments, &entries),
            0,
            "{why}"
        );
    }
    assert!(matches!(
        run_linked(&mut core, &mut world, &record),
        Err(SegmentsError::StartUnproven(_))
    ));
    assert_eq!(HandOn::recover_starts(&mut core.segments, &all), 1);
    let ran = run_linked(&mut core, &mut world, &record).expect("the linked route runs");
    assert!(matches!(
        segments(&ran)[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert_eq!(
        world.fleet.platform.operations_received(RESERVE),
        handled + 1,
        "the deposit alone: the holding account is asked nothing"
    );
    assert_eq!(count(&core, is_holding_withdrawal), 1, "no second record");
    assert!(core.log.borrow().entries.len() > entries_before);
    assert_eq!(part_of(&core, "ord-2").state, OrderState::Complete);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A restart between the hand-on and the holding account's proof of the start: the linked order's start as Core
/// recorded it is rebuilt from the log (the first order, the part's value at rest, the starting reserve, the route's
/// first segment), so the proof goes ahead. A second restart after the proof: the start the log took is Core's again,
/// read back from the holding account's answer the evidence store keeps, so the first thing asked of it, another
/// record of the started order, is refused before anything is recorded, and the same record is answered with its
/// receipt: Core's own rule beside the log's (`REQ-ORD-43`, `REQ-ORD-40`, `T-020-46`).
// verifies: REQ-ORD-43, REQ-ORD-40
#[test]
fn linked_start_and_its_spent_record_are_rebuilt_from_the_log_after_a_restart() {
    let (_world, mut core, _, _) = handed_on();
    let statement =
        segments_support::handing::holding_withdrawal_statement(&mut core, "ord-2", AMOUNT);
    let mut rebuilt = segments_support::reopen_hand_on(&core);
    let first = segments_support::handing::prove_citing(
        &mut core,
        &mut rebuilt,
        &id("ord-2"),
        &withdrawal_record(),
        &statement,
    )
    .expect("proven on the start rebuilt from the log");
    let mut again = segments_support::reopen_hand_on(&core);
    let entries = core.log.borrow().entries.len();
    let other = WithdrawalRecord {
        reference: format!("{RESERVE}-withdrawal-tx-another"),
        ..withdrawal_record()
    };
    let answered = segments_support::handing::prove_citing(
        &mut core,
        &mut again,
        &id("ord-2"),
        &other,
        &statement,
    );
    assert!(
        matches!(
            answered,
            Err(HandOnError::Unproven(
                rutent_core_orch::hand_on::Unproven::AlreadyStarted
            ))
        ),
        "another record of the started order is refused by Core's own rule: {answered:?}"
    );
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
    assert_eq!(
        segments_support::handing::prove_citing(
            &mut core,
            &mut again,
            &id("ord-2"),
            &withdrawal_record(),
            &statement,
        ),
        Ok(first),
        "the start the log took answers the same record"
    );
    assert_eq!(count(&core, is_holding_withdrawal), 1, "one start");
}

/// A start the log took whose holding record a reopened Core can't read back (the cited statement is no withdrawal
/// answer): failing closed, no record proves the started order again, the one it took included, and nothing more is
/// recorded (`REQ-ORD-43`, `T-020-46`).
// verifies: REQ-ORD-43
#[test]
fn linked_start_whose_record_cannot_be_read_back_is_proven_by_no_record_after_a_restart() {
    let (_world, mut core, _, _) = handed_on();
    let statement = segments_support::handing::holding_statement(&mut core, "ord-2");
    let mut rebuilt = segments_support::reopen_hand_on(&core);
    segments_support::handing::prove_citing(
        &mut core,
        &mut rebuilt,
        &id("ord-2"),
        &withdrawal_record(),
        &statement,
    )
    .expect("proven on the start rebuilt from the log");
    let entries = core.log.borrow().entries.len();
    for record in [
        WithdrawalRecord {
            reference: format!("{RESERVE}-withdrawal-tx-another"),
            ..withdrawal_record()
        },
        withdrawal_record(),
    ] {
        let mut again = segments_support::reopen_hand_on(&core);
        let answered = segments_support::handing::prove_citing(
            &mut core,
            &mut again,
            &id("ord-2"),
            &record,
            &statement,
        );
        assert!(
            matches!(
                answered,
                Err(HandOnError::Unproven(
                    rutent_core_orch::hand_on::Unproven::AlreadyStarted
                ))
            ),
            "{answered:?}"
        );
    }
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
}
