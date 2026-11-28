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
//! [`check_redemption`]) are statements of their own (3.1.0, folded into 4.0.0), handed back in a dry-run answer's `terms` and
//! a hold request's answer (4.0.0), and so is a reserve's notice of value arriving
//! ([`ArrivalNotice`], [`check_one_credit`]), and the operator's question of what arrived and the reserve's answer
//! ([`ArrivalQuestion`], [`ArrivalAnswer`], [`check_answer`]). A publisher's record of the segments inside its
//! private node ([`InnerSegments`], [`check_inner_segments`]) is labelled its publisher's word
//! ([`Standing::PublishersWord`]). A status question ([`StatusQuery`], [`check_status_answer`]) and a hold request
//! ([`HoldRequest`], [`check_hold_answer`]) are operations of their own (4.0.0), and so is the question of a private
//! node's record ([`InnerSegmentsQuery`], [`check_inner_segments_answer`]), which a publisher's provider answers with
//! its [`InnerSegments`] (`T-020-58`), and a reader takes statements under
//! the protocol versions in force ([`ProtocolVersions`], [`Statement::check_context`]).
//! `contracts/provider-role/README.md` lists which rule lives where, and what the catalogue must hold.

mod arrival;
pub mod canonical;
mod envelope;
mod inner;
mod operation;
mod outcome;
mod settlement;
mod stated;
mod types;
mod validate;

pub use arrival::{
    ARRIVAL_ANSWER_SHAPE, ARRIVAL_NOTICE_SHAPE, ARRIVAL_QUESTION_SHAPE, AnswerMismatch,
    ArrivalAnswer, ArrivalNotice, ArrivalOutcome, ArrivalQuestion, ArrivedCredits, CreditRestated,
    Credits, DepositDestination, check_answer, check_one_credit,
};
pub use canonical::{CanonicalError, canonicalize, member, signing_input, to_canonical};
pub use envelope::{
    ADVERTISEMENT_SHAPE, ContextMismatch, Cutover, Deployment, JUDGEMENT_SHAPE,
    NotSignedByOperator, OPERATION_SHAPE, ProtocolVersions, RESULT_SHAPE,
    REVERSAL_CONFIRMATION_SHAPE, ShapeMismatch, Signature, SignatureScheme, Statement,
    StatementShape,
};
pub use inner::{
    INNER_SEGMENTS_SHAPE, InnerSegment, InnerSegments, InnerSegmentsViolation, Standing,
    check_inner_segments,
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
pub const VERSION: &str = "4.0.0";
