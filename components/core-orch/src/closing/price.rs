//! The executed price of a completed order, reported by Core's closing (`REQ-QTE-7`, `T-020-47`).
//!
//! An order's price is reported once, when every segment of its route has executed: [`report_price`] reads what each
//! settled segment took in, charged and gave out from the segments' own record ([`Segments::charged`],
//! [`Segments::stated_out`]), builds the price from them ([`Priced`]) and records it through [`Prices::report`]. It
//! reports nothing while the order is not complete, and the same digest again, never a second entry, once it has.

use rutent_contracts::core_internal::calls::Log;
use rutent_contracts::core_internal::{Actor, Receipt};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::Fee;
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_contracts::state_machine::OrderState;
use rutent_router::quote::Executed;

use crate::quote::{
    ExecutedSegment, NotShown, PartRan, PriceError, Priced, Prices, RouteRan, Shown,
};
use crate::segments::Segments;
use crate::snapshot::Clock;

/// What a price is reported through: Core's record of the prices reported, the log, Core's clock and Core's actor.
pub struct PriceDesk<'a> {
    /// The prices reported so far.
    pub prices: &'a mut Prices,
    /// The log, through orchestration's handle.
    pub log: &'a mut dyn Log,
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Core, which writes the entry.
    pub core: &'a Actor,
}

/// The price Core reported for an order: the digest it is named by, the receipt of its entry, and the price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    /// The digest the price is named by.
    pub digest: Digest,
    /// The receipt of the `shown` entry.
    pub receipt: Receipt,
    /// The price as it was reported.
    pub price: Shown,
}

/// Why a completed order's price wasn't reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreported {
    /// The price wasn't built from what the segments did: the route that completed isn't the record's chosen one, or
    /// a segment's figures aren't kept.
    NotShown(NotShown),
    /// The price wasn't recorded.
    Price(PriceError),
    /// A record that chose a route of the order chose none: there is nothing to price.
    NoRoute,
    /// Core holds no figures for a segment of the order's last parts: what its result that counted states isn't kept.
    NotKept(Id),
    /// A record of a route the order ran isn't held, so the route can't be priced.
    RecordNotHeld(Id),
}

/// A route an order ran, as the price takes it: its record, and what ran of it by order part. A route that ran whole
/// names no part ([`RouteRun::whole`]).
#[derive(Debug, Clone)]
pub struct RouteRun<'a> {
    /// The record that chose the route.
    pub record: &'a DecisionRecord,
    /// What ran of it: for each order part, the part of its candidate and how many of that part's segments ran on it.
    /// Empty where the route ran whole, as the only route of an order does.
    pub parts: Vec<PartRan>,
}

impl<'a> RouteRun<'a> {
    /// A route that ran whole.
    pub fn whole(record: &'a DecisionRecord) -> Self {
        RouteRun {
            record,
            parts: Vec::new(),
        }
    }
}

/// Reports the executed price of `order`, once it is complete: every part complete and every segment settled on a result
/// that counted. `routes` are the routes it ran, in order, the last the one it completed on, each held to its own
/// record and the last's bounds binding the price: an order whose parts, or some of them, were recalculated once or
/// more is priced across all of them (`REQ-QTE-7`). `Ok(None)` while it isn't complete. A price already reported is
/// answered again, not recorded again. It is not reported (`NotShown`) where what ran doesn't add up to the segments
/// Core holds, where a record chose no route, or where a route's figures can't be composed (a segment past an amount,
/// the operator's fee above what arrives): fail closed, with the reason kept.
///
/// # Errors
///
/// If the price can't be built from what the segments kept, or the log can't take its entry (kept, and appended as
/// built on the next call).
pub fn report_price(
    desk: PriceDesk<'_>,
    order: &Id,
    routes: &[RouteRun<'_>],
    segments: &Segments,
) -> Result<Option<Reported>, Unreported> {
    if routes.is_empty() {
        return Ok(None);
    }
    let Some(followed) = segments.order(order) else {
        return Ok(None);
    };
    if followed.parts.is_empty()
        || followed
            .parts
            .iter()
            .any(|part| part.state != OrderState::Complete)
    {
        return Ok(None);
    }
    if routes.iter().any(|r| r.record.chosen().is_none()) {
        return Err(Unreported::NoRoute);
    }
    let mut executed = Vec::new();
    for part in &followed.parts {
        for segment in &part.segments {
            let Some(segment) = Id::new(segment.segment_id.as_str()) else {
                return Err(Unreported::NotShown(NotShown::Unexplained));
            };
            let (Some(charged), Some(stated_out)) = (
                segments.charged(order, &segment),
                segments.stated_out(order, &segment),
            ) else {
                return Err(Unreported::NotKept(segment));
            };
            executed.push(ExecutedSegment {
                executed: Executed {
                    amount_in: charged.amount_in.clone(),
                    fee: Fee {
                        asset: charged.charged_in.as_ref().map_or_else(
                            || charged.tuple.from.asset.id.clone(),
                            |asset| asset.id.clone(),
                        ),
                        amount: charged.charged.as_str().to_owned(),
                        billed_separately: None,
                    },
                    amount_out: stated_out.clone(),
                },
                result: charged.result.clone(),
            });
        }
    }
    let priced = if let [only] = routes
        && only.parts.is_empty()
    {
        Priced::new(order, only.record, chosen(only.record)?, &executed)
    } else {
        let mut ran = Vec::with_capacity(routes.len());
        for route in routes {
            ran.push(RouteRan {
                record: route.record,
                candidate: chosen(route.record)?,
                parts: route.parts.clone(),
            });
        }
        Priced::across(order, &ran, &executed)
    }
    .map_err(Unreported::NotShown)?;
    let (digest, receipt) = desk
        .prices
        .report(desk.log, desk.clock, desk.core, &priced)
        .map_err(Unreported::Price)?;
    Ok(Some(Reported {
        digest,
        receipt,
        price: priced.price().clone(),
    }))
}

/// The candidate `record` chose.
fn chosen(record: &DecisionRecord) -> Result<&rutent_contracts::router_api::Candidate, Unreported> {
    record
        .chosen()
        .ok_or(Unreported::NotShown(NotShown::NotTheRecords))
}

/// Reports the executed price of `order` from what Core holds of the routes it ran ([`Segments::routes_ran`], rebuilt
/// from the log by a Core that reopens): the record that chose each route, read through `records`, and what had settled
/// in a part when it was recalculated, which is what ran on the routes before. Otherwise as [`report_price`].
///
/// # Errors
///
/// As [`report_price`], and [`Unreported::RecordNotHeld`] where `records` holds none of a route's.
pub fn report_logged_price(
    desk: PriceDesk<'_>,
    order: &Id,
    segments: &Segments,
    records: &dyn Fn(&Id) -> Option<DecisionRecord>,
) -> Result<Option<Reported>, Unreported> {
    let (Some((first, later)), Some(followed)) =
        (segments.routes_ran(order), segments.order(order))
    else {
        return Ok(None);
    };
    let held = |id: &Id| records(id).ok_or_else(|| Unreported::RecordNotHeld(id.clone()));
    let first_record = held(first)?;
    let later_records = later
        .iter()
        .map(|r| held(&r.record))
        .collect::<Result<Vec<_>, _>>()?;
    let ops = |record: &DecisionRecord, part: usize| {
        record
            .chosen()
            .and_then(|c| c.parts.get(part))
            .map_or(0, |p| p.operations.len())
    };
    let part_of = |id: &Id| {
        followed
            .parts
            .iter()
            .position(|p| p.part_id.as_str() == id.as_str())
    };
    // What settled in a part before its first recalculation ran on the first route; what settled between two of its
    // recalculations, on the route the first of them chose.
    let share = |at: Option<usize>, record: &DecisionRecord, part: usize| {
        let mine = at.map(|k| &later[k]);
        let next = later
            .iter()
            .enumerate()
            .skip(at.map_or(0, |k| k + 1))
            .find(|(_, r)| part_of(&r.part) == Some(part));
        match next {
            Some((_, next)) => next
                .settled_before
                .saturating_sub(mine.map_or(0, |m| m.settled_before)),
            None => ops(record, if at.is_some() { 0 } else { part }),
        }
    };
    let mut routes = Vec::with_capacity(1 + later.len());
    if later.is_empty() {
        routes.push(RouteRun::whole(&first_record));
    } else {
        let count = first_record.chosen().map_or(0, |c| c.parts.len());
        routes.push(RouteRun {
            record: &first_record,
            parts: (0..count)
                .map(|p| PartRan {
                    order_part: p,
                    candidate_part: p,
                    ran: share(None, &first_record, p),
                })
                .collect(),
        });
        for (k, (event, record)) in later.iter().zip(&later_records).enumerate() {
            let Some(part) = part_of(&event.part) else {
                return Err(Unreported::NotShown(NotShown::Unexplained));
            };
            routes.push(RouteRun {
                record,
                parts: vec![PartRan {
                    order_part: part,
                    candidate_part: 0,
                    ran: share(Some(k), record, part),
                }],
            });
        }
    }
    report_price(desk, order, &routes, segments)
}
