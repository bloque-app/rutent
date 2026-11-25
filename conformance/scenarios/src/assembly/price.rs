//! The executed price an order reports once every segment has executed (`REQ-QTE-7`, `T-020-47`).
//!
//! Core's closing reports it, once, when the order completes: Core's schedule closes each order it follows on its tick (`Core::tick`, `T-020-48`) and asks
//! `rutent_core_orch::closing::report_price`, which builds the price from what the segments kept of each result that
//! counted. No scenario reports a price: it reads what Core reported ([`Core::price_reported`], [`Core::price_of`]).

use std::collections::BTreeMap;

use rutent_core_orch::closing::{PriceDesk, Reported, Unreported, report_logged_price};
use rutent_core_orch::quote::Shown;

use rutent_contracts::core_internal::Receipt;
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::Id;

use super::{Core, kit};

/// The prices Core reported as they were built, and why a completed order's price wasn't reported.
#[derive(Debug, Default)]
pub(super) struct Priced {
    built: BTreeMap<String, Reported>,
    unreported: BTreeMap<String, Unreported>,
}

impl Core {
    /// Core's closing of `order`: where the order is complete, its executed price is reported once. A price the log
    /// can't take now is tried again by the next tick. Called by Core's schedule alone (`T-020-48`); answers whether
    /// this call reported it.
    pub(super) fn close(&mut self, order: &Id) -> bool {
        let before = self.reported.of(order).is_some();
        // The record of each route the order ran, as Core signed it and the log names it by digest.
        let mut records = BTreeMap::new();
        if let Some((first, later)) = self.segments.routes_ran(order) {
            let ids: Vec<Id> = std::iter::once(first.clone())
                .chain(later.iter().map(|r| r.record.clone()))
                .collect();
            for id in ids {
                if let Ok(record) = self.record_of(order, &id) {
                    records.insert(id.as_str().to_owned(), record);
                }
            }
        }
        let desk = PriceDesk {
            prices: &mut self.reported,
            log: &mut self.general,
            clock: &self.clock,
            core: &kit::core(),
        };
        match report_logged_price(desk, order, &self.segments, &|id: &Id| {
            records.get(id.as_str()).cloned()
        }) {
            Ok(Some(reported)) => {
                self.closed.unreported.remove(order.as_str());
                self.closed
                    .built
                    .insert(order.as_str().to_owned(), reported);
            }
            Ok(None) => {}
            Err(why) => {
                self.closed
                    .unreported
                    .insert(order.as_str().to_owned(), why);
            }
        }
        !before && self.reported.of(order).is_some()
    }

    /// The digest and the entry's receipt of the price Core reported for `order`, if it did.
    pub fn price_reported(&self, order: &Id) -> Option<(Digest, Receipt)> {
        self.reported.of(order)
    }

    /// The price Core reported for `order`, as it built it in this run of Core (a reopened Core holds the entry, not the
    /// figures).
    pub fn price_of(&self, order: &Id) -> Option<&Shown> {
        self.closed.built.get(order.as_str()).map(|r| &r.price)
    }

    /// How many routes Core holds `order` ran: the one that was chosen and each recalculated since, as the log has them.
    pub fn routes_ran(&self, order: &Id) -> usize {
        self.segments
            .routes_ran(order)
            .map_or(0, |(_, later)| 1 + later.len())
    }

    /// Why `order`, complete, has no price reported, if that is so.
    pub fn price_unreported(&self, order: &Id) -> Option<&Unreported> {
        self.closed.unreported.get(order.as_str())
    }
}
