//! The one place the rounds reach the log, so the change of the log's attribution (`T-020-01c`: the log, not the
//! writer, attributes a stated entry) is a change here alone.
//!
//! - **A stated entry** is appended as its claimed maker's, `signature: unverified`, always: the rounds never
//!   write `verified`, which is the log's to find (core-internal's README, "Two kinds of entry"). Its bytes go to the
//!   evidence store first, and its `about` names the order and the transaction Core asked under, so the log binds
//!   the answer to the order through the transaction the provider signs (`REQ-ORD-32`, `REQ-ORD-38`).
//! - **A decision record** is appended as a verified `decision_record` entry by Core, chained to the order's
//!   previous one (`REQ-AUD-4`).
//!
//! Every append goes through orchestration's handle as a `GeneralEntry`, which refuses a decision kind.

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    About, Actor, Attribution, Entry, Outcome, Receipt, RoutingRound, ShapeId, SignedWith,
    StatedEntry, StatedKind, StatedOutcome, StatedStatus, StatedTimeSource, StatedWhat, StatedWhen,
    TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::Signature;
use rutent_contracts::router_api::{Id, Timestamp};
use serde::Deserialize;

use crate::snapshot::Clock;

/// The parts of an envelope a stated entry names, read without trusting anything in it.
#[derive(Deserialize)]
struct Claimed {
    shape: String,
    made_at: String,
    signature: Signature,
}

/// Why a statement couldn't be appended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unappended {
    /// Its envelope names no shape, time or key a stated entry can hold: it is recorded nowhere, and used nowhere.
    Unrecordable,
    /// The evidence store or the log refused or failed.
    Append(AppendFailure),
}

/// Keeps `bytes`, a statement as received or as Core signed it, and appends its stated entry, claimed by `maker`,
/// unverified, received at Core's clock, about `about`.
pub(super) fn append_stated(
    log: &mut dyn Log,
    evidence: &mut dyn Evidence,
    clock: &dyn Clock,
    bytes: &[u8],
    maker: Actor,
    about: Option<About>,
) -> Result<Receipt, Unappended> {
    let claimed: Claimed = serde_json::from_slice(bytes).map_err(|_| Unappended::Unrecordable)?;
    let shape = ShapeId::new(claimed.shape).ok_or(Unappended::Unrecordable)?;
    let made_at = Timestamp::new(claimed.made_at).ok_or(Unappended::Unrecordable)?;
    let received_at = clock.now().timestamp().ok_or(Unappended::Unrecordable)?;
    let kept = evidence.keep(bytes).map_err(Unappended::Append)?;
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
    entry
        .check()
        .map_err(|v| Unappended::Append(AppendFailure::Refused(v)))?;
    append(log, entry).map_err(Unappended::Append)
}

/// One round's `decision_record` entry.
pub(super) struct RecordEntry<'a> {
    pub who: &'a Actor,
    pub order: &'a Id,
    pub record_id: &'a Id,
    pub round: RoutingRound,
    pub digest: &'a Digest,
    pub previous: Option<&'a Receipt>,
    pub shortlisted: Option<Vec<Id>>,
    pub verifiers: Vec<Id>,
}

/// Appends one round's recorded decision, by digest, chained to the order's previous decision record.
pub(super) fn append_record(
    log: &mut dyn Log,
    clock: &dyn Clock,
    record: RecordEntry<'_>,
) -> Result<Receipt, AppendFailure> {
    let at = clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(Violation(
            "Core's clock reads a time a timestamp can hold",
        )))?;
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: record.who.clone(),
        what: What::DecisionRecord {
            decision_record: record.record_id.clone(),
            order: record.order.clone(),
            round: record.round,
            record: record.digest.clone(),
            previous: record.previous.cloned(),
            shortlisted: record.shortlisted,
            verifiers: record.verifiers,
        },
        when: When {
            at,
            source: TimeSource::CoreClock,
        },
        outcome: Outcome::Observed {},
    });
    entry.check().map_err(AppendFailure::Refused)?;
    append(log, entry)
}

fn append(log: &mut dyn Log, entry: Entry) -> Result<Receipt, AppendFailure> {
    let general = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
    log.append(&general)
}
