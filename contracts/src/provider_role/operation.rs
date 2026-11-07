//! Operations the operator asks of a provider (`urn:rutent:provider-role:operation`, `REQ-CAT-9`,
//! `REQ-CAT-14`). On the wire each is the operator's [`Statement`](super::Statement) of an [`Operation`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::envelope::{OPERATION_SHAPE, StatementShape};
use super::outcome::Direction;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
