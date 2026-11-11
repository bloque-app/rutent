//! Core's segments on the running platform with simulated providers (`T-020-08`): each segment begun and ended on
//! the platform, decided, held in flight and accounted for, and everything the platform and the proxy report handed
//! to the log.

mod segments_support;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::sim::{Act, Behaviour, Delivery};
use rutent_contracts::core_internal::{
    Answer, HandedOver, OrderRefusalClass, Outcome, RefusalClass, Role, UseOf, UseOutcome, What,
};
use rutent_contracts::module_interface::Identifier;
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::state_machine::{Observed, OrderState};
use rutent_core_orch::segments::{
    Accounted, NoProxy, NotCounted, ProvidersWord, Ran, SegmentOutcome, SegmentsError, StatusError,
    UnknownBy,
};
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

// verifies: REQ-MDL-5
#[test]
fn segments_record_of_its_versions_and_moves_reaches_the_log_when_it_ends() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert_eq!(done.len(), 2, "{done:?}");
    assert!(
        done.iter()
            .all(|d| matches!(d.outcome, SegmentOutcome::Settled { .. })),
        "{done:?}"
    );
    let log = core.log.borrow();
    let records: Vec<_> = log
        .records()
        .into_iter()
        .filter_map(|(_, w)| match w {
            What::HandOver {
                item:
                    HandedOver::SegmentRecord {
                        transaction,
                        provider,
                        began_on,
                        runs_on,
                        moves,
                    },
                ..
            } => Some((
                transaction.clone(),
                provider.clone(),
                began_on.clone(),
                runs_on.clone(),
                moves.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(records.len(), 2, "one record per segment, when it ended");
    for (place, (transaction, provider, began_on, runs_on, moves)) in records.iter().enumerate() {
        assert_eq!(Some(transaction), done[place].transaction.as_ref());
        assert_eq!(provider.as_str(), RESERVE);
        assert_eq!(began_on.as_str(), "1.0.0");
        assert_eq!(runs_on.as_str(), "1.0.0");
        assert!(moves.is_empty());
    }
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

/// The withdrawal settled and the deposit waits on the person: the order waits between segments.
// verifies: REQ-ORD-5
#[test]
fn orders_state_and_age_are_readable_by_its_customer_and_the_operator() {
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
    let mut reads = Policy::new(std::rc::Rc::clone(&core.log));
    for reader in [customer(), operator()] {
        let status = core
            .segments
            .status(&mut reads, &*core.clock, &mut core.ids, &reader, &id(ORDER))
            .expect("the read is allowed");
        assert_eq!(status.state, OrderState::FirstSegmentSettled, "{reader:?}");
        assert_eq!(
            status.parts,
            vec![(id(PART), OrderState::FirstSegmentSettled)]
        );
        assert_eq!(status.age_seconds, 120, "{reader:?}");
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
            &id(ORDER)
        ),
        Err(StatusError::Refused(_))
    ));
    // Each read is recorded, allowed or refused.
    assert_eq!(
        core.log
            .borrow()
            .count(|w| matches!(w, What::CheckRead { .. })),
        3
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
    // The same segment handed over again is refused before anything is asked or moved.
    let asked = core.policy.asked.len();
    let session = world.context().session;
    let again = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut ProvidersWord,
        &session,
    );
    assert_eq!(again, Err(SegmentsError::AlreadyHandedOver(id(WITHDRAWAL))));
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

/// Held from its hand-over until it is accounted for: a settled segment is released, a segment whose outcome is
/// unknown stays held.
// verifies: REQ-ORD-8
#[test]
fn segment_handed_over_is_held_in_flight_until_it_is_accounted_for() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Unknown));
    let held = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let catalogue = std::mem::replace(
        &mut world.catalogue,
        rutent_prov::catalogue::Catalogue::new(),
    );
    let ran = {
        let mut watched = Watched::new(&mut world.fleet.platform);
        core.run_over(&catalogue, &mut watched)
    };
    world.catalogue = catalogue;
    let done = segments(&ran);
    assert!(matches!(done[0].outcome, SegmentOutcome::Settled { .. }));
    assert!(matches!(done[1].outcome, SegmentOutcome::Unknown(_)));
    let holds: Vec<_> = core
        .exposures
        .held()
        .map(|h| (h.segment.as_str().to_owned(), h.amount.as_str().to_owned()))
        .collect();
    held.borrow_mut().extend(holds.clone());
    assert_eq!(
        holds,
        vec![(DEPOSIT.to_owned(), AMOUNT.to_owned())],
        "the withdrawal, accounted for, is released; the deposit, in flight, is held"
    );
    assert_eq!(in_flight(&core), AMOUNT);
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
    core.policy.answer = Answer::Refuse {
        class: RefusalClass::OutsideAuthorisation,
    };
    let before = handled(&world);
    let ran = core.run(&mut world);
    assert_eq!(
        segments(&ran)[0].outcome,
        SegmentOutcome::NotPermitted(core.policy.answer)
    );
    assert_eq!(handled(&world), before);
    assert_eq!(core.log.borrow().count(is_effect), 0);
    assert_eq!(in_flight(&core), "0");
}
