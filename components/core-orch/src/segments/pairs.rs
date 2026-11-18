//! The pair of reports a segment's end gives (`REQ-ORD-9`, `REQ-CAT-11`, `T-020-35`, `T-020-38`).
//!
//! What one provider reports sending is compared with what the next party reports receiving: when segment n+1 of a
//! route settles, [`Segments::pair_with_previous`] builds the [`ReportPair`] of segment n's result (the sent report:
//! what n says it gave out, about n) and n+1's (the received report: what n+1 says it took in, about n+1), under n's
//! tuple's terms (its fill increment, restated in the reports' unit and rounded down, and its gross or net
//! convention, with the charge a gross figure holds). Each report rests on the result Core took and judged as its
//! provider's own, whose stated entry the log holds about its own segment.
//!
//! **A comparison is a detection** (`ADR-0009`). A built pair is queued, once for its segments, and handed to
//! `core-audit`'s comparison by [`Segments::write_pairs`], the caller's call (as the measures': the segment's outcome,
//! its entries and what it moved depend on neither the writer nor its answer). A pair that can't be built is never
//! skipped, and never allowed to change the primary path: it is parked with why ([`Segments::unbuilt_pairs`]) and the
//! settlement goes on. The only segments that make no pair are those that have none to make: the first segment of a route (a
//! recalculated route's included: a pair spans one plan), and one run by the provider that ran the segment before it
//! (the log's two-parties rule: one party's word compares nothing).

use std::collections::{BTreeSet, VecDeque};

use rutent_contracts::core_internal::calls::{AppendFailure, Comparisons};
use rutent_contracts::core_internal::{Receipt, Report, ReportPair, ReportTerms, ReportedAs};
use rutent_contracts::provider_role::{Denomination, Fee, Reporting, Terms, charge_in_out_unit};
use rutent_contracts::router_api::{Amount, Id, Operation, PositiveAmount};

use super::{Live, Segments};

/// Why a segment's pair of reports couldn't be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unbuilt {
    /// Core holds no sent report of the segment before: its result that counted isn't in the flight.
    NoSentReport(Id),
    /// The segment's flight isn't held.
    NoFlight,
    /// A figure of one of the reports isn't an amount the contract carries.
    NotAnAmount(&'static str),
    /// The contract refuses the pair.
    Refused(&'static str),
}

/// What a settled segment reports having given out, kept for the segment after it.
#[derive(Debug, Clone)]
pub(super) struct Sent {
    report: Report,
    terms: ReportTerms,
}

/// A pair that couldn't be built, or that the log refused, with why: parked, not raised against the segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnbuiltPair {
    /// The order.
    pub order: Id,
    /// The later segment of the two: the one whose end owed the pair.
    pub segment: Id,
    /// Why.
    pub why: Unbuilt,
}

/// The pairs built and not yet handed over, and those parked.
#[derive(Debug, Clone, Default)]
pub(super) struct Pairing {
    waiting: VecDeque<ReportPair>,
    built: BTreeSet<(String, String)>,
    unbuilt: Vec<UnbuiltPair>,
    lost: bool,
}

/// `amount`'s tuple increment restated from the origin's unit to the out unit, rounded down: `None` where there is
/// none, or it comes to nothing in the out unit (compared exactly, which fails closed).
fn increment_in_out_unit(increment: &str, origin: u8, out: u8) -> Option<PositiveAmount> {
    let whole: u128 = increment.parse().ok()?;
    let restated = if out >= origin {
        whole.checked_mul(10u128.checked_pow(u32::from(out - origin))?)?
    } else {
        whole / 10u128.checked_pow(u32::from(origin - out))?
    };
    PositiveAmount::new(Amount::new(restated.to_string())?)
}

/// The terms the sending segment's pair is compared under: its tuple's fill increment and gross or net convention,
/// the charge a gross figure holds restated in the out unit (`from` and `to` are the segment's ends).
fn report_terms(
    from: &Denomination,
    to: &Denomination,
    terms: &Terms,
    fee: &str,
) -> Result<ReportTerms, Unbuilt> {
    let reporting = match terms.reporting {
        Reporting::Net => ReportedAs::Net {},
        // A conversion's charge is in the asset it takes in, which it doesn't give out: none to take off.
        Reporting::Gross => ReportedAs::Gross {
            charge: if from.id == to.id {
                charge_in_out_unit(fee, from.decimals, to.decimals).and_then(Amount::new)
            } else {
                Amount::new("0".to_owned())
            }
            .ok_or(Unbuilt::NotAnAmount("the charge a gross figure holds"))?,
        },
    };
    let increment = terms
        .increment
        .as_deref()
        .and_then(|i| increment_in_out_unit(i, from.decimals, to.decimals));
    Ok(ReportTerms {
        increment,
        reporting,
    })
}

impl Segments {
    /// What the settled segment `at` gives its pair (`REQ-ORD-9`): keeps its own report of what it gave out, for the
    /// segment after it, and builds the pair it makes with the segment before it. **A comparison is a detection**
    /// (`ADR-0009`): nothing here fails the settlement. A pair that can't be built is parked ([`Segments::unbuilt_pairs`])
    /// with why, and the segment goes on to its outcome as it would have.
    pub(super) fn report_end(
        &mut self,
        live: &Live<'_>,
        at: &(String, String),
        operation: &Operation,
        terms: &Terms,
        stated: &Receipt,
        figures: Figures<'_>,
    ) {
        let kept = self.note_report(at, operation, terms, stated, figures.out, figures.fee);
        let built = self.pair_with_previous(live, at, operation, stated, figures.amount_in);
        for failed in [kept, built].into_iter().filter_map(Result::err) {
            self.park(live.order, at, failed);
        }
    }

    /// Parks a pair that couldn't be built or compared, once for its later segment and reason.
    fn park(&mut self, order: &Id, at: &(String, String), why: Unbuilt) {
        let Some(segment) = Id::new(&at.1) else {
            return;
        };
        let parked = UnbuiltPair {
            order: order.clone(),
            segment,
            why,
        };
        if !self.pairing.unbuilt.contains(&parked) {
            self.pairing.unbuilt.push(parked);
        }
    }

    /// Keeps what the settled segment `at` reports having given out, with the terms it reports under, for the
    /// segment after it. `stated` is the receipt of the result that counted.
    fn note_report(
        &mut self,
        at: &(String, String),
        operation: &Operation,
        terms: &Terms,
        stated: &Receipt,
        amount_out: &str,
        fee: &Fee,
    ) -> Result<(), Unbuilt> {
        let provider = self.held(at)?.provider.clone();
        let segment = Id::new(&at.1).ok_or(Unbuilt::NoFlight)?;
        let amount = Amount::new(amount_out.to_owned())
            .ok_or(Unbuilt::NotAnAmount("what the segment gave out"))?;
        let (from, to) = (&operation.from.asset, &operation.to.asset);
        let terms = report_terms(from, to, terms, &fee.amount)?;
        if self.pairing.lost {
            return Ok(());
        }
        self.flight_mut(at).map_err(|_| Unbuilt::NoFlight)?.reported = Some(Sent {
            report: Report {
                party: provider,
                amount,
                asset: to.clone(),
                segment,
                statement: stated.clone(),
            },
            terms,
        });
        Ok(())
    }

    fn held(&self, at: &(String, String)) -> Result<&super::InFlight, Unbuilt> {
        self.flights.get(at).ok_or(Unbuilt::NoFlight)
    }

    /// Builds the pair of the segment before `at` and `at` itself, now that `at` settled: queued once, to be handed to
    /// the comparison ([`Segments::write_pairs`]). Nothing where `at` is its route's first segment, or the provider
    /// that ran the one before it ran it too.
    fn pair_with_previous(
        &mut self,
        live: &Live<'_>,
        at: &(String, String),
        operation: &Operation,
        stated: &Receipt,
        amount_in: &str,
    ) -> Result<(), Unbuilt> {
        let place = self.held(at)?.place;
        let Some(before) = place.checked_sub(1).and_then(|p| live.segments.get(p)) else {
            return Ok(());
        };
        if self.pairing.built.contains(at) {
            return Ok(());
        }
        let before_at = (at.0.clone(), before.as_str().to_owned());
        let provider = self.held(at)?.provider.clone();
        if self.held(&before_at)?.provider == provider {
            return Ok(());
        }
        let sent = self.held(&before_at)?.reported.clone();
        let received = Report {
            party: provider,
            amount: Amount::new(amount_in.to_owned())
                .ok_or(Unbuilt::NotAnAmount("what the segment took in"))?,
            asset: operation.from.asset.clone(),
            segment: Id::new(&at.1).ok_or(Unbuilt::NoFlight)?,
            statement: stated.clone(),
        };
        let pair = assemble(live.order, before, sent.as_ref(), received)?;
        self.pairing.built.insert(at.clone());
        self.pairing.waiting.push_back(pair);
        Ok(())
    }

    /// Hands the comparison the pairs built since the last call, oldest first, and answers how many it took
    /// (`REQ-ORD-9`, `T-020-35`). A pair the log can't take now waits, with those after it, for the next call. One the
    /// log refuses will be refused again, so it is parked with why ([`Segments::unbuilt_pairs`]) and the pairs after
    /// it still go.
    ///
    /// # Errors
    ///
    /// The log's unavailability, with the pair it couldn't take and those after it left waiting.
    pub fn write_pairs(
        &mut self,
        comparisons: &mut dyn Comparisons,
    ) -> Result<usize, AppendFailure> {
        let mut written = 0;
        while let Some(pair) = self.pairing.waiting.front() {
            match comparisons.report_pair(pair) {
                Ok(_) => written += 1,
                Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable),
                Err(AppendFailure::Refused(violation)) => {
                    let (order, segment) = (pair.order.clone(), pair.received.segment.clone());
                    let parked = UnbuiltPair {
                        order,
                        segment,
                        why: Unbuilt::Refused(violation.0),
                    };
                    self.pairing.unbuilt.push(parked);
                }
            }
            self.pairing.waiting.pop_front();
        }
        Ok(written)
    }

    /// How many pairs wait for [`Segments::write_pairs`].
    pub fn unpaired(&self) -> usize {
        self.pairing.waiting.len()
    }

    /// The pairs that couldn't be built or that the log refused, each with why: surfaced, never skipped, and no
    /// part of the primary path (`REQ-ORD-9`, `ADR-0009`).
    pub fn unbuilt_pairs(&self) -> &[UnbuiltPair] {
        &self.pairing.unbuilt
    }

    /// Makes the segments forget the reports they keep for the pair of the segment after them, as a restart that
    /// lost their in-memory state would, from now on: for a scenario of the pair that can't be built.
    #[doc(hidden)]
    pub fn lose_sent_reports(&mut self) {
        self.pairing.lost = true;
    }
}

/// The figures a settled segment states, as its pair needs them.
pub(super) struct Figures<'a> {
    pub(super) out: &'a str,
    pub(super) fee: &'a Fee,
    pub(super) amount_in: &'a str,
}

/// The pair of the sent report `sent` of segment `before` and `received`, held to the contract's own check.
fn assemble(
    order: &Id,
    before: &Id,
    sent: Option<&Sent>,
    received: Report,
) -> Result<ReportPair, Unbuilt> {
    let sent = sent.ok_or_else(|| Unbuilt::NoSentReport(before.clone()))?;
    let pair = ReportPair {
        order: order.clone(),
        segment: sent.report.segment.clone(),
        sent: sent.report.clone(),
        received,
        terms: sent.terms.clone(),
    };
    pair.check().map_err(|v| Unbuilt::Refused(v.0))?;
    Ok(pair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rutent_contracts::provider_role::{Band, Terms};

    fn asset(id: &str, decimals: u8) -> Denomination {
        Denomination {
            id: id.to_owned(),
            decimals,
        }
    }

    fn terms(reporting: Reporting, increment: Option<&str>) -> Terms {
        Terms {
            bands: vec![Band {
                min: "1".to_owned(),
                max: "9".to_owned(),
            }],
            increment: increment.map(str::to_owned),
            reporting,
            required_inputs: Vec::new(),
        }
    }

    fn amount(text: &str) -> Amount {
        Amount::new(text.to_owned()).unwrap()
    }

    // verifies: REQ-CAT-11, REQ-ORD-9
    #[test]
    fn a_net_tuple_without_an_increment_is_compared_exactly() {
        let usd = asset("USD", 2);
        let got = report_terms(&usd, &usd, &terms(Reporting::Net, None), "5").unwrap();
        assert_eq!(got.reporting, ReportedAs::Net {});
        assert_eq!(got.increment, None);
    }

    // verifies: REQ-CAT-11, REQ-ORD-9
    #[test]
    fn a_gross_tuples_charge_is_restated_in_the_out_unit_and_its_increment_rounded_down() {
        let (from, to) = (asset("USD", 2), asset("USD", 6));
        let got = report_terms(&from, &to, &terms(Reporting::Gross, Some("5")), "3").unwrap();
        assert_eq!(
            got.reporting,
            ReportedAs::Gross {
                charge: amount("30000")
            }
        );
        assert_eq!(got.increment.unwrap().amount(), &amount("50000"));
        // A coarser out unit: the charge rounds up, the increment down, and one that comes to nothing is exact.
        let (from, to) = (asset("USD", 6), asset("USD", 2));
        let got =
            report_terms(&from, &to, &terms(Reporting::Gross, Some("9999")), "15000").unwrap();
        assert_eq!(
            got.reporting,
            ReportedAs::Gross {
                charge: amount("2")
            }
        );
        assert_eq!(got.increment, None);
    }

    // verifies: REQ-CAT-11, REQ-ORD-9
    #[test]
    fn a_conversions_gross_figure_holds_no_charge_in_the_asset_it_gives_out() {
        let got = report_terms(
            &asset("COP", 2),
            &asset("USD", 2),
            &terms(Reporting::Gross, None),
            "7",
        )
        .unwrap();
        assert_eq!(
            got.reporting,
            ReportedAs::Gross {
                charge: amount("0")
            }
        );
    }

    // verifies: REQ-ORD-9
    #[test]
    fn a_charge_that_is_no_amount_is_an_unbuilt_pair() {
        let usd = asset("USD", 2);
        assert_eq!(
            report_terms(&usd, &usd, &terms(Reporting::Gross, None), "x"),
            Err(Unbuilt::NotAnAmount("the charge a gross figure holds"))
        );
    }

    fn receipt(index: u64) -> Receipt {
        serde_json::from_value(serde_json::json!({
            "log": "log-main",
            "index": index,
            "entry": { "algorithm": "sha-256", "value": "61".repeat(32) },
        }))
        .unwrap()
    }

    fn report(party: &str, segment: &str, amount: &str, index: u64) -> Report {
        Report {
            party: Id::new(party).unwrap(),
            amount: Amount::new(amount.to_owned()).unwrap(),
            asset: asset("USD", 2),
            segment: Id::new(segment).unwrap(),
            statement: receipt(index),
        }
    }

    fn sent(amount: &str, reporting: ReportedAs) -> Sent {
        Sent {
            report: report("a", "s1", amount, 1),
            terms: ReportTerms {
                increment: None,
                reporting,
            },
        }
    }

    // verifies: REQ-ORD-9
    #[test]
    fn a_pair_without_the_sent_report_or_holding_a_charge_above_it_is_unbuilt() {
        let (order, before) = (Id::new("o").unwrap(), Id::new("s1").unwrap());
        let received = report("b", "s2", "10", 2);
        assert_eq!(
            assemble(&order, &before, None, received.clone()),
            Err(Unbuilt::NoSentReport(before.clone()))
        );
        // A gross charge rounded up above what was sent: the contract refuses the pair.
        let gross = sent(
            "5",
            ReportedAs::Gross {
                charge: amount("6"),
            },
        );
        assert!(matches!(
            assemble(&order, &before, Some(&gross), received.clone()),
            Err(Unbuilt::Refused(_))
        ));
        let fine = sent("10", ReportedAs::Net {});
        assert!(assemble(&order, &before, Some(&fine), received).is_ok());
    }

    struct Refusing {
        refuse: usize,
        taken: Vec<Id>,
    }

    impl Comparisons for Refusing {
        fn report_pair(&mut self, pair: &ReportPair) -> Result<Receipt, AppendFailure> {
            if pair.received.segment.as_str() == format!("s{}", self.refuse) {
                return Err(AppendFailure::Refused(
                    rutent_contracts::core_internal::Violation("refused"),
                ));
            }
            self.taken.push(pair.received.segment.clone());
            Ok(receipt(9))
        }

        fn holding_reading(
            &mut self,
            _: &rutent_contracts::core_internal::HoldingReading,
        ) -> Result<Receipt, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
    }

    // verifies: REQ-ORD-9
    #[test]
    fn a_pair_the_log_refuses_is_parked_and_the_pairs_after_it_still_go() {
        let mut segments = Segments::default();
        let order = Id::new("o").unwrap();
        for n in 2..=4 {
            let pair = assemble(
                &order,
                &Id::new(format!("s{}", n - 1)).unwrap(),
                Some(&Sent {
                    report: report("a", &format!("s{}", n - 1), "10", 1),
                    terms: ReportTerms {
                        increment: None,
                        reporting: ReportedAs::Net {},
                    },
                }),
                report("b", &format!("s{n}"), "10", 2),
            )
            .unwrap();
            segments.pairing.waiting.push_back(pair);
        }
        let mut log = Refusing {
            refuse: 3,
            taken: Vec::new(),
        };
        assert_eq!(segments.write_pairs(&mut log), Ok(2));
        assert_eq!(log.taken.len(), 2, "the pair after the refused one went");
        assert_eq!(segments.unpaired(), 0);
        assert_eq!(
            segments.unbuilt_pairs(),
            [UnbuiltPair {
                order,
                segment: Id::new("s3").unwrap(),
                why: Unbuilt::Refused("refused"),
            }]
        );
    }
}
