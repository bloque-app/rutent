//! Execution and closing scenarios over the assembled Core against the simulated providers (`T-024-04`): a route of
//! one reserve's withdrawal and deposit (`AC-28`), a final deposit closing on its public record and the reserve's
//! statement and not on a statement alone (`AC-32`), one movement shared by two orders (`AC-31`), value arriving with
//! no order (`AC-10`) and a deposit filed by an authorised reserve (`AC-11`). Each drives Core's own `request`,
//! `quote`, `accept` and `run`, or its `arrive` and `file`, over [`execution_support`]'s world.

mod execution_support;

use execution_support::*;
use rutent_conformance::sim::Behaviour;
use rutent_contracts::core_internal::What;
use rutent_contracts::provider_role::EvidenceGrade;
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::segments::{Ran, SegmentOutcome};
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{self, AMOUNT, RESERVE};

/// The segments of the one part `ran` ran, as it accounted for each.
fn accounted(ran: &[Ran]) -> &[rutent_core_orch::segments::Accounted] {
    let [Ran::Segments(segments)] = ran else {
        panic!("one part ran its segments: {ran:?}");
    };
    segments
}

// verifies: AC-28
#[test]
fn one_reserves_withdrawal_and_deposit_make_a_route_of_two_segments() {
    let (mut core, session) = core_with("AC-28", &[RESERVE]);
    let bank = format!("{RESERVE}-bank");
    let ledger = format!("{RESERVE}-ledger");
    // Out of one account and into another, and into the same one: each is a route of two segments, the reserve's
    // withdrawal into its ledger and its deposit back out of it, over two nodes.
    for (name, account) in [("req-other", OTHER), ("req-same", SAME)] {
        let request = within(name, &session, AMOUNT, RESERVE, account);
        let (order, told) = quoted(&mut core, request);
        let record = record_of(&core, &told);
        let chosen = record.chosen().expect("a route was chosen");
        let [part] = chosen.parts.as_slice() else {
            panic!("one part: {chosen:?}");
        };
        assert_eq!(part.operations.len(), 2, "two segments, for {account}");
        let (withdrawal, deposit) = (&part.operations[0], &part.operations[1]);
        assert_eq!(
            (
                node_name(&withdrawal.from.node),
                node_name(&withdrawal.to.node)
            ),
            (bank.clone(), ledger.clone()),
            "it starts by a withdrawal from the outer node to the inner one"
        );
        assert_eq!(
            (node_name(&deposit.from.node), node_name(&deposit.to.node)),
            (ledger.clone(), bank.clone()),
            "and ends by a deposit from the inner node to the outer one"
        );
        assert!(matches!(withdrawal.from.node, NodeRef::Outer(_)));
        assert!(matches!(deposit.to.node, NodeRef::Outer(_)));

        let Told::Exists {
            status: Status::Quoted { digest, .. },
            ..
        } = told
        else {
            panic!("quoted")
        };
        core.accept(&kit::customer(), &order, &digest)
            .expect("accepted");
        let ran = core.run(&order).expect("the order ran");
        let segments = accounted(&ran);
        assert_eq!(segments.len(), 2, "two segments ran, for {account}");
        assert!(
            segments
                .iter()
                .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
            "both settled: {segments:?}"
        );
        assert_eq!(state(&mut core, &order), OrderState::Complete);

        // It closes under `REQ-ORD-33` on its public records: the deposit's, at the destination, the movement the
        // bank's network recorded, and the reserve's own statement, each said to be whose it is.
        let evidence = evidence_of(&core, &order);
        let deposit_segment = segments[1].segment.clone();
        let closing: Vec<_> = evidence
            .iter()
            .filter(|e| e.segment == deposit_segment)
            .collect();
        assert_eq!(closing.len(), 2, "the network's and the reserve's");
        assert_eq!(closing[0].whose.as_str(), bank);
        assert_eq!(closing[0].grade, EvidenceGrade::PublicRecord);
        let movement = closing[0].movement.as_ref().expect("a public movement");
        assert_eq!(movement.network.as_str(), bank);
        assert_eq!(closing[1].whose.as_str(), RESERVE);
        assert!(closing[1].movement.is_none(), "a statement is no record");
        // And the withdrawal's own movement was a public one, on the same network.
        let first = evidence
            .iter()
            .find(|e| e.segment == segments[0].segment)
            .expect("the withdrawal's evidence");
        assert_eq!(
            first.movement.as_ref().map(|m| m.network.as_str()),
            Some(bank.as_str())
        );
    }

    // A transfer inside the reserve's book, from its ledger to its ledger, has no public record, so it can't close: it
    // is no route at all (`REQ-ORD-16`), since a route starts by a withdrawal from an outer node and ends by a
    // deposit to one. Nothing is quoted, and nothing moves.
    let books = core.world().journal(RESERVE).len();
    let effects = entries(&core)
        .iter()
        .filter(|v| matches!(v.what, What::Effect { .. }))
        .count();
    let inside = request(
        "req-inside",
        &session,
        AMOUNT,
        RESERVE,
        inner(&ledger, "COP"),
        inner(&ledger, "COP"),
        RESERVE,
        OTHER,
        "lowest_total_cost",
    );
    match core.request(inside) {
        Err(_) | Ok(Told::Refused { .. }) => {}
        Ok(told) => {
            let order = Core::order_of(&told).expect("an order");
            let quoted = core.quote(&kit::customer(), &order);
            assert!(
                !matches!(
                    quoted,
                    Ok(Told::Exists {
                        status: Status::Quoted { .. },
                        ..
                    })
                ),
                "a transfer inside a book is quoted no route: {quoted:?}"
            );
        }
    }
    assert_eq!(core.world().journal(RESERVE).len(), books);
    assert_eq!(
        entries(&core)
            .iter()
            .filter(|v| matches!(v.what, What::Effect { .. }))
            .count(),
        effects
    );
}

// verifies: AC-32
#[test]
fn final_deposit_closes_on_the_public_record_and_the_reserves_statement_and_not_on_a_statement_alone()
 {
    let bank = format!("{RESERVE}-bank");

    // The reserve gives the public record of the deposit's movement and its signed statement: the order closes, and
    // the record says whose evidence each is.
    let (mut core, session) = core_with("AC-32", &[RESERVE]);
    let (order, ran) = run(
        &mut core,
        within("req-closes", &session, AMOUNT, RESERVE, OTHER),
    );
    let segments = accounted(&ran);
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    let deposit = segments[1].segment.clone();
    let closing: Vec<_> = evidence_of(&core, &order)
        .into_iter()
        .filter(|e| e.segment == deposit)
        .collect();
    assert_eq!(closing.len(), 2);
    assert_eq!(
        (closing[0].whose.as_str(), closing[0].grade),
        (bank.as_str(), EvidenceGrade::PublicRecord),
        "the network's record is the network's"
    );
    assert!(closing[0].movement.is_some());
    assert_eq!(
        closing[1].whose.as_str(),
        RESERVE,
        "the statement is the reserve's"
    );
    assert_ne!(closing[1].grade, EvidenceGrade::PublicRecord);
    assert!(closing[1].movement.is_none());

    // The reserve's signed statement alone: its deposit's movement never reaches the public network. The statement
    // is kept as the reserve's, and it closes nothing: no evidence is graded for the deposit, the segment doesn't
    // settle, and the order doesn't complete.
    let (mut alone, session) = core_with("AC-32", &[RESERVE]);
    alone.world().script_for(
        RESERVE,
        "deposit",
        Behaviour {
            unrecorded: true,
            ..Behaviour::default()
        },
    );
    let (order, ran) = run(
        &mut alone,
        within("req-alone", &session, AMOUNT, RESERVE, OTHER),
    );
    let segments = accounted(&ran);
    assert_eq!(
        segments.len(),
        2,
        "the deposit was handed over and answered"
    );
    assert!(matches!(
        segments[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert!(
        !matches!(segments[1].outcome, SegmentOutcome::Settled { .. }),
        "a statement alone doesn't settle the final deposit: {:?}",
        segments[1].outcome
    );
    assert_ne!(state(&mut alone, &order), OrderState::Complete);
    let deposit = segments[1].segment.clone();
    assert!(
        evidence_of(&alone, &order)
            .iter()
            .all(|e| e.segment != deposit),
        "no evidence is graded for a deposit with no public record"
    );
    // The reserve did move its book: it said so, and that is all there is of it.
    assert!(alone.world().journal(RESERVE).iter().any(|e| matches!(
        e.operation.statement,
        rutent_contracts::provider_role::Operation::Deposit(_)
    )));
}

// verifies: AC-10
#[test]
fn value_arriving_with_no_order_opens_none() {
    use rutent_contracts::core_internal::{ArrivalMatch, Outcome};
    use rutent_core_orch::arrival::Arrived;
    let (mut core, _) = core_with("AC-10", &[RESERVE]);
    let bank = format!("{RESERVE}-bank");
    let before = entries(&core).len();

    let reported = report(&core, RESERVE, &bank, unit("COP"), AMOUNT, "credit-1");
    let arrived = core.arrive(&reported).expect("the arrival is taken");
    let Arrived::NoOrder { .. } = arrived else {
        panic!("value arriving with no order opens none: {arrived:?}");
    };

    // What the log took of it is the arrival, naming no order, matching none, and the reserve's notice: nothing else.
    let taken: Vec<_> = entries(&core).split_off(before);
    let arrivals: Vec<_> = taken
        .iter()
        .filter_map(|v| match &v.what {
            What::Arrival { order, matched, .. } => Some((order.clone(), *matched, v.outcome)),
            _ => None,
        })
        .collect();
    assert_eq!(
        arrivals,
        [(None, ArrivalMatch::NoOrder, Outcome::Observed {})]
    );
    assert!(
        taken.iter().all(|v| matches!(v.what, What::Arrival { .. })),
        "no order, no decision, no acceptance and no effect: {taken:?}"
    );
    // No order exists, so the state machine was told of none, no value is held anywhere, and no order is stuck:
    // nothing counts against the operator.
    assert!(core.inputs().is_empty(), "no order was opened");
    assert_eq!(core.exposures().held().count(), 0);
    assert!(core.exposures().stuck().is_empty());
    assert!(
        core.world().journal(RESERVE).is_empty(),
        "the reserve was asked nothing"
    );
}

// verifies: AC-11
#[test]
fn automatic_deposit_filed_by_an_authorised_reserve_counts_as_acceptance() {
    use rutent_conformance::sim::Spec;
    use rutent_contracts::core_internal::{AcceptanceBasis, Role};
    use rutent_core_orch::arrival::{Arrived, Filed};
    use rutent_scenarios::assembly::component;
    let bank = format!("{RESERVE}-bank");
    let reserve = component(RESERVE, RESERVE, Role::Provider);
    // The reserve that declared it starts routes on automatic deposits files the order itself, or the operator files
    // it for the reserve: each way, the deposit counts as the acceptance, and the record says so.
    for (n, filer) in [reserve, kit::operator_login()].into_iter().enumerate() {
        let mut given = given(&[]);
        given.providers = vec![
            Spec::of(rutent_conformance::sim::Role::Reserve, RESERVE)
                .starting_routes_on_automatic_deposits(),
        ];
        let (mut core, session) = core_of("AC-11", given);
        // The deployment's register of actors lists the reserve's component as acting for the reserve.
        core.registers().list(&filer);
        // An order the customer accepts as ever, run to its end so it is open no more, for the record to be told apart
        // from.
        let (given_order, _) = run(
            &mut core,
            within("req-given", &session, AMOUNT, RESERVE, OTHER),
        );

        let reported = report(
            &core,
            RESERVE,
            &bank,
            unit("COP"),
            AMOUNT,
            &format!("credit-{n}"),
        );
        let Arrived::NoOrder { arrival } = core.arrive(&reported).expect("the arrival is taken")
        else {
            panic!("the arrival is held against no order");
        };
        let asked = within("req-deposit", &session, AMOUNT, RESERVE, OTHER);
        let filing = filing("deposit-1", filer.clone(), arrival, &session, asked);
        let filed = core.file(filing);
        let Ok(Filed::Started { order, .. }) = filed else {
            panic!("{filer:?}: the deposit starts: {filed:?}");
        };

        let acceptances: Vec<_> = entries(&core)
            .into_iter()
            .filter_map(|v| match v.what {
                What::Acceptance {
                    order: o,
                    basis,
                    yeses,
                    session,
                    ..
                } => Some((o, basis, yeses.len(), session, v.who)),
                _ => None,
            })
            .collect();
        let of = |o: &rutent_contracts::router_api::Id| {
            acceptances
                .iter()
                .find(|(order, ..)| order == o)
                .cloned()
                .expect("the order was accepted")
        };
        // The deposit is the acceptance: its basis says so, with no yes of the reserve's and no session of the user's,
        // and its `who` is the filer.
        let (_, basis, yeses, session, who) = of(&order);
        assert_eq!(basis, AcceptanceBasis::AutomaticDeposit);
        assert_eq!((yeses, session), (0, None));
        assert_eq!(who, filer);
        // An explicit acceptance is another thing in the same log: given, by the customer, with the user's session.
        let (_, basis, yeses, session, who) = of(&given_order);
        assert_eq!(basis, AcceptanceBasis::Given);
        assert_eq!((yeses, session.is_some()), (1, true));
        assert_eq!(who, kit::customer());
    }
}

// verifies: AC-31
#[test]
fn shared_movement_keeps_each_orders_own_segment() {
    let bank = format!("{RESERVE}-bank");
    let (mut core, session) = core_with("AC-31", &[RESERVE]);
    // The reserve pays two orders to one account of the same amount with one payment: the network ties each to the
    // first movement it made to that destination for that amount (the harness's reading of a shared movement).
    core.tie_payments_by_destination();
    let (first, ran_first) = run(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    let (second, ran_second) = run(&mut core, within("req-2", &session, AMOUNT, RESERVE, OTHER));

    // Each order keeps its own segments: its own ids, each handed over and recorded under the order it belongs to.
    let (one, two) = (accounted(&ran_first), accounted(&ran_second));
    let ids = |segments: &[rutent_core_orch::segments::Accounted]| -> Vec<String> {
        segments
            .iter()
            .map(|s| s.segment.as_str().to_owned())
            .collect()
    };
    assert!(
        ids(one).iter().all(|a| !ids(two).contains(a)),
        "no segment belongs to two orders"
    );
    for (order, segments) in [(&first, one), (&second, two)] {
        for accounted in segments {
            let moves = entries(&core)
                .into_iter()
                .filter(|v| matches!(&v.what, What::Effect { action: rutent_contracts::core_internal::Action::MoveValue { order: o, segment, .. }, .. } if o == order && segment == &accounted.segment))
                .count();
            assert_eq!(
                moves, 1,
                "{order:?}'s segment {:?} moved once, as its own",
                accounted.segment
            );
        }
    }

    // The first order closes on its own public movement: the network's evidence.
    assert_eq!(state(&mut core, &first), OrderState::Complete);
    let closing: Vec<_> = evidence_of(&core, &first)
        .into_iter()
        .filter(|e| e.segment == one[1].segment)
        .collect();
    assert_eq!(closing[0].whose.as_str(), bank);
    let movement = closing[0].movement.clone().expect("its movement");

    // The second order's outcome rests on the same movement, which backs nothing for it: its record says whose word
    // it is, the reserve's, at the grade its own pieces earn, with no public movement of the network's.
    let theirs = evidence_of(&core, &second);
    assert!(!theirs.is_empty(), "its segments were graded");
    for e in &theirs {
        assert_eq!(e.whose.as_str(), RESERVE, "the provider's word: {e:?}");
        assert!(e.movement.is_none(), "no public movement backs it: {e:?}");
        assert_ne!(e.grade, EvidenceGrade::PublicRecord);
    }
    assert!(
        theirs
            .iter()
            .all(|e| e.movement.as_ref() != Some(&movement)),
        "no movement evidences two orders' segments"
    );
    // And its final deposit, on that movement alone, doesn't close: the order waits.
    assert_ne!(state(&mut core, &second), OrderState::Complete);
    assert!(
        two.iter()
            .all(|s| !matches!(s.outcome, SegmentOutcome::Failed)),
        "{two:?}"
    );
}
