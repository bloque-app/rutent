//! A settled segment's charge checked against the formula that bound it, raised to the operator where it is above
//! (`REQ-QTE-5`, `AC-14`; `T-024-03`).
//!
//! Core's orchestration holds the check ([`rutent_core_orch::quote::Charges`]) and nothing in its segments calls it, so
//! the assembly does, for the segments a scenario ran: it reads each settled segment's charge from its fee record in the
//! log, the result that counted and the time its effect was recorded, and holds the charge to the formula Core priced the
//! segment's tuple at. Terms a provider stated aren't read from the log yet: no step takes them in (`F-020`'s notes, the
//! wiring of signed terms), so every charge is held to the price.

use rutent_contracts::core_internal::calls::{Delivery, Operator};
use rutent_contracts::core_internal::{Entry, FeeTo, Raise, Receipt, What};
use rutent_contracts::router_api::{Amount, Id, TupleKey};
use rutent_core_orch::quote::{ChargeDesk, ChargeError, Charged, Checked};
use rutent_core_orch::segments::{Ran, SegmentOutcome};

use super::Core;
use crate::kit;

/// The operator's channel: every raise it is handed, in order, and all of them delivered.
#[derive(Debug, Clone, Default)]
pub struct Channel {
    /// What was raised to the operator.
    pub raised: Vec<Raise>,
}

impl Operator for Channel {
    fn raise(&mut self, raise: &Raise) -> Delivery {
        self.raised.push(raise.clone());
        Delivery::Delivered
    }
}

impl Core {
    /// Checks the charge of each segment `ran` settled for `order` against the formula that bound it, and raises one
    /// above it to the operator ([`Core::channel`]). A segment is checked once, whatever the number of times this is
    /// called.
    ///
    /// # Panics
    ///
    /// If `order` didn't start, or a settled segment's records aren't in the log.
    pub fn check_charges(
        &mut self,
        order: &Id,
        ran: &[Ran],
    ) -> Vec<(Id, Result<Checked, ChargeError>)> {
        let started = self.started.get(order.as_str()).expect("the order started");
        let planned: Vec<_> = started
            .plan
            .iter()
            .flat_map(|part| part.segments.iter().cloned())
            .collect();
        let entries: Vec<(Receipt, Entry)> = self.log.borrow().entries().to_vec();
        let mut checked = Vec::new();
        for accounted in ran.iter().flat_map(|r| match r {
            Ran::Segments(segments) => segments.as_slice(),
            Ran::Refused { .. } => &[],
        }) {
            let SegmentOutcome::Settled { amount_out, .. } = &accounted.outcome else {
                continue;
            };
            let (Some(effect), Some(transaction)) = (&accounted.effect, &accounted.transaction)
            else {
                continue;
            };
            let segment = planned
                .iter()
                .find(|s| s.segment_id.as_str() == accounted.segment.as_str())
                .expect("a planned segment");
            let charged = entries
                .iter()
                .find_map(|(_, e)| match e {
                    Entry::Verified(v) => match &v.what {
                        What::Fee {
                            order: o,
                            segment: s,
                            to: FeeTo::Provider,
                            amount,
                            ..
                        } if o == order && s == &accounted.segment => Some(amount.clone()),
                        _ => None,
                    },
                    Entry::Stated(_) => None,
                })
                .expect("the segment's fee record");
            let result = entries
                .iter()
                .filter_map(|(receipt, e)| match e {
                    Entry::Stated(s)
                        if s.about.as_ref().and_then(|a| a.transaction.as_ref())
                            == Some(transaction)
                            && s.what.statement.as_str()
                                == rutent_contracts::provider_role::RESULT_SHAPE =>
                    {
                        Some(receipt.clone())
                    }
                    _ => None,
                })
                .next_back()
                .expect("the segment's result");
            let started_at = match &entries[usize::try_from(u64::from(effect.index)).expect("a place")].1
            {
                Entry::Verified(v) => v.when.at.clone(),
                Entry::Stated(_) => panic!("an effect is a verified entry"),
            };
            let tuple = TupleKey {
                provider: kit::id(segment.provider.as_str()),
                from: segment.from.clone(),
                to: segment.to.clone(),
            };
            let advertised = self
                .prices
                .iter()
                .find(|p| p.tuple == tuple)
                .expect("Core priced the tuple")
                .fee
                .clone();
            // Reported net: what entered is what came out and what was charged.
            let taken = amount_out
                .as_str()
                .parse::<u128>()
                .expect("an amount")
                + charged.as_str().parse::<u128>().expect("an amount");
            let charge = Charged {
                order: order.clone(),
                segment: accounted.segment.clone(),
                provider: kit::id(segment.provider.as_str()),
                tuple,
                started_at,
                amount_in: Amount::new(taken.to_string()).expect("an amount"),
                charged,
                advertised,
                result,
            };
            let core = kit::core();
            let desk = ChargeDesk {
                terms: &self.terms,
                log: &mut self.general,
                clock: &self.clock,
                fresh: &mut self.fresh,
                operator: &mut self.channel,
                core: &core,
            };
            checked.push((
                accounted.segment.clone(),
                self.charges.check(desk, &charge),
            ));
        }
        checked
    }
}
