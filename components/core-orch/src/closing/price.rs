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

use crate::quote::{ExecutedSegment, NotShown, PriceError, Priced, Prices, Shown};
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
}

/// Reports the executed price of `order` as `record`, the record of the route it ran, chose it, once it is complete:
/// every part complete and every segment settled on a result that counted. `Ok(None)` while it isn't. A price already
/// reported is answered again, not recorded again.
///
/// # Errors
///
/// If the price can't be built from what the segments kept, or the log can't take its entry (kept, and appended as
/// built on the next call).
pub fn report_price(
    desk: PriceDesk<'_>,
    order: &Id,
    record: &DecisionRecord,
    segments: &Segments,
) -> Result<Option<Reported>, Unreported> {
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
    let Some(candidate) = record.chosen() else {
        return Ok(None);
    };
    let mut executed = Vec::new();
    for part in &followed.parts {
        for segment in &part.segments {
            let Some(segment) = Id::new(segment.segment_id.as_str()) else {
                return Ok(None);
            };
            let (Some(charged), Some(stated_out)) = (
                segments.charged(order, &segment),
                segments.stated_out(order, &segment),
            ) else {
                return Ok(None);
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
    let priced = Priced::new(order, record, candidate, &executed).map_err(Unreported::NotShown)?;
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
