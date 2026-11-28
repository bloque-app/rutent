//! Signed holds wired into intake (`T-020-15`): an acceptance names the hold of the quote it accepts and redeems it,
//! for its order only, in the session intake verified for it, after the statement kept behind the hold's entry is held
//! to the entry's commitment; the receipt of the hold's entry is the acceptance's `held`. Which order redeemed which
//! hold is rebuilt from the log when Core is reopened. Every log a test writes is replayed as the log itself would
//! admit it.

mod intake_support;
mod rounds_support;
mod snapshot_support;

use intake_support::*;
use rounds_support::{Sim, customer};
use rutent_contracts::core_internal::calls::{GeneralEntry, Log};
use rutent_contracts::core_internal::{
    About, Actor, ActorKind, Attribution, Entry, Receipt, Role, ShapeId, SignedWith, StatedEntry,
    StatedKind, StatedOutcome, StatedStatus, StatedTimeSource, StatedWhat, StatedWhen, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    HOLD_SHAPE, Hold, RedemptionRefused, SignatureScheme, Statement,
};
use rutent_contracts::router_api::{DecisionRecord, Id, Timestamp};
use rutent_core_orch::intake::{IntakeError, Status, Told};
use rutent_core_orch::quote::{HoldError, Holds, RecordedAs, Signed};
use rutent_prov::catalogue::Instant;
use rutent_router::quote::HeldQuoteRefused;
use serde_json::{Value, json};
use snapshot_support::{RESERVE, deployment, envelope, id, sign, statements_key};

/// The log replays as the log itself would admit it.
fn assert_admitted(env: &Env) {
    if let Err((place, why)) = env.admitted() {
        panic!("the log refuses entry {place}: {why}");
    }
}

/// A reserve's hold of a quote shown, as it sits in the log and as Core was handed it.
struct Held {
    /// The hold's transaction.
    transaction: Id,
    /// The statement Core was handed.
    statement: Statement<Hold>,
    /// Its stated entry, as the log attributed it.
    stated: StatedEntry,
    receipt: Receipt,
    record: DecisionRecord,
    record_id: Id,
    record_digest: Digest,
    /// Until when the hold is valid, in seconds.
    until: u64,
}

impl Held {
    fn signed(&self) -> Signed<'_> {
        Signed {
            hold: &self.statement,
            stated: &self.stated,
            receipt: &self.receipt,
        }
    }

    fn recorded(&self) -> RecordedAs<'_> {
        RecordedAs {
            id: &self.record_id,
            digest: &self.record_digest,
        }
    }

    /// Core keeps the hold.
    fn kept_by(&self, holds: &mut Holds) {
        holds
            .hold(
                self.signed(),
                self.recorded(),
                self.record.clone(),
                Timestamp::new(Instant(self.until).rfc3339().unwrap()).unwrap(),
            )
            .expect("Core keeps the reserve's hold");
    }
}

/// The decision record the quote of `order` was chosen on, as the rounds signed and kept it, and the id and digest the
/// log names it by.
fn record_of(env: &Env, order: &Id) -> (DecisionRecord, Id, Digest) {
    let book = env.book.borrow();
    let (id, digest) = book
        .verified()
        .into_iter()
        .rev()
        .find_map(|what| match what {
            What::DecisionRecord {
                decision_record,
                record,
                order: of,
                ..
            } if of == order => Some((decision_record.clone(), record.clone())),
            _ => None,
        })
        .expect("the routing recorded its decision");
    let record = env
        .key
        .signed
        .iter()
        .rev()
        .find_map(|bytes| {
            let value: Value = serde_json::from_slice(bytes).ok()?;
            let record: DecisionRecord =
                serde_json::from_value(value.get("statement")?.clone()).ok()?;
            (record.record_id() == &id).then_some(record)
        })
        .expect("the operator signed the record");
    (record, id, digest)
}

/// The reserve's signed hold of `order`'s quote, `held_for`, until `until` seconds; the bytes it signed.
fn signed_hold(
    record: &DecisionRecord,
    record_id: &Id,
    digest: &Digest,
    transaction: &str,
    held_for: Value,
    until: u64,
) -> Vec<u8> {
    let chosen = record.chosen().expect("the record chose a candidate");
    let body = json!({
        "transaction": transaction,
        "record": { "id": record_id, "digest": { "algorithm": "sha-256", "value": digest.value } },
        "candidate": chosen.candidate_id,
        "for": held_for,
        "valid_until": Instant(until).rfc3339().unwrap(),
    });
    sign(
        envelope(&deployment(), HOLD_SHAPE, RESERVE, 1_900, body),
        &statements_key(RESERVE, 1),
    )
}

/// Keeps `bytes`, a hold of the reserve's, in the evidence store and appends its stated entry about its own
/// transaction and no order: the receipt, and the entry as the log attributed it.
fn log_hold(env: &mut Env, bytes: &[u8], transaction: &str) -> (Receipt, StatedEntry) {
    use rutent_contracts::core_internal::calls::Evidence;
    let kept = env.evidence.keep(bytes).expect("the store keeps it");
    let key: Value = serde_json::from_slice(bytes).unwrap();
    let entry = Entry::Stated(StatedEntry {
        kind: StatedKind::Stated,
        attribution: Attribution::Unverified {
            claimed_by: Actor {
                party: id("acme"),
                acts: id(RESERVE),
                role: Role::Provider,
                kind: ActorKind::Component,
            },
        },
        what: StatedWhat {
            statement: ShapeId::new(HOLD_SHAPE).unwrap(),
        },
        when: StatedWhen {
            at: Timestamp::new(Instant(1_900).rfc3339().unwrap()).unwrap(),
            source: StatedTimeSource::PartyStated,
        },
        received_at: Timestamp::new(Instant(START).rfc3339().unwrap()).unwrap(),
        outcome: StatedOutcome {
            status: StatedStatus::Stated,
        },
        signed_with: SignedWith {
            scheme: SignatureScheme::Ed25519,
            key: serde_json::from_value(key["signature"]["key"].clone()).unwrap(),
        },
        commitment: kept.commitment,
        about: Some(About {
            order: None,
            segment: None,
            transaction: Some(id(transaction)),
            node: None,
        }),
        relayed_by: None,
    });
    let receipt = env
        .log
        .append(&GeneralEntry::try_from(entry).expect("a stated entry"))
        .expect("the log takes the hold");
    let recorded = env.history();
    let (_, Entry::Stated(stated)) = &recorded[receipt.index.get() as usize] else {
        panic!("the hold's entry is a stated one");
    };
    assert!(
        matches!(stated.attribution, Attribution::Verified { .. }),
        "the log attributes the reserve's hold"
    );
    (receipt, stated.clone())
}

/// The reserve holds `order`'s quote, for `held_for`, and Core is handed its signed hold; kept in `env.holds`
/// unless `keep` says not.
fn held_quote(env: &mut Env, order: &Id, transaction: &str, held_for: Value, keep: bool) -> Held {
    let (record, record_id, record_digest) = record_of(env, order);
    let chosen = record.chosen().unwrap();
    // As long as the figures the route stands on allow.
    let until = (record
        .earliest_expiry(chosen)
        .unwrap_or(1_000_000_000 * 40_000)
        / 1_000_000_000) as u64;
    let bytes = signed_hold(
        &record,
        &record_id,
        &record_digest,
        transaction,
        held_for,
        until,
    );
    let (receipt, stated) = log_hold(env, &bytes, transaction);
    let held = Held {
        transaction: id(transaction),
        statement: serde_json::from_slice(&bytes).expect("a hold statement"),
        stated,
        receipt,
        record,
        record_id,
        record_digest,
        until,
    };
    if keep {
        held.kept_by(&mut env.holds);
    }
    held
}

fn for_order(order: &Id) -> Value {
    json!({ "kind": "order", "order": order })
}

fn for_session(session: &str) -> Value {
    json!({ "kind": "session", "session": session })
}

/// The acceptances of the log.
fn held_of_acceptances(env: &Env) -> Vec<Option<Receipt>> {
    env.acceptances()
        .into_iter()
        .map(|a| match a.what {
            What::Acceptance { held, .. } => held,
            _ => unreachable!(),
        })
        .collect()
}

/// A request quoted: its order and the digest of the quote shown.
fn quoted(env: &mut Env, sim: &mut Sim, request: &str) -> (Id, Digest) {
    env.quoted(sim, &order_request(request, customer()))
}

fn is_quoted(env: &mut Env, request: &str) -> bool {
    matches!(
        env.told_customer(request),
        Some(Told::Exists {
            status: Status::Quoted { .. },
            ..
        })
    )
}

fn hold_error(answer: Result<Told, IntakeError>) -> HoldError {
    match answer {
        Err(IntakeError::Hold(error)) => error,
        other => panic!("the hold was not refused: {other:?}"),
    }
}

/// An acceptance of a quote a reserve holds redeems the hold it names: the hold of the quote shown, for the order,
/// is bound to it, and the acceptance cites the receipt of the hold's entry as the hold it redeems, which the log
/// admits (`acceptance-cites-hold`'s reading) with the rest.
// verifies: REQ-QTE-15
#[test]
fn accepting_a_quote_held_by_a_reserve_redeems_the_hold_into_the_acceptance() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = quoted(&mut env, &mut sim, "req-held");
    let hold = held_quote(&mut env, &order, "hold-1", for_order(&order), true);

    let told = env
        .with(&mut sim, |intake, desk| {
            intake.accept_holding(
                desk,
                &order,
                &customer(),
                &digest,
                None,
                Some(&hold.transaction),
            )
        })
        .expect("the acceptance redeems the hold");
    assert!(matches!(
        told,
        Told::Exists {
            status: Status::Accepted { .. },
            ..
        }
    ));
    assert_eq!(
        held_of_acceptances(&env),
        vec![Some(hold.receipt.clone())],
        "the acceptance cites the receipt of the hold's stated entry"
    );
    assert_eq!(env.holds.redeemed_by(&hold.transaction), Some(&order));
    assert_eq!(env.kept.asked, 1, "the kept bytes were read once");
    assert_admitted(&env);
}

/// A hold is redeemed by one order only: a second order that names it is refused before anything is recorded for
/// it, though it is the same session's.
// verifies: REQ-QTE-15
#[test]
fn a_hold_is_redeemed_by_one_order_only() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (first, first_digest) = quoted(&mut env, &mut sim, "req-first");
    let (second, second_digest) = quoted(&mut env, &mut sim, "req-second");
    // A hold for the session, so either order of it may redeem it: the first to do so.
    let hold = held_quote(&mut env, &first, "hold-1", for_session(SESSION), true);

    env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &first,
            &customer(),
            &first_digest,
            None,
            Some(&hold.transaction),
        )
    })
    .expect("the first order redeems it");
    let recorded = env.len();
    let refused = env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &second,
            &customer(),
            &second_digest,
            None,
            Some(&hold.transaction),
        )
    });
    assert_eq!(hold_error(refused), HoldError::RedeemedByAnother);
    assert_eq!(
        env.len(),
        recorded + destination_checks_since(&env, recorded),
        "nothing but the destination's check was recorded for the second"
    );
    assert_eq!(env.holds.redeemed_by(&hold.transaction), Some(&first));
    assert!(
        is_quoted(&mut env, "req-second"),
        "its order still waits on its quote"
    );
    assert_eq!(held_of_acceptances(&env).len(), 1);
    assert_admitted(&env);
}

/// A hold made for one session is not redeemed in another: the session handed to the redemption is the one intake
/// verified for the acceptance, never one the caller names, so a hold for another session is refused, and one for the
/// session the order was accepted in is not.
// verifies: REQ-QTE-15
#[test]
fn hold_redeemed_in_another_session_than_the_one_it_was_made_for_is_refused() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = quoted(&mut env, &mut sim, "req-held");
    let elsewhere = held_quote(
        &mut env,
        &order,
        "hold-elsewhere",
        for_session("session-9"),
        true,
    );

    let refused = env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &order,
            &customer(),
            &digest,
            None,
            Some(&elsewhere.transaction),
        )
    });
    assert_eq!(
        hold_error(refused),
        HoldError::NotForThisOrder(RedemptionRefused::ForAnotherSession)
    );
    assert!(
        held_of_acceptances(&env).is_empty(),
        "no acceptance is recorded"
    );
    assert_eq!(env.holds.redeemed_by(&elsewhere.transaction), None);
    assert!(is_quoted(&mut env, "req-held"));

    // A hold for another order is refused alike.
    let theirs = held_quote(
        &mut env,
        &order,
        "hold-theirs",
        for_order(&id("ord-someone")),
        true,
    );
    let refused = env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &order,
            &customer(),
            &digest,
            None,
            Some(&theirs.transaction),
        )
    });
    assert_eq!(
        hold_error(refused),
        HoldError::NotForThisOrder(RedemptionRefused::ForAnotherOrder)
    );

    // The session the order is accepted in redeems a hold for it.
    let ours = held_quote(&mut env, &order, "hold-ours", for_session(SESSION), true);
    env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &order,
            &customer(),
            &digest,
            None,
            Some(&ours.transaction),
        )
    })
    .expect("a hold for the session of the acceptance is redeemed");
    assert_eq!(env.holds.redeemed_by(&ours.transaction), Some(&order));
    assert_admitted(&env);
}

/// A hold whose kept statement is not what its entry commits to is not redeemed: the bytes Core was handed are not
/// the ones behind the entry, the store keeps nothing there, or it answers other bytes than it kept.
// verifies: REQ-QTE-15, REQ-AUD-2
#[test]
fn hold_whose_kept_statement_does_not_match_its_entrys_commitment_is_not_redeemed() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = quoted(&mut env, &mut sim, "req-held");

    // The entry in the log commits to a hold for the session, and Core is handed the signed hold for the order:
    // both the reserve's, both valid, only one behind the entry.
    let (record, record_id, record_digest) = record_of(&env, &order);
    let until = (record
        .earliest_expiry(record.chosen().unwrap())
        .unwrap_or(1_000_000_000 * 40_000)
        / 1_000_000_000) as u64;
    let behind = signed_hold(
        &record,
        &record_id,
        &record_digest,
        "hold-1",
        for_session("session-9"),
        until,
    );
    let handed = signed_hold(
        &record,
        &record_id,
        &record_digest,
        "hold-1",
        for_order(&order),
        until,
    );
    let (receipt, stated) = log_hold(&mut env, &behind, "hold-1");
    let swapped = Held {
        transaction: id("hold-1"),
        statement: serde_json::from_slice(&handed).unwrap(),
        stated,
        receipt,
        record,
        record_id,
        record_digest,
        until,
    };
    swapped.kept_by(&mut env.holds);
    let refused = env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &order,
            &customer(),
            &digest,
            None,
            Some(&swapped.transaction),
        )
    });
    assert_eq!(
        hold_error(refused),
        HoldError::NotWhatIsKept,
        "other bytes than the entry's"
    );
    assert_eq!(env.holds.redeemed_by(&swapped.transaction), None);

    // The honest hold: the store keeps nothing, then answers bytes it did not keep.
    let honest = held_quote(&mut env, &order, "hold-2", for_order(&order), true);
    for (forgets, alters, why) in [
        (true, false, "keeps nothing"),
        (false, true, "answers other bytes"),
    ] {
        env.kept.forgets = forgets;
        env.kept.alters = alters;
        let refused = env.with(&mut sim, |intake, desk| {
            intake.accept_holding(
                desk,
                &order,
                &customer(),
                &digest,
                None,
                Some(&honest.transaction),
            )
        });
        assert_eq!(
            hold_error(refused),
            HoldError::NotWhatIsKept,
            "the store {why}"
        );
        assert_eq!(
            env.holds.redeemed_by(&honest.transaction),
            None,
            "the store {why}"
        );
    }
    env.kept.forgets = false;
    env.kept.alters = false;
    assert!(
        held_of_acceptances(&env).is_empty(),
        "no acceptance was recorded"
    );

    env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &order,
            &customer(),
            &digest,
            None,
            Some(&honest.transaction),
        )
    })
    .expect("the hold whose kept bytes are its entry's is redeemed");
    assert_admitted(&env);
}

/// A restarted Core does not lean on the log to refuse a second order: which order redeemed which hold is rebuilt
/// from the log's acceptances, and the hold, kept again, meets the order that redeemed it.
// verifies: REQ-QTE-15
#[test]
fn redemption_is_rebuilt_from_the_log_when_core_is_reopened() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (first, first_digest) = quoted(&mut env, &mut sim, "req-first");
    let (second, second_digest) = quoted(&mut env, &mut sim, "req-second");
    let hold = held_quote(&mut env, &first, "hold-1", for_session(SESSION), true);
    env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &first,
            &customer(),
            &first_digest,
            None,
            Some(&hold.transaction),
        )
    })
    .expect("the first order redeems it");

    // Core is reopened: its holds are gone, its log is not.
    let mut reopened = Holds::recorded(&env.history());
    assert_eq!(reopened.redeemed_by(&hold.transaction), Some(&first));
    assert_eq!(
        Holds::recorded(&[]).redeemed_by(&hold.transaction),
        None,
        "a log without the acceptance rebuilds nothing"
    );
    // The deployment keeps the hold again, and a second order names it.
    hold.kept_by(&mut reopened);
    env.holds = reopened;
    let recorded = env.len();
    let refused = env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &second,
            &customer(),
            &second_digest,
            None,
            Some(&hold.transaction),
        )
    });
    assert_eq!(
        hold_error(refused),
        HoldError::RedeemedByAnother,
        "refused by Core, before the log is asked"
    );
    assert_eq!(
        env.len(),
        recorded + destination_checks_since(&env, recorded)
    );
    assert_eq!(env.holds.redeemed_by(&hold.transaction), Some(&first));
    assert_admitted(&env);
}

/// A hold past its validity redeems nothing: the acceptance naming it is refused, nothing is recorded and the hold is
/// bound to no order, at the validity it is still usable.
// verifies: REQ-QTE-15
#[test]
fn acceptance_on_a_hold_past_its_validity_redeems_nothing() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = quoted(&mut env, &mut sim, "req-held");
    let hold = held_quote(&mut env, &order, "hold-1", for_order(&order), true);
    assert!(
        hold.until < SESSION_ENDS && hold.until > START,
        "the hold ends inside the session: {}",
        hold.until
    );

    env.clock.set(hold.until + 1);
    let recorded = env.len();
    let refused = env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &order,
            &customer(),
            &digest,
            None,
            Some(&hold.transaction),
        )
    });
    assert!(
        matches!(
            hold_error(refused),
            HoldError::Redeem(HeldQuoteRefused::PastValidity { .. })
        ),
        "past its validity"
    );
    assert_eq!(env.holds.redeemed_by(&hold.transaction), None);
    assert!(held_of_acceptances(&env).is_empty());
    assert_eq!(
        env.len(),
        recorded + destination_checks_since(&env, recorded)
    );

    // At its validity it is usable.
    env.clock.set(hold.until);
    env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &order,
            &customer(),
            &digest,
            None,
            Some(&hold.transaction),
        )
    })
    .expect("at its validity the hold is redeemed");
    assert_eq!(held_of_acceptances(&env), vec![Some(hold.receipt.clone())]);
    assert_admitted(&env);
}

/// What the refused acceptance recorded besides the acceptance: the destination's check it resolved first.
fn destination_checks_since(env: &Env, from: usize) -> usize {
    let book = env.book.borrow();
    book.entries[from..]
        .iter()
        .filter(|(_, e)| matches!(e, Entry::Verified(v) if matches!(v.what, What::DestinationCheck { .. })))
        .count()
}

/// An acceptance that names no hold leaves every hold unredeemed, though a hold of the very quote it accepts is kept,
/// and cites none.
// verifies: REQ-QTE-15
#[test]
fn an_acceptance_naming_no_hold_leaves_every_hold_unredeemed() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = quoted(&mut env, &mut sim, "req-held");
    let for_it = held_quote(&mut env, &order, "hold-1", for_order(&order), true);
    let for_session = held_quote(&mut env, &order, "hold-2", for_session(SESSION), true);

    env.accept(&mut sim, &order, &customer(), &digest)
        .expect("accepted on the quote shown");
    assert_eq!(held_of_acceptances(&env), vec![None], "it cites no hold");
    assert_eq!(env.holds.redeemed_by(&for_it.transaction), None);
    assert_eq!(env.holds.redeemed_by(&for_session.transaction), None);
    assert_eq!(env.kept.asked, 0, "no kept statement was read");
    assert_admitted(&env);
}

/// A hold of another quote is not redeemed by an acceptance of this one, whoever it is for, and a hold nobody kept is
/// not redeemed at all.
// verifies: REQ-QTE-15
#[test]
fn a_hold_of_another_quote_or_none_kept_is_not_redeemed() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (first, _) = quoted(&mut env, &mut sim, "req-first");
    let (second, second_digest) = quoted(&mut env, &mut sim, "req-second");
    let hold = held_quote(&mut env, &first, "hold-1", for_session(SESSION), true);

    let refused = env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &second,
            &customer(),
            &second_digest,
            None,
            Some(&hold.transaction),
        )
    });
    assert_eq!(hold_error(refused), HoldError::NotThisQuote);
    let refused = env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &second,
            &customer(),
            &second_digest,
            None,
            Some(&id("hold-none")),
        )
    });
    assert_eq!(hold_error(refused), HoldError::NoSuchHold);
    assert!(held_of_acceptances(&env).is_empty());
    assert_eq!(env.holds.redeemed_by(&hold.transaction), None);
}

/// What a Core that reopens reads beside its log, here: the decision records the operator signed, held to the digest
/// the log names them by, and the statements the evidence store keeps.
struct Reading<'a> {
    env: &'a Env,
}

impl rutent_core_orch::segments::Reopening for Reading<'_> {
    fn record(&self, id: &Id) -> Option<DecisionRecord> {
        let book = self.env.book.borrow();
        let named = book.verified().into_iter().find_map(|what| match what {
            What::DecisionRecord {
                decision_record, ..
            } => (decision_record == id).then_some(()),
            _ => None,
        });
        named?;
        self.env.key.signed.iter().rev().find_map(|bytes| {
            let value: Value = serde_json::from_slice(bytes).ok()?;
            let record: DecisionRecord =
                serde_json::from_value(value.get("statement")?.clone()).ok()?;
            (record.record_id() == id).then_some(record)
        })
    }

    fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>> {
        if self.env.kept.forgets {
            return None;
        }
        let store = self.env.kept.store.borrow();
        let found = kept_as(&store, commitment, false)?;
        store
            .kept
            .iter()
            .find(|bytes| snapshot_support::base64url(bytes) == found.bytes.as_str())
            .cloned()
    }
}

/// Core keeps a reserve's hold of the quote it shows, and stops: its holds are gone, its log and evidence store are
/// not. Reopened, Core keeps the hold again from what the log and the store hold (`Holds::reopened`), never a copy
/// the deployment keeps, and the acceptance of the quote held redeems it, citing the hold's entry (`REQ-QTE-15`,
/// `NFR-6`, `T-020-57`). A store that can't give the hold's bytes back keeps nothing again (failing closed).
// verifies: REQ-QTE-15, NFR-6
#[test]
fn a_hold_kept_before_a_restart_is_redeemed_after_it() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = quoted(&mut env, &mut sim, "req-held");
    let hold = held_quote(&mut env, &order, "hold-1", for_session(SESSION), true);

    // Core stops before the acceptance: its holds are gone.
    env.holds = Holds::new();
    let history = env.history();
    let forgetting = {
        env.kept.forgets = true;
        let rebuilt = Holds::reopened(&history, &mut Reading { env: &env });
        env.kept.forgets = false;
        rebuilt
    };
    assert!(
        forgetting.held(&hold.transaction).is_none(),
        "a hold whose bytes can't be read back is kept no more"
    );
    let reopened = Holds::reopened(&history, &mut Reading { env: &env });
    let kept = reopened
        .held(&hold.transaction)
        .expect("Core keeps the hold again from its log");
    assert_eq!(kept.hold, hold.receipt, "the hold's own entry");
    assert_eq!(
        kept.valid_until(),
        &Timestamp::new(Instant(hold.until).rfc3339().unwrap()).unwrap()
    );
    env.holds = reopened;

    env.with(&mut sim, |intake, desk| {
        intake.accept_holding(
            desk,
            &order,
            &customer(),
            &digest,
            None,
            Some(&hold.transaction),
        )
    })
    .expect("the hold kept before the restart is redeemed after it");
    assert_eq!(held_of_acceptances(&env), vec![Some(hold.receipt.clone())]);
    assert_eq!(env.holds.redeemed_by(&hold.transaction), Some(&order));
    assert_admitted(&env);
}
