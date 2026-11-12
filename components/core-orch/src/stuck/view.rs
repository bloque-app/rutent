//! The operator's view of stuck orders, by value and age, so volume can't hide a large one (`REQ-OPS-2`,
//! `THR-58`): a pure ranking over what each stuck order holds, in each asset.

use std::cmp::Ordering;

use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Id};

/// One stuck order's value in one asset, and how long it has been stuck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StuckValue {
    /// The order.
    pub order: Id,
    /// The asset and its unit.
    pub asset: Denomination,
    /// What the order's stuck parts hold in it: held in flight at a provider, or resting at the node where its
    /// value is evidenced, which no hold counts once the segment that took it there settled.
    pub amount: Amount,
    /// How long it has been stuck, in whole seconds, by Core's clock: from its first part declared stuck.
    pub age_seconds: u64,
}

/// Ranks `values` as the operator sees them: asset by asset, the largest first, then the oldest, then by order. No
/// rate converts one asset into another here, so values are ranked within their asset, never across.
pub fn rank(mut values: Vec<StuckValue>) -> Vec<StuckValue> {
    values.sort_by(|a, b| {
        asset_order(&a.asset, &b.asset)
            .then_with(|| b.amount.cmp(&a.amount))
            .then_with(|| b.age_seconds.cmp(&a.age_seconds))
            .then_with(|| a.order.as_str().cmp(b.order.as_str()))
    });
    values
}

fn asset_order(a: &Denomination, b: &Denomination) -> Ordering {
    (a.id.as_str(), a.decimals).cmp(&(b.id.as_str(), b.decimals))
}
