//! Bulk measures and the regulator's read.
//!
//! Declared by `T-020-01`; `T-022-06` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! The module has three parts, kept apart so that the measures can reach nothing that moves value:
//!
//! - [`compute`], pure: [`measure`] reads the log's entries and a [`Window`] and answers a [`Report`], per provider
//!   or verifier, of `REQ-AUD-7`'s measures: quotes against executed prices, dry runs against outcomes, failures
//!   against the market's direction, refusals, reversals and the rate of no. It takes no figure from a caller beside
//!   the log's own records.
//! - [`record`]: [`Record`] writes each measure record orchestration hands over, through a general handle on the log.
//! - [`trail`]: [`read_trail`], a regulator's read of the trail, which rests on `T-021-02`'s permission and is
//!   recorded, allowed or refused, before anything is released (`REQ-AUD-12`).
//!
//! A measure detects and changes nothing: it answers a report, never an order's state, and the record writer answers
//! a receipt and nothing else.

pub mod compute;
pub mod record;
pub mod trail;

use rutent_contracts::core_internal::calls::{AppendFailure, Measures};
use rutent_contracts::core_internal::{MeasureRecord, Receipt};

pub use compute::{
    Answers, COUNTS, DryRuns, Failures, Provider, QuoteMeasure, Rate, Report, Window, measure,
};
pub use record::Record;
pub use trail::{Failure, Handed, Read, ReadChecks, Trail, TrailRequest, hand_trail, read_trail};

use crate::log::Unbuilt;

/// Until the assembly builds the measures: no measure record is kept.
impl Measures for Unbuilt {
    fn measure(&mut self, _record: &MeasureRecord) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
