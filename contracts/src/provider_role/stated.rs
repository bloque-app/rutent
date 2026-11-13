//! A provider's signed terms for one tuple, and a reserve's signed hold of a quote (`REQ-QTE-5`, `REQ-QTE-6`,
//! `REQ-QTE-15`, round 3 of `T-020-01`).
//!
//! Before these, Core kept a provider's fee terms and a held quote only in memory, with nothing signed behind them.
//! Each is now its maker's own statement, in the envelope, which Core keeps and appends as a stated entry.
//!
//! - **Terms** (`urn:rutent:provider-role:terms`) are made by the tuple's own provider. They name the tuple, by its
//!   provider and both ends, its fee formula (flat, proportion, optional maximum) and, where the tuple sets a rate,
//!   its slippage regime. They bind from the later of their `made_at` and Core's `received_at`, never `made_at`
//!   alone, so terms dated before a segment started but received after it never bind it.
//! - **A hold** (`urn:rutent:provider-role:hold`) is made by the reserve that starts each part of the route it holds.
//!   It names the choice round's decision record, by id and digest, the candidate, the order or session it is for,
//!   and its validity, at most the earliest validity of the figures its candidate cites (router-api's
//!   `HeldQuote::check_hold`, which has the record). It is redeemed for that order alone ([`check_redemption`]).

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use super::envelope::{Statement, StatementShape};
use super::outcome::Slippage;
use super::types::{Denomination, NodeRef, present};

/// The `shape` of a provider's terms statement.
pub const TERMS_SHAPE: &str = "urn:rutent:provider-role:terms";

/// The `shape` of a reserve's hold statement.
pub const HOLD_SHAPE: &str = "urn:rutent:provider-role:hold";

/// A provider's terms for one tuple, as it states them (`REQ-QTE-5`, `REQ-QTE-6`): the body of a statement of shape
/// [`TERMS_SHAPE`]. Schema: `urn:rutent:provider-role:terms#/$defs/terms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TupleTerms {
    /// The tuple they are for, by its provider and both ends.
    pub tuple: TermsTuple,
    /// The fee formula, in the tuple's input unit.
    pub fee: FeeFormula,
    /// The slippage regime: present exactly where the tuple sets a rate, an exchange's, whose ends hold different
    /// assets.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub slippage: Option<Slippage>,
}

impl StatementShape for TupleTerms {
    const SHAPE: &'static str = TERMS_SHAPE;
}

/// A tuple, by its provider and both ends. Schema: `urn:rutent:provider-role:terms#/$defs/tuple`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermsTuple {
    /// The provider: the statement's maker.
    pub provider: String,
    /// Where the tuple starts.
    pub from: TupleEnd,
    /// Where the tuple ends.
    pub to: TupleEnd,
}

impl TermsTuple {
    /// Whether the tuple sets a rate: its two ends hold different assets, as an exchange's do.
    pub fn sets_a_rate(&self) -> bool {
        self.from.asset.id != self.to.asset.id
    }
}

/// One end of a tuple: a node, inner or outer, and the asset with the unit it is counted in there. Schema:
/// `urn:rutent:provider-role:terms#/$defs/end`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TupleEnd {
    /// The node.
    pub node: NodeRef,
    /// The asset and its unit.
    pub asset: Denomination,
}

/// A fee formula, depending only on amount and tuple (`REQ-QTE-5`): on an amount `a` entering the tuple,
/// `flat + ceil(a × numerator / denominator)`, in the tuple's input unit; where a `maximum` is given, the charge is
/// known only after execution and stays within it. Amounts are decimal integer strings. Schema:
/// `urn:rutent:provider-role:terms#/$defs/fee_formula`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeFormula {
    /// The flat part.
    pub flat: String,
    /// The proportional part.
    pub proportional: Proportion,
    /// The most the charge may come to, where it is known only after execution.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub maximum: Option<String>,
}

/// A proportion, `numerator / denominator`, in whole numbers; the denominator is positive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proportion {
    /// The numerator.
    pub numerator: String,
    /// The denominator, above zero.
    pub denominator: String,
}

/// A rule a terms statement breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermsViolation {
    /// Its maker isn't its tuple's provider: a provider states only its own terms.
    OfAnotherMaker,
    /// Its tuple sets a rate, and it states no slippage regime (`REQ-QTE-6`, `REQ-QTE-9`).
    NoSlippageWhereARate,
    /// Its tuple sets no rate, and it states a slippage regime.
    SlippageWhereNoRate,
}

/// Checks a terms statement's rules a schema can't state: its maker is its tuple's provider, and it states a
/// slippage regime exactly where its tuple sets a rate. Every rule broken is returned.
pub fn check_terms(statement: &Statement<TupleTerms>) -> Result<(), Vec<TermsViolation>> {
    let terms = &statement.statement;
    let mut broken = Vec::new();
    if statement.maker != terms.tuple.provider {
        broken.push(TermsViolation::OfAnotherMaker);
    }
    match (terms.tuple.sets_a_rate(), terms.slippage.is_some()) {
        (true, false) => broken.push(TermsViolation::NoSlippageWhereARate),
        (false, true) => broken.push(TermsViolation::SlippageWhereNoRate),
        _ => {}
    }
    if broken.is_empty() {
        Ok(())
    } else {
        Err(broken)
    }
}

/// A reserve's hold of a quote for a new route before it starts (`REQ-QTE-15`): the body of a statement of shape
/// [`HOLD_SHAPE`], made by the reserve that starts each part of the route. It stays a quote until each segment
/// starts and binds no provider. Schema: `urn:rutent:provider-role:hold#/$defs/hold`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hold {
    /// The choice round's decision record whose chosen candidate is the quote held.
    pub record: HeldRecord,
    /// The candidate held: the record's chosen one.
    pub candidate: String,
    /// The order or session it is for, and the only one that may redeem it.
    #[serde(rename = "for")]
    pub held_for: HeldFor,
    /// Its validity: at most the earliest validity among the figures its candidate cites.
    pub valid_until: String,
}

impl StatementShape for Hold {
    const SHAPE: &'static str = HOLD_SHAPE;
}

/// A decision record as a hold names it: the id and the digest Core's log names it by (core-internal's
/// `decision_record` entry, its `decision_record` and its `record`). Schema:
/// `urn:rutent:provider-role:hold#/$defs/record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldRecord {
    /// The record's id.
    pub id: String,
    /// Its digest.
    pub digest: RecordDigest,
}

/// A SHA-256 digest, as 64 lower-case hexadecimal characters. Schema: `urn:rutent:provider-role:hold#/$defs/digest`,
/// the module interface's digest restated here, since this contract refers to no other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordDigest {
    /// The algorithm: `sha-256`.
    pub algorithm: DigestAlgorithm,
    /// The digest, in lower-case hexadecimal.
    pub value: String,
}

/// A digest's algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DigestAlgorithm {
    /// SHA-256 (FIPS 180-4).
    #[serde(rename = "sha-256")]
    Sha256,
}

/// What a hold is for: an order, or a session in which the order it is redeemed for is accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HeldFor {
    /// An order.
    Order {
        /// The order.
        order: String,
    },
    /// A session, before its order exists.
    Session {
        /// The session.
        session: String,
    },
}

/// A redemption a hold refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedemptionRefused {
    /// The hold is for another order.
    ForAnotherOrder,
    /// The hold is for a session the redeeming order wasn't accepted in.
    ForAnotherSession,
}

/// Checks that `hold` may be redeemed for `order`, accepted in `session` where it was: a hold for an order only for
/// that order, a hold for a session only for an order accepted in it (`REQ-QTE-15`). That the hold is still valid
/// at the time, and its maker the reserve each part starts at, are the reader's, with its clock and the record.
pub fn check_redemption(
    hold: &Statement<Hold>,
    order: &str,
    session: Option<&str>,
) -> Result<(), RedemptionRefused> {
    match &hold.statement.held_for {
        HeldFor::Order { order: o } if o == order => Ok(()),
        HeldFor::Order { .. } => Err(RedemptionRefused::ForAnotherOrder),
        HeldFor::Session { session: s } if Some(s.as_str()) == session => Ok(()),
        HeldFor::Session { .. } => Err(RedemptionRefused::ForAnotherSession),
    }
}
