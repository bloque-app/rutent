//! The executed price an order reports once every segment has executed (`REQ-QTE-7`, `T-020-47`).
//!
//! Core's closing reports it, once, when the order completes: Core's schedule closes each order it follows on its tick (`Core::tick`, `T-020-48`) and asks
//! `rutent_core_orch::closing::report_price`, which builds the price from what the segments kept of each result that
//! counted. No scenario reports a price: it reads what Core reported ([`Core::price_reported`], [`Core::price_of`]).

use std::collections::BTreeMap;

use rutent_core_orch::closing::{Reported, Unreported};
use rutent_core_orch::quote::Shown;

use rutent_contracts::core_internal::Receipt;
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::Id;

use super::Core;

/// The prices Core reported as they were built, and why a completed order's price wasn't reported.
#[derive(Debug, Default)]
pub(super) struct Priced {
    built: BTreeMap<String, Reported>,
    unreported: BTreeMap<String, Unreported>,
}

impl Priced {
    /// Keeps what a round's closing came to, for the readers below: each price reported as it was built, and why a
    /// completed order's wasn't.
    pub(super) fn took(&mut self, priced: &[rutent_core_orch::schedule::Priced]) {
        for one in priced {
            match &one.reported {
                Ok(reported) => {
                    self.unreported.remove(one.order.as_str());
                    self.built
                        .insert(one.order.as_str().to_owned(), reported.clone());
                }
                Err(why) => {
                    self.unreported
                        .insert(one.order.as_str().to_owned(), why.clone());
                }
            }
        }
    }
}

impl Core {
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
