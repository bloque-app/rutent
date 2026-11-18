//! The one place the quote module reaches the log: orchestration's handle, a `GeneralEntry` built once and appended
//! again unchanged while the log can't take it now, so a retried append is one entry (`INV-8`). It writes only
//! Core's own records, a `shown` of the price and a `raise`; no stated entry and no decision kind.

use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Actor, Entry, Outcome, Receipt, TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::router_api::Timestamp;

use crate::snapshot::Clock;

/// How many times in all an append the log can't take now is tried, each call.
pub const APPEND_TRIES: usize = 3;

/// Core's clock now, as an entry's time.
pub(super) fn now(clock: &dyn Clock) -> Result<Timestamp, AppendFailure> {
    clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(Violation(
            "Core's clock reads a time a timestamp can hold",
        )))
}

/// One of Core's own records, built and checked: a verified entry by `who` at `at`, Core's clock.
pub(super) fn built(
    who: &Actor,
    what: What,
    at: &Timestamp,
    outcome: Outcome,
) -> Result<GeneralEntry, AppendFailure> {
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: who.clone(),
        what,
        when: When {
            at: at.clone(),
            source: TimeSource::CoreClock,
        },
        outcome,
    });
    entry.check().map_err(AppendFailure::Refused)?;
    GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)
}

/// A step built once: its entry, and its receipt once the log took it.
#[derive(Debug, Clone)]
pub(super) struct Step {
    pub(super) entry: GeneralEntry,
    pub(super) receipt: Option<Receipt>,
}

impl Step {
    pub(super) fn new(entry: GeneralEntry) -> Self {
        Step {
            entry,
            receipt: None,
        }
    }

    /// Appends the step as built unless the log took it already, keeping its receipt; tried again while the log
    /// can't take it now, never once it refuses it.
    pub(super) fn append(&mut self, log: &mut dyn Log) -> Result<Receipt, AppendFailure> {
        if let Some(receipt) = &self.receipt {
            return Ok(receipt.clone());
        }
        let mut last = AppendFailure::Unavailable;
        for _ in 0..APPEND_TRIES {
            match log.append(&self.entry) {
                Ok(receipt) => {
                    self.receipt = Some(receipt.clone());
                    return Ok(receipt);
                }
                Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
                Err(refused) => return Err(refused),
            }
        }
        Err(last)
    }
}

/// A stated entry, unverified, appended as it is built, tried again while the log can't take it now: the log
/// attributes it, and the receipt is of the entry it recorded.
pub(super) fn built_stated(log: &mut dyn Log, entry: Entry) -> Result<Receipt, AppendFailure> {
    entry.check().map_err(AppendFailure::Refused)?;
    let general = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
    let mut last = AppendFailure::Unavailable;
    for _ in 0..APPEND_TRIES {
        match log.append(&general) {
            Ok(receipt) => return Ok(receipt),
            Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
            Err(refused) => return Err(refused),
        }
    }
    Err(last)
}
