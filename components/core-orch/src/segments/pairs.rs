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
//! skipped: the segment's end answers [`SegmentsError::Pair`], naming why, and building is tried again on the next
//! run. The only segments that make no pair are those that have none to make: the first segment of a route (a
//! recalculated route's included: a pair spans one plan), and one run by the provider that ran the segment before it
//! (the log's two-parties rule: one party's word compares nothing).

use std::collections::{BTreeSet, VecDeque};

use rutent_contracts::core_internal::calls::{AppendFailure, Comparisons};
use rutent_contracts::core_internal::{Receipt, Report, ReportPair, ReportTerms, ReportedAs};
use rutent_contracts::provider_role::{Denomination, Fee, Reporting, Terms, charge_in_out_unit};
use rutent_contracts::router_api::{Amount, Id, Operation, PositiveAmount};

use super::{Live, Segments, SegmentsError};

/// Why a segment's pair of reports couldn't be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unbuilt {
    /// Core holds no sent report of the segment before: its result that counted isn't in the flight.
    NoSentReport(Id),
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

/// The pairs built and not yet handed over.
#[derive(Debug, Clone, Default)]
pub(super) struct Pairing {
    waiting: VecDeque<ReportPair>,
    built: BTreeSet<(String, String)>,
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
    /// Keeps what the settled segment `at` reports having given out, with the terms it reports under, for the
    /// segment after it. `stated` is the receipt of the result that counted.
    pub(super) fn note_report(
        &mut self,
        at: &(String, String),
        operation: &Operation,
        terms: &Terms,
        stated: &Receipt,
        figures: (&str, &Fee),
    ) -> Result<(), SegmentsError> {
        let (amount_out, fee) = figures;
        let flight = self.flight(at)?;
        let (provider, segment) = (
            flight.provider.clone(),
            Id::new(&at.1).ok_or(SegmentsError::Plan("a segment is an id"))?,
        );
        let amount = Amount::new(amount_out.to_owned())
            .ok_or(Unbuilt::NotAnAmount("what the segment gave out"))
            .map_err(SegmentsError::Pair)?;
        let (from, to) = (&operation.from.asset, &operation.to.asset);
        let terms = report_terms(from, to, terms, &fee.amount).map_err(SegmentsError::Pair)?;
        self.flight_mut(at)?.reported = Some(Sent {
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

    /// Builds the pair of the segment before `at` and `at` itself, now that `at` settled: queued once, to be handed to
    /// the comparison ([`Segments::write_pairs`]). Nothing where `at` is its route's first segment, or the provider
    /// that ran the one before it ran it too; an error where the pair can't be built (`REQ-ORD-9`).
    pub(super) fn pair_with_previous(
        &mut self,
        live: &Live<'_>,
        at: &(String, String),
        operation: &Operation,
        stated: &Receipt,
        amount_in: &str,
    ) -> Result<(), SegmentsError> {
        let place = self.flight(at)?.place;
        let Some(before) = place.checked_sub(1).and_then(|p| live.segments.get(p)) else {
            return Ok(());
        };
        if self.pairing.built.contains(at) {
            return Ok(());
        }
        let before_at = (at.0.clone(), before.as_str().to_owned());
        let provider = self.flight(at)?.provider.clone();
        if self.flight(&before_at)?.provider == provider {
            return Ok(());
        }
        let sent = self
            .flights
            .get(&before_at)
            .and_then(|f| f.reported.clone())
            .ok_or(SegmentsError::Pair(Unbuilt::NoSentReport(before.clone())))?;
        let received = Report {
            party: provider,
            amount: Amount::new(amount_in.to_owned())
                .ok_or(Unbuilt::NotAnAmount("what the segment took in"))
                .map_err(SegmentsError::Pair)?,
            asset: operation.from.asset.clone(),
            segment: Id::new(&at.1).ok_or(SegmentsError::Plan("a segment is an id"))?,
            statement: stated.clone(),
        };
        let pair = ReportPair {
            order: live.order.clone(),
            segment: sent.report.segment.clone(),
            sent: sent.report,
            received,
            terms: sent.terms,
        };
        pair.check()
            .map_err(|v| SegmentsError::Pair(Unbuilt::Refused(v.0)))?;
        self.pairing.built.insert(at.clone());
        self.pairing.waiting.push_back(pair);
        Ok(())
    }

    /// Hands the comparison the pairs built since the last call, oldest first, and answers how many it took
    /// (`REQ-ORD-9`, `T-020-35`). A pair the log can't take now waits, with those after it, for the next call; one it
    /// refuses stays too and is the first failure answered, so a refusal is loud and never dropped.
    ///
    /// # Errors
    ///
    /// The first failure of the comparison's record, with the pairs after it left waiting.
    pub fn write_pairs(&mut self, comparisons: &mut dyn Comparisons) -> Result<usize, AppendFailure> {
        let mut written = 0;
        while let Some(pair) = self.pairing.waiting.front() {
            comparisons.report_pair(pair)?;
            self.pairing.waiting.pop_front();
            written += 1;
        }
        Ok(written)
    }

    /// How many pairs wait for [`Segments::write_pairs`].
    pub fn unpaired(&self) -> usize {
        self.pairing.waiting.len()
    }
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
        let got = report_terms(&from, &to, &terms(Reporting::Gross, Some("9999")), "15000").unwrap();
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
}
