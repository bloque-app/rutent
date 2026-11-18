//! Exposures from Core's records (`REQ-ORD-8`): the value in flight at each provider, in each asset, which the
//! snapshot hands the router so that its ceilings per provider and per publisher hold.
//!
//! Core holds a segment's value in flight from when it hands the segment's operation over until the segment's value
//! is accounted for (`T-020-08` holds and releases; this module keeps the record). Each held operation counts as the
//! router counts a candidate's operations against a ceiling: its `amount_in` in its `from` asset, and, where an
//! exchange gives out another asset, its `amount_out` in its `to` asset too. Every segment of every part of every
//! order adds up at its provider, so a split order's parts count together; the publisher's sum is the router's, over
//! the exposures of its providers.
//!
//! A stuck order's exposure stays held until its resolution releases it, and is readable at any time
//! ([`Exposures::stuck`]): the read needs no snapshot, no clock and no grant in force. The value a stuck order rests at
//! a node, which no segment holds once the one that took it there settled, is held there too
//! ([`Exposures::hold_resting`], `T-020-11`'s addition).

use std::collections::BTreeMap;

use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Id, Operation};

/// One held amount: a segment's value at its provider, in one asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The order.
    pub order: Id,
    /// The segment.
    pub segment: Id,
    /// The provider it is in flight at.
    pub provider: Id,
    /// The asset and its unit.
    pub asset: Denomination,
    /// How much.
    pub amount: Amount,
}

/// Why a hold wasn't taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldError {
    /// The segment is held already, under this order or another.
    AlreadyHeld,
    /// The sum in flight at the provider would not be a whole number the contract can carry.
    Overflow,
}

/// Core's record of value in flight.
#[derive(Debug, Clone, Default)]
pub struct Exposures {
    /// Each held segment, by its order's and segment's text, with what it holds.
    held: BTreeMap<(String, String), Vec<Held>>,
    /// The orders declared stuck.
    stuck: Vec<Id>,
}

impl Exposures {
    /// An empty record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds a segment's value in flight at its operation's provider: `amount_in` in the `from` asset and, where the
    /// operation gives out another asset, `amount_out` in the `to` asset. A segment is held once.
    pub fn hold(
        &mut self,
        order: &Id,
        segment: &Id,
        operation: &Operation,
    ) -> Result<(), HoldError> {
        if self.held.keys().any(|(_, s)| s == segment.as_str()) {
            return Err(HoldError::AlreadyHeld);
        }
        let mut held = vec![Held {
            order: order.clone(),
            segment: segment.clone(),
            provider: operation.provider.clone(),
            asset: operation.from.asset.clone(),
            amount: operation.amount_in.amount().clone(),
        }];
        if operation.to.asset != operation.from.asset {
            held.push(Held {
                order: order.clone(),
                segment: segment.clone(),
                provider: operation.provider.clone(),
                asset: operation.to.asset.clone(),
                amount: operation.amount_out.clone(),
            });
        }
        // Refuse a hold the totals can't carry, so a reading never fails later.
        for h in &held {
            self.in_flight(&h.provider, &h.asset)
                .checked_add(&h.amount)
                .ok_or(HoldError::Overflow)?;
        }
        self.held.insert(
            (order.as_str().to_owned(), segment.as_str().to_owned()),
            held,
        );
        Ok(())
    }

    /// Holds the value resting at a node for a stuck order (`REQ-ORD-8`, `T-020-11`): `amount` of `asset` at
    /// `provider`, under `segment`, the settled segment that took it there. The value sits at that provider's node
    /// until the order's resolution moves it, so the ceilings in flight count it as they count a segment's. A
    /// segment is held once.
    pub fn hold_resting(
        &mut self,
        order: &Id,
        segment: &Id,
        provider: &Id,
        asset: &Denomination,
        amount: &Amount,
    ) -> Result<(), HoldError> {
        if self.held.keys().any(|(_, s)| s == segment.as_str()) {
            return Err(HoldError::AlreadyHeld);
        }
        self.in_flight(provider, asset)
            .checked_add(amount)
            .ok_or(HoldError::Overflow)?;
        self.held.insert(
            (order.as_str().to_owned(), segment.as_str().to_owned()),
            vec![Held {
                order: order.clone(),
                segment: segment.clone(),
                provider: provider.clone(),
                asset: asset.clone(),
                amount: amount.clone(),
            }],
        );
        Ok(())
    }

    /// Releases a segment's value: it is no longer in flight at that provider.
    pub fn release(&mut self, order: &Id, segment: &Id) {
        self.held
            .remove(&(order.as_str().to_owned(), segment.as_str().to_owned()));
    }

    /// Releases every segment of an order, and its stuck mark: the order is closed or resolved.
    pub fn release_order(&mut self, order: &Id) {
        self.held.retain(|(o, _), _| o != order.as_str());
        self.stuck.retain(|o| o != order);
    }

    /// Marks an order stuck (`REQ-ORD-28`). What it holds stays held, and readable through [`Self::stuck`].
    pub fn mark_stuck(&mut self, order: &Id) {
        if !self.stuck.contains(order) {
            self.stuck.push(order.clone());
        }
    }

    /// Every amount held, by order and segment.
    pub fn held(&self) -> impl Iterator<Item = &Held> {
        self.held.values().flatten()
    }

    /// The exposure on stuck orders: every amount a stuck order holds, by order and segment. Readable at any time
    /// (`REQ-ORD-8`).
    pub fn stuck(&self) -> Vec<&Held> {
        self.held()
            .filter(|h| self.stuck.contains(&h.order))
            .collect()
    }

    /// The value in flight at one provider, in one asset (id and decimals), across every order and part.
    pub fn in_flight(&self, provider: &Id, asset: &Denomination) -> Amount {
        self.held()
            .filter(|h| &h.provider == provider && &h.asset == asset)
            .fold(Amount::zero(), |sum, h| {
                sum.checked_add(&h.amount)
                    .expect("a hold that would overflow is refused")
            })
    }

    /// The value in flight per provider and asset, each pair once, in order: what the snapshot's exposure figures
    /// state. A pair with nothing in flight is left out.
    pub fn totals(&self) -> Vec<(Id, Denomination, Amount)> {
        let mut pairs: Vec<(Id, Denomination)> = Vec::new();
        for h in self.held() {
            if !pairs.iter().any(|(p, a)| p == &h.provider && a == &h.asset) {
                pairs.push((h.provider.clone(), h.asset.clone()));
            }
        }
        pairs.sort_by(|(p, a), (q, b)| {
            (p.as_str(), a.id.as_str(), a.decimals).cmp(&(q.as_str(), b.id.as_str(), b.decimals))
        });
        pairs
            .into_iter()
            .map(|(provider, asset)| {
                let amount = self.in_flight(&provider, &asset);
                (provider, asset, amount)
            })
            .filter(|(_, _, amount)| !amount.is_zero())
            .collect()
    }
}
