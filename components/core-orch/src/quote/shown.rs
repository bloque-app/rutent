//! What the customer is shown of a route's price, before execution and after it (`REQ-QTE-2`, `REQ-QTE-7`,
//! `REQ-QTE-9`, `REQ-QTE-10`).

use rutent_contracts::core_internal::calls::{AppendFailure, Log};
use rutent_contracts::core_internal::{Actor, OrderRefusalClass, Outcome, Receipt, ShownAs, What};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{Fee, Reporting, to_canonical};
use rutent_contracts::router_api::{
    Amount, Candidate, DecisionRecord, Endpoint, FeeFormula, FigureValue, Id, NonEmpty, Operation,
    OperationKind, Part,
};
use rutent_router::quote::{
    Commitment, Executed, Firmness, PriceRefused, commitments, declared_times, executed_price,
    firmness, operator_fee, quoted_price,
};
use serde::{Deserialize, Serialize};

use super::journal::{self, Step};
use crate::snapshot::Clock;

/// What the customer is shown of a route's price: a quote before execution, the price after it.
///
/// Every difference between what the providers' terms deliver (`providers_out`, the sum of the quotes) and what the
/// customer receives (`shown_out`) is a line of its own: each charge a provider reports within what it gives out
/// (`within`), then the operator's own fee by its formula (`operator_fee`). `difference` is their sum, and nothing
/// else (`REQ-QTE-2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    /// How it is shown: a quote, a quote without a firm rate, or the price (`REQ-QTE-7`, `REQ-QTE-10`).
    pub shown_as: ShownAs,
    /// What leaves the origin, in its unit.
    pub amount_in: Amount,
    /// Each segment, in order, part after part, with its provider's fee and what it is held to.
    pub segments: Vec<SegmentShown>,
    /// What the providers' terms deliver at the destination, as they state it, in its unit: each part's last
    /// segment's `amount_out`, summed over the parts.
    pub providers_out: Amount,
    /// Each charge a part's last segment reports within what it gives out (its tuple reports `gross`), taken off
    /// `providers_out`: what it gives out less its charge is what arrives (`REQ-CAT-11`).
    pub within: Vec<ReportedWithin>,
    /// What arrives at the destination: `providers_out` less every charge in `within`.
    pub arrives: Amount,
    /// The operator's own fee, by its formula, on what arrives, shown apart (`REQ-QTE-2`).
    pub operator_fee: OperatorFee,
    /// What the customer receives: `arrives` less the operator's fee.
    pub shown_out: Amount,
    /// `providers_out` less `shown_out`: the charges in `within` and the operator's fee, and nothing else.
    pub difference: Amount,
    /// The segments over three days, which leave the route no firm rate (`REQ-QTE-10`); empty where it has one.
    pub no_firm_rate: Vec<SlowSegment>,
}

/// One segment of the route shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentShown {
    /// The part, by index.
    pub part: u32,
    /// The segment within the part, by index.
    pub operation: u32,
    /// The provider.
    pub provider: Id,
    /// Where it starts.
    pub from: Endpoint,
    /// Where it ends.
    pub to: Endpoint,
    /// What it takes in.
    pub amount_in: Amount,
    /// Its provider's fee, in the asset it takes in.
    pub fee: Fee,
    /// What it gives out, as its provider states it.
    pub amount_out: Amount,
    /// Whether its tuple reports what it gives out gross or net of its charge.
    pub reporting: Reporting,
    /// Its declared settlement time, in seconds from its start; `None` where no figure it cites declares one.
    pub declared_seconds: Option<u64>,
    /// What its provider is held to from its start (`REQ-QTE-9`).
    pub held_to: HeldTo,
}

/// What a segment's provider is held to from the segment's start (`REQ-QTE-9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HeldTo {
    /// It settles within three days: its fee is committed from its start, and where it sets a rate, the rate moves
    /// against the customer by at most `rate_bps`.
    Committed {
        /// The largest move of its rate against the customer, in basis points: the tighter of its quote's own
        /// regime and the operator's limit for its tuple; `None` for a segment that sets no rate.
        rate_bps: Option<u16>,
        /// The operator's slippage limit for its tuple, as the bounds the record ran under set it; `None` for a
        /// segment that sets no rate.
        operator_limit_bps: Option<u16>,
    },
    /// It settles over three days, so it is free of those bounds.
    Free,
}

/// A charge a segment reports within what it gives out, taken off what is shown as arriving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedWithin {
    /// The part, by index.
    pub part: u32,
    /// The segment within the part, by index.
    pub operation: u32,
    /// The provider.
    pub provider: Id,
    /// The charge, in the destination's unit.
    pub charge: Amount,
}

/// The operator's own fee, by its formula, shown apart (`REQ-QTE-2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorFee {
    /// The formula, from the bounds the route's record ran under (`PolicyBounds::operator_fee`).
    pub formula: FeeFormula,
    /// What it comes to on what arrives, in the destination's unit.
    pub amount: Amount,
}

/// A segment over three days, named (`REQ-QTE-10`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlowSegment {
    /// The part, by index.
    pub part: u32,
    /// The segment within the part, by index.
    pub operation: u32,
    /// The provider.
    pub provider: Id,
    /// Its tuple figure in the record's snapshot, by id.
    pub tuple: Id,
    /// Its declared settlement time, in seconds; `None` where nothing declares one, which counts as over.
    pub declared_seconds: Option<u64>,
}

/// Why a route's price isn't shown: it would show a price the route can't deliver, or a line it can't state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotShown {
    /// The candidate isn't one the record considered.
    NotTheRecords,
    /// A segment cites no tuple of its provider in the record's snapshot, so how it reports is unknown.
    NoTerms {
        /// The part, by index.
        part: u32,
        /// The segment within the part, by index.
        operation: u32,
    },
    /// What a segment hands on, net of a charge it reports within what it gives out, is less than the next segment
    /// was quoted to take in: the route can't deliver as composed.
    Undeliverable {
        /// The part, by index.
        part: u32,
        /// The segment within the part, by index, whose output falls short.
        operation: u32,
    },
    /// A segment settling within three days sets a rate, and the bounds set no slippage limit for its tuple.
    NoSlippageLimit {
        /// The part, by index.
        part: u32,
        /// The segment within the part, by index.
        operation: u32,
    },
    /// The operator's fee would be above what arrives.
    OperatorFeeAboveArrival,
    /// What the customer would receive is below the floor the request set.
    BelowFloor,
    /// A sum is past an amount's 78 digits.
    PastAnAmount,
    /// One result is cited for two segments, so the second hasn't executed on its own result (`REQ-QTE-7`).
    ResultCitedTwice {
        /// The part, by index.
        part: u32,
        /// The segment within the part, by index, whose result an earlier one cites.
        operation: u32,
    },
    /// A segment hasn't executed, so what is shown stays the quote (`REQ-QTE-7`).
    NotExecuted {
        /// The part, by index.
        part: u32,
        /// The segment within the part, by index.
        operation: u32,
    },
    /// The lines don't add up: what is shown differs from the providers' terms by more than the lines name.
    Unexplained,
}

impl NotShown {
    /// The class an order is refused with when its route can't be shown: `no_route_for_amount` where the amount
    /// is what fails it (the operator's fee, the floor, a sum), and `no_route_right_now` otherwise.
    pub fn class(&self) -> OrderRefusalClass {
        match self {
            NotShown::OperatorFeeAboveArrival | NotShown::BelowFloor | NotShown::PastAnAmount => {
                OrderRefusalClass::NoRouteForAmount
            }
            _ => OrderRefusalClass::NoRouteRightNow,
        }
    }
}

/// The quote shown for `candidate`, chosen by `record`, a choice round's: labelled a quote, or a quote without a
/// firm rate naming each segment over three days, with every difference from the providers' terms named, and never
/// below `floor` (`REQ-QTE-2`, `REQ-QTE-9`, `REQ-QTE-10`).
pub fn show(
    record: &DecisionRecord,
    candidate: &Candidate,
    floor: Option<&Amount>,
) -> Result<Shown, NotShown> {
    let legs: Vec<Leg<'_>> = indexed(candidate)
        .map(|(part, operation, evaluated)| Leg {
            part,
            operation,
            evaluated,
            amounts: Executed {
                amount_in: evaluated.amount_in.amount().clone(),
                fee: evaluated.fee.clone(),
                amount_out: evaluated.amount_out.clone(),
            },
        })
        .collect();
    let mut shown = compose(record, candidate, &legs, false, floor)?;
    // A segment over three days leaves the route no firm rate, and names it (`REQ-QTE-10`).
    if let Firmness::NoFirmRate { slow } = firmness(record, candidate) {
        shown.shown_as = ShownAs::QuoteWithoutFirmRate;
        // A slow segment the candidate doesn't hold can't be named: nothing is shown.
        shown.no_firm_rate = slow
            .into_iter()
            .map(|time| {
                Some(SlowSegment {
                    provider: provider_at(&legs, time.part, time.operation)?,
                    part: time.part,
                    operation: time.operation,
                    tuple: time.tuple,
                    declared_seconds: known(time.declared_seconds),
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(NotShown::NotTheRecords)?;
    }
    Ok(shown)
}

/// One segment as executed: its amounts as the result that counted for it states them, and the receipt of that
/// result's stated entry, which the price cites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedSegment {
    /// What it took in, charged and gave out.
    pub executed: Executed,
    /// The receipt of its result's stated entry.
    pub result: Receipt,
}

/// The price executed: the same lines from what each segment did, in order, part after part, labelled the price,
/// each segment's result cited once. Until every segment has executed it is refused, and what was shown stays the
/// quote (`REQ-QTE-7`).
pub fn executed(
    record: &DecisionRecord,
    candidate: &Candidate,
    segments: &[ExecutedSegment],
) -> Result<Shown, NotShown> {
    let mut legs = Vec::new();
    for (i, (part, operation, evaluated)) in indexed(candidate).enumerate() {
        let segment = segments
            .get(i)
            .ok_or(NotShown::NotExecuted { part, operation })?;
        // Each segment rests on its own result: one result cited for two is no execution of the second.
        if segments[..i].iter().any(|s| s.result == segment.result) {
            return Err(NotShown::ResultCitedTwice { part, operation });
        }
        legs.push(Leg {
            part,
            operation,
            evaluated,
            amounts: segment.executed.clone(),
        });
    }
    if segments.len() != legs.len() {
        return Err(NotShown::Unexplained);
    }
    compose(record, candidate, &legs, true, None)
}

/// One segment, where it sits, as the router evaluated it, and its amounts: quoted or executed.
struct Leg<'a> {
    part: u32,
    operation: u32,
    evaluated: &'a Operation,
    amounts: Executed,
}

/// Each operation of a candidate, with its part's and its own index.
fn indexed(candidate: &Candidate) -> impl Iterator<Item = (u32, u32, &Operation)> {
    candidate.parts.iter().enumerate().flat_map(|(p, part)| {
        part.operations
            .iter()
            .enumerate()
            .map(move |(o, operation)| (index(p), index(o), operation))
    })
}

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// A declared time, `None` where the router counts it unknown (`u64::MAX`).
fn known(seconds: u64) -> Option<u64> {
    (seconds != u64::MAX).then_some(seconds)
}

/// The provider of the segment at `part` and `operation`; `None` where no segment sits there.
fn provider_at(legs: &[Leg<'_>], part: u32, operation: u32) -> Option<Id> {
    legs.iter()
        .find(|l| l.part == part && l.operation == operation)
        .map(|l| l.evaluated.provider.clone())
}

/// How the tuple an operation runs through reports its charge: the terms of the tuple figure it cites, from its own
/// provider, in the record's snapshot.
fn reporting_of(record: &DecisionRecord, operation: &Operation) -> Option<Reporting> {
    operation.figures.iter().find_map(|id| {
        let figure = record
            .snapshot
            .figures
            .iter()
            .find(|f| &f.figure_id == id)?;
        if figure.origin.provider() != Some(&operation.provider) {
            return None;
        }
        match &figure.value {
            FigureValue::ReserveTuple { tuple } => Some(tuple.terms.reporting),
            FigureValue::RampTuple { tuple } => Some(tuple.terms.reporting),
            FigureValue::ExchangeTuple { tuple } => Some(tuple.terms.reporting),
            _ => None,
        }
    })
}

/// What a segment hands on, exactly as `segments::judge` hands it on, so nothing is shown arriving that the judge
/// wouldn't hand on: a conversion's output as it is, since its charge is in the asset it takes in, and its quote and
/// slippage bound it; a segment that keeps its asset by the judge's own rule (`REQ-CAT-11`, `T-020-08`). Under `net`
/// what it gives out, which with its charge is at most what it takes in; under `gross` what it gives out, at most
/// what it takes in, less its charge. `None` where the amounts break that: a segment claiming more out than it takes
/// in can't deliver it.
fn handed_on(kind: OperationKind, reporting: Reporting, amounts: &Executed) -> Option<Amount> {
    if kind == OperationKind::Exchange {
        return Some(amounts.amount_out.clone());
    }
    let handed = crate::segments::handed_on(
        reporting,
        amounts.amount_in.as_str(),
        amounts.amount_out.as_str(),
        &amounts.fee.amount,
    )?;
    Amount::new(handed)
}

/// The lines shown, from each segment's amounts, labelled the price where `executed`, and otherwise a quote.
fn compose(
    record: &DecisionRecord,
    candidate: &Candidate,
    legs: &[Leg<'_>],
    executed: bool,
    floor: Option<&Amount>,
) -> Result<Shown, NotShown> {
    if !record.candidates().iter().any(|c| c == candidate) || legs.is_empty() {
        return Err(NotShown::NotTheRecords);
    }
    let mut amount_in = Amount::zero();
    let mut providers_out = Amount::zero();
    let mut arrives = Amount::zero();
    let mut within = Vec::new();
    let mut handed = Vec::with_capacity(legs.len());
    for (i, leg) in legs.iter().enumerate() {
        let (part, operation) = (leg.part, leg.operation);
        let reporting =
            reporting_of(record, leg.evaluated).ok_or(NotShown::NoTerms { part, operation })?;
        let hands_on = handed_on(leg.evaluated.kind, reporting, &leg.amounts)
            .ok_or(NotShown::Undeliverable { part, operation })?;
        if operation == 0 {
            amount_in = amount_in
                .checked_add(&leg.amounts.amount_in)
                .ok_or(NotShown::PastAnAmount)?;
        }
        match legs.get(i + 1).filter(|next| next.part == part) {
            // The next segment was quoted, or ran, at what it takes in: this one must hand that on.
            Some(next) => {
                if next.amounts.amount_in > hands_on {
                    return Err(NotShown::Undeliverable { part, operation });
                }
            }
            // A part's last segment: what it gives out, and what of that arrives.
            None => {
                providers_out = providers_out
                    .checked_add(&leg.amounts.amount_out)
                    .ok_or(NotShown::PastAnAmount)?;
                arrives = arrives
                    .checked_add(&hands_on)
                    .ok_or(NotShown::PastAnAmount)?;
                if hands_on < leg.amounts.amount_out {
                    within.push(ReportedWithin {
                        part,
                        operation,
                        provider: leg.evaluated.provider.clone(),
                        charge: leg
                            .amounts
                            .amount_out
                            .checked_sub(&hands_on)
                            .ok_or(NotShown::Unexplained)?,
                    });
                }
            }
        }
        handed.push(hands_on);
    }

    // The operator's fee on what arrives, by the formula of the bounds the record ran under, through the router's
    // own price, so the floor the router judged and the price shown can't disagree.
    // Each part's last segment gives out what arrives of it; the rest as they are.
    let formula = &record.bounds.operator_fee;
    let mut amounts = Vec::with_capacity(legs.len());
    let mut parts: Vec<Vec<Operation>> = vec![Vec::new(); candidate.parts.len()];
    for (i, leg) in legs.iter().enumerate() {
        let last = legs.get(i + 1).is_none_or(|next| next.part != leg.part);
        let out = if last {
            handed[i].clone()
        } else {
            leg.amounts.amount_out.clone()
        };
        let part = parts
            .get_mut(leg.part as usize)
            .ok_or(NotShown::NotTheRecords)?;
        part.push(Operation {
            amount_out: out.clone(),
            ..leg.evaluated.clone()
        });
        amounts.push(Executed {
            amount_out: out,
            ..leg.amounts.clone()
        });
    }
    let arriving = Candidate {
        parts: NonEmpty::new(
            parts
                .into_iter()
                .map(|operations| NonEmpty::new(operations).map(|operations| Part { operations }))
                .collect::<Option<Vec<Part>>>()
                .ok_or(NotShown::NotTheRecords)?,
        )
        .ok_or(NotShown::NotTheRecords)?,
        ..candidate.clone()
    };
    let price = if executed {
        executed_price(&arriving, &amounts, formula)
    } else {
        quoted_price(&arriving, formula)
    }
    .map_err(|refused| match refused {
        PriceRefused::OperatorFeeAboveArrival { .. } => NotShown::OperatorFeeAboveArrival,
        PriceRefused::PastAnAmount => NotShown::PastAnAmount,
        PriceRefused::NotExecuted { part, operation } => NotShown::NotExecuted { part, operation },
    })?;
    let operator_fee = operator_fee(formula, &arrives).ok_or(NotShown::PastAnAmount)?;
    let shown_out = arrives
        .checked_sub(&operator_fee)
        .ok_or(NotShown::OperatorFeeAboveArrival)?;
    // Nothing shown but what the lines explain (`REQ-QTE-2`).
    price
        .check_shown(&shown_out)
        .map_err(|_| NotShown::Unexplained)?;
    let named = within
        .iter()
        .try_fold(operator_fee.clone(), |sum, w| sum.checked_add(&w.charge))
        .ok_or(NotShown::PastAnAmount)?;
    let difference = providers_out
        .checked_sub(&shown_out)
        .ok_or(NotShown::Unexplained)?;
    if difference != named || price.providers_out != arrives {
        return Err(NotShown::Unexplained);
    }
    if floor.is_some_and(|floor| &shown_out < floor) {
        return Err(NotShown::BelowFloor);
    }

    // What each segment is held to from its start (`REQ-QTE-9`): its fee committed and its rate within the tighter
    // of its quote's slippage and the operator's limit within three days, free past them.
    let times = declared_times(record, candidate);
    let held = commitments(record, candidate, &record.bounds.slippage_limits);
    let mut segments = Vec::with_capacity(legs.len());
    for ((leg, time), (_, held)) in legs.iter().zip(times).zip(held) {
        let held_to = match held {
            Ok(Commitment::Free) => HeldTo::Free,
            Ok(Commitment::Bound { rate_bps }) => HeldTo::Committed {
                rate_bps,
                operator_limit_bps: rate_bps.and_then(|_| operator_limit(record, leg.evaluated)),
            },
            Err(_) => {
                return Err(NotShown::NoSlippageLimit {
                    part: leg.part,
                    operation: leg.operation,
                });
            }
        };
        segments.push(SegmentShown {
            part: leg.part,
            operation: leg.operation,
            provider: leg.evaluated.provider.clone(),
            from: leg.evaluated.from.clone(),
            to: leg.evaluated.to.clone(),
            amount_in: leg.amounts.amount_in.clone(),
            fee: leg.amounts.fee.clone(),
            amount_out: leg.amounts.amount_out.clone(),
            reporting: reporting_of(record, leg.evaluated).ok_or(NotShown::NoTerms {
                part: leg.part,
                operation: leg.operation,
            })?,
            declared_seconds: known(time.declared_seconds),
            held_to,
        });
    }
    Ok(Shown {
        shown_as: if executed {
            ShownAs::Price
        } else {
            ShownAs::Quote
        },
        amount_in,
        segments,
        providers_out,
        within,
        arrives,
        operator_fee: OperatorFee {
            formula: formula.clone(),
            amount: operator_fee,
        },
        shown_out,
        difference,
        no_firm_rate: Vec::new(),
    })
}

/// The operator's slippage limit for an operation's tuple, matched on its provider and ends as the router's
/// `commitments` matches it.
fn operator_limit(record: &DecisionRecord, operation: &Operation) -> Option<u16> {
    record
        .bounds
        .slippage_limits
        .iter()
        .find(|l| {
            l.tuple.provider == operation.provider
                && l.tuple.from == operation.from
                && l.tuple.to == operation.to
        })
        .map(|l| l.max_bps.get())
}

/// The digest of what is shown: SHA-256 of its RFC 8785 canonical JSON, the recorded decision's own rule.
pub fn digest<T: Serialize>(shown: &T) -> Option<Digest> {
    let canonical = to_canonical(shown).ok()?;
    let value = rutent_prov::module::wire::sha256(&canonical);
    serde_json::from_value(serde_json::json!({ "algorithm": "sha-256", "value": value })).ok()
}

/// An order's executed price, as reported to its customer: the order, the record and candidate it ran, the receipt
/// of each segment's result that counted, and the price from them. It is made only by [`Priced::new`], from
/// [`executed`], so nothing labelled the price stands on anything but every segment's result (`REQ-QTE-7`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Priced {
    order: Id,
    decision_record: Id,
    candidate: Id,
    results: Vec<Receipt>,
    price: Shown,
}

impl Priced {
    /// `order`'s price, from each segment of `candidate`, chosen by `record`, as executed: refused until every
    /// segment has executed, each on its own result.
    pub fn new(
        order: &Id,
        record: &DecisionRecord,
        candidate: &Candidate,
        segments: &[ExecutedSegment],
    ) -> Result<Self, NotShown> {
        let price = executed(record, candidate, segments)?;
        Ok(Priced {
            order: order.clone(),
            decision_record: record.record_id().clone(),
            candidate: candidate.candidate_id.clone(),
            results: segments.iter().map(|s| s.result.clone()).collect(),
            price,
        })
    }

    /// The order.
    pub fn order(&self) -> &Id {
        &self.order
    }

    /// The receipts of the results it rests on, one per segment, in order.
    pub fn results(&self) -> &[Receipt] {
        &self.results
    }

    /// The price.
    pub fn price(&self) -> &Shown {
        &self.price
    }
}

/// Why a price couldn't be reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PriceError {
    /// What was handed in isn't labelled the price.
    NotThePrice,
    /// Its digest couldn't be taken.
    Undigestable,
    /// Another price was reported for the order already: an order has one price.
    AnotherPrice,
    /// The log didn't take its `shown` entry; the entry is kept and appended as built on the next call.
    Append(AppendFailure),
}

impl From<AppendFailure> for PriceError {
    fn from(failure: AppendFailure) -> Self {
        PriceError::Append(failure)
    }
}

/// Core's record of each order's price reported: one `shown` entry of `price` per order, built once.
#[derive(Debug, Default)]
pub struct Prices {
    reported: std::collections::BTreeMap<String, (Digest, Step)>,
}

impl Prices {
    /// No price reported yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports `priced` as its order's price: a `shown` entry, by `core` at Core's clock, naming its digest and
    /// `price` (`REQ-QTE-7`). Reported again, the entry first built is appended, or its receipt answered.
    pub fn report(
        &mut self,
        log: &mut dyn Log,
        clock: &dyn Clock,
        core: &Actor,
        priced: &Priced,
    ) -> Result<(Digest, Receipt), PriceError> {
        // Made only from every segment's result; checked again, so nothing else is reported as the price.
        if priced.price.shown_as != ShownAs::Price
            || priced.results.len() != priced.price.segments.len()
        {
            return Err(PriceError::NotThePrice);
        }
        let digest = digest(priced).ok_or(PriceError::Undigestable)?;
        let key = priced.order.as_str().to_owned();
        if !self.reported.contains_key(&key) {
            let entry = journal::built(
                core,
                What::Shown {
                    order: priced.order.clone(),
                    shown: digest.clone(),
                    shown_as: ShownAs::Price,
                },
                &journal::now(clock)?,
                Outcome::Observed {},
            )?;
            self.reported
                .insert(key.clone(), (digest.clone(), Step::new(entry)));
        }
        let (reported, step) = self
            .reported
            .get_mut(&key)
            .ok_or(PriceError::Undigestable)?;
        if reported != &digest {
            return Err(PriceError::AnotherPrice);
        }
        let receipt = step.append(log)?;
        Ok((digest, receipt))
    }
}
