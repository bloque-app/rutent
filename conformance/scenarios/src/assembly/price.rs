//! The executed price an order reports once every segment has executed (`REQ-QTE-7`, `T-020-47`).
//!
//! Core's closing reports it, once, when the order completes: `Core::close` runs after every part Core runs and asks
//! `rutent_core_orch::closing::report_price`, which builds the price from what the segments kept of each result that
//! counted. No scenario reports a price: it reads what Core reported ([`Core::price_reported`], [`Core::price_of`]).

use std::collections::BTreeMap;

use rutent_core_orch::closing::{PriceDesk, Reported, RouteRun, Unreported, report_price};
use rutent_core_orch::quote::PartRan;
use rutent_core_orch::quote::Shown;

use rutent_contracts::core_internal::Receipt;
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::{DecisionRecord, Id};

use super::{Core, kit};

/// The prices Core reported as they were built, and why a completed order's price wasn't reported.
#[derive(Debug, Default)]
pub(super) struct Priced {
    built: BTreeMap<String, Reported>,
    unreported: BTreeMap<String, Unreported>,
}

impl Core {
    /// Core's closing of `order`, run under `record`: where the order is complete, its executed price is reported
    /// once. A price the log can't take now is tried again by the next run.
    pub(super) fn close(&mut self, order: &Id, record: &DecisionRecord) {
        let ran = self.life.ran_on(order, record);
        let routes = routes_run(&ran);
        let desk = PriceDesk {
            prices: &mut self.reported,
            log: &mut self.general,
            clock: &self.clock,
            core: &kit::core(),
        };
        match report_price(desk, order, &routes, &self.segments) {
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

    /// Why `order`, complete, has no price reported, if that is so.
    pub fn price_unreported(&self, order: &Id) -> Option<&Unreported> {
        self.closed.unreported.get(order.as_str())
    }
}

/// The routes an order ran as the price takes them: the first over all its parts, the part recalculated cut to what
/// settled on it, and each recalculated route over that part alone.
fn routes_run(ran: &[(DecisionRecord, Option<usize>)]) -> Vec<RouteRun<'_>> {
    if let [(only, _)] = ran {
        return vec![RouteRun::whole(only)];
    }
    let ops = |record: &DecisionRecord, part: usize| {
        record
            .chosen()
            .and_then(|c| c.parts.get(part))
            .map_or(0, |p| p.operations.len())
    };
    ran.iter()
        .enumerate()
        .map(|(k, (record, settled))| {
            let parts = if k == 0 {
                let count = record.chosen().map_or(0, |c| c.parts.len());
                (0..count)
                    .map(|p| PartRan {
                        order_part: p,
                        candidate_part: p,
                        ran: if p == 0 {
                            settled.unwrap_or_else(|| ops(record, 0))
                        } else {
                            ops(record, p)
                        },
                    })
                    .collect()
            } else {
                vec![PartRan {
                    order_part: 0,
                    candidate_part: 0,
                    ran: settled.unwrap_or_else(|| ops(record, 0)),
                }]
            };
            RouteRun { record, parts }
        })
        .collect()
}
