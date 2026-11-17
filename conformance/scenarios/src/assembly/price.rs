//! The executed price an order reports once every segment has executed (`REQ-QTE-7`, `T-024-04`).
//!
//! Core's segments keep what each settled segment took in and charged, and the receipt of the result that counted for
//! it (`Segments::charged`), and `quote::Prices` records a price once it is built from them. F-020's notes name who
//! calls it: "closing, `T-020-09`, or the assembly". Closing doesn't, so the assembly does, here, for the orders a
//! scenario ran; `F-030`'s production assembly calls it where its orders complete.

use rutent_contracts::core_internal::calls::KeptStatements as _;
use rutent_contracts::core_internal::{Entry, Receipt, calls::AppendFailure};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    ConversionOutcome, Fee, ReserveMovementOutcome, ResultBody, Statement, TransferOutcome,
};
use rutent_contracts::router_api::Id;
use rutent_core_orch::quote::{
    ExecutedSegment, NotShown, PriceError, Priced, Recorded as _, Shown,
};
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
    /// of the entry, and the price as it was reported.
    ///
    /// # Errors
    ///
    /// If the order isn't followed, a segment hasn't settled, or the price isn't built or recorded.
    pub fn report_price(
        &mut self,
        order: &Id,
        ran: &[Ran],
    ) -> Result<(Digest, Receipt, Shown), PriceNotReported> {
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
                let SegmentOutcome::Settled { .. } = &one.outcome else {
                    return Err(PriceNotReported::NotExecuted("a segment didn't settle"));
                };
                let charged = self.segments.charged(order, &one.segment).cloned().ok_or(
                    PriceNotReported::NotExecuted("a segment's charge isn't kept"),
                )?;
                // What the result states of what the segment gave out, as the log keeps it: under a tuple that reports
                // gross, that includes the charge, and what the segment handed on (`amount_out` of its outcome) is
                // that less the charge, which the price takes off itself (`T-024-13`).
                let stated =
                    self.stated_out(&charged.result)
                        .ok_or(PriceNotReported::NotExecuted(
                            "a segment's result isn't kept",
                        ))?;
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
                        amount_out: stated,
                    },
                    result: charged.result.clone(),
                });
            }
        }
        let priced = Priced::new(order, &started.record, candidate, &segments)
            .map_err(PriceNotReported::NotShown)?;
        let (digest, receipt) = self
            .reported
            .report(&mut self.general, &self.clock, &kit::core(), &priced)
            .map_err(PriceNotReported::Price)?;
        Ok((digest, receipt, priced.price().clone()))
    }
}

impl Core {
    /// The `amount_out` the result that counted for a segment states, read from the statement the log's entry
    /// `result` commits to, as the evidence store kept it: the figure as its provider stated it.
    fn stated_out(&mut self, result: &Receipt) -> Option<rutent_contracts::router_api::Amount> {
        let Entry::Stated(entry) =
            super::charges::Log(self.log.borrow().entries()).entry(result)?
        else {
            return None;
        };
        let kept = self.kept.kept(&entry.commitment).ok()??;
        let bytes = from_base64url(kept.bytes.as_str())?;
        let body: Statement<ResultBody> = serde_json::from_slice(&bytes).ok()?;
        let out = match body.statement {
            ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match r.outcome {
                ReserveMovementOutcome::Succeeded(s) => s.amount_out,
                _ => return None,
            },
            ResultBody::Transfer(r) => match r.outcome {
                TransferOutcome::Succeeded(s) => s.amount_out,
                _ => return None,
            },
            ResultBody::Conversion(r) => match r.outcome {
                ConversionOutcome::Succeeded(s) => s.amount_out,
                _ => return None,
            },
            _ => return None,
        };
        rutent_contracts::router_api::Amount::new(out)
    }
}

/// The bytes base64url text without padding writes, or `None` where it isn't such text.
fn from_base64url(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in text.bytes() {
        let v = u32::try_from(ALPHABET.iter().position(|a| *a == c)?).ok()?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits).ok()?);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}
