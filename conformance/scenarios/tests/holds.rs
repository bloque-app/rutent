//! Signed holds a reserve gave in its own answer, redeemed in intake (`T-020-15`, `T-020-56`), over the assembled Core
//! against the simulated providers: Core asks a reserve to hold the quote it shows only where the reserve's declaration
//! says it answers hold requests, takes the hold its answer carries, and the acceptance redeems it, naming the receipt
//! of the hold's entry; a hold is redeemed by one order only, also after a restart; and the log admits everything it
//! takes. No scenario hands Core a hold: each is the reserve's own, as its module answered the platform.

mod routing_support;

use routing_support::*;
use rutent_contracts::core_internal::{Attribution, Entry, Receipt, What};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{HOLD_SHAPE, OPERATION_SHAPE, RESULT_SHAPE};
use rutent_contracts::router_api::Id;
use rutent_core_orch::intake::{IntakeError, Status, Told};
use rutent_core_orch::quote::HoldError;
use rutent_scenarios::assembly::Core;
use rutent_scenarios::assembly::StepError;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::harness::Scenario;
use rutent_scenarios::kit;
use rutent_scenarios::on_tick::OnTick;

/// The simulated reserve whose declaration says it answers hold requests.
const HOLDING: &str = "sim-reserve-holding";

/// The acceptances the log holds: their order and the hold each cites.
fn acceptances(core: &Core) -> Vec<(Id, Option<Receipt>)> {
    core.verified_whats()
        .into_iter()
        .filter_map(|w| match w {
            What::Acceptance { order, held, .. } => Some((order, held)),
            _ => None,
        })
        .collect()
}

/// An order for the whole amount at `reserve`, quoted: its id and its quote's digest.
fn offered_at(core: &mut Core, reserve: &str, session: &Id, name: &str) -> (Id, Digest) {
    let (order, told) = quoted(
        core,
        request_at(reserve, name, AMOUNT, session, "lowest_total_cost"),
    );
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted: {told:?}");
    };
    (order, digest)
}

/// `core` stopped and opened again on its disk, as the deployment reopens it.
fn reopened(core: Core) -> Core {
    Scenario::new("routing", "AC-23", given(&[HOLDING])).reopen(core, 1, &mut Reference)
}

/// The digest of the quote `told` shows.
fn digest_of(told: &Told) -> Digest {
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted: {told:?}");
    };
    digest.clone()
}

/// The operator's hold requests the log holds, each with the order it is about: Core appends each before it is sent.
fn hold_requests(core: &mut Core) -> Vec<(Id, serde_json::Value)> {
    stated(core, OPERATION_SHAPE)
        .into_iter()
        .filter(|(_, statement)| statement["statement"]["kind"] == "hold")
        .collect()
}

/// Each statement of `shape` the log holds, with the order its entry is about, read back from the evidence store.
fn stated(core: &mut Core, shape: &str) -> Vec<(Id, serde_json::Value)> {
    let entries = core.log().entries().to_vec();
    entries
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Stated(s) if s.what.statement.as_str() == shape => Some((
                s.about.as_ref()?.order.clone()?,
                serde_json::from_slice(&core.kept_bytes(&s.commitment)?).ok()?,
            )),
            _ => None,
        })
        .collect()
}

// verifies: REQ-QTE-15
#[test]
fn the_acceptance_an_assembled_core_records_names_the_redeemed_holds_receipt() {
    let (mut core, session) = core_with("AC-23", &[HOLDING]);
    let (order, digest) = offered_at(&mut core, HOLDING, &session, "req-held");
    let [(hold, receipt)] = core.holds_taken().try_into().unwrap_or_else(|_| {
        panic!("Core took one hold: {:?}", core.holds_taken());
    });

    core.accept(&kit::customer(), &order, &digest)
        .expect("the acceptance redeems the hold");
    assert_eq!(
        acceptances(&core),
        vec![(order.clone(), Some(receipt))],
        "the acceptance names the receipt of the hold's entry"
    );
    assert_eq!(core.holds().redeemed_by(&hold), Some(&order));
}

// verifies: REQ-QTE-15
#[test]
fn a_hold_is_redeemed_by_one_order_only_also_after_a_restart() {
    let (mut core, session) = core_with("AC-23", &[HOLDING]);
    let (first, first_digest) = offered_at(&mut core, HOLDING, &session, "req-first");
    let (second, second_digest) = offered_at(&mut core, HOLDING, &session, "req-second");
    let held = core.holds_taken();
    assert_eq!(held.len(), 2, "each quote shown is held: {held:?}");
    let hold = held[0].0.clone();
    core.accept(&kit::customer(), &first, &first_digest)
        .expect("the first order redeems its hold");
    assert_eq!(core.holds().redeemed_by(&hold), Some(&first));

    // Core stops and is opened again on its disk: it keeps each hold again from its own log (`T-020-57`).
    let mut core = reopened(core);
    assert_eq!(core.holds().redeemed_by(&hold), Some(&first));
    assert!(core.holds().held(&hold).is_some(), "the hold is kept again");

    // The second order, its quote shown again on a new routing after the reopening, accepted in the same session the
    // hold is for, names the first order's hold.
    let _ = second_digest;
    let second_digest = digest_of(&core.quote(&kit::customer(), &second).expect("quoted again"));
    let recorded = acceptances(&core).len();
    let refused = core.accept_holding(&kit::customer(), &second, &second_digest, Some(&hold));
    assert!(
        matches!(
            refused,
            Err(StepError::Intake(IntakeError::Hold(
                HoldError::RedeemedByAnother
            )))
        ),
        "{refused:?}"
    );
    assert_eq!(acceptances(&core).len(), recorded, "nothing was recorded");
}

/// A reserve whose declaration says it answers hold requests is asked to hold the quote Core shows: Core's request,
/// signed and appended before it was sent, names the choice round's record by the digest the log names it by, its
/// chosen candidate, and the session the reserve's yes was asked in; the reserve's answer and the hold nested in it are
/// the log's, the hold its own statement as the platform's signer signed it and the log attributed it. Accepting the
/// quote redeems that hold, naming its entry, and the order starts on the quote held (`REQ-QTE-15`, `T-020-56`).
// verifies: REQ-QTE-15
#[test]
fn hold_a_reserve_gave_in_its_answer_is_redeemed_on_acceptance() {
    let (mut core, session) = core_with("AC-23", &[HOLDING]);
    let (order, digest) = offered_at(&mut core, HOLDING, &session, "req-held");

    let requests = hold_requests(&mut core);
    assert_eq!(requests.len(), 1, "one hold asked: {requests:?}");
    let (about, request) = &requests[0];
    assert_eq!(about, &order, "appended about the order");
    let asked = &request["statement"];
    assert_eq!(asked["provider"], HOLDING);
    assert_eq!(asked["for"]["kind"], "session");
    assert_eq!(asked["for"]["session"], session.as_str());
    let (record, named) = core
        .verified_whats()
        .into_iter()
        .rev()
        .find_map(|w| match w {
            What::DecisionRecord {
                decision_record,
                order: o,
                record,
                ..
            } if o == order => Some((decision_record, record)),
            _ => None,
        })
        .expect("the choice round's record");
    assert_eq!(asked["record"]["id"], record.as_str());
    assert_eq!(asked["record"]["digest"]["value"], named.value.as_str());

    // The reserve's answer, and the hold nested in it, its own and verified by the log.
    let answers: Vec<_> = stated(&mut core, RESULT_SHAPE)
        .into_iter()
        .filter(|(o, s)| o == &order && s["statement"]["kind"] == "hold")
        .collect();
    assert_eq!(answers.len(), 1, "the reserve's answer is the log's");
    assert_eq!(answers[0].1["statement"]["outcome"]["status"], "held");
    let [(hold, receipt)] = core.holds_taken().try_into().expect("one hold taken");
    assert_eq!(asked["transaction"], hold.as_str());
    let entry = core
        .log()
        .entries()
        .iter()
        .find(|(r, _)| r == &receipt)
        .map(|(_, e)| e.clone())
        .expect("the hold's entry");
    let Entry::Stated(entry) = entry else {
        panic!("a stated entry");
    };
    assert_eq!(entry.what.statement.as_str(), HOLD_SHAPE);
    assert!(
        matches!(&entry.attribution, Attribution::Verified { who } if who.acts.as_str() == HOLDING),
        "the reserve's own hold, as the platform signed it: {:?}",
        entry.attribution
    );

    // The acceptance names no hold: it redeems the one Core took of the quote shown.
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted on the hold");
    assert_eq!(acceptances(&core), vec![(order.clone(), Some(receipt))]);
    assert_eq!(core.holds().redeemed_by(&hold), Some(&order));
    core.ran_on_tick(&order)
        .expect("the order runs on the quote held");
}

/// A reserve whose declaration says nothing of hold requests is asked none: quoting at it appends no hold request and
/// takes no hold, the platform hands its module only the dry runs Core asked, and its quote's acceptance redeems
/// nothing. A reserve beside it that declares them is asked, for its own quote alone (`REQ-QTE-15`, `T-020-56`).
// verifies: REQ-QTE-15
#[test]
fn reserve_declaring_no_hold_requests_is_asked_for_none() {
    let (mut core, session) = core_with("AC-23", &[HOLDING]);
    let before = core.operations_received(A);
    let (order, digest) = offered_at(&mut core, A, &session, "req-unheld");
    assert!(hold_requests(&mut core).is_empty(), "no hold asked");
    assert!(core.holds_taken().is_empty(), "no hold taken");
    let asked_of_a = stated(&mut core, OPERATION_SHAPE)
        .into_iter()
        .filter(|(o, s)| o == &order && s["statement"]["provider"] == A)
        .count();
    assert_eq!(
        core.operations_received(A) - before,
        asked_of_a,
        "the platform handed the reserve only the questions Core appended"
    );
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    assert_eq!(acceptances(&core), vec![(order, None)], "nothing redeemed");

    // The reserve that declares them is asked, about its own quote.
    let (held, _) = offered_at(&mut core, HOLDING, &session, "req-held");
    let requests = hold_requests(&mut core);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, held);
}

/// The hold Core takes lasts as long as the quote's figures, never less (`T-020-56`'s review, finding 3): asked until
/// the earliest expiry among the figures its candidate cites, so the quote's acceptance still redeems it once more time
/// has passed than the 300 seconds a quote whose figures name no expiry is held for (`REQ-QTE-15`).
// verifies: REQ-QTE-15
#[test]
fn hold_core_takes_lasts_as_long_as_its_quotes_figures() {
    let (mut core, session) = core_with("AC-23", &[HOLDING]);
    let (order, digest) = offered_at(&mut core, HOLDING, &session, "req-held");
    let [(hold, receipt)] = core.holds_taken().try_into().expect("one hold taken");
    let record = core
        .verified_whats()
        .into_iter()
        .rev()
        .find_map(|w| match w {
            What::DecisionRecord {
                decision_record,
                order: o,
                ..
            } if o == order => Some(decision_record),
            _ => None,
        })
        .expect("the choice round's record");
    let record = record_named(&core, &record);
    let chosen = record.chosen().expect("chosen");
    let expiry = record
        .earliest_expiry(chosen)
        .expect("the stand-in's figures name their expiry");
    let held = core
        .holds()
        .held(&hold)
        .expect("held")
        .valid_until()
        .unix_nanos();
    assert_eq!(
        held,
        expiry.div_euclid(1_000_000_000) * 1_000_000_000,
        "held to the whole second below the earliest expiry of the quote's figures"
    );
    // Ten minutes on, past the 300 seconds a figure naming no expiry is held for, within the customer's session.
    core.clock.advance(600);
    core.accept(&kit::customer(), &order, &digest)
        .expect("the quote's acceptance still redeems its hold");
    assert_eq!(acceptances(&core), vec![(order.clone(), Some(receipt))]);
}

/// The reserve that holds each quote for a minute at most, less than its figures stay valid.
const BRIEF: &str = "sim-reserve-holding-briefly";

/// A reserve holds the quote Core shows for less than the figures its route stands on stay valid: the quote is shown
/// valid until its hold's validity, no later, since its acceptance redeems that hold; past it the acceptance is
/// refused and the order stays quoted, as the quote said (`REQ-QTE-15`,
/// `T-020-57`, the milestone reviewer's check of `T-020-56`).
// verifies: REQ-QTE-15
#[test]
fn quote_held_for_less_than_its_figures_is_valid_no_longer_than_its_hold() {
    let (mut core, session) = core_with("AC-23", &[BRIEF]);
    let (order, told) = quoted(
        &mut core,
        request_at(BRIEF, "req-brief", AMOUNT, &session, "lowest_total_cost"),
    );
    let Told::Exists {
        status:
            Status::Quoted {
                digest,
                valid_until,
                ..
            },
        ..
    } = told
    else {
        panic!("quoted: {told:?}");
    };
    let [(hold, _)] = core.holds_taken().try_into().expect("one hold taken");
    let held_until = core
        .holds()
        .held(&hold)
        .expect("held")
        .valid_until()
        .clone();
    let record = core
        .verified_whats()
        .into_iter()
        .rev()
        .find_map(|w| match w {
            What::DecisionRecord {
                decision_record,
                order: o,
                ..
            } if o == order => Some(decision_record),
            _ => None,
        })
        .expect("the choice round's record");
    let record = record_named(&core, &record);
    let figures = record
        .earliest_expiry(record.chosen().expect("chosen"))
        .expect("the stand-in's figures name their expiry");
    assert!(
        held_until.unix_nanos() < figures,
        "the reserve held it for less than its figures stay valid"
    );
    assert_eq!(
        valid_until.as_ref(),
        Some(&held_until),
        "the quote is shown valid until its hold, no later"
    );

    // Past the hold, within the figures: the acceptance is refused, and the order stays quoted.
    core.clock
        .advance(rutent_conformance::sim::BRIEF_HOLD_SECONDS + 1);
    let refused = core.accept(&kit::customer(), &order, &digest);
    assert!(
        matches!(refused, Err(StepError::Intake(IntakeError::Hold(_)))),
        "{refused:?}"
    );
    assert!(acceptances(&core).is_empty(), "nothing was recorded");
}

/// A quote shown and held before Core stops, its acceptance not in the log: the reopened Core rebuilds it from the log
/// while its validity stands, the same quote under the same digest on the record the log holds to its digest, with the
/// reserve's hold (`T-020-61`, item 7, the milestone reviewer's check of `T-020-57`). The customer's acceptance of the quote
/// shown before the restart is recorded after it, redeeming that hold, and the order starts. Past the quote's validity the
/// quote shown is gone, and the order is quoted again on a new routing (`T-020-46`'s default).
// verifies: NFR-6, REQ-QTE-3
#[test]
fn quote_held_before_a_restart_is_accepted_after_core_reopens_while_its_validity_stands() {
    let (mut core, session) = core_with("AC-23", &[HOLDING]);
    let (order, digest) = offered_at(&mut core, HOLDING, &session, "req-held-restart");
    let (lapsing, lapsing_digest) = offered_at(&mut core, HOLDING, &session, "req-lapsing");
    let held = core.holds_taken();
    assert_eq!(held.len(), 2, "each quote shown is held: {held:?}");
    let (hold, receipt) = held[0].clone();
    let mut core = reopened(core);
    core.accept(&kit::customer(), &order, &digest)
        .expect("the quote shown before the restart is accepted after it");
    assert!(
        acceptances(&core).contains(&(order.clone(), Some(receipt))),
        "the acceptance names the hold the reserve gave before the restart: {:?}",
        acceptances(&core)
    );
    assert_eq!(core.holds().redeemed_by(&hold), Some(&order));
    assert!(core.started(&order).is_some(), "the order starts");
    // Past its validity, the other quote shown is gone: its acceptance is refused, and nothing is recorded.
    core.clock.advance(30 * 86_400);
    let recorded = acceptances(&core).len();
    assert!(
        core.accept(&kit::customer(), &lapsing, &lapsing_digest)
            .is_err(),
        "a quote past its validity isn't accepted"
    );
    assert_eq!(acceptances(&core).len(), recorded, "nothing was recorded");
}
