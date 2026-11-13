//! The provider role contract (`REQ-CAT-8`, `REQ-CAT-9`, `DEC-4`): what a provider advertises and declares to
//! join the catalogue, the operations each role is asked, the results it gives, and the envelope every party
//! statement travels in (`REQ-PRT-1`).
//!
//! The language-neutral artefacts are the JSON Schemas under `contracts/provider-role/schemas/`. The types here
//! mirror them with `serde`, and [`validate`] enforces the advertisement rules a schema can't express, such as a
//! ramp's two nodes being different. [`validate_statement`] and [`validate_successor`] hold the key purposes
//! ([`KeyPurpose`]): an advertisement is signed with an `advertising` key, and a later one only with a key the
//! accepted one lists for `advertising`. Every key is a [`PublicKey`], compared by its decoded bytes.
//! [`check_amounts`] ties a result's amounts to the operation it answers and names the segment's [`Remainder`].
//! A provider's signed terms ([`TupleTerms`], [`check_terms`]) and a reserve's signed hold ([`Hold`],
//! [`check_redemption`]) are statements of their own (3.1.0), and so is a reserve's notice of value arriving
//! ([`ArrivalNotice`], [`check_one_credit`]).
//! `contracts/provider-role/README.md` lists which rule lives where, and what the catalogue must hold.

mod arrival;
pub mod canonical;
mod envelope;
mod operation;
mod outcome;
mod settlement;
mod stated;
mod types;
mod validate;

pub use arrival::{ARRIVAL_NOTICE_SHAPE, ArrivalNotice, CreditRestated, Credits, check_one_credit};
pub use canonical::{CanonicalError, canonicalize, member, signing_input, to_canonical};
pub use envelope::{
    ADVERTISEMENT_SHAPE, ContextMismatch, Deployment, JUDGEMENT_SHAPE, NotSignedByOperator,
    OPERATION_SHAPE, RESULT_SHAPE, REVERSAL_CONFIRMATION_SHAPE, ShapeMismatch, Signature,
    SignatureScheme, Statement, StatementShape,
};
pub use operation::*;
pub use outcome::*;
pub use settlement::{
    AmountMismatch, Remainder, charge_in_out_unit, check_amounts, less_the_charge,
};
pub use stated::{
    DigestAlgorithm, FeeFormula, HOLD_SHAPE, HeldFor, HeldRecord, Hold, Proportion, RecordDigest,
    RedemptionRefused, TERMS_SHAPE, TermsTuple, TermsViolation, TupleEnd, TupleTerms,
    check_redemption, check_terms,
};
pub use types::*;
pub use validate::{RuleViolation, validate, validate_statement, validate_successor};

/// The contract's version: the first heading of `contracts/provider-role/CHANGELOG.md`.
pub const VERSION: &str = "3.1.0";
