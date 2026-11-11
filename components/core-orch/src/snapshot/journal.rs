//! The one place this module reaches the log. Only orchestration's own records go through it: a refused
//! statement's stated entry and a refused route's `order_refusal`, never a decision kind (`decide`, `authorise`,
//! `stuck_decide`, `stuck_move`, `open_session`, `check_read`, `grant_permission`, `revoke_permission`, `charge`
//! or `ask_about_person`), which only policy appends. A change to orchestration's handle on the log changes this
//! file alone.

use rutent_contracts::core_internal::calls::{AppendFailure, Log};
use rutent_contracts::core_internal::{Entry, Receipt};

/// The log handle this module appends through: orchestration's.
pub use rutent_contracts::core_internal::calls::Log as OrchestrationLog;

/// Appends one of orchestration's own records.
pub(super) fn append(log: &mut dyn Log, entry: &Entry) -> Result<Receipt, AppendFailure> {
    log.append(entry)
}
