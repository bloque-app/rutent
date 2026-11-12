//! The one place the stuck orders reach the log: orchestration's handle, a `GeneralEntry` built once and appended
//! again unchanged while the log can't take it now, so a retried append is one entry (`INV-8`). A receipt is taken
//! only where it is this entry's: its digest the SHA-256 of the entry's canonical JSON, the receipt rule
//! (core-internal's README). An answer with another entry's receipt is no answer, and the append is tried again, so
//! nothing Core does rests on a record the log didn't take.

use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Actor, Entry, Outcome, Receipt, TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::to_canonical;
use rutent_contracts::router_api::Timestamp;

/// How many times in all an append the log can't take now is tried.
pub const APPEND_TRIES: usize = 3;

/// The refusal of a time Core's clock reads that no timestamp holds.
pub(super) const NO_TIME: Violation = Violation("Core's clock reads a time a timestamp can hold");

/// One of Core's own records, built and checked: a verified entry by `who` at `at`, Core's clock, on
/// orchestration's handle, which refuses a decision kind.
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

/// Whether `receipt` is the log's receipt of `entry`: its digest is the entry's own.
pub(super) fn is_receipt_of(receipt: &Receipt, entry: &GeneralEntry) -> bool {
    let Ok(canonical) = to_canonical(entry.entry()) else {
        return false;
    };
    let value = rutent_prov::module::wire::sha256(&canonical);
    serde_json::from_value::<Digest>(serde_json::json!({ "algorithm": "sha-256", "value": value }))
        .is_ok_and(|digest| digest == receipt.entry)
}

/// Appends an entry built already, trying again while the log can't take it now, or answers with a receipt that
/// isn't this entry's. A refusal is never retried.
pub(super) fn append(log: &mut dyn Log, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
    let mut last = AppendFailure::Unavailable;
    for _ in 0..APPEND_TRIES {
        match log.append(entry) {
            Ok(receipt) if is_receipt_of(&receipt, entry) => return Ok(receipt),
            Ok(_) => last = AppendFailure::Unavailable,
            Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
            Err(refused) => return Err(refused),
        }
    }
    Err(last)
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

    /// Appends the step unless the log took it already, keeping its receipt.
    pub(super) fn append(&mut self, log: &mut dyn Log) -> Result<Receipt, AppendFailure> {
        if let Some(receipt) = &self.receipt {
            return Ok(receipt.clone());
        }
        let receipt = append(log, &self.entry)?;
        self.receipt = Some(receipt.clone());
        Ok(receipt)
    }
}
