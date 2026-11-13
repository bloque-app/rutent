//! The executed price an order reports once every segment has executed (`REQ-QTE-7`, `T-024-04`).
//!
//! Core's segments keep what each settled segment took in and charged, and the receipt of the result that counted for
//! it (`Segments::charged`), and `quote::Prices` records a price once it is built from them. F-020's notes name who
//! calls it: "closing, `T-020-09`, or the assembly". Closing doesn't, so the assembly does, here, for the orders a
//! scenario ran; `F-030`'s production assembly calls it where its orders complete.

use rutent_contracts::core_internal::{Receipt, calls::AppendFailure};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::Fee;
use rutent_contracts::router_api::Id;
use rutent_core_orch::quote::{ExecutedSegment, NotShown, PriceError, Priced};
use rutent_core_orch::segments::{Ran, SegmentOutcome};
use rutent_router::quote::Executed;

use super::{Core, kit};

/// Why an order's price wasn't reported.
#[derive(Debug)]
pub enum PriceNotReported {
    /// Core follows no such order.
    NotStarted,
    /// A part didn't run its segments, or a segment hasn't settled: there is no price until every one has.
    NotExecuted(&'static str),
    /// The price wasn't built from what the segments did.
    NotShown(NotShown),
    /// The price wasn't recorded.
    Price(PriceError),
    /// The log couldn't take the entry.
    Append(AppendFailure),
}

impl Core {
    /// Reports `order`'s executed price: built from what each segment `ran` took in, charged and gave out, each on its
    /// own result, and recorded as a `shown` entry labelled the price, once. The digest it is named by and the receipt
    /// of the entry.
    ///
    /// # Errors
    ///
    /// If the order isn't followed, a segment hasn't settled, or the price isn't built or recorded.
    pub fn report_price(
        &mut self,
        order: &Id,
        ran: &[Ran],
    ) -> Result<(Digest, Receipt), PriceNotReported> {
        let started = self
            .started
            .get(order.as_str())
            .cloned()
            .ok_or(PriceNotReported::NotStarted)?;
        let candidate = started
            .record
            .chosen()
            .ok_or(PriceNotReported::NotExecuted("no route was chosen"))?;
        let mut segments = Vec::new();
        for part in ran {
            let Ran::Segments(accounted) = part else {
                return Err(PriceNotReported::NotExecuted("a part ran no segment"));
            };
            for one in accounted {
                let SegmentOutcome::Settled { amount_out, .. } = &one.outcome else {
                    return Err(PriceNotReported::NotExecuted("a segment didn't settle"));
                };
                let charged = self.segments.charged(order, &one.segment).ok_or(
                    PriceNotReported::NotExecuted("a segment's charge isn't kept"),
                )?;
                segments.push(ExecutedSegment {
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
                        amount_out: amount_out.clone(),
                    },
                    result: charged.result.clone(),
                });
            }
        }
        let priced = Priced::new(order, &started.record, candidate, &segments)
            .map_err(PriceNotReported::NotShown)?;
        self.reported
            .report(&mut self.general, &self.clock, &kit::core(), &priced)
            .map_err(PriceNotReported::Price)
    }
}
