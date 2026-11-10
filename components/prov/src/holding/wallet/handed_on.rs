//! A route handed on (`T-016-03`; `REQ-ORD-40`, `REQ-ORD-16`, `REQ-CAT-17`, `INV-5`).
//!
//! A route handed on (`REQ-ORD-43`) starts at an inner node, where an earlier order's value rests in the node's
//! holding account. That account counts as the route's starting reserve, and no outer reserve withdraws anything:
//! the first segment is the account's own withdrawal, from the account to its node, and the account's own record of
//! it proves it. At a public node the record is the operator's own module's (the plain wallet's); at a private node
//! it would be its publisher's word, labelled as such, which is not this file's concern.
//!
//! Nothing here reaches the evidence checker. The record is not a segment's outcome from a provider the route runs
//! through, so the grading of `REQ-ORD-39` does not apply to it; it is the holding module's answer to the host's
//! own withdrawal, and it is accepted only when it is the wallet's, for that withdrawal (`DEC-2`: this component
//! imports only the contracts).

use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::state_machine::{Id, SegmentPlan};

use super::{Holdings, Operator, WalletError, WalletHolding};
use crate::module::platform::{Platform, World};

/// The holding account's own record of a withdrawal: the proof that value left the account for the route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalRecord {
    /// The module that keeps the account, and made the record.
    pub by: String,
    /// The node the account is at.
    pub node: String,
    /// The asset withdrawn.
    pub asset: String,
    /// The order whose value it was.
    pub order: String,
    /// The amount, in whole units.
    pub amount: u128,
    /// The wallet's reference for the withdrawal, as it stated it.
    pub reference: String,
}

/// A route handed on, started: its holding account stands as its origin reserve and its own record proves the
/// withdrawal, which settles the route's first segment (`REQ-ORD-40`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandedOnStart {
    /// The route's starting reserve: the provider of the node's holding account.
    pub reserve: String,
    /// The inner node where the value sat and the route starts.
    pub node: String,
    /// The route's first segment, which the record settles.
    pub first_segment: Id,
    /// The proof of the withdrawal.
    pub record: WithdrawalRecord,
}

/// Why a route handed on could not start. Nothing moved, and the account is as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// The route is empty or starts at an outer node: it is not handed on, and starts with a reserve's withdrawal.
    NotHandedOn,
    /// The route starts at an inner node with no holding account, so no value rests there (`value_not_at_start`).
    ValueNotAtStart(String),
    /// The withdrawal was refused, or the wallet did not say it succeeded: no value has moved and the route does
    /// not start.
    Wallet(WalletError),
    /// The wallet said the withdrawal succeeded and gave no record of its own for it.
    NoRecord,
}

impl From<WalletError> for StartError {
    fn from(error: WalletError) -> Self {
        Self::Wallet(error)
    }
}

impl WalletHolding {
    /// Withdraws `amount` of an order's value from the account for a route handed on, and returns the wallet's
    /// own record of it. The account changes only when the wallet's record is its own.
    fn withdraw_with_record<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        asset: &str,
        order: &str,
        amount: &str,
    ) -> Result<WithdrawalRecord, StartError> {
        let node = self.node().to_owned();
        let (next, settled) =
            self.movement(platform, operator, false, &node, asset, order, amount)?;
        let own = format!("{}-withdrawal-", self.provider);
        let piece = settled
            .evidence
            .iter()
            .find(|p| p.by == self.provider && p.reference.starts_with(&own))
            .ok_or(StartError::NoRecord)?;
        let units = settled
            .amount_out
            .parse()
            .map_err(|_| StartError::NoRecord)?;
        let record = WithdrawalRecord {
            by: self.provider.clone(),
            node,
            asset: asset.to_owned(),
            order: order.to_owned(),
            amount: units,
            reference: piece.reference.clone(),
        };
        self.ledger = next;
        Ok(record)
    }
}

impl Holdings {
    /// Starts a route handed on: `route` begins at an inner node where `order`'s value rests, the node's holding
    /// account is its starting reserve, and the account's withdrawal of `amount` for the order, proven by the
    /// wallet's own record, is the route's first segment (`REQ-ORD-40`). The value leaves the account for the route
    /// and nowhere else, so rutent holds none (`INV-5`).
    ///
    /// Refused, with nothing moved, when the route is not handed on, when its node has no holding account, or when
    /// the account refuses the withdrawal or does not record it: a failed withdrawal starts no route.
    pub fn start_handed_on<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        route: &[SegmentPlan],
        order: &str,
        amount: &str,
    ) -> Result<HandedOnStart, StartError> {
        let first = route.first().ok_or(StartError::NotHandedOn)?;
        let NodeRef::Inner(node) = &first.from.node else {
            return Err(StartError::NotHandedOn);
        };
        let account = self
            .accounts
            .get_mut(node)
            .ok_or_else(|| StartError::ValueNotAtStart(node.clone()))?;
        let record = account.withdraw_with_record(
            platform,
            operator,
            &first.from.asset.id,
            order,
            amount,
        )?;
        Ok(HandedOnStart {
            reserve: account.provider().to_owned(),
            node: node.clone(),
            first_segment: first.segment_id.clone(),
            record,
        })
    }
}
