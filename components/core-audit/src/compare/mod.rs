//! Comparisons that only detect: a segment's two reports, and each holding account's balance against what its orders
//! hold.
//!
//! Declared by `T-020-01`; `T-022-05` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, Comparisons};
use rutent_contracts::core_internal::{HoldingReading, Receipt, ReportPair};

use crate::log::Unbuilt;

/// Until this row builds it: nothing is compared or recorded.
impl Comparisons for Unbuilt {
    fn report_pair(&mut self, _pair: &ReportPair) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn holding_reading(&mut self, _reading: &HoldingReading) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
