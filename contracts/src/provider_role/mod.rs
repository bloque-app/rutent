//! The provider role contract (`REQ-CAT-8`, `REQ-CAT-9`, `DEC-4`): what a provider advertises and declares to
//! join the catalogue, the operations each role is asked, the results it gives, and the envelope every party
//! statement travels in (`REQ-PRT-1`).
//!
//! The language-neutral artefacts are the JSON Schemas under `contracts/provider-role/schemas/`. The types here
//! mirror them with `serde`, and [`validate`] enforces the advertisement rules a schema can't express, such as a
//! ramp's two nodes being different. `contracts/provider-role/README.md` lists which rule lives where.

mod envelope;
mod operation;
mod outcome;
mod types;
mod validate;

pub use envelope::{
    ADVERTISEMENT_SHAPE, ContextMismatch, Deployment, JUDGEMENT_SHAPE, NotSignedByOperator,
    OPERATION_SHAPE, RESULT_SHAPE, REVERSAL_CONFIRMATION_SHAPE, ShapeMismatch, Signature,
    SignatureScheme, Statement, StatementShape,
};
pub use operation::*;
pub use outcome::*;
pub use types::*;
pub use validate::{RuleViolation, validate, validate_statement};

/// The contract's version: the first heading of `contracts/provider-role/CHANGELOG.md`.
pub const VERSION: &str = "0.4.0";
