//! The read-only evidence checker, component CMP-EVIDENCE of the SAD.
//!
//! It holds the four-grade scale of ADR-0007 (the scale itself lives in the contracts), grades an outcome's
//! evidence against what its provider declared, and defines the per-network interface through which grade 1 and 2
//! claims are checked. It imports nothing of ours but the contracts: F-001's reading, stricter than `DEC-2`,
//! which the boundary check enforces.

mod grade;
mod network;
pub mod replay;
mod route;
mod simnet;

pub use grade::{
    Check, Claim, GradeExt, Graded, GradedPiece, PublisherId, Publishers, Receipt, ReceiptRefused,
    ReceiptVerifier, Refusal, VerifiedReceipt, distinct_movements, grade_outcome, receipt_check,
    verify_receipt,
};
pub use network::{
    Expected, Finality, Movement, MovementId, NetworkRules, PublicNetwork, ReorgRule, Status,
    Unmatched, public_record_check, verify_movement,
};
pub use route::{DestinationViolation, one_destination};
pub use simnet::{RecordRefused, SimulatedNetwork};
