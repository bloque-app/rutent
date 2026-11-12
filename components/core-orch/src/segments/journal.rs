//! The one place the segments reach the log. Every append goes through orchestration's handle as a `GeneralEntry`,
//! which refuses a decision kind; a stated entry is appended unverified, its claimed maker named and its bytes kept
//! first, and the log attributes it (core-internal's README, "The log, not the writer, attributes a statement").
//!
//! An append the log can't take now (`AppendFailure::Unavailable`) is tried again with the same entry, at most
//! [`APPEND_TRIES`] times in all: the log appends an equal entry once, so a retried append whose first answer was
//! lost gives the same receipt (`INV-8`). A refusal is never retried.

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    About, Actor, Attribution, Entry, Outcome, Receipt, ShapeId, SignedWith, StatedEntry,
    StatedKind, StatedOutcome, StatedStatus, StatedTimeSource, StatedWhat, StatedWhen, TimeSource,
    VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::provider_role::Signature;
use rutent_contracts::router_api::Timestamp;
use serde::Deserialize;

/// How many times in all an append the log can't take now is tried.
pub const APPEND_TRIES: usize = 3;

/// The parts of an envelope a stated entry names, read without trusting anything in it.
#[derive(Deserialize)]
struct Claimed {
    shape: String,
    made_at: String,
    signature: Signature,
}

/// Appends `entry` through orchestration's handle, trying again while the log can't take it now.
pub(super) fn append(log: &mut dyn Log, entry: Entry) -> Result<Receipt, AppendFailure> {
    entry.check().map_err(AppendFailure::Refused)?;
    let general = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
    append_general(log, &general)
}

/// Appends an entry built already, trying again while the log can't take it now.
pub(super) fn append_general(
    log: &mut dyn Log,
    entry: &GeneralEntry,
) -> Result<Receipt, AppendFailure> {
    let mut last = AppendFailure::Unavailable;
    for _ in 0..APPEND_TRIES {
        match log.append(entry) {
            Ok(receipt) => return Ok(receipt),
            Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
            Err(refused) => return Err(refused),
        }
    }
    Err(last)
}

/// One of Core's own records, as a verified entry by `who` at `at`, Core's clock.
pub(super) fn verified(who: &Actor, what: What, at: &Timestamp, outcome: Outcome) -> Entry {
    Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: who.clone(),
        what,
        when: When {
            at: at.clone(),
            source: TimeSource::CoreClock,
        },
        outcome,
    })
}

/// Why a statement wasn't appended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unappended {
    /// Its envelope names no shape, time or key a stated entry can hold: it is recorded nowhere, and counts for
    /// nothing.
    Unrecordable,
    /// The evidence store or the log refused or failed.
    Append(AppendFailure),
}

/// Keeps `bytes`, a statement as received or as Core signed it, and builds its stated entry, claimed by `maker`,
/// unverified, received at `received_at`, about `about`: built once, and appended unchanged however often it is
/// tried, so its bytes are kept once.
pub(super) fn stated_entry(
    evidence: &mut dyn Evidence,
    received_at: &Timestamp,
    bytes: &[u8],
    maker: Actor,
    about: About,
) -> Result<Entry, Unappended> {
    let claimed: Claimed = serde_json::from_slice(bytes).map_err(|_| Unappended::Unrecordable)?;
    let shape = ShapeId::new(claimed.shape).ok_or(Unappended::Unrecordable)?;
    let made_at = Timestamp::new(claimed.made_at).ok_or(Unappended::Unrecordable)?;
    let kept = evidence.keep(bytes).map_err(Unappended::Append)?;
    Ok(Entry::Stated(StatedEntry {
        kind: StatedKind::Stated,
        attribution: Attribution::Unverified { claimed_by: maker },
        what: StatedWhat { statement: shape },
        when: StatedWhen {
            at: made_at,
            source: StatedTimeSource::PartyStated,
        },
        received_at: received_at.clone(),
        outcome: StatedOutcome {
            status: StatedStatus::Stated,
        },
        signed_with: SignedWith {
            scheme: claimed.signature.scheme,
            key: claimed.signature.key,
        },
        commitment: kept.commitment,
        about: Some(about),
        relayed_by: None,
    }))
}

/// The refusal of a time Core's clock reads that no timestamp holds.
pub(super) const NO_TIME: Violation = Violation("Core's clock reads a time a timestamp can hold");
