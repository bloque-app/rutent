//! The one place this module reaches the log. Only orchestration's own records go through it: a refused
//! statement's stated entry and a refused route's `order_refusal`, never a decision kind, which only policy appends
//! through its own handle. Orchestration's handle, [`calls::Log`], takes a [`GeneralEntry`], built here with its
//! `TryFrom`, which refuses the decision kinds (`calls::is_decision`).
//!
//! [`calls::Log`]: rutent_contracts::core_internal::calls::Log

use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log};
use rutent_contracts::core_internal::{Entry, Receipt};

/// The log handle this module appends through: orchestration's.
pub use rutent_contracts::core_internal::calls::Log as OrchestrationLog;

/// Appends one of orchestration's own records. A decision kind is refused before it reaches the log.
pub(super) fn append(log: &mut dyn Log, entry: &Entry) -> Result<Receipt, AppendFailure> {
    let general = GeneralEntry::try_from(entry.clone()).map_err(AppendFailure::Refused)?;
    log.append(&general)
}
