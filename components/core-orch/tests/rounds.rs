//! The routing rounds (`T-020-06`): two rounds per routing, questions only to the shortlist, each its own
//! transaction, the reserve's yes in the user's session, and every record chained and logged in order.

mod rounds_support;
mod snapshot_support;

use rounds_support::*;
use rutent_contracts::core_internal::{About, Entry, RoutingRound, What};
use rutent_contracts::router_api::{DropReason, FigureValue, Round};
use rutent_core_orch::rounds::{History, Need, NotOffered, Offer, RoutingError};
use rutent_core_orch::rounds::{Presence, Routed};
use rutent_prov::catalogue::CheckRefusal;
use serde_json::json;
use snapshot_support::*;

fn run(env: &mut Env, sim: &mut Sim, inputs: &Inputs) -> Routed {
    env.route(sim, inputs.routing(), &mut History::new())
        .expect("the routing runs")
}

fn last_choice(routed: &Routed) -> &rutent_contracts::router_api::DecisionRecord {
    &routed.choices.last().expect("a choice round").record
}

// verifies: REQ-ORD-32
#[test]
fn route_is_not_offered_without_the_reserves_yes_in_the_users_session() {
    // With no session of the user's open, the catalogue lets no withdrawal's dry run out: the reserve is asked
    // nothing about the holder's funds, and no route is offered.
    let inputs = on_polygon(&[RESERVE], RESERVE);
    let mut env = Env::new(catalogue(&[(RESERVE, "acme")], 100_000), false, 100);
    let mut sim = env.sim();
    let routed = run(&mut env, &mut sim, &inputs);
    assert!(
        matches!(routed.offer, Offer::NotOffered(_)),
        "{:?}",
        routed.offer
    );
    assert!(
        sim.asked(RESERVE)
            .iter()
            .all(|op| op["kind"] != "withdrawal"),
        "the reserve was asked about the holder's funds outside a session"
    );
    assert_eq!(
        routed.refused_checks,
        vec![(id(RESERVE), CheckRefusal::NoSession)]
    );

    // In the user's session, the route is offered on the reserve's yes: the receipt it names is of the reserve's
    // own answer, appended about the order and the transaction Core asked it under.
    let mut env = Env::new(catalogue(&[(RESERVE, "acme")], 100_000), true, 100);
    let mut sim = env.sim();
    let routed = run(&mut env, &mut sim, &inputs);
    let Offer::Offered {
        reserve_yes,
        session,
        ..
    } = &routed.offer
    else {
        panic!("no route offered: {:?}", routed.offer);
    };
    assert_eq!(session.as_ref(), Some(&id(SESSION)));
    let [yes] = reserve_yes.as_slice() else {
        panic!("one yes per part: {reserve_yes:?}");
    };
    let withdrawal = routed
        .sent
        .iter()
        .find(|s| s.provider == id(RESERVE) && s.tuple == withdrawal_tuple(RESERVE))
        .expect("the withdrawal was asked");
    let Entry::Stated(entry) = &env.log.entries[yes.index.get() as usize] else {
        panic!("the yes is a stated entry");
    };
    assert_eq!(entry.attribution.maker().acts, id(RESERVE));
    assert_eq!(
        entry.about,
        Some(About {
            order: Some(id(ORDER)),
            segment: None,
            transaction: withdrawal.transaction.clone(),
        })
    );
}

// verifies: REQ-ORD-32
#[test]
fn yes_carried_by_the_user_is_refused() {
    // The user carries a yes the reserve really signed, for this amount, under a transaction of the user's own, and
    // it comes back in place of the reserve's answer to Core's question.
    let inputs = on_polygon(&[RESERVE], RESERVE);
    let mut env = Env::new(catalogue(&[(RESERVE, "acme")], 100_000), true, 100);
    let mut sim = env.sim();
    let carried = json!({
        "kind": "withdrawal", "transaction": "users-own-tx", "provider": RESERVE, "session": SESSION,
        "origin": { "network": "polygon" }, "destination": "polygon", "asset": "USDC", "amount": "1000",
        "dry_run": true, "inputs": {},
    });
    let yes = sign(
        answer(RESERVE, &carried, START, 0, false),
        &statements_key(RESERVE, 1),
    );
    sim.relay.insert(RESERVE.into(), yes.clone());
    let routed = run(&mut env, &mut sim, &inputs);

    assert!(
        matches!(routed.offer, Offer::NotOffered(_)),
        "{:?}",
        routed.offer
    );
    let choice = last_choice(&routed);
    assert!(
        !choice
            .snapshot
            .figures
            .iter()
            .any(|f| matches!(&f.value, FigureValue::DryRun { result, .. }
                if serde_json::to_value(result).unwrap()["statement"]["transaction"] == "users-own-tx")),
        "the carried yes entered the choice round"
    );
    // It is kept on the record, as what came back, about the order and the transaction Core asked under.
    let at = env
        .stated_at(&yes)
        .expect("the carried yes is on the record");
    let Entry::Stated(entry) = &env.log.entries[at] else {
        unreachable!()
    };
    let asked = routed
        .sent
        .iter()
        .find(|s| s.tuple == withdrawal_tuple(RESERVE))
        .unwrap();
    assert_eq!(entry.about.as_ref().unwrap().transaction, asked.transaction);
}

// verifies: REQ-CAT-3
#[test]
fn own_order_is_claimed_only_for_a_check_that_belongs_to_the_holders_order() {
    let inputs = on_polygon(&[RESERVE, SIBLING], RESERVE);
    let mut env = Env::new(
        catalogue(&[(RESERVE, "acme"), (SIBLING, "acme")], 100_000),
        true,
        100,
    );
    let mut sim = env.sim();
    let _ = run(&mut env, &mut sim, &inputs);
    let checks = env.catalogue.check_log("acct-1");
    let of = |provider: &str| {
        checks
            .iter()
            .find(|c| c.provider == provider)
            .unwrap_or_else(|| panic!("{provider} was checked"))
    };
    // The withdrawal at the order's origin reserve is the holder's own order: logged, not counted.
    assert_eq!(of(RESERVE).own_order.as_deref(), Some(ORDER));
    assert!(!of(RESERVE).counted);
    // The one at the other reserve isn't: it counts.
    assert_eq!(of(SIBLING).own_order, None);
    assert!(of(SIBLING).counted);
    assert_eq!(
        env.catalogue
            .counted_checks("acct-1", rutent_prov::catalogue::Instant(START + 60)),
        1
    );
}

// verifies: REQ-QTE-1
#[test]
fn operation_asked_at_another_amount_is_dropped_as_quote_voided_citing_that_quote() {
    // The ramp is asked at the free reserve's 1 000, the path ranked first. The free reserve then refuses, so the
    // choice round has only the dear reserve's path, which reaches the ramp with 990: the ramp's quote at 1 000
    // lists `amount_changes`, so it is void for 990, and Core asks again at 990.
    let inputs = polygon_to_base_inputs();
    let mut env = Env::new(ramp_catalogue(), true, 100);
    let mut sim = env.sim();
    sim.refuse.insert(FREE_RESERVE.into());
    sim.fees.insert(DEAR_RESERVE.into(), 10);
    let routed = run(&mut env, &mut sim, &inputs);

    let first = &routed.choices[0].record;
    assert!(first.chosen.is_none());
    let [voided] = routed.voided.as_slice() else {
        panic!("one voided quote: {:?}", routed.voided);
    };
    assert_eq!(voided.record, first.request.request_id);
    assert_eq!(voided.tuple, ramp_tuple());
    assert_eq!(
        (voided.quoted.as_str(), voided.needed.amount().as_str()),
        ("1000", "990")
    );
    let figure = first
        .snapshot
        .figures
        .iter()
        .find(|f| f.figure_id == voided.figure)
        .expect("the voided quote is in the round's snapshot");
    let FigureValue::FreshQuote { quote, .. } = &figure.value else {
        panic!("the voided figure is a fresh quote");
    };
    assert_eq!(quote.maker, RAMP);
    // The router's record keeps the dear path unconfirmed, at the ramp, at 990.
    let dear = first
        .candidates
        .iter()
        .find(|c| c.parts.first().operations.first().provider == id(DEAR_RESERVE))
        .unwrap();
    assert!(matches!(
        dear.dropped,
        Some(DropReason::Unconfirmed {
            part: 0,
            operation: 1
        })
    ));

    // Asked again at 990, the ramp confirms, and the dear path is chosen.
    let ramp_amounts: Vec<&str> = sim
        .asked(RAMP)
        .iter()
        .map(|op| op["amount"].as_str().unwrap())
        .collect();
    assert_eq!(ramp_amounts, ["1000", "990"]);
    assert!(
        matches!(routed.offer, Offer::Offered { .. }),
        "{:?}",
        routed.offer
    );
}

// verifies: REQ-CAT-5
#[test]
fn questions_at_one_tuple_at_two_amounts_share_no_transaction_identifier() {
    let inputs = polygon_to_base_inputs();
    let mut env = Env::new(ramp_catalogue(), true, 100);
    let mut sim = env.sim();
    sim.refuse.insert(FREE_RESERVE.into());
    sim.fees.insert(DEAR_RESERVE.into(), 10);
    let routed = run(&mut env, &mut sim, &inputs);

    let at_ramp: Vec<&str> = sim
        .asked(RAMP)
        .iter()
        .map(|op| op["transaction"].as_str().unwrap())
        .collect();
    assert_eq!(at_ramp.len(), 2, "the ramp is asked at both amounts");
    assert_ne!(at_ramp[0], at_ramp[1]);
    // No two questions share one, and none names the order.
    let all: Vec<&str> = sim
        .received
        .iter()
        .map(|(_, op)| op["transaction"].as_str().unwrap())
        .collect();
    let mut distinct = all.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), all.len(), "{all:?}");
    assert!(all.iter().all(|t| !t.contains(ORDER)));
    // Each question was logged before it was sent, about the order and its own transaction.
    for sent in &routed.sent {
        let Entry::Stated(entry) = &env.log.entries[sent.receipt.index.get() as usize] else {
            panic!("a question is a stated entry");
        };
        assert_eq!(entry.attribution.maker().party, id("rutent-operator"));
        assert_eq!(entry.about.as_ref().unwrap().transaction, sent.transaction);
    }

    // A source that draws one identifier twice stops the routing before a second question shares it.
    let mut env = Env::new(ramp_catalogue(), true, 100);
    env.ids = Box::new(SameId);
    let mut sim = env.sim();
    assert_eq!(
        env.route(&mut sim, inputs.routing(), &mut History::new())
            .unwrap_err(),
        RoutingError::Transaction
    );
    assert!(sim.received.len() <= 1);
}

// verifies: REQ-CPL-6, REQ-CAT-5
#[test]
fn provider_off_the_shortlist_is_asked_nothing() {
    const COLD: &str = "polygon-cold-reserve";
    let mut inputs = on_polygon(&[RESERVE, COLD], RESERVE);
    inputs.bounds = excluding(&inputs.bounds, &[COLD]);
    let need = |provider: &str| Need {
        tuple: deposit_tuple(provider),
        person: id("person-1"),
        verifier: id("kyc-verifier"),
        subject: format!("subject-at-{provider}"),
        class: "kyc-basic".into(),
    };
    inputs.order.needs = vec![need(RESERVE), need(COLD)];
    let mut env = Env::new(
        catalogue(&[(RESERVE, "acme"), (COLD, "acme")], 100_000),
        true,
        100,
    );
    let mut sim = env.sim();
    let routed = run(&mut env, &mut sim, &inputs);

    assert!(
        sim.asked(RESERVE).len() >= 2,
        "the shortlisted reserve is asked"
    );
    assert!(sim.asked(COLD).is_empty(), "{:?}", sim.asked(COLD));
    // Eligibility is put to policy, and to the verifier, only for the shortlisted provider's tuple.
    let asked: Vec<_> = env.people.questions.iter().map(|q| q.ask.clone()).collect();
    assert_eq!(asked, vec![vec![id(RESERVE)]]);
    let judged: Vec<_> = sim
        .asked("kyc-verifier")
        .iter()
        .map(|op| op["subject"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(judged, [format!("subject-at-{RESERVE}")]);
    // The shortlist record names who it shortlisted, and the questions' authority is that record.
    let (_, shortlist) = env.records()[0];
    let What::DecisionRecord {
        shortlisted,
        record,
        ..
    } = shortlist
    else {
        unreachable!()
    };
    assert_eq!(shortlisted.as_deref(), Some(&[id(RESERVE)][..]));
    let rutent_contracts::core_internal::Authority::ShortlistedRoute {
        shortlist,
        providers,
    } = &env.people.questions[0].authority
    else {
        panic!("a shortlisted route's authority");
    };
    assert_eq!(
        (shortlist, providers.as_slice()),
        (record, &[id(RESERVE)][..])
    );
    // With no judgement, the tuple that needs one isn't offered.
    assert_eq!(
        routed.offer,
        Offer::NotOffered(NotOffered::NotEligible {
            tuple: deposit_tuple(RESERVE)
        })
    );
}

// verifies: REQ-OPS-1
#[test]
fn questions_fanned_out_count_against_the_callers_bound() {
    let inputs = on_polygon(&[RESERVE, SIBLING], RESERVE);
    let reserves = [(RESERVE, "acme"), (SIBLING, "acme")];

    // Within the bound, every question sent is charged to the caller's party, before any is sent.
    let mut env = Env::new(catalogue(&reserves, 100_000), true, 100);
    let mut sim = env.sim();
    let _ = run(&mut env, &mut sim, &inputs);
    let charged: u64 = env.bound.charges.iter().map(|c| c.count.get()).sum();
    assert_eq!(charged, sim.received.len() as u64);
    assert!(
        env.bound
            .charges
            .iter()
            .all(|c| c.party == customer().party && c.order == Some(id(ORDER)))
    );

    // Over it, none is.
    let mut env = Env::new(catalogue(&reserves, 100_000), true, 3);
    let mut sim = env.sim();
    let routed = run(&mut env, &mut sim, &inputs);
    assert!(sim.received.is_empty(), "{:?}", sim.received);
    assert_eq!(env.bound.charges.len(), 1);
    assert_eq!(env.bound.charges[0].count.get(), 4);
    assert!(matches!(
        routed.offer,
        Offer::NotOffered(NotOffered::OverBound { .. })
    ));
}

// verifies: REQ-AUD-4
#[test]
fn every_routing_calls_decision_record_is_chained_to_the_orders_history() {
    let inputs = polygon_to_base_inputs();
    let mut env = Env::new(ramp_catalogue(), true, 100);
    let mut sim = env.sim();
    sim.refuse.insert(FREE_RESERVE.into());
    sim.fees.insert(DEAR_RESERVE.into(), 10);
    let mut history = History::new();
    let first = env.route(&mut sim, inputs.routing(), &mut history).unwrap();
    // And the order routed again, as at a recalculation, under the same history.
    let second = env.route(&mut sim, inputs.routing(), &mut history).unwrap();

    let records = env.records();
    assert_eq!(records.len(), 2 * (1 + first.choices.len()));
    assert_eq!(second.choices.len(), first.choices.len());
    let mut before: Option<usize> = None;
    for (at, what) in &records {
        let What::DecisionRecord {
            order, previous, ..
        } = what
        else {
            unreachable!()
        };
        assert_eq!(order, &id(ORDER));
        assert_eq!(
            previous.as_ref().map(|r| r.index.get() as usize),
            before,
            "the entry at {at}"
        );
        before = Some(*at);
    }
    assert_eq!(history.previous().map(|r| r.index.get() as usize), before);
    // Each entry names its round's record by id and digest.
    let rounds = std::iter::once(&first.shortlist)
        .chain(&first.choices)
        .chain(std::iter::once(&second.shortlist))
        .chain(&second.choices);
    for ((_, what), recorded) in records.iter().zip(rounds) {
        let What::DecisionRecord {
            decision_record,
            record,
            ..
        } = what
        else {
            unreachable!()
        };
        assert_eq!(decision_record, &recorded.record.request.request_id);
        assert_eq!(record, &recorded.digest);
    }
    assert!(is_round(records[0].1, RoutingRound::Shortlist));
    assert!(
        records[1..=first.choices.len()]
            .iter()
            .all(|(_, w)| is_round(w, RoutingRound::Choice))
    );
}

// verifies: REQ-AUD-4
#[test]
fn refines_names_the_shortlist_record_it_was_taken_from() {
    let inputs = polygon_to_base_inputs();
    let mut env = Env::new(ramp_catalogue(), true, 100);
    let mut sim = env.sim();
    sim.refuse.insert(FREE_RESERVE.into());
    sim.fees.insert(DEAR_RESERVE.into(), 10);
    let routed = run(&mut env, &mut sim, &inputs);

    let shortlist = &routed.shortlist.record;
    assert!(routed.choices.len() > 1, "later rounds refine it too");
    for choice in &routed.choices {
        let Round::Choice { refines } = &choice.record.request.round else {
            panic!("a choice round");
        };
        assert_eq!(refines.request_id, shortlist.request.request_id);
        assert_eq!(refines.taken_at, shortlist.snapshot.taken_at);
        // Core's clock moved on while the providers answered; refines keeps the shortlist's own time.
        assert_ne!(refines.taken_at, choice.record.snapshot.taken_at);
    }
}

// verifies: REQ-AUD-2, REQ-AUD-4
#[test]
fn statement_refused_before_the_record_reaches_the_log_before_the_record_is_formed() {
    // The sibling answers with a key its advertisement doesn't list: the choice snapshot refuses it.
    let inputs = on_polygon(&[RESERVE, SIBLING], RESERVE);
    let reserves = [(RESERVE, "acme"), (SIBLING, "acme")];
    let mut env = Env::new(catalogue(&reserves, 100_000), true, 100);
    let mut sim = env.sim();
    sim.wrong_key.insert(SIBLING.into());
    let routed = run(&mut env, &mut sim, &inputs);

    let refused: Vec<Vec<u8>> = env
        .evidence
        .kept
        .iter()
        .filter(|b| value_of(b)["maker"] == SIBLING)
        .cloned()
        .collect();
    assert!(!refused.is_empty());
    let (choice_at, _) = env
        .records()
        .into_iter()
        .find(|(_, w)| is_round(w, RoutingRound::Choice))
        .unwrap();
    for bytes in &refused {
        let at = env
            .stated_at(bytes)
            .expect("the refused statement is on the record");
        assert!(
            at < choice_at,
            "recorded at {at}, after the record at {choice_at}"
        );
    }
    assert!(
        matches!(routed.offer, Offer::Offered { .. }),
        "{:?}",
        routed.offer
    );

    // Where its record can't be appended, no choice record is formed.
    let mut env = Env::new(catalogue(&reserves, 100_000), true, 100);
    env.evidence.refuse = Some(SIBLING.into());
    let mut sim = env.sim();
    sim.wrong_key.insert(SIBLING.into());
    assert!(matches!(
        env.route(&mut sim, inputs.routing(), &mut History::new()),
        Err(RoutingError::Log(_))
    ));
    assert!(
        !env.records()
            .iter()
            .any(|(_, w)| is_round(w, RoutingRound::Choice))
    );
}

// verifies: REQ-ORD-32
#[test]
fn automatic_deposit_is_routed_without_a_users_yes() {
    let mut inputs = on_polygon(&[RESERVE], RESERVE);
    inputs.order.presence = Presence::AutomaticDeposit {
        session: id(SESSION),
    };
    let mut env = Env::new(catalogue(&[(RESERVE, "acme")], 100_000), true, 100);
    let mut sim = env.sim();
    let routed = run(&mut env, &mut sim, &inputs);
    assert_eq!(
        routed.offer,
        Offer::Offered {
            candidate: last_choice(&routed).chosen.clone().unwrap(),
            reserve_yes: Vec::new(),
            session: None,
        }
    );
}
