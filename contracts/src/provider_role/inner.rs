//! A publisher's signed record of the segments inside one of its private nodes (`REQ-ORD-41`, `REQ-ORD-3`,
//! `T-020-18`).
//!
//! Before it, provider-role had no shape for the record `CMP-EVIDENCE`'s replay reads, so the assembly's `Books` was
//! a seam filled by hand. The record (`urn:rutent:provider-role:inner-segments`) is its publisher's word: a publisher
//! holds no key, so one of its providers makes and signs it, stating for the publisher, as any statement a publisher
//! stands behind is made. It names the private node, the publisher, each segment that ran inside the node with its
//! transaction, the asset and amount in and out and the fee, and a label, [`Standing::PublishersWord`], that travels
//! with it. [`check_inner_segments`] holds it to its maker's advertisement: the maker declares the node among its
//! private nodes, and its publisher is the one the record names. Inside a private node there is no other
//! witness, so the replay of the record against the public movement out shows consistency with the exit and never
//! that the segments happened (`THR-131`); a route ending inside the node stays its publisher's word (`AC-35`).
//!
//! The contract refuses no figure: a record whose segments don't add up, or don't chain, is still the publisher's
//! record, and the replay records the mismatch.

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use super::envelope::{Statement, StatementShape};
use super::types::Advertisement;

/// The `shape` of a publisher's record of inner segments.
pub const INNER_SEGMENTS_SHAPE: &str = "urn:rutent:provider-role:inner-segments";

/// A publisher's record of the segments that ran inside one of its private nodes (`REQ-ORD-41`): the body of a
/// statement of shape [`INNER_SEGMENTS_SHAPE`], made by a provider of the publisher's that declares the node. Schema:
/// `urn:rutent:provider-role:inner-segments#/$defs/record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InnerSegments {
    /// The private node, by its identifier.
    pub node: String,
    /// The publisher whose word the record is: the publisher behind the statement's maker, which states for it.
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

/// A rule a record of inner segments breaks against its maker's advertisement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InnerSegmentsViolation {
    /// The advertisement isn't its maker's: made by, or for, another provider.
    NotItsMakersAdvertisement,
    /// The publisher behind its maker isn't the publisher it names: a provider states only for its own publisher.
    OfAnotherPublisher,
    /// Its maker doesn't declare its node among its private nodes (`REQ-CAT-20`, `REQ-CAT-22`).
    NodeNotDeclared,
}

/// Checks a record of inner segments against `advertisement`, its maker's: the advertisement is the record maker's
/// own, its publisher is the publisher the record names, and it declares the record's node among its private nodes.
/// Every rule broken is returned.
///
/// The reader passes the advertisement its catalogue accepted last for the record's maker, never one that arrives
/// with the record (as `validate_assertion_key` is given the catalogue's). That the record is signed with a key that
/// advertisement lists for `statements` is the reader's too, as for every statement (attribution, `F-012`). That its
/// segments add up and hand on to the public exit is the replay's (`REQ-ORD-41`): the contract refuses no figure.
pub fn check_inner_segments(
    record: &Statement<InnerSegments>,
    advertisement: &Statement<Advertisement>,
) -> Result<(), Vec<InnerSegmentsViolation>> {
    let (inner, advertised) = (&record.statement, &advertisement.statement);
    let mut broken = Vec::new();
    if advertised.provider != record.maker || advertisement.maker != record.maker {
        broken.push(InnerSegmentsViolation::NotItsMakersAdvertisement);
    }
    if advertised.publisher != inner.publisher {
        broken.push(InnerSegmentsViolation::OfAnotherPublisher);
    }
    if !advertised.private_nodes.contains(&inner.node) {
        broken.push(InnerSegmentsViolation::NodeNotDeclared);
    }
    if broken.is_empty() {
        Ok(())
    } else {
        Err(broken)
    }
}
