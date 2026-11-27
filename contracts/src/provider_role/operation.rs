//! Operations the operator asks of a provider (`urn:rutent:provider-role:operation`, `REQ-CAT-9`,
//! `REQ-CAT-14`). On the wire each is the operator's [`Statement`](super::Statement) of an [`Operation`].

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use super::envelope::{OPERATION_SHAPE, StatementShape};
use super::outcome::Direction;
use super::stated::{HeldFor, HeldRecord};
use super::types::OuterNode;

/// One operation, by kind (`REQ-CAT-9`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    /// A reserve's withdrawal, from its outer node into an inner node.
    Withdrawal(Withdrawal),
    /// A reserve's deposit, from an inner node out to its outer node.
    Deposit(Deposit),
    /// A ramp's transfer of one asset between two inner nodes.
    Transfer(Transfer),
    /// An exchange's conversion, within one node.
    Conversion(Conversion),
    /// A reserve's balance verification for a stated amount (`INV-9`).
    BalanceVerification(BalanceVerification),
    /// A question to a verifier (`DEC-16`).
    Judgement(JudgementRequest),
    /// An exchange's or ramp's true undo of a segment (`REQ-CAT-15`).
    Reversal(Reversal),
    /// A question to a reversed segment's origin reserve about one credit (ADR-0008).
    ReversalConfirmation(ReversalConfirmationRequest),
    /// The operator's question to a holding account's module: its balance at a node (`REQ-AUD-15`).
    HoldingBalance(HoldingBalance),
    /// The operator's question to a provider that declares `status_query` tracking of where a transaction it was
    /// handed stands (`REQ-ORD-1`, `REQ-CAT-16`; 4.0.0).
    StatusQuery(StatusQuery),
    /// The operator's request that a reserve hold the quote a choice round chose before its route starts
    /// (`REQ-QTE-15`; 4.0.0).
    Hold(HoldRequest),
    /// The operator's question to a private node's publisher's provider: its record of the segments that ran inside the
    /// node (`REQ-ORD-41`; 4.0.0).
    InnerSegmentsQuery(InnerSegmentsQuery),
}

impl StatementShape for Operation {
    const SHAPE: &'static str = OPERATION_SHAPE;
}

/// The inputs a tuple declared it needs, by name (`REQ-CAT-12`).
pub type Inputs = BTreeMap<String, String>;

/// A withdrawal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Withdrawal {
    /// The transaction's identifier; every result names it.
    pub transaction: String,
    /// The reserve asked.
    pub provider: String,
    /// The session of the account holder or their delegate (`REQ-CAT-3`). Required on a dry run, which
    /// reveals whether funds suffice (`REQ-CAT-4`); the schema enforces that.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub session: Option<String>,
    /// The reserve's outer node.
    pub origin: OuterNode,
    /// The inner node the value goes into.
    pub destination: String,
    /// The asset.
    pub asset: String,
    /// The amount, in the origin unit.
    pub amount: String,
    /// True asks what would happen and changes nothing (`REQ-CAT-14`).
    pub dry_run: bool,
    /// The inputs the tuple needs.
    pub inputs: Inputs,
}

/// A deposit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deposit {
    /// The transaction's identifier; every result names it.
    pub transaction: String,
    /// The reserve asked.
    pub provider: String,
    /// The inner node the value leaves.
    pub origin: String,
    /// The reserve's outer node.
    pub destination: OuterNode,
    /// The asset.
    pub asset: String,
    /// The amount, in the origin unit.
    pub amount: String,
    /// True asks what would happen and changes nothing (`REQ-CAT-14`).
    pub dry_run: bool,
    /// The inputs the tuple needs.
    pub inputs: Inputs,
}

/// A transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    /// The transaction's identifier; every result names it.
    pub transaction: String,
    /// The ramp asked.
    pub provider: String,
    /// The inner node the value leaves.
    pub origin: String,
    /// The inner node the value reaches.
    pub destination: String,
    /// The asset, kept across the transfer.
    pub asset: String,
    /// Which amount is stated.
    pub direction: Direction,
    /// The stated amount, in the unit its direction names.
    pub amount: String,
    /// True asks what would happen and changes nothing (`REQ-CAT-14`).
    pub dry_run: bool,
    /// The inputs the tuple needs.
    pub inputs: Inputs,
}

/// A conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conversion {
    /// The transaction's identifier; every result names it.
    pub transaction: String,
    /// The exchange asked.
    pub provider: String,
    /// The one node the conversion happens at.
    pub node: String,
    /// The asset taken in.
    pub from: String,
    /// The asset given out.
    pub to: String,
    /// Which amount is stated.
    pub direction: Direction,
    /// The stated amount, in the unit its direction names.
    pub amount: String,
    /// True asks for a detailed quote and changes nothing (`REQ-CAT-14`).
    pub dry_run: bool,
    /// The inputs the tuple needs.
    pub inputs: Inputs,
}

/// A balance verification: whether what can be withdrawn covers a stated amount. Never a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BalanceVerification {
    /// The transaction's identifier; every result names it.
    pub transaction: String,
    /// The reserve asked.
    pub provider: String,
    /// The session of the account holder or their delegate (`REQ-CAT-3`).
    pub session: String,
    /// The account at the reserve.
    pub account: String,
    /// The asset.
    pub asset: String,
    /// The stated amount (`INV-9`).
    pub amount: String,
}

/// A holding account's balance: the operator asks the module that is a node's holding account for the balance it
/// holds there, in one asset, so that `CMP-CORE-AUDIT` can check it against the value held for each order
/// (`REQ-AUD-15`, `INV-1`). It is the operator's own question, not a customer's, so it carries no session; and it
/// never changes anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldingBalance {
    /// The transaction's identifier; every result names it.
    pub transaction: String,
    /// The holding account's provider.
    pub provider: String,
    /// The node the account is at.
    pub node: String,
    /// The asset.
    pub asset: String,
}

/// A question to a verifier: a person, by subject reference, and a class only (`REQ-CPL-4`, ADR-0012).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgementRequest {
    /// The verifier asked.
    pub provider: String,
    /// The subject reference, one per relying provider.
    pub subject: String,
    /// The class asked about.
    pub class: String,
}

/// An exchange's or ramp's true undo of a segment: the value back at the segment's origin node, in full and in the
/// asset it left (`REQ-CAT-15`, `REQ-ORD-3`). A reserve doesn't reverse. Never a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reversal {
    /// This reversal's transaction identifier.
    pub transaction: String,
    /// The exchange or ramp asked.
    pub provider: String,
    /// The transaction of the segment to undo.
    pub reverses: String,
    /// The segment's origin node, where the value goes back.
    pub node: String,
    /// The asset the value left in.
    pub asset: String,
    /// The amount that left, in full, in the origin unit.
    pub amount: String,
}

/// A question to the reserve at a reversed segment's origin node: has this credit arrived? It names only the
/// segment's transaction, amount and asset, never sufficiency in general (ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReversalConfirmationRequest {
    /// This question's transaction identifier.
    pub transaction: String,
    /// The reserve asked.
    pub provider: String,
    /// The transaction of the reversed segment.
    pub segment: String,
    /// The asset the value should be back in.
    pub asset: String,
    /// The amount that should be back, in full.
    pub amount: String,
}

/// A question to a provider of where a transaction it was handed stands, late (`REQ-ORD-1`, `REQ-CAT-16`; finding 4
/// of `M3`'s milestone review, 4.0.0). Asked only of a provider whose declarations list `status_query` tracking, under
/// a transaction of its own, never the one asked about, and never a dry run: it changes nothing, and what the provider
/// answers is its statement about `about`, which moves nothing either. The provider answers with a
/// [`StatusQueryResult`](super::StatusQueryResult) naming both transactions
/// ([`check_status_answer`](super::check_status_answer)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusQuery {
    /// This question's own transaction identifier: its answer names it (`REQ-ORD-1`).
    pub transaction: String,
    /// The provider asked.
    pub provider: String,
    /// The transaction asked about: one the provider was handed an operation under.
    pub about: String,
}

/// The operator's request that a reserve hold a quote for a new route before it starts (`REQ-QTE-15`; finding 4 of
/// `M3`'s milestone review, 4.0.0): the choice round's decision record by its id and digest, the candidate it chose,
/// the order or session the hold is for, and the validity asked, at most the earliest validity among the figures the
/// candidate cites. The reserve answers with its own signed [`Hold`](super::Hold) of exactly these, under this
/// request's transaction, which is the hold's own; or refuses ([`check_hold_answer`](super::check_hold_answer)). Never
/// a dry run, and it moves nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldRequest {
    /// The hold's own transaction: its answer and the hold it carries name it.
    pub transaction: String,
    /// The reserve asked: the one that starts each part of the route.
    pub provider: String,
    /// The choice round's decision record whose chosen candidate is the quote to hold.
    pub record: HeldRecord,
    /// The candidate to hold: the record's chosen one.
    pub candidate: String,
    /// The order or session the hold is for.
    #[serde(rename = "for")]
    pub held_for: HeldFor,
    /// The validity asked: the hold holds no longer.
    pub valid_until: String,
}

/// The operator's question to a provider of a private node's publisher: what it records of the segments that ran inside
/// the node (`REQ-ORD-41`, `REQ-ORD-3`; `T-020-58`, 4.0.0). Until it, no operation asked a provider for its record, and
/// the record `CMP-EVIDENCE`'s replay reads was a seam filled by the deployment. Asked of a provider whose module's
/// `handles` lists `inner_segments_query`, under a transaction of its own, and never a dry run: it changes nothing, and
/// the answer is a statement that moves nothing. The provider answers with an
/// [`InnerSegmentsQueryResult`](super::InnerSegmentsQueryResult) naming this question's transaction and the node, carrying
/// its own signed [`InnerSegments`](super::InnerSegments) of that node, or says it has none
/// ([`check_inner_segments_answer`](super::check_inner_segments_answer)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InnerSegmentsQuery {
    /// This question's own transaction identifier: its answer names it.
    pub transaction: String,
    /// The provider asked.
    pub provider: String,
    /// The private node whose record is asked for.
    pub node: String,
    /// The transactions the question is about: the segments whose record is asked for, at least one. The provider
    /// answers with the segments it ran under these transactions alone, never its whole book of the node, so the answer
    /// is the asker's own and bounded by what was asked ([`check_inner_segments_answer`](super::check_inner_segments_answer)).
    pub about: Vec<String>,
}
