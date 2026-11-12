//! Core's segments on the running platform with simulated providers (`T-020-08`): each segment begun and ended on
//! the platform, decided, held in flight and accounted for, and everything the platform and the proxy report handed
//! to the log.

mod segments_support;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::sim::{Act, Behaviour, Delivery};
use rutent_contracts::core_internal::{
    Answer, CatalogueChange, HandedOver, OrderRefusalClass, Outcome, RefusalClass, Role, UseOf,
    UseOutcome, What,
};
use rutent_contracts::module_interface::Identifier;
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::state_machine::{Observed, OrderState};
use rutent_core_orch::segments::{
    Accounted, NoProxy, NotCounted, PromoteRefused, Promotion, ProvidersWord, Ran, SegmentOutcome,
    SegmentsError, StatusError, UnknownBy,
};
use rutent_prov::module::lifecycle::Staging;
use segments_support::*;
use serde_json::json;

fn segments(ran: &Ran) -> &[Accounted] {
    match ran {
        Ran::Segments(done) => done,
        Ran::Refused { refused, .. } => panic!("the part was refused: {refused:?}"),
    }
}

fn observed(core: &Core) -> Vec<Observed> {
    core.log
        .borrow()
        .inputs()
        .into_iter()
        .map(|i| i.observed.clone())
        .filter(|o| !matches!(o, Observed::RouteChosen { .. }))
        .collect()
}

fn cop() -> Denomination {
    Denomination {
        id: "COP".into(),
        decimals: 2,
    }
}

fn in_flight(core: &Core) -> String {
    core.exposures
        .in_flight(&id(RESERVE), &cop())
        .as_str()
        .to_owned()
}

fn handled(world: &World) -> usize {
    world.fleet.platform.operations_received(RESERVE)
}

/// The withdrawal begins on 1.0.0 while 1.1.0 is staged; mid-flight the operator puts 1.1.0 in force and marks
/// 1.0.0 stale, so the segment moves. Each segment's record reaches the log when it ends, with the version it began
/// on, the one it ran on and its move, before its result.
// verifies: REQ-MDL-5
#[test]
fn segments_record_of_its_versions_and_moves_reaches_the_log_when_it_ends() {
    let mut world = World::of(&[RESERVE]);
    admit_release(&mut world, "1.1.0", Staging::new(1, 0, 0));
    assert_eq!(
        world.fleet.platform.lifecycle().staged(RESERVE),
        Some("1.1.0")
    );
    let mut core = Core::over(&mut world);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    let mut first = true;
    watched.during = Some(Box::new(move |platform, _| {
        if std::mem::take(&mut first) {
            let at = rutent_prov::catalogue::Instant(START);
            platform
                .promote(RESERVE, "ops-lead", at)
                .expect("1.1.0 in force");
            assert_eq!(platform.mark_stale(RESERVE, "1.0.0", "ops-lead", at), Ok(1));
        }
    }));
    let ran = core.run_over(&catalogue, &mut watched);
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert_eq!(done.len(), 2, "{done:?}");
    assert!(
        done.iter()
            .all(|d| matches!(d.outcome, SegmentOutcome::Settled { .. })),
        "{done:?}"
    );
    let log = core.log.borrow();
    let at = |pick: &dyn Fn(&What) -> bool| {
        log.records()
            .into_iter()
            .find(|(_, w)| pick(w))
            .map(|(i, _)| i)
    };
    let records: Vec<_> = log
        .records()
        .into_iter()
        .filter_map(|(i, w)| match w {
            What::HandOver {
                item:
                    HandedOver::SegmentRecord {
                        transaction,
                        began_on,
                        runs_on,
                        moves,
                        ..
                    },
                ..
            } => Some((
                i,
                transaction.clone(),
                began_on.clone(),
                runs_on.clone(),
                moves.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(records.len(), 2, "one record per segment, when it ended");
    let (index, transaction, began_on, runs_on, moves) = &records[0];
    assert_eq!(Some(transaction), done[0].transaction.as_ref());
    assert_eq!((began_on.as_str(), runs_on.as_str()), ("1.0.0", "1.1.0"));
    assert_eq!(moves.len(), 1);
    assert_eq!(
        (
            moves[0].from.as_str(),
            moves[0].to.as_str(),
            moves[0].reason
        ),
        (
            "1.0.0",
            "1.1.0",
            rutent_contracts::core_internal::MoveReason::Stale
        )
    );
    let (_, _, began_on, runs_on, moves) = &records[1];
    assert_eq!((began_on.as_str(), runs_on.as_str()), ("1.1.0", "1.1.0"));
    assert!(moves.is_empty());
    // The record precedes the withdrawal's result.
    let result = at(&|w| matches!(w, What::Result { .. })).unwrap();
    assert!(
        *index < result,
        "the record reaches the log before the result"
    );
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Every outbound call record the platform reports reaches the log, each in its drain at its place, though the log
/// couldn't take one drain at first: it is kept and appended whole, once.
// verifies: REQ-MDL-8
#[test]
fn every_outbound_call_record_the_platform_reports_reaches_the_log() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    // The first drain's appends fail every try.
    for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::HandOver, Fault::Unavailable));
    }
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    let ran = core.run_over(&catalogue, &mut watched);
    let drained = watched.drained.clone();
    drop(watched);
    world.catalogue = catalogue;
    assert_eq!(segments(&ran).len(), 2);
    let mut general = General(std::rc::Rc::clone(&core.log));
    core.segments.flush(&mut general).unwrap();
    assert_eq!(core.segments.undelivered(), 0);
    let reported: Vec<_> = drained.iter().flat_map(|e| e.records.clone()).collect();
    assert!(
        reported.len() >= 2,
        "the reserve called its rail: {reported:?}"
    );
    let log = core.log.borrow();
    let logged: Vec<_> = log
        .records()
        .into_iter()
        .filter_map(|(_, w)| match w {
            What::HandOver {
                item: HandedOver::OutboundCall { record },
                ..
            } => Some(record.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(logged, reported, "every record, once, in order");
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

// verifies: REQ-MDL-6
#[test]
fn every_secret_use_reaches_the_log_with_its_transaction() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    let mut uses = UsesOf(std::rc::Rc::clone(&watched.transactions), 0);
    let session = SuiteContext::default().session;
    let ran = core.run_with(
        &catalogue,
        &mut watched,
        &mut uses,
        &mut ProvidersWord,
        &session,
    );
    let transactions = watched.transactions.borrow().clone();
    drop(watched);
    world.catalogue = catalogue;
    assert_eq!(segments(&ran).len(), 2);
    let log = core.log.borrow();
    let logged: Vec<_> = log
        .records()
        .into_iter()
        .filter_map(|(_, w)| match w {
            What::HandOver {
                item:
                    HandedOver::SecretUse {
                        transaction,
                        of,
                        outcome,
                        module_version,
                        ..
                    },
                ..
            } => Some((
                transaction.as_str().to_owned(),
                of.clone(),
                outcome.clone(),
                module_version.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(logged.len(), 4, "two uses for each operation: {logged:?}");
    for (place, transaction) in transactions.iter().enumerate() {
        let (t, of, outcome, version) = &logged[2 * place];
        assert_eq!(t, transaction);
        assert_eq!(of, &UseOf::StatementKey {});
        assert_eq!(outcome, &UseOutcome::Resolved {});
        assert_eq!(version.as_ref().map(|v| v.as_str()), Some("1.0.0"));
        let (t, _, outcome, _) = &logged[2 * place + 1];
        assert_eq!(t, transaction);
        assert!(
            matches!(outcome, UseOutcome::Refused { reason } if reason.as_str() == "undeclared_secret"),
            "{outcome:?}"
        );
    }
}

// verifies: REQ-ORD-1, INV-1
#[test]
fn result_whose_amounts_break_check_amounts_does_not_count() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let key = statements_key_of(RESERVE);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    // The withdrawal's result, signed by its provider, says it took in more than it was asked: its fee on top.
    watched.alter = Some((
        0,
        Box::new(move |bytes| {
            resigned(&bytes, &key, |v| {
                v["statement"]["outcome"]["amount_in"] = "1000001".into();
            })
        }),
    ));
    let ran = core.run_over(&catalogue, &mut watched);
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert_eq!(done.len(), 1, "no later segment is handed over");
    assert!(
        matches!(
            &done[0].outcome,
            SegmentOutcome::NoResult(NotCounted::Amounts(_))
        ),
        "{done:?}"
    );
    let log = core.log.borrow();
    assert_eq!(log.count(is_result), 0, "the effect has no result");
    drop(log);
    assert_eq!(observed(&core), vec![Observed::ProviderBound {}]);
    assert_eq!(in_flight(&core), AMOUNT, "the withdrawal stays held");
}

/// The reserve refuses the withdrawal, finally: it cancels it, nothing moved, and the order ends. A withdrawal the
/// reserve says failed does the same.
// verifies: REQ-ORD-40
#[test]
fn failed_withdrawal_cancels_the_order_with_nothing_moved() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Refuse));
    let before = handled(&world);
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert_eq!(done.len(), 1, "{done:?}");
    assert_eq!(done[0].outcome, SegmentOutcome::WithdrawalCancelled);
    assert_eq!(
        handled(&world) - before,
        1,
        "the deposit was never handed over"
    );
    assert_eq!(
        observed(&core),
        vec![Observed::ProviderBound {}, Observed::WithdrawalCancelled {}]
    );
    let log = core.log.borrow();
    assert_eq!(log.count(is_effect), 1, "one move decided and recorded");
    let results: Vec<_> = log
        .verified()
        .into_iter()
        .filter(|v| is_result(&v.what))
        .map(|v| v.outcome)
        .collect();
    assert_eq!(results, vec![Outcome::Failed {}]);
    drop(log);
    assert_eq!(
        in_flight(&core),
        "0",
        "nothing is held for an order with nothing moved"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
    let mut reads = Policy::new(std::rc::Rc::clone(&core.log));
    let status = core
        .segments
        .status(
            &mut reads,
            &*core.clock,
            &mut core.ids,
            &customer(),
            None,
            &id(ORDER),
        )
        .unwrap();
    assert_eq!(status.state, OrderState::EndedWithoutCompleting);

    // The reserve's own word that the withdrawal failed.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let key = statements_key_of(RESERVE);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.alter = Some((
        0,
        Box::new(move |bytes| {
            resigned(&bytes, &key, |v| {
                v["statement"]["outcome"] = json!({
                    "status": "failed",
                    "evidence": [{ "grade": 4, "reference": "ref-1", "by": RESERVE }],
                });
            })
        }),
    ));
    let ran = core.run_over(&catalogue, &mut watched);
    let carried = watched.carried.len();
    drop(watched);
    world.catalogue = catalogue;
    assert_eq!(
        segments(&ran)[0].outcome,
        SegmentOutcome::WithdrawalCancelled
    );
    assert_eq!(carried, 1, "the deposit was never handed over");
    assert_eq!(in_flight(&core), "0");
}

/// The withdrawal delivers 1 % short of its quote; the deposit is handed what arrived.
// verifies: REQ-ORD-21
#[test]
fn next_segment_moves_what_is_evidenced_at_the_node_and_not_what_was_quoted() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::delivering(Delivery::Short { bps: 100 }),
    );
    let quoted = core.record.chosen().unwrap().parts[0].operations[1]
        .amount_in
        .amount()
        .as_str()
        .to_owned();
    assert_eq!(quoted, AMOUNT);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    let ran = core.run_over(&catalogue, &mut watched);
    let amounts = watched.amounts();
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    let SegmentOutcome::Settled { amount_out, .. } = &done[0].outcome else {
        panic!("{done:?}");
    };
    assert_eq!(amount_out.as_str(), "990000");
    assert_eq!(
        amounts,
        vec![AMOUNT.to_owned(), "990000".to_owned()],
        "the deposit moves what the withdrawal gave out, not the quote's {quoted}"
    );
}

/// What arrives short is the withdrawal's: the deposit, handed what arrived, settles.
// verifies: REQ-ORD-21
#[test]
fn shortfall_at_a_node_is_not_the_next_providers_failure() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::delivering(Delivery::Short { bps: 100 }),
    );
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert_eq!(done.len(), 2);
    assert!(
        matches!(&done[1].outcome, SegmentOutcome::Settled { amount_out, .. } if amount_out.as_str() == "990000"),
        "{done:?}"
    );
    let inputs = observed(&core);
    assert!(
        !inputs.contains(&Observed::SegmentFailed {}),
        "no segment failed: {inputs:?}"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

// verifies: REQ-ORD-1
#[test]
fn result_from_another_provider_or_transaction_does_not_count() {
    // Another provider's word, signed with its own key, about the withdrawal's transaction.
    let mut world = World::of(&[RESERVE, "sim-reserve-b"]);
    let mut core = Core::over(&mut world);
    let other = statements_key_of("sim-reserve-b");
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.alter = Some((
        0,
        Box::new(move |bytes| resigned(&bytes, &other, |v| v["maker"] = "sim-reserve-b".into())),
    ));
    let ran = core.run_over(&catalogue, &mut watched);
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert_eq!(
        done[0].outcome,
        SegmentOutcome::NoResult(NotCounted::AnotherProvider)
    );
    assert_eq!(observed(&core), vec![Observed::ProviderBound {}]);

    // The segment's provider's word, signed with its key, about another transaction.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let key = statements_key_of(RESERVE);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.alter = Some((
        0,
        Box::new(move |bytes| {
            resigned(&bytes, &key, |v| {
                v["statement"]["transaction"] = "tx-of-another-segment".into();
            })
        }),
    ));
    let ran = core.run_over(&catalogue, &mut watched);
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert_eq!(
        done[0].outcome,
        SegmentOutcome::NoResult(NotCounted::AnotherOperation)
    );
    assert_eq!(observed(&core), vec![Observed::ProviderBound {}]);
    assert_eq!(core.log.borrow().count(is_result), 0);
}

/// Outcome unknown on the provider's own word, and on the platform's record that the module passed something out
/// and then didn't answer; a module that never got as far as the operation leaves no outcome unknown.
// verifies: REQ-ORD-1
#[test]
fn outcome_unknown_is_left_only_on_stated_evidence() {
    // The provider says it doesn't know.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert!(
        matches!(
            done[0].outcome,
            SegmentOutcome::Unknown(UnknownBy::Provider(_))
        ),
        "{done:?}"
    );
    assert_eq!(
        observed(&core),
        vec![
            Observed::ProviderBound {},
            Observed::SegmentOutcomeUnknown {}
        ]
    );
    assert_eq!(core.log.borrow().count(is_result), 0, "no final result yet");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");

    // The module moved value, then answered too late.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour {
            late: true,
            ..Behaviour::default()
        },
    );
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert!(
        matches!(
            done[0].outcome,
            SegmentOutcome::Unknown(UnknownBy::Platform(_))
        ),
        "{done:?}"
    );
    assert_eq!(
        observed(&core),
        vec![
            Observed::ProviderBound {},
            Observed::SegmentOutcomeUnknown {}
        ]
    );
    let log = core.log.borrow();
    assert!(
        log.count(|w| matches!(
            w,
            What::HandOver {
                item: HandedOver::OutboundCall { .. },
                ..
            }
        )) > 0,
        "what it passed out is in the log"
    );
    drop(log);
    assert!(
        !core
            .segments
            .unknown_outcomes(&id(ORDER), &id(WITHDRAWAL))
            .is_empty(),
        "what ended it is kept with the segment"
    );

    // The module never got as far as the operation: nothing moved, and no outcome is unknown.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour {
            silent: true,
            ..Behaviour::default()
        },
    );
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert!(
        matches!(done[0].outcome, SegmentOutcome::NotHandedOver(_)),
        "{done:?}"
    );
    assert_eq!(observed(&core), vec![Observed::ProviderBound {}]);
    assert_eq!(in_flight(&core), "0");
}

// verifies: REQ-ORD-26
#[test]
fn approval_place_is_labelled_as_the_providers_statement() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::act(Act::RefuseWaitingOnThePerson),
    );
    let ran = core.run(&mut world);
    let done = segments(&ran);
    let SegmentOutcome::WaitingOnThePerson(statement) = &done[0].outcome else {
        panic!("{done:?}");
    };
    assert_eq!(statement.maker.as_str(), RESERVE);
    let admitted = core
        .admitted(&world.catalogue)
        .expect("the log admits every entry");
    let index = usize::try_from(u64::from(statement.statement.index)).unwrap();
    let rutent_contracts::core_internal::Entry::Stated(stated) = &admitted[index] else {
        panic!("the place is a stated entry");
    };
    assert!(
        matches!(&stated.attribution, rutent_contracts::core_internal::Attribution::Verified { who }
            if who.acts.as_str() == RESERVE && who.role == Role::Provider),
        "the log attributes it to the provider: {:?}",
        stated.attribution
    );

    // An altered response, not signed by the provider, is labelled as no one's and points the person nowhere.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::act(Act::RefuseWaitingOnThePerson),
    );
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.alter = Some((
        0,
        Box::new(|bytes| {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value["statement"]["outcome"]["refusal"] = json!({ "class": "final" });
            serde_json::to_vec(&value).unwrap()
        }),
    ));
    let ran = core.run_over(&catalogue, &mut watched);
    drop(watched);
    world.catalogue = catalogue;
    assert_eq!(
        segments(&ran)[0].outcome,
        SegmentOutcome::NoResult(NotCounted::Unattributable)
    );
}

// verifies: REQ-QTE-3
#[test]
fn segments_actual_settlement_time_is_recorded_beside_the_declared_one() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.takes = Some((std::rc::Rc::clone(&core.clock), 30));
    let ran = core.run_over(&catalogue, &mut watched);
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    let log = core.log.borrow();
    let times: Vec<_> = log
        .records()
        .into_iter()
        .filter_map(|(_, w)| match w {
            What::Settlement {
                segment,
                provider,
                declared_seconds,
                actual_seconds,
                ..
            } => Some((
                segment.clone(),
                provider.as_str().to_owned(),
                u64::from(*declared_seconds),
                u64::from(*actual_seconds),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        times,
        vec![
            (done[0].segment.clone(), RESERVE.to_owned(), DECLARED, 30),
            (done[1].segment.clone(), RESERVE.to_owned(), DECLARED, 30),
        ]
    );
}

/// An order whose withdrawal settled and whose deposit waits on the person, its clock moved on 120 seconds.
fn order_waiting_between_segments() -> (World, Core) {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "deposit",
        Behaviour::act(Act::RefuseWaitingOnThePerson),
    );
    let ran = core.run(&mut world);
    assert!(matches!(
        segments(&ran)[1].outcome,
        SegmentOutcome::WaitingOnThePerson(_)
    ));
    core.clock.advance(120);
    (world, core)
}

/// The order waits between segments. Its customer and the operator read its state and how long it has been in it,
/// each after a read check the policy engine allows and records: the customer as the order's party, the operator's
/// login as one the register lists for reading an order's state, neither naming a permission. Another customer's
/// read is refused, and recorded too.
// verifies: REQ-ORD-5, REQ-AUZ-7
#[test]
fn orders_state_and_age_are_readable_by_its_customer_and_the_operator() {
    let (_world, mut core) = order_waiting_between_segments();
    let mut reads = Policy::new(std::rc::Rc::clone(&core.log));
    for reader in [customer(), operator()] {
        let status = core
            .segments
            .status(
                &mut reads,
                &*core.clock,
                &mut core.ids,
                &reader,
                None,
                &id(ORDER),
            )
            .unwrap_or_else(|e| panic!("{reader:?} reads it: {e:?}"));
        assert_eq!(status.state, OrderState::FirstSegmentSettled);
        assert_eq!(
            status.parts,
            vec![(id(PART), OrderState::FirstSegmentSettled)]
        );
        assert_eq!(
            status.age_seconds, 120,
            "the time it has waited in its state"
        );
    }
    let stranger = rutent_contracts::core_internal::Actor {
        party: id("another-customer"),
        ..customer()
    };
    assert!(matches!(
        core.segments.status(
            &mut reads,
            &*core.clock,
            &mut core.ids,
            &stranger,
            None,
            &id(ORDER)
        ),
        Err(StatusError::Refused(_))
    ));
    assert_eq!(
        core.log
            .borrow()
            .count(|w| matches!(w, What::CheckRead { .. })),
        3,
        "each read is recorded, allowed or refused"
    );
}

/// The withdrawal's result is judged and counts, then the log can't take its stated entry. Between the tries the
/// catalogue no longer knows the reserve's key. The run that goes on reads the first verdict: the withdrawal settles
/// on the result that counted when it came.
// verifies: REQ-ORD-1, INV-8
#[test]
fn first_verdict_on_a_result_stands_when_its_key_is_retired_between_tries() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::StatedResult, Fault::Unavailable));
    }
    let session = world.context().session;
    let stopped = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut ProvidersWord,
        &session,
    );
    assert!(stopped.is_err(), "{stopped:?}");
    let mut retired = rutent_prov::catalogue::Catalogue::new();
    let context = world.context();
    retired.set_context(context.deployment.clone(), &context.protocol_version);
    let ran = core.run_over(&retired, &mut world.fleet.platform);
    let done = segments(&ran);
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
}

/// The log takes the withdrawal's effect and its answer is lost; Core appends the same entry again, gets its
/// receipt, and acts on it once. Where the log can't take the effect at all, nothing is handed over.
// verifies: INV-8, INV-7
#[test]
fn retried_effect_append_is_acted_on_once() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    core.log
        .borrow_mut()
        .faults
        .push_back((On::Effect, Fault::LostAnswer));
    let before = handled(&world);
    let ran = core.run(&mut world);
    let done = segments(&ran).to_vec();
    assert_eq!(done.len(), 2);
    assert_eq!(
        handled(&world) - before,
        2,
        "each segment's operation, once"
    );
    let log = core.log.borrow();
    assert_eq!(log.count(is_effect), 2, "the retried effect is one entry");
    assert_eq!(log.count(is_result), 2, "and has one result");
    drop(log);
    let receipt = done[0].effect.clone().unwrap();
    assert!(core.segments.acted_on(&receipt));
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
    // The part run again stands as it is: nothing is asked or moved.
    let asked = core.policy.asked.len();
    let session = world.context().session;
    let again = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut ProvidersWord,
        &session,
    );
    assert_eq!(again, Ok(Ran::Segments(done.clone())));
    assert_eq!(core.policy.asked.len(), asked);
    assert_eq!(handled(&world) - before, 2);

    // The log can't take the effect: nothing is handed over, and nothing stays held.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Effect, Fault::Unavailable));
    }
    let before = handled(&world);
    let session = world.context().session;
    let ran = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut ProvidersWord,
        &session,
    );
    assert_eq!(
        ran,
        Err(SegmentsError::Append(
            rutent_contracts::core_internal::calls::AppendFailure::Unavailable
        ))
    );
    assert_eq!(handled(&world), before, "no operation without its record");
    assert_eq!(core.log.borrow().count(is_effect), 0);
    assert_eq!(in_flight(&core), "0");
}

/// Held from its hand-over until it is accounted for, at the amount it moves: the withdrawal, settled, is released;
/// the deposit, whose outcome is unknown, stays held at what arrived, not at the quote.
// verifies: REQ-ORD-8
#[test]
fn segment_handed_over_is_held_in_flight_until_it_is_accounted_for() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::delivering(Delivery::Short { bps: 100 }),
    );
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Unknown));
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert!(matches!(done[0].outcome, SegmentOutcome::Settled { .. }));
    assert!(matches!(done[1].outcome, SegmentOutcome::Unknown(_)));
    let holds: Vec<_> = core
        .exposures
        .held()
        .map(|h| (h.segment.as_str().to_owned(), h.amount.as_str().to_owned()))
        .collect();
    assert_eq!(
        holds,
        vec![(DEPOSIT.to_owned(), "990000".to_owned())],
        "the withdrawal, accounted for, is released; the deposit, in flight, is held at what it moves"
    );
}

/// The chosen part starts at the reserve's ledger with its deposit, and no withdrawal: refused, recorded, and nothing
/// asked or handed over.
// verifies: REQ-ORD-16, REQ-ORD-37
#[test]
fn part_breaking_the_route_guard_is_refused_before_any_segment_is_handed_over() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut record = serde_json::to_value(&core.record).unwrap();
    record["request"]["from"] = ledger(RESERVE);
    record["candidates"][0]["parts"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    core.record = serde_json::from_value(record).expect("the router's contract takes it");
    let before = handled(&world);
    let ran = core.run(&mut world);
    let Ran::Refused { refused, .. } = &ran else {
        panic!("{ran:?}");
    };
    assert_eq!(refused.class, OrderRefusalClass::NoRouteRightNow);
    assert!(core.policy.asked.is_empty(), "no move is asked of policy");
    assert_eq!(handled(&world), before, "nothing is handed over");
    let log = core.log.borrow();
    assert_eq!(
        log.count(|w| matches!(
            w,
            What::OrderRefusal {
                class: OrderRefusalClass::NoRouteRightNow,
                ..
            }
        )),
        1
    );
    assert_eq!(log.count(is_effect), 0);
    drop(log);
    assert!(world.fleet.platform.lifecycle().in_force(RESERVE).is_some());
    let _ = Identifier::new(RESERVE);
}

/// Policy refusing the move: nothing is recorded as an effect, and nothing is handed over.
// verifies: REQ-AUZ-1
#[test]
fn move_policy_does_not_permit_hands_nothing_over() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    core.policy.refuse(RefusalClass::OutsideAuthorisation);
    let before = handled(&world);
    let ran = core.run(&mut world);
    assert_eq!(
        segments(&ran)[0].outcome,
        SegmentOutcome::NotPermitted(Answer::Refuse {
            class: RefusalClass::OutsideAuthorisation
        })
    );
    assert_eq!(handled(&world), before);
    assert_eq!(core.log.borrow().count(is_effect), 0);
    assert_eq!(in_flight(&core), "0");
}

/// A withdrawal's result, signed by its provider, says it gave out twice what it took in. It doesn't count, and the
/// deposit is never handed the surplus.
// verifies: REQ-ORD-21, INV-1
#[test]
fn result_giving_out_more_than_it_took_in_does_not_count() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let key = statements_key_of(RESERVE);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.alter = Some((
        0,
        Box::new(move |bytes| {
            resigned(&bytes, &key, |v| {
                v["statement"]["outcome"]["amount_out"] = "2000000".into();
            })
        }),
    ));
    let ran = core.run_over(&catalogue, &mut watched);
    let carried = watched.carried.len();
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert_eq!(
        done[0].outcome,
        SegmentOutcome::NoResult(NotCounted::GivesOutMore)
    );
    assert_eq!(carried, 1, "the deposit is never handed over");
    assert_eq!(in_flight(&core), AMOUNT, "only what was moved is held");
    assert_eq!(core.log.borrow().count(is_result), 0);
}

/// A withdrawal settles with nothing given out: the part can't go on, and the order is told so.
// verifies: REQ-ORD-21
#[test]
fn segment_settling_with_nothing_given_out_ends_the_part_with_its_refusal() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let key = statements_key_of(RESERVE);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.alter = Some((
        0,
        Box::new(move |bytes| {
            resigned(&bytes, &key, |v| {
                v["statement"]["outcome"]["amount_out"] = "0".into();
            })
        }),
    ));
    let ran = core.run_over(&catalogue, &mut watched);
    let carried = watched.carried.len();
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert!(
        matches!(&done[0].outcome, SegmentOutcome::Settled { amount_out, .. } if amount_out.as_str() == "0"),
        "{done:?}"
    );
    assert_eq!(carried, 1);
    assert_eq!(
        core.log.borrow().count(|w| matches!(
            w,
            What::OrderRefusal {
                class: OrderRefusalClass::NoRouteForAmount,
                ..
            }
        )),
        1
    );
}

/// The log takes the withdrawal's result, then its provider-bound input, then the segment record, and loses every
/// answer: each time the run stops with what came back kept, and running the part again goes on from there, moving nothing twice and
/// recording each entry once.
// verifies: REQ-MDL-8, REQ-MDL-5, INV-7, INV-8
#[test]
fn what_comes_back_is_kept_when_a_later_step_fails() {
    for on in [On::StatedResult, On::Bound, On::Record] {
        let mut world = World::of(&[RESERVE]);
        let mut core = Core::over(&mut world);
        for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
            // The log takes the entry, and each answer is lost.
            core.log
                .borrow_mut()
                .faults
                .push_back((on, Fault::LostAnswer));
        }
        let before = handled(&world);
        let session = world.context().session;
        let stopped = core.try_run_with(
            &world.catalogue,
            &mut world.fleet.platform,
            &mut NoProxy,
            &mut ProvidersWord,
            &session,
        );
        assert_eq!(
            stopped,
            Err(SegmentsError::Append(
                rutent_contracts::core_internal::calls::AppendFailure::Unavailable
            )),
            "{on:?}"
        );
        let ran = core.run(&mut world);
        let done = segments(&ran);
        assert!(
            done.iter()
                .all(|d| matches!(d.outcome, SegmentOutcome::Settled { .. })),
            "{on:?}: {done:?}"
        );
        assert_eq!(handled(&world) - before, 2, "{on:?}: each operation once");
        let log = core.log.borrow();
        let stated_results = log
            .entries
            .iter()
            .filter(|(e, _)| {
                // A segment's result, apart from the reserve's report the order's arrival rests on.
                matches!(e, rutent_contracts::core_internal::Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about.as_ref().is_some_and(|a| a.segment.is_some()))
            })
            .count();
        assert_eq!(stated_results, 2, "{on:?}: each result kept once");
        assert_eq!(
            log.count(|w| matches!(
                w,
                What::HandOver {
                    item: HandedOver::SegmentRecord { .. },
                    ..
                }
            )),
            2,
            "{on:?}"
        );
        assert_eq!(log.count(is_effect), 2, "{on:?}");
        drop(log);
        core.admitted(&world.catalogue)
            .unwrap_or_else(|e| panic!("{on:?}: {e:?}"));
    }
}

/// A drain naming something the contract can't carry is kept whole, the error answered every time it is tried,
/// and what the operation gave back stays with its segment.
// verifies: REQ-MDL-8
#[test]
fn drain_the_log_cant_take_is_kept_whole_and_answered() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.spoil = true;
    let session = SuiteContext::default().session;
    let stopped = core.try_run_with(
        &catalogue,
        &mut watched,
        &mut NoProxy,
        &mut ProvidersWord,
        &session,
    );
    drop(watched);
    world.catalogue = catalogue;
    assert!(
        matches!(stopped, Err(SegmentsError::Uncarried(_))),
        "{stopped:?}"
    );
    assert_eq!(core.segments.undelivered(), 1, "the drain is kept whole");
    let mut general = General(std::rc::Rc::clone(&core.log));
    assert!(matches!(
        core.segments.flush(&mut general),
        Err(SegmentsError::Uncarried(_))
    ));
    assert_eq!(core.segments.undelivered(), 1);
    assert!(
        core.segments
            .in_flight(&id(ORDER), &id(WITHDRAWAL))
            .is_some(),
        "the withdrawal and what it gave back stay in flight"
    );
}

/// The log takes the withdrawal's effect three times and loses each answer. Running the part again appends the same
/// entry again, not a second effect, and acts on it once.
// verifies: INV-8, INV-7
#[test]
fn effect_whose_answers_were_lost_is_appended_again_unchanged() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Effect, Fault::LostAnswer));
    }
    let before = handled(&world);
    let session = world.context().session;
    let stopped = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut ProvidersWord,
        &session,
    );
    assert!(stopped.is_err());
    assert_eq!(handled(&world), before, "nothing without its receipt");
    let ran = core.run(&mut world);
    assert_eq!(segments(&ran).len(), 2);
    assert_eq!(
        core.log.borrow().count(is_effect),
        2,
        "one effect per segment"
    );
    assert_eq!(core.policy.asked.len(), 2, "one decision per segment");
    assert_eq!(handled(&world) - before, 2);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The log answers the deposit's effect with the withdrawal's receipt. Core has acted on that receipt, so it hands
/// the deposit over to no one, whatever segment the receipt came back for.
// verifies: INV-8
#[test]
fn effect_receipt_acted_on_before_is_not_acted_on_again() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    core.log
        .borrow_mut()
        .faults
        .push_back((On::EffectOf(DEPOSIT), Fault::Replay));
    let before = handled(&world);
    let session = world.context().session;
    let ran = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut ProvidersWord,
        &session,
    );
    assert!(
        matches!(ran, Err(SegmentsError::AlreadyActed(_))),
        "{ran:?}"
    );
    assert_eq!(
        handled(&world) - before,
        1,
        "the deposit was never handed over"
    );
}

/// 1.1.0 is staged beside 1.0.0. A promotion naming another version is refused before policy is asked; one naming
/// 1.1.0 is decided, recorded as an effect, carried out by the decision's caller, and its result recorded.
// verifies: REQ-AUZ-6
#[test]
fn promotion_whose_staged_version_differs_from_its_permit_is_refused() {
    let mut world = World::of(&[RESERVE]);
    admit_release(&mut world, "1.1.0", Staging::new(1, 0, 0));
    let mut core = Core::over(&mut world);
    let (provider, caller, core_actor) = (id(RESERVE), operator(), segments_support::core());
    let mut general = General(std::rc::Rc::clone(&core.log));
    let other = id("1.0.5");
    let refused = core.segments.promote(
        &mut general,
        &mut core.policy,
        &mut world.fleet.platform,
        &*core.clock,
        &mut core.ids,
        &Promotion {
            provider: &provider,
            version: &other,
            caller: &caller,
            core: &core_actor,
        },
    );
    assert_eq!(
        refused,
        Err(PromoteRefused::NotTheStagedVersion {
            staged: "1.1.0".into()
        })
    );
    assert!(core.policy.asked.is_empty(), "policy isn't asked");
    assert_eq!(
        world.fleet.platform.lifecycle().in_force(RESERVE),
        Some("1.0.0")
    );
    let staged = id("1.1.0");
    let promoted = core.segments.promote(
        &mut general,
        &mut core.policy,
        &mut world.fleet.platform,
        &*core.clock,
        &mut core.ids,
        &Promotion {
            provider: &provider,
            version: &staged,
            caller: &caller,
            core: &core_actor,
        },
    );
    assert_eq!(promoted, Ok("1.1.0".into()));
    assert_eq!(
        world.fleet.platform.lifecycle().in_force(RESERVE),
        Some("1.1.0")
    );
    let decision = world
        .fleet
        .platform
        .lifecycle()
        .decisions()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        decision.by.as_str(),
        caller.acts.as_str(),
        "by the decision's caller"
    );
    let log = core.log.borrow();
    let results: Vec<_> = log
        .verified()
        .into_iter()
        .filter(|v| is_result(&v.what))
        .map(|v| v.outcome)
        .collect();
    assert_eq!(results, vec![Outcome::Done {}]);
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The reserve's module stands for the operator's party, so the log verifies its statements by the release it
/// accepted, as it does a module the operator publishes (core-internal's `release_statement_keys`). Mid-flight, its
/// release is revoked. The segment's record, handed over before its result, shows it ran on that release, so its
/// result is still the module's and the segment settles on it; were the result first, it would be no one's.
// verifies: REQ-MDL-20, REQ-MDL-5
#[test]
fn segment_on_a_release_revoked_mid_flight_settles_on_its_result() {
    let mut world = World::of(&[RESERVE]);
    // The operator states for the party the reserve's module states for, and the log took the release's acceptance
    // before the order's arrival, so the module's statements are verified by the release's key from the first.
    let mut module_operator = operator();
    module_operator.party = id(&format!("{RESERVE}-publisher"));
    let key = world.installed[0].declaration.outside.statement_key.clone();
    let mut core = Core::over_with(&mut world, module_operator, |log| {
        seed_change(
            log,
            release_change(CatalogueChange::AcceptModule, "1.0.0", Some(key)),
        )
    });
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    let log = std::rc::Rc::clone(&core.log);
    let mut first = true;
    watched.during = Some(Box::new(move |_, _| {
        if std::mem::take(&mut first) {
            seed_change(
                &log,
                release_change(CatalogueChange::RevokeRelease, "1.0.0", None),
            );
        }
    }));
    let ran = core.run_over(&catalogue, &mut watched);
    drop(watched);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    let admitted = core
        .admitted(&world.catalogue)
        .expect("the log admits every entry");
    let verified = admitted.iter().any(|e| matches!(e,
        rutent_contracts::core_internal::Entry::Stated(s)
            if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                && s.about.as_ref().and_then(|a| a.segment.as_ref()).map(|x| x.as_str()) == Some(WITHDRAWAL)
                && matches!(s.attribution, rutent_contracts::core_internal::Attribution::Verified { .. })));
    assert!(verified, "the withdrawal's result is the module's");
}

/// One order's withdrawal leaves a drain the contract can't carry. Another order runs after it: its segments settle,
/// each with its record before its result, while the bad drain stays kept and is answered each time it is tried.
// verifies: REQ-MDL-8, REQ-MDL-20
#[test]
fn drain_the_log_cant_take_holds_up_no_other_segment() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let session = SuiteContext::default().session;
    {
        let mut watched = Watched::new(&mut world.fleet.platform);
        watched.spoil = true;
        let stopped = core.try_run_with(
            &catalogue,
            &mut watched,
            &mut NoProxy,
            &mut ProvidersWord,
            &session,
        );
        assert!(
            matches!(stopped, Err(SegmentsError::Uncarried(_))),
            "{stopped:?}"
        );
    }
    assert_eq!(core.segments.undelivered(), 1);
    core.take_up(Named::another(2));
    let ran = core.run_over(&catalogue, &mut world.fleet.platform);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert!(
        done.iter()
            .all(|d| matches!(d.outcome, SegmentOutcome::Settled { .. })),
        "{done:?}"
    );
    assert_eq!(
        core.segments.undelivered(),
        1,
        "the bad drain is still kept"
    );
    let mut general = General(std::rc::Rc::clone(&core.log));
    assert!(matches!(
        core.segments.flush(&mut general),
        Err(SegmentsError::Uncarried(_))
    ));
    let log = core.log.borrow();
    let records = log
        .records()
        .into_iter()
        .filter(|(_, w)| {
            matches!(
                w,
                What::HandOver {
                    item: HandedOver::SegmentRecord { .. },
                    ..
                }
            )
        })
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let results = log
        .records()
        .into_iter()
        .filter(|(_, w)| is_result(w))
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2, "the second order's two records");
    assert_eq!(results.len(), 2);
    assert!(
        records[0] < results[0] && records[1] < results[1],
        "each record before its result"
    );
}

/// The deposit stops after the withdrawal settled: first its effect can't be appended, then its result can't. Running
/// the part again takes the settled withdrawal as it stands, hands the deposit what it gave out, and settles it,
/// moving nothing twice.
// verifies: INV-7, INV-8, REQ-ORD-21
#[test]
fn part_resumes_after_its_first_segment_settled() {
    for on in [On::EffectOf(DEPOSIT), On::LaterResult] {
        let mut world = World::of(&[RESERVE]);
        let mut core = Core::over(&mut world);
        for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
            core.log
                .borrow_mut()
                .faults
                .push_back((on, Fault::Unavailable));
        }
        let before = handled(&world);
        let session = world.context().session;
        let stopped = core.try_run_with(
            &world.catalogue,
            &mut world.fleet.platform,
            &mut NoProxy,
            &mut ProvidersWord,
            &session,
        );
        assert!(stopped.is_err(), "{on:?}: {stopped:?}");
        let ran = core.run(&mut world);
        let done = segments(&ran);
        assert_eq!(done.len(), 2, "{on:?}: {done:?}");
        assert!(
            done.iter()
                .all(|d| matches!(d.outcome, SegmentOutcome::Settled { .. })),
            "{on:?}: {done:?}"
        );
        assert_eq!(handled(&world) - before, 2, "{on:?}: each operation once");
        assert_eq!(core.log.borrow().count(is_result), 2, "{on:?}");
        core.admitted(&world.catalogue)
            .unwrap_or_else(|e| panic!("{on:?}: {e:?}"));
        // Run again once all is final: the outcomes as they stand, and nothing asked or moved.
        let asked = core.policy.asked.len();
        let again = core.run(&mut world);
        assert_eq!(segments(&again), done, "{on:?}");
        assert_eq!(core.policy.asked.len(), asked, "{on:?}");
        assert_eq!(handled(&world) - before, 2, "{on:?}");
    }
}

/// The platform won't begin the deposit, and the log can't take its `unanswered` result. Run again with the platform
/// willing: the deposit is finished as it was, refused, its result `unanswered`; it is never begun or carried out
/// under a result that says it wasn't.
// verifies: REQ-ORD-1, INV-7
#[test]
fn segment_the_platform_wouldnt_begin_is_finished_as_refused() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::LaterResult, Fault::Unavailable));
    }
    let before = handled(&world);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let session = SuiteContext::default().session;
    {
        let mut watched = Watched::new(&mut world.fleet.platform);
        watched.refuse_begin = Some(1);
        let stopped = core.try_run_with(
            &catalogue,
            &mut watched,
            &mut NoProxy,
            &mut ProvidersWord,
            &session,
        );
        assert!(stopped.is_err(), "{stopped:?}");
    }
    let ran = core.run_over(&catalogue, &mut world.fleet.platform);
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert!(
        matches!(done[1].outcome, SegmentOutcome::NotHandedOver(_)),
        "{done:?}"
    );
    assert_eq!(
        handled(&world) - before,
        1,
        "the deposit was never carried out"
    );
    let log = core.log.borrow();
    let results: Vec<_> = log
        .verified()
        .into_iter()
        .filter(|v| is_result(&v.what))
        .map(|v| v.outcome)
        .collect();
    assert_eq!(results, vec![Outcome::Done {}, Outcome::Unanswered {}]);
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The withdrawal charges 1 000. Under its tuple's `net` reporting, what it gave out and its charge are within what
/// it took in, and the deposit is handed what it gave out; a result whose output and charge together come to more
/// than it took in doesn't count. Under `gross`, its output includes its charge, and the deposit is handed what it
/// gave out less the charge.
// verifies: REQ-ORD-21, INV-1
#[test]
fn next_segment_is_handed_what_arrived_net_of_the_charge_as_the_tuple_reports_it() {
    let run = |gross: bool, out: &'static str| {
        let mut world = World::of(&[RESERVE]);
        let mut core = Core::over(&mut world);
        if gross {
            let mut record = serde_json::to_value(&core.record).unwrap();
            for figure in record["snapshot"]["figures"].as_array_mut().unwrap() {
                if figure["value"]["kind"] == "reserve_tuple" {
                    figure["value"]["tuple"]["terms"]["reporting"] = "gross".into();
                }
            }
            core.record = serde_json::from_value(record).expect("the record takes gross terms");
        }
        let key = statements_key_of(RESERVE);
        let catalogue = std::mem::replace(
            &mut world.catalogue,
            rutent_prov::catalogue::Catalogue::new(),
        );
        let mut watched = Watched::new(&mut world.fleet.platform);
        watched.alter = Some((
            0,
            Box::new(move |bytes| {
                resigned(&bytes, &key, |v| {
                    v["statement"]["outcome"]["fee"]["amount"] = "1000".into();
                    v["statement"]["outcome"]["amount_out"] = out.into();
                })
            }),
        ));
        let ran = core.run_over(&catalogue, &mut watched);
        let amounts = watched.amounts();
        drop(watched);
        (segments(&ran).to_vec(), amounts)
    };
    let (done, amounts) = run(false, "999000");
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert_eq!(
        amounts,
        vec![AMOUNT.to_owned(), "999000".to_owned()],
        "net: what it gave out"
    );
    let (done, amounts) = run(false, "999500");
    assert_eq!(
        done[0].outcome,
        SegmentOutcome::NoResult(NotCounted::GivesOutMore)
    );
    assert_eq!(
        amounts.len(),
        1,
        "net: out and charge beyond what came in hands nothing on"
    );
    let (done, amounts) = run(true, "1000000");
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert_eq!(
        amounts,
        vec![AMOUNT.to_owned(), "999000".to_owned()],
        "gross: what it gave out less its charge"
    );
}

/// The withdrawal settles with nothing given out, and the log can't take the part's refusal. Running the part again
/// takes the settled withdrawal as it stands and records the refusal it still owes, once.
// verifies: REQ-ORD-21, INV-7
#[test]
fn refusal_owed_after_a_settled_segment_is_recorded_on_resume() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Refusal, Fault::Unavailable));
    }
    let key = statements_key_of(RESERVE);
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let session = SuiteContext::default().session;
    {
        let mut watched = Watched::new(&mut world.fleet.platform);
        watched.alter = Some((
            0,
            Box::new(move |bytes| {
                resigned(&bytes, &key, |v| {
                    v["statement"]["outcome"]["amount_out"] = "0".into();
                })
            }),
        ));
        let stopped = core.try_run_with(
            &catalogue,
            &mut watched,
            &mut NoProxy,
            &mut ProvidersWord,
            &session,
        );
        assert!(stopped.is_err(), "{stopped:?}");
    }
    let refusals = |core: &Core| {
        core.log.borrow().count(|w| {
            matches!(
                w,
                What::OrderRefusal {
                    class: OrderRefusalClass::NoRouteForAmount,
                    ..
                }
            )
        })
    };
    assert_eq!(refusals(&core), 0);
    let ran = core.run_over(&catalogue, &mut world.fleet.platform);
    world.catalogue = catalogue;
    assert_eq!(segments(&ran).len(), 1, "the part ends at the withdrawal");
    assert_eq!(
        refusals(&core),
        1,
        "the refusal it owed, recorded on resume"
    );
    let _ = core.run_over(&world.catalogue, &mut world.fleet.platform);
    assert_eq!(refusals(&core), 1, "and once");
}
