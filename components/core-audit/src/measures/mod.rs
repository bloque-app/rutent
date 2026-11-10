//! Bulk measures and the regulator's read.
//!
//! Declared by `T-020-01`; `T-022-06` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, Measures};
use rutent_contracts::core_internal::{MeasureRecord, Receipt};

use crate::log::Unbuilt;

/// Until this row builds it: no measure record is kept.
impl Measures for Unbuilt {
    fn measure(&mut self, _record: &MeasureRecord) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
