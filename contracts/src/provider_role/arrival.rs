//! A reserve's signed notice of value arriving (`REQ-ORD-14`, `REQ-ORD-23`, `T-020-18`).
//!
//! Before it, provider-role had no statement a reserve makes of value arriving, so Core took what arrived (outer
//! node, asset, amount, account, holder) from the assembly's port and kept the reserve's notice as bytes it couldn't
//! read, with nothing tying the figures to the signed bytes (`T-020-04`'s gap).
//!
//! - **The notice** (`urn:rutent:provider-role:arrival-notice`) is the reserve's own statement: the transaction the
//!   reserve gives the credit, its per-credit reference, the outer node the value arrived at, the asset with its
//!   unit, the amount, and the account and its holder. Core's log keys the deposit on that transaction
//!   (core-internal's `arrival-names-its-notice`, `one-order-per-deposit`), and the notice's stated entry is about
//!   it.
//! - **One credit, one arrival.** Every notice of one credit names its one transaction, whenever signed, and a reader
//!   knows two such notices as one credit ([`check_one_credit`]): the same reserve, the same transaction, the same
//!   figures. The same transaction with other figures is refused, never taken as a second credit.

use alloc::string::String;
use serde::{Deserialize, Serialize};

use super::envelope::{Statement, StatementShape};
use super::types::{Denomination, OuterNode};

/// The `shape` of a reserve's arrival notice.
pub const ARRIVAL_NOTICE_SHAPE: &str = "urn:rutent:provider-role:arrival-notice";

/// A reserve's notice that value arrived at one of its accounts (`REQ-ORD-14`, `REQ-ORD-23`): the body of a
/// statement of shape [`ARRIVAL_NOTICE_SHAPE`], made by the reserve. Schema:
/// `urn:rutent:provider-role:arrival-notice#/$defs/notice`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrivalNotice {
    /// The transaction the reserve gives this one credit: its per-credit reference, the same in every notice of the
    /// credit, whenever signed. The deposit is known by it, and the notice's stated entry is about it.
    pub transaction: String,
    /// The reserve's outer node the value arrived at: the node belongs to the notice's maker.
    pub node: OuterNode,
    /// The asset, and the unit the amount is counted in.
    pub asset: Denomination,
    /// How much arrived, in the asset's unit: a positive decimal integer string.
    pub amount: String,
    /// The account at the reserve the value reached.
    pub account: String,
    /// The party that holds the account.
    pub holder: String,
}

impl StatementShape for ArrivalNotice {
    const SHAPE: &'static str = ARRIVAL_NOTICE_SHAPE;
}

impl ArrivalNotice {
    /// Whether `other` states the same arrival: the same outer node, asset and unit, amount, account and holder.
    /// The transaction, the credit's reference, is left aside.
    pub fn same_figures(&self, other: &Self) -> bool {
        self.node == other.node
            && self.asset == other.asset
            && self.amount == other.amount
            && self.account == other.account
            && self.holder == other.holder
    }
}

/// Whether two arrival notices are of one credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credits {
    /// One credit: the same reserve's, by the same transaction, with the same figures. A reader knows the two as one
    /// arrival, whenever each was signed.
    One,
    /// Two credits: another reserve's, or another transaction.
    Two,
}

/// A notice that restates a credit with figures another notice of it doesn't state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditRestated {
    /// The same reserve and transaction, with another node, asset, unit, amount, account or holder.
    OtherFigures,
}

/// Compares two arrival notices by their credit (`REQ-ORD-23`). A credit's transaction is its reserve's own, so
/// notices of two makers are never one credit, whatever they name. The same maker's notices naming one transaction
/// are one credit when they state the same figures, and refused when they don't: a credit doesn't change once it
/// arrived.
pub fn check_one_credit(
    earlier: &Statement<ArrivalNotice>,
    later: &Statement<ArrivalNotice>,
) -> Result<Credits, CreditRestated> {
    if earlier.maker != later.maker || earlier.statement.transaction != later.statement.transaction
    {
        return Ok(Credits::Two);
    }
    if earlier.statement.same_figures(&later.statement) {
        Ok(Credits::One)
    } else {
        Err(CreditRestated::OtherFigures)
    }
}
