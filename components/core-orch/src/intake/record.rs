//! The one place intake reaches the log. Every entry it writes is orchestration's own record, a verified entry
//! appended through orchestration's handle as a `GeneralEntry`, which refuses a decision kind: an `order_refusal`, a
//! `destination_check`, a `shown`, an `acceptance` and the `route_chosen` input that opens the order. Intake writes
//! no stated entry: every question to a provider, the reserve's request for its yes among them, is the rounds'
//! (`rounds/journal.rs`), asked under a fresh transaction for the order and appended before it is sent.

use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Actor, Entry, Outcome, Receipt, TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::router_api::Timestamp;

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

/// Appends `entry` as built, through orchestration's handle: an entry appended again after a failed append, or one
/// whose answer was lost, is the same entry.
pub(super) fn append_entry(log: &mut dyn Log, entry: &Entry) -> Result<Receipt, AppendFailure> {
    let general = GeneralEntry::try_from(entry.clone()).map_err(AppendFailure::Refused)?;
    log.append(&general)
}

/// Appends `what`, observed by `who` at Core's clock, through orchestration's handle.
pub(super) fn append(
    log: &mut dyn Log,
    clock: &dyn Clock,
    who: &Actor,
    what: What,
) -> Result<Receipt, AppendFailure> {
    append_entry(log, &build(clock, who, what)?)
}
