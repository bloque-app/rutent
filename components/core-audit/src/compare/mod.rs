//! Comparisons that only detect: a segment's two reports, each holding account's balance against what its orders
//! hold, and, across orders, the value evidenced at nodes against the balances (`REQ-ORD-9`, `REQ-AUD-15`, `INV-1`,
//! `AC-39`, ADR-0009).
//!
//! Declared by `T-020-01`; `T-022-05` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! The module has two halves, kept apart so that detection can reach nothing that moves value:
//!
//! - the comparisons, [`report`], [`holding`] and [`evidenced`]: pure functions of data types the contracts define.
//!   They import nothing of this crate and no capability, take shared references and return a verdict.
//! - the detectors, [`detect`]: [`Compare`] calls those functions, appends the record of each comparison through a
//!   general handle on the log, and raises a difference through the operator's channel. Its type says what it can
//!   reach, and that is all.
//!
//! A difference is raised and nothing else follows: the answer to orchestration is a receipt and says nothing of
//! the comparison, and the operator's channel answers only whether a raise was delivered.

pub mod detect;
pub mod evidenced;
pub mod holding;
pub mod report;

use rutent_contracts::core_internal::calls::{AppendFailure, Comparisons};
use rutent_contracts::core_internal::{HoldingReading, Receipt, ReportPair};
use rutent_contracts::provider_role::Denomination;

pub use detect::Compare;
pub use evidenced::{Evidenced, Finding, value_evidenced_against_balances};
pub use holding::{HoldingVerdict, check_holding};
pub use report::{ReportsVerdict, compare_reports};

use crate::log::Unbuilt;

/// Why two figures can't be compared. Nothing is raised for it: the contract's `Difference` has no shape for it
/// (a gap, noted in `F-022`'s notes), so it stays in the verdict, which the caller may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incomparable {
    /// The two reports are of two assets, or of one asset in two units.
    AssetsDiffer {
        /// The sending party's.
        sent: Denomination,
        /// The receiving party's.
        received: Denomination,
    },
    /// The sum held passes what an amount can state (78 digits).
    SumPastTheBound,
    /// The evidence at an account is in another asset than the account's.
    EvidencedInAnotherAsset,
    /// Value is evidenced at an account no reading of its balance was given for.
    NoReading,
}

/// Until the assembly builds the checks: nothing is compared or recorded.
impl Comparisons for Unbuilt {
    fn report_pair(&mut self, _pair: &ReportPair) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn holding_reading(&mut self, _reading: &HoldingReading) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
