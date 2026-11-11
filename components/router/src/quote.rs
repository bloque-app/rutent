//! The quote rules, as pure checks over the router's records and the providers' statements (`REQ-QTE-1`,
//! `REQ-QTE-2`, `REQ-QTE-3`, `REQ-QTE-5`, `REQ-QTE-6`, `REQ-QTE-7`, `REQ-QTE-9`, `REQ-QTE-10`, `REQ-QTE-15`).
//!
//! Like [`route`](crate::RouteFinder), none of these reads a clock, a provider or anything but its arguments:
//! a time enters as a [`Timestamp`] the caller passes, so each answer recomputes from what it was given.
//!
//! - [`quote_fields`]: a fresh quote states its provider, segment, amount, direction, fee, validity and what voids
//!   it, and a conversion's quote its rate and slippage regime; one lacking any is refused (`REQ-QTE-1`).
//! - [`quoted_price`] and [`executed_price`]: what the customer is shown, with the providers' terms, the operator's
//!   own fee by its formula and the difference between them stated apart, labelled a quote before execution and
//!   the price after it (`REQ-QTE-2`, `REQ-QTE-7`).
//! - [`declared_times`]: each segment's declared settlement time, measurable against the actual one
//!   (`REQ-QTE-3`).
//! - [`formula_fee`] and [`check_charge`]: the fee a formula gives before use, and a charge above it, or above its
//!   maximum where the charge is known only after execution, flagged (`REQ-QTE-5`, `AC-14`).
//! - [`binding`]: the formula and slippage that bind a provider are the ones stated when its segment starts
//!   (`REQ-QTE-6`).
//! - [`commitments`] and [`rate_within`]: a segment settling within three days has its fees committed and its rate
//!   held within the operator's slippage for its tuple; a longer one is free of both (`REQ-QTE-9`).
//! - [`firmness`] and [`may_start`]: a route with a segment over three days has no firm rate, names that segment,
//!   and starts only once the customer accepts it (`REQ-QTE-10`, `AC-13`).
//! - [`hold`] and [`redeem`]: a quote held before its route starts, with a stated validity, and refused past it
//!   (`REQ-QTE-15`).
//!
//! The arguments these take that a decision record doesn't hold are router-api's types (`T-020-02`, `DEC-2`), so
//! Core reaches them through the contract: the operator's fee formula and the slippage limits, which the
//! [`PolicyBounds`](rutent_contracts::router_api::PolicyBounds) carry, so a record names the bounds it ran under;
//! a provider's [`StatedTerms`] with their times; a segment's [`SettlementTimes`], declared and actual; and the
//! [`HeldQuote`]. The checks stay here, as public functions.

use num_bigint::BigUint;
use rutent_contracts::provider_role::{
    ConversionOutcome, Direction, Fee, ResultBody, Slippage, TransferOutcome, VoidedBy,
};
pub use rutent_contracts::router_api::THREE_DAYS_SECONDS;
use rutent_contracts::router_api::{
    Amount, Candidate, Decimal, DecisionRecord, FeeFormula, Figure, FigureValue, HeldQuote, Id,
    Operation, OperationKind, PositiveAmount, SettlementTimes, SlippageLimit, StatedTerms,
    Timestamp, Violation,
};

use crate::price::{fraction, number};
use crate::search::{Search, answer_of};

/// How a price shown to the customer is labelled (`REQ-QTE-7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// Shown before execution: a quote, not a promise.
    Quote,
    /// The final executed price, once every segment has executed.
    Price,
}

// ---------------------------------------------------------------------------------------------------------------
// REQ-QTE-1: a quote's fields.

/// The elements a fresh quote states (`REQ-QTE-1`), read from its figure and its provider's signed result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteFields<'a> {
    /// The provider, who made and signed the quote.
    pub provider: &'a Id,
    /// The segment it quotes: its tuple figure, by id.
    pub tuple: &'a Id,
    /// What the provider would take in.
    pub amount_in: PositiveAmount,
    /// What it would give out.
    pub amount_out: Amount,
    /// Which amount the quote fixes.
    pub direction: Direction,
    /// Its fee, in the asset the segment takes in.
    pub fee: Fee,
    /// Until when the quote holds.
    pub valid_until: Timestamp,
    /// What voids it: at least one condition.
    pub voided_by: &'a [VoidedBy],
    /// Its rate and slippage regime, for a conversion's quote, whose category sets a rate.
    pub rate: Option<(Decimal, &'a Slippage)>,
    /// The declared settlement time, in seconds from segment start (`REQ-QTE-3`).
    pub settlement_seconds: u64,
}

/// An element a quote must state (`REQ-QTE-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteField {
    /// Its provider: a quote's figure comes from a provider.
    Provider,
    /// Its amount in, a positive whole number.
    AmountIn,
    /// Its amount out, a whole number.
    AmountOut,
    /// Its fee: an asset, and a whole number of it.
    Fee,
    /// Its validity: a `valid_until` time, and the figure's validity in seconds.
    Validity,
    /// What voids it: at least one condition.
    VoidConditions,
    /// Its rate, for a conversion's quote.
    Rate,
}

/// Why a figure is refused as a quote (`REQ-QTE-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteRefused {
    /// It isn't a fresh quote: another figure, or a result that quotes nothing.
    NotAQuote,
    /// It lacks an element, or states one malformed.
    Lacks(QuoteField),
    /// Its statement was made by another party than the figure's provider, so the provider can't be held to it.
    NotAttributable,
}

/// The elements a fresh quote states, or why it is refused (`REQ-QTE-1`).
///
/// A quote is attributable to its provider by its envelope's `maker`, which must be the figure's provider. That the
/// signature verifies under one of the provider's keys is Core's to check before the snapshot is formed (router-api
/// README, "Unchecked by either"); a router given only the snapshot holds no keys.
pub fn quote_fields(figure: &Figure) -> Result<QuoteFields<'_>, QuoteRefused> {
    use QuoteRefused::{Lacks, NotAQuote, NotAttributable};
    let FigureValue::FreshQuote { tuple, quote } = &figure.value else {
        return Err(NotAQuote);
    };
    let provider = figure
        .origin
        .provider()
        .ok_or(Lacks(QuoteField::Provider))?;
    if quote.maker != provider.as_str() {
        return Err(NotAttributable);
    }
    let (q, rate) = match &quote.statement {
        ResultBody::Transfer(result) => match &result.outcome {
            TransferOutcome::DryRunAccepted(q) => (
                Stated {
                    amount_in: &q.amount_in,
                    amount_out: &q.amount_out,
                    fee: &q.fee,
                    direction: q.direction,
                    valid_until: &q.valid_until,
                    voided_by: &q.voided_by,
                    settlement_seconds: q.settlement_seconds.get(),
                },
                None,
            ),
            _ => return Err(NotAQuote),
        },
        ResultBody::Conversion(result) => match &result.outcome {
            ConversionOutcome::DryRunAccepted(q) => (
                Stated {
                    amount_in: &q.amount_in,
                    amount_out: &q.amount_out,
                    fee: &q.fee,
                    direction: q.direction,
                    valid_until: &q.valid_until,
                    voided_by: &q.voided_by,
                    settlement_seconds: q.settlement_seconds.get(),
                },
                Some((&q.rate, &q.slippage)),
            ),
            _ => return Err(NotAQuote),
        },
        _ => return Err(NotAQuote),
    };
    let amount_in = PositiveAmount::new(q.amount_in).ok_or(Lacks(QuoteField::AmountIn))?;
    let amount_out = Amount::new(q.amount_out).ok_or(Lacks(QuoteField::AmountOut))?;
    if q.fee.asset.is_empty() || Amount::new(q.fee.amount.as_str()).is_none() {
        return Err(Lacks(QuoteField::Fee));
    }
    let valid_until = Timestamp::new(q.valid_until)
        .filter(|_| figure.valid_for_seconds.is_some())
        .ok_or(Lacks(QuoteField::Validity))?;
    if q.voided_by.is_empty() {
        return Err(Lacks(QuoteField::VoidConditions));
    }
    let rate = match rate {
        Some((rate, slippage)) => Some((
            Decimal::new(rate.as_str()).ok_or(Lacks(QuoteField::Rate))?,
            slippage,
        )),
        None => None,
    };
    Ok(QuoteFields {
        provider,
        tuple,
        amount_in,
        amount_out,
        direction: q.direction,
        fee: q.fee.clone(),
        valid_until,
        voided_by: q.voided_by,
        rate,
        settlement_seconds: q.settlement_seconds,
    })
}

/// The elements a transfer's and a conversion's quote share.
struct Stated<'a> {
    amount_in: &'a str,
    amount_out: &'a str,
    fee: &'a Fee,
    direction: Direction,
    valid_until: &'a str,
    voided_by: &'a [VoidedBy],
    settlement_seconds: u64,
}

// ---------------------------------------------------------------------------------------------------------------
// REQ-QTE-2, REQ-QTE-7: the price shown.

/// One operation's provider fee, as shown (`REQ-QTE-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFee {
    /// The part, by index.
    pub part: u32,
    /// The operation within the part, by index.
    pub operation: u32,
    /// The provider.
    pub provider: Id,
    /// Its fee, in the asset and unit the operation takes in.
    pub fee: Fee,
}

/// What the customer is shown (`REQ-QTE-2`, `REQ-QTE-7`): what leaves the origin, each provider's fee, what the
/// providers' terms deliver, the operator's own fee apart from them, and what the customer receives.
///
/// The providers' terms compose: each segment's price is applied to what reaches it (`REQ-QTE-8`, read as
/// composition by the owner, item `spec-route-price-composition`), so `providers_out` is what the last operation of
/// each part delivers, summed over the parts. The operator's fee is taken from that, in the destination's unit, so
/// the only difference between the providers' terms and what is shown is [`ShownPrice::difference`], which is the
/// operator's fee and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownPrice {
    /// A quote before execution, the price after it (`REQ-QTE-7`).
    pub label: Label,
    /// What leaves the origin, in its unit.
    pub amount_in: Amount,
    /// Each operation's fee, in order, part after part.
    pub provider_fees: Vec<ProviderFee>,
    /// What the providers' terms deliver at the destination, in its unit.
    pub providers_out: Amount,
    /// The operator's own fee, by its formula on `providers_out`, in the destination's unit: `flat +
    /// ceil(providers_out × numerator / denominator)`, or its maximum where that is lower.
    pub operator_fee: Amount,
    /// What the customer receives: `providers_out` less `operator_fee`.
    pub shown_out: Amount,
}

impl ShownPrice {
    /// The difference between the providers' terms and the price shown: the operator's fee.
    pub fn difference(&self) -> Amount {
        let difference =
            number(&self.providers_out) - number(&self.shown_out).min(number(&self.providers_out));
        to_amount(&difference)
    }

    /// Whether `shown`, an amount shown to the customer as arriving, is this price's, with no difference beyond the
    /// operator's fee.
    pub fn check_shown(&self, shown: &Amount) -> Result<(), UnexplainedDifference> {
        if shown == &self.shown_out {
            Ok(())
        } else {
            Err(UnexplainedDifference {
                shown: shown.clone(),
                explained: self.shown_out.clone(),
            })
        }
    }
}

/// An amount shown to the customer that the providers' terms and the operator's fee don't explain (`REQ-QTE-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnexplainedDifference {
    /// What was shown.
    pub shown: Amount,
    /// What the providers' terms less the operator's fee give.
    pub explained: Amount,
}

/// What one operation did, as its provider's result states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executed {
    /// What left the origin, in its unit.
    pub amount_in: Amount,
    /// The fee charged.
    pub fee: Fee,
    /// What arrived, in the destination's unit.
    pub amount_out: Amount,
}

/// Why no price can be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PriceRefused {
    /// The operator's fee would be above what the providers deliver.
    OperatorFeeAboveArrival {
        /// The operator's fee.
        operator_fee: Amount,
        /// What the providers deliver.
        providers_out: Amount,
    },
    /// A sum of the amounts is past an amount's 78 digits.
    PastAnAmount,
    /// An operation hasn't executed, so what is shown stays a quote (`REQ-QTE-7`).
    NotExecuted {
        /// The part, by index.
        part: u32,
        /// The operation within the part, by index.
        operation: u32,
    },
}

/// The price shown before execution: a quote, from the candidate's operations as the router evaluated them, with
/// the operator's fee by `operator`'s formula (`REQ-QTE-2`, `REQ-QTE-7`). Core passes the formula of the bounds
/// the candidate's record ran under, [`PolicyBounds::operator_fee`](rutent_contracts::router_api::PolicyBounds),
/// the one the router judged the floor against.
pub fn quoted_price(
    candidate: &Candidate,
    operator: &FeeFormula,
) -> Result<ShownPrice, PriceRefused> {
    let operations = indexed(candidate).map(|(part, operation, o)| {
        (
            part,
            operation,
            Executed {
                amount_in: o.amount_in.amount().clone(),
                fee: o.fee.clone(),
                amount_out: o.amount_out.clone(),
            },
            o,
        )
    });
    shown(operations.collect(), operator, Label::Quote)
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

/// A whole number known to be within an amount's 78 digits.
fn to_amount(n: &BigUint) -> Amount {
    Amount::new(n.to_string()).unwrap_or_else(Amount::zero)
}

/// The price shown from each operation's amounts, in order, with the operation it was evaluated as.
fn shown(
    operations: Vec<(u32, u32, Executed, &Operation)>,
    operator: &FeeFormula,
    label: Label,
) -> Result<ShownPrice, PriceRefused> {
    let mut amount_in = Amount::zero();
    let mut providers_out = Amount::zero();
    let mut provider_fees = Vec::with_capacity(operations.len());
    let last_of_part = |i: usize| {
        operations
            .get(i + 1)
            .is_none_or(|(part, ..)| *part != operations[i].0)
    };
    for (i, (part, operation, executed, evaluated)) in operations.iter().enumerate() {
        if *operation == 0 {
            amount_in = amount_in
                .checked_add(&executed.amount_in)
                .ok_or(PriceRefused::PastAnAmount)?;
        }
        if last_of_part(i) {
            providers_out = providers_out
                .checked_add(&executed.amount_out)
                .ok_or(PriceRefused::PastAnAmount)?;
        }
        provider_fees.push(ProviderFee {
            part: *part,
            operation: *operation,
            provider: evaluated.provider.clone(),
            fee: executed.fee.clone(),
        });
    }
    let arrives = number(&providers_out);
    let operator_fee = operator_fee_of(operator, &arrives);
    if operator_fee > arrives {
        return Err(PriceRefused::OperatorFeeAboveArrival {
            operator_fee: Amount::new(operator_fee.to_string())
                .ok_or(PriceRefused::PastAnAmount)?,
            providers_out,
        });
    }
    Ok(ShownPrice {
        label,
        amount_in,
        provider_fees,
        shown_out: to_amount(&(&arrives - &operator_fee)),
        operator_fee: to_amount(&operator_fee),
        providers_out,
    })
}

/// The final executed price: from what each operation did, given in order, part after part, with the operator's
/// fee by `operator`'s formula. Until every operation has executed it is refused, and what is shown stays the
/// quote (`REQ-QTE-7`).
pub fn executed_price(
    candidate: &Candidate,
    executed: &[Executed],
    operator: &FeeFormula,
) -> Result<ShownPrice, PriceRefused> {
    let mut operations = Vec::new();
    for (i, (part, operation, evaluated)) in indexed(candidate).enumerate() {
        let done = executed
            .get(i)
            .ok_or(PriceRefused::NotExecuted { part, operation })?;
        operations.push((part, operation, done.clone(), evaluated));
    }
    shown(operations, operator, Label::Price)
}

// ---------------------------------------------------------------------------------------------------------------
// REQ-QTE-3: declared settlement time.

/// One segment's declared settlement time (`REQ-QTE-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentTime {
    /// The part, by index.
    pub part: u32,
    /// The operation within the part, by index.
    pub operation: u32,
    /// The segment's tuple figure, by id.
    pub tuple: Id,
    /// The declared settlement time, in seconds from segment start: from the usable fresh quote the operation
    /// cites, the slowest if it cites several, or else from its price, as the router's `fastest_settlement` counts
    /// it.
    pub declared_seconds: u64,
}

/// A segment's declared settlement time against its actual one (`REQ-QTE-3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    /// The declared time, in seconds.
    pub declared_seconds: u64,
    /// The actual time from start to settlement, in seconds, rounded up.
    pub actual_seconds: u64,
    /// How far the actual time passed the declared one; zero if it didn't.
    pub over_seconds: u64,
}

impl SegmentTime {
    /// This segment's declared time and its actual one, from when it started to when it settled: the pair Core
    /// keeps for it (`REQ-QTE-3`). `None` if the declared time is unknown ([`declared_times`]), past a safe
    /// integer.
    pub fn settled(
        &self,
        started_at: &Timestamp,
        settled_at: &Timestamp,
    ) -> Option<SettlementTimes> {
        Some(SettlementTimes {
            declared_seconds: self.declared_seconds.try_into().ok()?,
            started_at: started_at.clone(),
            settled_at: settled_at.clone(),
        })
    }

    /// This segment's declared time against the actual one, from when it started to when it settled; `None` if it
    /// settled before it started, or its declared time is unknown.
    pub fn measure(&self, started_at: &Timestamp, settled_at: &Timestamp) -> Option<Measured> {
        measure(&self.settled(started_at, settled_at)?)
    }
}

/// A segment's declared settlement time against its actual one, rounded up to whole seconds (`REQ-QTE-3`);
/// `None` if it settled before it started, which [`SettlementTimes::check`] refuses.
pub fn measure(times: &SettlementTimes) -> Option<Measured> {
    let nanos = times
        .settled_at
        .unix_nanos()
        .checked_sub(times.started_at.unix_nanos())?;
    if nanos < 0 {
        return None;
    }
    let actual_seconds = u64::try_from((nanos + 999_999_999) / 1_000_000_000).ok()?;
    let declared_seconds = times.declared_seconds.get();
    Some(Measured {
        declared_seconds,
        actual_seconds,
        over_seconds: actual_seconds.saturating_sub(declared_seconds),
    })
}

/// Every segment's declared settlement time, in order, part after part, read from the record alone (`REQ-QTE-3`).
/// An operation that cites neither a usable fresh quote nor a price, which no record the contract accepts holds,
/// counts as never settling, `u64::MAX`, so no route is taken as firmer than its record shows.
pub fn declared_times(record: &DecisionRecord, candidate: &Candidate) -> Vec<SegmentTime> {
    let search = Search::new(&record.request, &record.snapshot, &record.bounds);
    indexed(candidate)
        .map(|(part, operation, o)| {
            let cited: Vec<&Figure> = o
                .figures
                .iter()
                .filter_map(|id| search.figure(id.as_str()))
                .collect();
            let tuple = cited
                .iter()
                .find(|f| {
                    matches!(
                        f.value,
                        FigureValue::ReserveTuple { .. }
                            | FigureValue::ExchangeTuple { .. }
                            | FigureValue::RampTuple { .. }
                    )
                })
                .map_or_else(|| o.figures.first().clone(), |f| f.figure_id.clone());
            let quoted = cited
                .iter()
                .filter(|f| matches!(f.value, FigureValue::FreshQuote { .. }))
                .filter(|f| search.is_usable(f))
                .filter_map(|f| answer_of(f).map(|a| a.settlement_seconds))
                .max();
            let priced = cited.iter().find_map(|f| match &f.value {
                FigureValue::Price {
                    settlement_seconds, ..
                } => Some(settlement_seconds.get()),
                _ => None,
            });
            SegmentTime {
                part,
                operation,
                tuple,
                declared_seconds: quoted.or(priced).unwrap_or(u64::MAX),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// REQ-QTE-5, AC-14: formula against charge.

/// The fee `formula` gives on `amount_in`, before use: `flat + ceil(amount_in × numerator / denominator)`, its
/// maximum aside; `None` if it is past an amount's 78 digits (`REQ-QTE-5`).
pub fn formula_fee(formula: &FeeFormula, amount_in: &Amount) -> Option<Amount> {
    Amount::new(formula_fee_of(formula, &number(amount_in)).to_string())
}

/// The operator's fee on what arrives at the destination (`REQ-QTE-2`): `operator`'s formula, capped by its
/// maximum where it gives one, but never raised to it; `None` if it is past an amount's 78 digits. Unlike
/// [`formula_fee`], which leaves a maximum aside to check a provider's charge, this is the fee the customer is
/// shown and the floor is judged after.
pub fn operator_fee(operator: &FeeFormula, arrives: &Amount) -> Option<Amount> {
    Amount::new(operator_fee_of(operator, &number(arrives)).to_string())
}

/// What the customer receives at the destination after the operator's fee (`REQ-QTE-2`, `REQ-ORD-27`): what
/// arrives less [`operator_fee`], and nothing where the fee would take it all. The router judges the floor on it,
/// the price shown is it, and Core's checks at each settled segment (`AC-42`) read it from here, so all three
/// agree.
pub fn received_after_operator_fee(operator: &FeeFormula, arrives: &Amount) -> Amount {
    to_amount(&received_of(operator, &number(arrives)))
}

/// What is left of `arrives` after the operator's fee, never below nothing.
pub(crate) fn received_of(operator: &FeeFormula, arrives: &BigUint) -> BigUint {
    let fee = operator_fee_of(operator, arrives);
    if &fee <= arrives {
        arrives - fee
    } else {
        BigUint::ZERO
    }
}

/// The operator's fee on what arrives: its formula's fee, capped by its maximum where it gives one, but never
/// raised to it (`REQ-QTE-2`). The price shown takes it, and the router judges the floor after it, so the two
/// can't disagree.
pub(crate) fn operator_fee_of(operator: &FeeFormula, arrives: &BigUint) -> BigUint {
    let fee = formula_fee_of(operator, arrives);
    match &operator.maximum {
        Some(maximum) => fee.min(number(maximum)),
        None => fee,
    }
}

/// `flat + ceil(amount_in × numerator / denominator)`, exactly.
fn formula_fee_of(formula: &FeeFormula, amount_in: &BigUint) -> BigUint {
    let numerator = number(&formula.proportional.numerator);
    let denominator = number(formula.proportional.denominator.amount());
    number(&formula.flat) + (amount_in * numerator + &denominator - 1u8) / denominator
}

/// The most a charge under `formula` may come to on `amount_in`: its maximum where it gives one, and else its fee.
fn charge_bound(formula: &FeeFormula, amount_in: &BigUint) -> BigUint {
    match &formula.maximum {
        Some(maximum) => number(maximum),
        None => formula_fee_of(formula, amount_in),
    }
}

/// What a charge is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// The formula's fee on the amount.
    Formula,
    /// The formula's maximum, for a charge known only after execution.
    Maximum,
}

/// A charge above what its formula allows, flagged to the operator (`REQ-QTE-5`, `AC-14`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargeFlag {
    /// What was charged.
    pub charged: Amount,
    /// The most the formula allows.
    pub bound: Amount,
    /// Which bound it is.
    pub against: Bound,
    /// How far above it the charge is.
    pub over_by: Amount,
}

/// Whether `charged` on `amount_in` is within `formula` (`REQ-QTE-5`): at most its maximum where it gives one, the
/// charge being known only after execution, and else at most the fee it gives. A charge above is flagged.
pub fn check_charge(
    formula: &FeeFormula,
    amount_in: &Amount,
    charged: &Amount,
) -> Result<(), ChargeFlag> {
    let bound = charge_bound(formula, &number(amount_in));
    let charge = number(charged);
    if charge <= bound {
        return Ok(());
    }
    // The bound is below a charge that is an amount, so it is one too.
    Err(ChargeFlag {
        charged: charged.clone(),
        over_by: to_amount(&(&charge - &bound)),
        bound: to_amount(&bound),
        against: if formula.maximum.is_some() {
            Bound::Maximum
        } else {
            Bound::Formula
        },
    })
}

// ---------------------------------------------------------------------------------------------------------------
// REQ-QTE-6: binding from segment start.

/// What binds a provider for one segment (`REQ-QTE-6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding<'a> {
    /// The segment hasn't started: its terms, and the route's total, are a quote and bind nobody.
    Quote,
    /// The terms stated last at or before the segment started bind it; nothing stated later applies.
    Binds(&'a StatedTerms),
    /// The segment started before any terms were stated.
    Unstated,
}

/// The terms that bind a provider for a segment that started at `started_at`, from the history of its terms for
/// the tuple, or a quote if it hasn't started (`REQ-QTE-6`). Of terms stated at the same time, the one listed
/// last binds.
pub fn binding<'a>(history: &'a [StatedTerms], started_at: Option<&Timestamp>) -> Binding<'a> {
    let Some(started_at) = started_at else {
        return Binding::Quote;
    };
    let start = started_at.unix_nanos();
    history
        .iter()
        .filter(|t| t.stated_at.unix_nanos() <= start)
        .fold(None, |latest: Option<&StatedTerms>, t| match latest {
            Some(l) if l.stated_at.unix_nanos() > t.stated_at.unix_nanos() => Some(l),
            _ => Some(t),
        })
        .map_or(Binding::Unstated, Binding::Binds)
}

// ---------------------------------------------------------------------------------------------------------------
// REQ-QTE-9: committed within three days.

/// What a segment's provider is held to from segment start (`REQ-QTE-9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commitment {
    /// It settles within three days: its fees are committed from segment start, and, where it sets a rate, the
    /// rate moves against the customer by at most `rate_bps` basis points: the quote's own regime where that is
    /// tighter, and else the operator's slippage for the tuple. `None` for a segment that sets no rate.
    Bound {
        /// The largest move of the rate, in basis points; `0` for a firm rate.
        rate_bps: Option<u16>,
    },
    /// It settles over three days, so it is free of those bounds.
    Free,
}

/// A segment settling within three days that sets a rate, on a tuple the operator set no slippage for
/// (`REQ-QTE-9`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoSlippageLimit {
    /// The tuple figure, by id.
    pub tuple: Id,
}

/// What each segment's provider is held to, in order, part after part (`REQ-QTE-9`). An exchange's own regime is
/// read from the usable conversion quote it cites; one it doesn't cite, or a floating regime, leaves the
/// operator's slippage as its bound. A segment whose time is unknown ([`declared_times`]) is held as one within
/// three days, never freed. Core passes the limits of the bounds the record ran under,
/// [`PolicyBounds::slippage_limits`](rutent_contracts::router_api::PolicyBounds); a limit matches an operation
/// on its provider and its two ends.
pub fn commitments(
    record: &DecisionRecord,
    candidate: &Candidate,
    limits: &[SlippageLimit],
) -> Vec<(SegmentTime, Result<Commitment, NoSlippageLimit>)> {
    let search = Search::new(&record.request, &record.snapshot, &record.bounds);
    declared_times(record, candidate)
        .into_iter()
        .zip(indexed(candidate))
        .map(|(time, (_, _, o))| {
            // An unknown time (`u64::MAX`, past any declared one, at most 2^53 − 1) is taken as bound, so this check
            // fails closed, as `firmness` does by taking it as over three days.
            let unknown = time.declared_seconds == u64::MAX;
            let commitment = if time.declared_seconds > THREE_DAYS_SECONDS && !unknown {
                Ok(Commitment::Free)
            } else if o.kind != OperationKind::Exchange {
                Ok(Commitment::Bound { rate_bps: None })
            } else {
                let limit = limits.iter().find(|l| {
                    l.tuple.provider == o.provider && l.tuple.from == o.from && l.tuple.to == o.to
                });
                match limit {
                    None => Err(NoSlippageLimit {
                        tuple: time.tuple.clone(),
                    }),
                    Some(limit) => {
                        let own = o
                            .figures
                            .iter()
                            .filter_map(|id| search.figure(id.as_str()))
                            .filter(|f| search.is_usable(f))
                            .filter_map(quoted_slippage)
                            .min();
                        let operator = limit.max_bps.get();
                        Ok(Commitment::Bound {
                            rate_bps: Some(own.map_or(operator, |own| own.min(operator))),
                        })
                    }
                }
            };
            (time, commitment)
        })
        .collect()
}

/// A conversion quote's own bound on its rate, in basis points: `0` for a firm rate, its bound for a bounded one,
/// and none for a floating one or any other figure.
fn quoted_slippage(figure: &Figure) -> Option<u16> {
    let FigureValue::FreshQuote { quote, .. } = &figure.value else {
        return None;
    };
    let ResultBody::Conversion(result) = &quote.statement else {
        return None;
    };
    let ConversionOutcome::DryRunAccepted(q) = &result.outcome else {
        return None;
    };
    match &q.slippage {
        Slippage::Firm(_) => Some(0),
        Slippage::Bounded(bounded) => Some(bounded.max_bps.get()),
        Slippage::Floating(_) => None,
    }
}

/// Whether an executed rate is within a commitment, against the quoted rate (`REQ-QTE-9`). Rates are whole
/// to-assets per whole from-asset, so a move against the customer is a lower rate: `executed` is within `b` basis
/// points when `executed × 10000 ≥ quoted × (10000 − b)`, compared exactly.
pub fn rate_within(commitment: &Commitment, quoted: &Decimal, executed: &Decimal) -> bool {
    let Commitment::Bound {
        rate_bps: Some(bps),
    } = commitment
    else {
        return true;
    };
    let ((q, q_places), (e, e_places)) = (fraction(quoted), fraction(executed));
    let ten = |power: u32| BigUint::from(10u8).pow(power);
    // e / 10^ep × 10000 ≥ q / 10^qp × (10000 − b), both sides times 10^ep × 10^qp.
    e * ten(q_places) * 10_000u32 >= q * ten(e_places) * 10_000u32.saturating_sub(u32::from(*bps))
}

// ---------------------------------------------------------------------------------------------------------------
// REQ-QTE-10, AC-13: no firm rate past three days.

/// Whether a route has a firm rate (`REQ-QTE-10`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Firmness {
    /// Every segment settles within three days.
    Firm,
    /// A segment settles over three days, so the route has no firm rate. These are those segments.
    NoFirmRate {
        /// The segments over three days, in order.
        slow: Vec<SegmentTime>,
    },
}

/// Whether the candidate's route has a firm rate, naming each segment over three days if not (`REQ-QTE-10`).
pub fn firmness(record: &DecisionRecord, candidate: &Candidate) -> Firmness {
    let slow: Vec<SegmentTime> = declared_times(record, candidate)
        .into_iter()
        .filter(|t| t.declared_seconds > THREE_DAYS_SECONDS)
        .collect();
    if slow.is_empty() {
        Firmness::Firm
    } else {
        Firmness::NoFirmRate { slow }
    }
}

/// A route with no firm rate the customer hasn't accepted (`REQ-QTE-10`, `AC-13`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedsAcceptance {
    /// The segments over three days, the reason.
    pub slow: Vec<SegmentTime>,
}

/// Whether a route may start: a route with no firm rate only once the customer has accepted it (`REQ-QTE-10`,
/// `AC-13`).
pub fn may_start(firmness: &Firmness, accepted: bool) -> Result<(), NeedsAcceptance> {
    match firmness {
        Firmness::NoFirmRate { slow } if !accepted => Err(NeedsAcceptance { slow: slow.clone() }),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// REQ-QTE-15: held quotes.

/// Why a quote can't be held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldRefused {
    /// The record chose no candidate, so there is no quote to hold.
    NothingChosen,
    /// The validity stated for the hold is before the record's snapshot was taken.
    ValidityBeforeQuote,
    /// The record breaks router-api's rules: a value built field by field, never checked.
    InvalidRecord(Violation),
}

/// Why a held quote is refused when it is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldQuoteRefused {
    /// It is past its validity.
    PastValidity {
        /// Its validity.
        valid_until: Timestamp,
    },
    /// Its record chose nothing: a hold built field by field that [`HeldQuote::check`] would refuse.
    NothingChosen,
}

/// Holds the quote a choice round's record chose, until `valid_until` (`REQ-QTE-15`). The [`HeldQuote`] it returns
/// passes [`HeldQuote::check`], which it runs, the record's own check included; its fields are public, so one built
/// by hand is held to the same check only when someone calls it.
pub fn hold(record: DecisionRecord, valid_until: Timestamp) -> Result<HeldQuote, HoldRefused> {
    if record.chosen().is_none() {
        return Err(HoldRefused::NothingChosen);
    }
    if valid_until.unix_nanos() < record.snapshot.taken_at.unix_nanos() {
        return Err(HoldRefused::ValidityBeforeQuote);
    }
    let held = HeldQuote {
        record,
        valid_until,
    };
    held.check().map_err(HoldRefused::InvalidRecord)?;
    Ok(held)
}

/// How a held quote is labelled: always a quote (`REQ-QTE-7`, `REQ-QTE-15`).
pub fn held_label(_held: &HeldQuote) -> Label {
    Label::Quote
}

/// The quoted candidate of a held quote, used at `now`: refused past the hold's validity, and at it still usable
/// (`REQ-QTE-15`).
pub fn redeem<'a>(held: &'a HeldQuote, now: &Timestamp) -> Result<&'a Candidate, HeldQuoteRefused> {
    let past = HeldQuoteRefused::PastValidity {
        valid_until: held.valid_until.clone(),
    };
    if now.unix_nanos() > held.valid_until.unix_nanos() {
        return Err(past);
    }
    held.record.chosen().ok_or(HeldQuoteRefused::NothingChosen)
}
