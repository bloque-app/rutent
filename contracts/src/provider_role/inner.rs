//! A publisher's signed record of the segments inside one of its private nodes (`REQ-ORD-41`, `REQ-ORD-3`,
//! `T-020-18`).
//!
//! Before it, provider-role had no shape for the record `CMP-EVIDENCE`'s replay reads, so the assembly's `Books` was
//! a seam filled by hand. The record (`urn:rutent:provider-role:inner-segments`) is the publisher's own statement:
//! the private node, each segment that ran inside it with its transaction, the asset and amount in and out and the
//! fee, and a label, [`Standing::PublishersWord`], that travels with it. Inside a private node there is no other
//! witness, so the replay of the record against the public movement out shows consistency with the exit and never
//! that the segments happened (`THR-131`); a route ending inside the node stays its publisher's word (`AC-35`).
//!
//! The contract refuses no figure: a record whose segments don't add up, or don't chain, is still the publisher's
//! record, and the replay records the mismatch.

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use super::envelope::{Statement, StatementShape};

/// The `shape` of a publisher's record of inner segments.
pub const INNER_SEGMENTS_SHAPE: &str = "urn:rutent:provider-role:inner-segments";

/// A publisher's record of the segments that ran inside one of its private nodes (`REQ-ORD-41`): the body of a
/// statement of shape [`INNER_SEGMENTS_SHAPE`], made by the publisher. Schema:
/// `urn:rutent:provider-role:inner-segments#/$defs/record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InnerSegments {
    /// The private node, by its identifier.
    pub node: String,
    /// The publisher whose word the record is: the statement's maker.
    pub publisher: String,
    /// The record's standing: its publisher's word, labelled as such (`REQ-ORD-3`).
    pub standing: Standing,
    /// The segments, in the order they ran: at least one.
    pub segments: Vec<InnerSegment>,
}

impl StatementShape for InnerSegments {
    const SHAPE: &'static str = INNER_SEGMENTS_SHAPE;
}

/// What a record of inner segments stands for. It has one value: a record inside a private node is its publisher's
/// word, never proof (`REQ-ORD-3`, `AC-35`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Its publisher's word, labelled as such.
    PublishersWord,
}

/// One segment inside a private node, as its publisher recorded it. Amounts are decimal integer strings, in each
/// asset's unit at the node. Schema: `urn:rutent:provider-role:inner-segments#/$defs/segment`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InnerSegment {
    /// The transaction the segment ran under.
    pub transaction: String,
    /// The asset that went in.
    pub asset_in: String,
    /// The amount that went in.
    pub amount_in: String,
    /// The asset that came out.
    pub asset_out: String,
    /// The amount that came out.
    pub amount_out: String,
    /// The fee, in the asset that went in.
    pub fee: String,
}

/// A rule a record of inner segments breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InnerSegmentsViolation {
    /// Its maker isn't the publisher it names: a record is labelled only its own maker's word.
    OfAnotherMaker,
}

/// Checks a record's rule a schema can't state: it is made by the publisher whose word it says it is. That the
/// publisher declares the node, and that the segments add up, are its reader's: the catalogue's (`REQ-CAT-20`,
/// `REQ-CAT-22`) and the replay's (`REQ-ORD-41`).
pub fn check_inner_segments(
    statement: &Statement<InnerSegments>,
) -> Result<(), InnerSegmentsViolation> {
    if statement.maker == statement.statement.publisher {
        Ok(())
    } else {
        Err(InnerSegmentsViolation::OfAnotherMaker)
    }
}
