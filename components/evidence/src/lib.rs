//! The read-only evidence checker, component CMP-EVIDENCE of the SAD.
//!
//! It holds the four-grade scale of ADR-0007 (the scale itself lives in the contracts), grades an outcome's
//! evidence against what its provider declared, and defines the per-network interface through which grade 1 and 2
//! claims are checked. It imports nothing of ours but the contracts (`DEC-2`).

mod grade;
mod network;
pub mod replay;
mod route;

pub use grade::{
    Check, Claim, GradeExt, Graded, GradedPiece, PublisherId, Refusal, distinct_movements,
    grade_outcome, receipt_check,
};
pub use network::{
    Expected, Finality, Movement, MovementId, NetworkRules, PublicNetwork, ReorgRule, Status,
    Unmatched, public_record_check, verify_movement,
};
pub use route::{DestinationViolation, one_destination};
