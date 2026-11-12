//! The one place arrivals reach the log. Every entry is built once and appended as built, so an append retried after
//! the log couldn't take it, or after its answer was lost, is the same entry.
//!
//! - **A stated entry** (the reserve's notice, Core's question, the reserve's answer) is appended as its claimed
//!   maker's, `signature: unverified`, always: attributing it is the log's (`T-020-01c`). Its bytes go to the evidence
//!   store when it is built, before it is appended.
//! - **A verified entry** (an `arrival`, a `refiling`, an `order_refusal`, a `destination_check`, a `shown`, an
//!   `acceptance`, the `route_chosen` input) is orchestration's own record, observed at Core's clock.
//!
//! Every append goes through orchestration's handle as a `GeneralEntry`, which refuses a decision kind.

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    About, Actor, Attribution, Entry, Outcome, Receipt, ShapeId, SignedWith, StatedEntry,
    StatedKind, StatedOutcome, StatedStatus, StatedTimeSource, StatedWhat, StatedWhen, TimeSource,
    VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::provider_role::Signature;
use rutent_contracts::router_api::Timestamp;
use serde::Deserialize;

use super::ArrivalError;
use crate::snapshot::Clock;

/// Core's clock now, as an entry's time.
pub(super) fn now(clock: &dyn Clock) -> Result<Timestamp, AppendFailure> {
    clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(Violation(
            "Core's clock reads a time a timestamp can hold",
        )))
}

/// The entry of `what`, observed by `who` at Core's clock, checked.
pub(super) fn build(clock: &dyn Clock, who: &Actor, what: What) -> Result<Entry, AppendFailure> {
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: who.clone(),
        what,
        when: When {
            at: now(clock)?,
            source: TimeSource::CoreClock,
        },
        outcome: Outcome::Observed {},
    });
    entry.check().map_err(AppendFailure::Refused)?;
    Ok(entry)
}

/// The parts of an envelope a stated entry names, read without trusting anything in it.
#[derive(Deserialize)]
struct Claimed {
    shape: String,
    made_at: String,
    signature: Signature,
}

/// Keeps `bytes`, a statement as received or as Core signed it, and builds its stated entry, claimed by `maker`,
/// unverified, received at Core's clock, about `about`.
pub(super) fn stated(
    evidence: &mut dyn Evidence,
    clock: &dyn Clock,
    bytes: &[u8],
    maker: Actor,
    about: Option<About>,
) -> Result<Entry, ArrivalError> {
    let claimed: Claimed = serde_json::from_slice(bytes).map_err(|_| ArrivalError::Unrecordable)?;
    let shape = ShapeId::new(claimed.shape).ok_or(ArrivalError::Unrecordable)?;
    let made_at = Timestamp::new(claimed.made_at).ok_or(ArrivalError::Unrecordable)?;
    let received_at = now(clock)?;
    let kept = evidence.keep(bytes)?;
    let entry = Entry::Stated(StatedEntry {
        kind: StatedKind::Stated,
        attribution: Attribution::Unverified { claimed_by: maker },
        what: StatedWhat { statement: shape },
        when: StatedWhen {
            at: made_at,
            source: StatedTimeSource::PartyStated,
        },
        received_at,
        outcome: StatedOutcome {
            status: StatedStatus::Stated,
        },
        signed_with: SignedWith {
            scheme: claimed.signature.scheme,
            key: claimed.signature.key,
        },
        commitment: kept.commitment,
        about,
        relayed_by: None,
    });
    entry.check().map_err(AppendFailure::Refused)?;
    Ok(entry)
}

/// Appends `entry` as built, through orchestration's handle.
pub(super) fn append(log: &mut dyn Log, entry: &Entry) -> Result<Receipt, AppendFailure> {
    let general = GeneralEntry::try_from(entry.clone()).map_err(AppendFailure::Refused)?;
    log.append(&general)
}

/// Appends `entry` once: the receipt kept where the log took it, the entry appended as built until it does.
pub(super) fn once(
    log: &mut dyn Log,
    entry: &Entry,
    receipt: &mut Option<Receipt>,
) -> Result<Receipt, AppendFailure> {
    if let Some(receipt) = receipt {
        return Ok(receipt.clone());
    }
    let taken = append(log, entry)?;
    *receipt = Some(taken.clone());
    Ok(taken)
}
