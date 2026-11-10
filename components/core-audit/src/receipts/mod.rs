//! Receipts, and evidence apart from the log.
//!
//! Declared by `T-020-01`; `T-022-03` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::ReadCheck;
use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, Fetched, Kept};

use crate::log::Unbuilt;

/// Until this row builds it: no evidence is kept, so no stated entry can name any, and none is handed out.
impl Evidence for Unbuilt {
    fn keep(&mut self, _received: &[u8]) -> Result<Kept, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn fetch(&mut self, _check: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
