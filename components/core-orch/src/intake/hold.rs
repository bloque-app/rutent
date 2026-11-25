//! A reserve's hold of the quote intake shows, asked for by Core (`REQ-QTE-15`; `T-020-56`, finding 4 of `M3`'s
//! milestone review).
//!
//! Once a quote is shown, intake asks the reserve its route starts at to hold it, and only a reserve whose latest
//! declaration says it answers hold requests (provider-role 4.0.0's `answers_hold_requests`): one that says nothing is
//! asked nothing, and its quote is shown unheld. The request is the operator's `hold` operation, under a transaction of
//! its own, naming the choice round's decision record by its id and the digest the log names it by, its chosen
//! candidate, the session the reserve's yes was asked in (which tells the reserve nothing it wasn't told), and a validity
//! no later than the earliest expiry of the figures the candidate cites. It is appended before it is sent, and the
//! answer after it arrives ([`crate::rounds`]' lone question), as a round's are. The answer is held to the request with
//! provider-role's `check_hold_answer`, both signed statements, and the hold it carries is taken through
//! [`Holds::take`](crate::quote::Holds::take): kept, appended as its own stated entry about its own transaction, and
//! held to the record and its chosen candidate. Intake keeps its transaction with the quote shown, and the acceptance of
//! that quote redeems it ([`super::Intake::accept_holding`]).
//!
//! Every step fails closed into an unheld quote: a reserve that refuses, answers another request, holds another quote
//! or for longer, or a hold the log or the store can't take, leaves the quote shown as it was, with no hold for an
//! acceptance to redeem. A held quote binds no provider (`REQ-QTE-15`), so a hold the reserve gave that Core didn't take
//! has no effect beyond it.

use rutent_contracts::core_internal::{Actor, Role};
use rutent_contracts::provider_role::{
    DigestAlgorithm, HeldFor, HeldRecord, HoldOutcome, HoldRequest, Operation, RecordDigest,
    ResultBody, Statement, check_hold_answer,
};
use rutent_contracts::router_api::{Candidate, Id, Timestamp};
use rutent_prov::catalogue::Instant;

use super::Desk;
use crate::quote::{HoldDesk, RecordedAs};
use crate::rounds::{self, Recorded as Round};

/// How long Core asks a reserve to hold a quote whose figures name no expiry, in seconds: the default
/// `T-020-56` takes, capped at the earliest expiry of the figures the candidate cites where they name one.
pub const HOLD_SECONDS: u64 = 300;

/// Whether the reserve `origin`'s latest declaration says it answers hold requests (`REQ-QTE-15`): one with no
/// declaration on record never said so.
pub(super) fn declared(desk: &Desk<'_, '_>, origin: &Id) -> bool {
    desk.parts
        .catalogue
        .declarations()
        .records_of(origin.as_str())
        .last()
        .is_some_and(|d| d.answers_hold_requests)
}

/// Asks `origin`, the reserve each part of `candidate` starts at, to hold the quote `chosen` chose for `order`, shown in
/// `session`, where its declaration says it answers hold requests; the hold's transaction once Core holds it, and
/// `None` wherever it doesn't (see the module's docs).
pub(super) fn ask(
    desk: &mut Desk<'_, '_>,
    order: &Id,
    origin: &Id,
    chosen: &Round,
    candidate: &Candidate,
    session: &Id,
) -> Option<Id> {
    // Only the reserve each part starts at holds its quote, and only the origin reserve redeems it.
    if candidate
        .parts
        .iter()
        .any(|part| &part.operations.first().provider != origin)
        || !declared(desk, origin)
    {
        return None;
    }
    let valid_until = validity(desk, chosen, candidate)?;
    let transaction = rounds::fresh_transaction(desk.parts, order).ok()?;
    let request = Operation::Hold(HoldRequest {
        transaction: transaction.as_str().to_owned(),
        provider: origin.as_str().to_owned(),
        record: HeldRecord {
            id: chosen.record.record_id().as_str().to_owned(),
            digest: RecordDigest {
                algorithm: DigestAlgorithm::Sha256,
                value: chosen.digest.value.as_str().to_owned(),
            },
        },
        candidate: candidate.candidate_id.as_str().to_owned(),
        held_for: HeldFor::Session {
            session: session.as_str().to_owned(),
        },
        valid_until: valid_until.as_str().to_owned(),
    });
    let asked = rounds::ask_alone(
        desk.parts,
        desk.bounds,
        desk.operator,
        order,
        origin,
        &transaction,
        request,
    )
    .ok()?;
    let question: Statement<Operation> = serde_json::from_slice(&asked.question).ok()?;
    let answer: Statement<ResultBody> = serde_json::from_slice(asked.answer.as_deref()?).ok()?;
    check_hold_answer(&question, &answer).ok()?;
    let ResultBody::Hold(held) = &answer.statement else {
        return None;
    };
    let HoldOutcome::Held(held) = &held.outcome else {
        return None;
    };
    let bytes = serde_json::to_vec(&*held.hold).ok()?;
    let until = Timestamp::new(held.hold.statement.valid_until.as_str())?;
    let presented_by = reserve(desk, origin);
    let hold_desk = HoldDesk {
        evidence: &mut *desk.parts.evidence,
        log: &mut *desk.parts.log,
        recorded: desk.recorded,
        clock: desk.parts.clock,
        catalogue: desk.parts.catalogue,
        presented_by: &presented_by,
    };
    let taken = desk
        .holds
        .take(
            hold_desk,
            &bytes,
            RecordedAs {
                id: chosen.record.record_id(),
                digest: &chosen.digest,
            },
            chosen.record.clone(),
            until,
        )
        .ok()?;
    Some(taken.transaction.clone())
}

/// The validity Core asks: [`HOLD_SECONDS`] from now, to the whole second, and no later than the earliest expiry of
/// the figures `candidate` cites; `None` where that is already past, so nothing is held.
fn validity(desk: &Desk<'_, '_>, chosen: &Round, candidate: &Candidate) -> Option<Timestamp> {
    let now = desk.parts.clock.now();
    let asked = now.seconds.checked_add(HOLD_SECONDS)?;
    let until = match chosen.record.earliest_expiry(candidate) {
        Some(expiry) => asked.min(u64::try_from(expiry.div_euclid(1_000_000_000)).ok()?),
        None => asked,
    };
    if until <= now.seconds {
        return None;
    }
    Timestamp::new(Instant(until).rfc3339()?)
}

/// The reserve as the actor that hands its hold to Core, through the platform that ran its module.
fn reserve(desk: &Desk<'_, '_>, origin: &Id) -> Actor {
    crate::relay::presenter(desk.parts.catalogue, origin, Role::Provider)
}
