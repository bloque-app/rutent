//! The router suite's reference double: a minimal, deterministic router over a snapshot (ADR-0006).
//!
//! It searches every path of at most `max_hops` operations from the request's origin to its destination, using
//! each tuple at most once in a path, and evaluates each at the actual amount. It never splits, so every candidate
//! is one part (`DEC-8`). An operation cites every statement in the snapshot about its tuple at its amount in (a
//! shortlist snapshot should hold none), and takes its terms from the first usable accepted answer among them, or
//! else from its price. Candidate ids follow the paths' tuple ids in order, so the record doesn't depend on the snapshot's order of
//! anything but its answers, and nothing iterates a hash map.
//!
//! It drops a candidate for the first of these that holds: excluded by policy, a void statement, a stale figure,
//! a refused dry run, an ineligible judgement, outside a band, over a ceiling, a part arriving with nothing,
//! below the floor, a profitable rate cycle, unconfirmed (choice round). Among the rest, the choice round chooses
//! the first by the objective and `candidate_id`, and drops the others as outranked by it; the shortlist round
//! keeps them all.
//!
//! Its arithmetic is its own, written apart from the suite's, so the suite's amount check isn't the double agreeing
//! with itself. `Reference<FLAW>` with a flaw from below breaks one rule, to show the suite refuses it.

use std::cmp::Ordering;

use num_bigint::BigUint;
use rutent_contracts::provider_role::{
    Band, ConversionOutcome, Fee, JudgementAnswer, NodeRef, ReserveMovementOutcome, ResultBody,
    TransferOutcome,
};
use rutent_contracts::router_api::{
    Amount, Candidate, CatalogueSnapshot, CeilingScope, Decimal, DecisionRecord, DropReason,
    Endpoint, FeeFormula, Figure, FigureValue, Id, NonEmpty, Objective, Operation, OperationKind,
    Part, PolicyBounds, PositiveAmount, Pricing, Request, Round, Router, Version,
};

/// No flaw: the reference double itself.
pub const NONE: u8 = 0;
/// Charges a price's formula even where it gives a maximum.
pub const FEE_IGNORES_MAXIMUM: u8 = 1;
/// Rounds each level of a price down on its own, not the sum once.
pub const ROUNDS_EACH_LEVEL: u8 = 2;
/// Converts amounts without scaling between the two units' decimals.
pub const IGNORES_DECIMALS: u8 = 3;
/// Ranks by lowest total cost whatever the request's objective.
pub const IGNORES_OBJECTIVE: u8 = 4;
/// Breaks ties toward the higher `candidate_id`.
pub const BREAKS_TIES_DESCENDING: u8 = 5;
/// Never drops a candidate as a suspect rate cycle.
pub const FOLLOWS_RATE_CYCLES: u8 = 6;
/// Ranks unconfirmed candidates with the rest, choosing the first confirmed one and outranking the others by it.
pub const OUTRANKS_UNCONFIRMED: u8 = 7;

/// The reference double, with one flaw or none.
pub struct Reference<const FLAW: u8>;

/// The reference double, with no flaw.
pub type Correct = Reference<NONE>;

impl<const FLAW: u8> Router for Reference<FLAW> {
    fn route(
        request: &Request,
        snapshot: &CatalogueSnapshot,
        bounds: &PolicyBounds,
    ) -> DecisionRecord {
        Search::<FLAW> {
            request,
            snapshot,
            bounds,
            hops: hops(snapshot),
        }
        .record()
    }
}

fn big(text: &str) -> BigUint {
    BigUint::parse_bytes(text.as_bytes(), 10).expect("decimal digits")
}

fn amount(value: &BigUint) -> Amount {
    Amount::new(value.to_string()).expect("at most 78 digits")
}

/// A tuple the double can route through: its figure, its one price, and its two ends.
struct Hop<'a> {
    tuple: &'a Figure,
    price: &'a Figure,
    provider: Id,
    kind: OperationKind,
    from: Endpoint,
    to: Endpoint,
    bands: &'a [Band],
}

fn hops(snapshot: &CatalogueSnapshot) -> Vec<Hop<'_>> {
    let unit = |id: &str, decimals| rutent_contracts::provider_role::Denomination {
        id: id.to_owned(),
        decimals,
    };
    let end = |node: NodeRef, asset| Endpoint { node, asset };
    snapshot
        .figures
        .iter()
        .filter_map(|tuple| {
            let (kind, from, to, bands) = match &tuple.value {
                FigureValue::ReserveTuple { tuple: t } => (
                    if matches!(t.origin, NodeRef::Outer(_)) {
                        OperationKind::Withdrawal
                    } else {
                        OperationKind::Deposit
                    },
                    end(t.origin.clone(), unit(&t.asset, t.decimals.origin)),
                    end(t.destination.clone(), unit(&t.asset, t.decimals.destination)),
                    &t.terms.bands,
                ),
                FigureValue::RampTuple { tuple: t } => (
                    OperationKind::Route,
                    end(NodeRef::Inner(t.origin.clone()), unit(&t.asset, t.decimals.origin)),
                    end(
                        NodeRef::Inner(t.destination.clone()),
                        unit(&t.asset, t.decimals.destination),
                    ),
                    &t.terms.bands,
                ),
                FigureValue::ExchangeTuple { tuple: t } => (
                    OperationKind::Exchange,
                    end(NodeRef::Inner(t.node.clone()), t.from.clone()),
                    end(NodeRef::Inner(t.node.clone()), t.to.clone()),
                    &t.terms.bands,
                ),
                _ => return None,
            };
            let price = snapshot.figures.iter().find(
                |f| matches!(&f.value, FigureValue::Price { tuple: t, .. } if t == &tuple.figure_id),
            )?;
            Some(Hop {
                tuple,
                price,
                provider: tuple.origin.provider()?.clone(),
                kind,
                from,
                to,
                bands,
            })
        })
        .collect()
}

/// An accepted answer's terms, or `None` for a refusal or any other result.
fn accepted_terms(body: &ResultBody) -> Option<(&str, &Fee, &str, u64)> {
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            ReserveMovementOutcome::DryRunAccepted(a) => Some((
                a.amount_in.as_str(),
                &a.fee,
                a.amount_out.as_str(),
                a.settlement_seconds.get(),
            )),
            _ => None,
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::DryRunAccepted(q) => Some((
                q.amount_in.as_str(),
                &q.fee,
                q.amount_out.as_str(),
                q.settlement_seconds.get(),
            )),
            _ => None,
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::DryRunAccepted(q) => Some((
                q.amount_in.as_str(),
                &q.fee,
                q.amount_out.as_str(),
                q.settlement_seconds.get(),
            )),
            _ => None,
        },
        _ => None,
    }
}

fn refused(body: &ResultBody) -> bool {
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => {
            matches!(r.outcome, ReserveMovementOutcome::Refused(_))
        }
        ResultBody::Transfer(r) => matches!(r.outcome, TransferOutcome::Refused(_)),
        ResultBody::Conversion(r) => matches!(r.outcome, ConversionOutcome::Refused(_)),
        _ => false,
    }
}

/// A rate as the fraction `digits / 10^places`.
fn fraction(rate: &Decimal) -> (BigUint, BigUint) {
    let (whole, part) = rate.as_str().split_once('.').unwrap_or((rate.as_str(), ""));
    (
        big(&format!("{whole}{part}")),
        BigUint::from(10u8).pow(part.len() as u32),
    )
}

/// One operation as evaluated, and whether a provider's answer confirmed its terms.
struct Step {
    operation: Operation,
    confirmed: bool,
}

/// A path found, before it is named and judged.
struct Found {
    tuples: Vec<String>,
    steps: Vec<Step>,
}

struct Search<'a, const FLAW: u8> {
    request: &'a Request,
    snapshot: &'a CatalogueSnapshot,
    bounds: &'a PolicyBounds,
    hops: Vec<Hop<'a>>,
}

impl<'a, const FLAW: u8> Search<'a, FLAW> {
    fn choice(&self) -> bool {
        matches!(self.request.round, Round::Choice { .. })
    }

    fn figure(&self, id: &Id) -> &'a Figure {
        self.snapshot
            .figures
            .iter()
            .find(|f| &f.figure_id == id)
            .expect("the double cites only snapshot figures")
    }

    /// The fee a price charges on `a`, counted at its maximum where it gives one.
    fn fee(&self, formula: &FeeFormula, a: &BigUint) -> BigUint {
        match &formula.maximum {
            Some(maximum) if FLAW != FEE_IGNORES_MAXIMUM => big(maximum.as_str()),
            _ => {
                big(formula.flat.as_str())
                    + a * big(formula.proportional.numerator.as_str())
                        / big(formula.proportional.denominator.amount().as_str())
            }
        }
    }

    /// What a net amount converts to: an exact sum of fractions over the levels, floored once at the end.
    fn convert(&self, rate: Option<&Pricing>, net: &BigUint, hop: &Hop<'_>) -> BigUint {
        let (up, down) = if FLAW == IGNORES_DECIMALS {
            (BigUint::from(1u8), BigUint::from(1u8))
        } else {
            (
                BigUint::from(10u8).pow(u32::from(hop.to.asset.decimals)),
                BigUint::from(10u8).pow(u32::from(hop.from.asset.decimals)),
            )
        };
        let levels: Vec<(Option<BigUint>, &Decimal)> = match rate {
            None => return net * up / down,
            Some(Pricing::Fixed { rate }) => vec![(None, rate)],
            Some(Pricing::Levels { levels }) => {
                let last = big(levels.last().up_to.amount().as_str());
                if net > &last {
                    return BigUint::ZERO;
                }
                levels
                    .iter()
                    .map(|l| (Some(big(l.up_to.amount().as_str())), &l.rate))
                    .collect()
            }
        };
        let (mut numerator, mut denominator) = (BigUint::ZERO, BigUint::from(1u8));
        let mut floored = BigUint::ZERO;
        let mut below = BigUint::ZERO;
        for (bound, rate) in levels {
            let top = bound.map_or(net.clone(), |b| b.min(net.clone()));
            if top <= below {
                break;
            }
            let take = &top - &below;
            let (digits, scale) = fraction(rate);
            floored += &take * &digits * &up / (&scale * &down);
            numerator = numerator * &scale + take * digits * &denominator;
            denominator *= scale;
            below = top;
        }
        if FLAW == ROUNDS_EACH_LEVEL {
            return floored;
        }
        numerator * up / (denominator * down)
    }

    /// The operation through `hop` on `a` entering it, or `None` if no operation can state it.
    fn step(&self, hop: &Hop<'_>, a: &BigUint) -> Option<Step> {
        let amount_in = PositiveAmount::new(a.to_string())?;
        let asked = amount_in.amount().as_str();
        let tuple = &hop.tuple.figure_id;
        // Every statement about the tuple at this amount, in either round: the contract asks an operation to cite
        // each usable one, and a shortlist snapshot shouldn't hold any (the suite says so if it does).
        let statements: Vec<&Figure> = self
            .snapshot
            .figures
            .iter()
            .filter(|f| match &f.value {
                FigureValue::FreshQuote { tuple: t, quote } => {
                    t == tuple && accepted_terms(&quote.statement).is_some_and(|(i, ..)| i == asked)
                }
                FigureValue::DryRun {
                    tuple: t,
                    amount_in,
                    ..
                } => t == tuple && amount_in.amount().as_str() == asked,
                FigureValue::Eligibility { tuple: t, .. } => t == tuple,
                _ => false,
            })
            .collect();
        let answer = |f: &&Figure| -> Option<(Fee, Amount)> {
            let body = match &f.value {
                FigureValue::FreshQuote { quote, .. } => &quote.statement,
                FigureValue::DryRun { result, .. } => &result.statement,
                _ => return None,
            };
            let (_, fee, out, _) = accepted_terms(body)?;
            let stateable = fee.asset == hop.from.asset.id
                && Amount::new(fee.amount.as_str()).is_some_and(|f| &big(f.as_str()) <= a);
            let out = Amount::new(out)?;
            stateable.then(|| (fee.clone(), out))
        };
        // Terms come from a usable answer; one past its validity or void binds nothing.
        let confirmed = statements
            .iter()
            .filter(|f| f.is_within_validity() && !self.statement_is_void(f))
            .find_map(answer);
        let (fee, amount_out, is_confirmed) = match confirmed {
            Some((fee, out)) => (fee, out, true),
            None => {
                let FigureValue::Price { fee, rate, .. } = &hop.price.value else {
                    unreachable!("a price figure");
                };
                let charged = self.fee(fee, a);
                if &charged > a {
                    return None;
                }
                let out = self.convert(rate.as_ref(), &(a - &charged), hop);
                (
                    Fee {
                        asset: hop.from.asset.id.clone(),
                        amount: charged.to_string(),
                    },
                    amount(&out),
                    false,
                )
            }
        };
        // Cite every usable statement, as the contract asks. A statement past its validity or void is cited only
        // where no usable answer covers the operation: there it is what the operation stands on, and drops it;
        // beside a usable answer it may not drop the candidate, so it is left out.
        let mut figures = vec![hop.tuple.figure_id.clone(), hop.price.figure_id.clone()];
        figures.extend(
            statements
                .iter()
                .filter(|f| !is_confirmed || (f.is_within_validity() && !self.statement_is_void(f)))
                .map(|f| f.figure_id.clone()),
        );
        Some(Step {
            operation: Operation {
                kind: hop.kind,
                provider: hop.provider.clone(),
                from: hop.from.clone(),
                to: hop.to.clone(),
                amount_in,
                fee,
                amount_out,
                figures: NonEmpty::new(figures).expect("a tuple and its price"),
            },
            confirmed: is_confirmed,
        })
    }

    fn extend(
        &self,
        at: &Endpoint,
        a: &BigUint,
        path: &mut Vec<usize>,
        steps: &mut Vec<Step>,
        found: &mut Vec<Found>,
    ) {
        for (i, hop) in self.hops.iter().enumerate() {
            if path.contains(&i) || &hop.from != at {
                continue;
            }
            let Some(step) = self.step(hop, a) else {
                continue;
            };
            let out = big(step.operation.amount_out.as_str());
            path.push(i);
            steps.push(step);
            if hop.to == self.request.to {
                found.push(Found {
                    tuples: path
                        .iter()
                        .map(|&j| self.hops[j].tuple.figure_id.as_str().to_owned())
                        .collect(),
                    steps: steps
                        .iter()
                        .map(|s| Step {
                            operation: s.operation.clone(),
                            confirmed: s.confirmed,
                        })
                        .collect(),
                });
            }
            if steps.len() < usize::from(self.bounds.max_hops.get()) && out > BigUint::ZERO {
                self.extend(&hop.to, &out, path, steps, found);
            }
            steps.pop();
            path.pop();
        }
    }

    fn statement_is_void(&self, figure: &Figure) -> bool {
        let (deployment, version) = (
            &self.bounds.deployment,
            self.bounds.protocol_version.as_str(),
        );
        match &figure.value {
            FigureValue::FreshQuote { quote, .. } => {
                quote.check_context(deployment, version).is_err()
            }
            FigureValue::DryRun { result, .. } => {
                result.check_context(deployment, version).is_err()
            }
            FigureValue::Eligibility { judgement, .. } => {
                judgement.check_context(deployment, version).is_err()
            }
            _ => false,
        }
    }

    fn within_band(&self, hop_of: &Hop<'_>, operation: &Operation) -> bool {
        let a = big(operation.amount_in.amount().as_str());
        let in_band = hop_of
            .bands
            .iter()
            .any(|b| big(&b.min) <= a && a <= big(&b.max));
        let net = &a - big(&operation.fee.amount);
        let in_levels = match &hop_of.price.value {
            FigureValue::Price {
                rate: Some(Pricing::Levels { levels }),
                ..
            } => net <= big(levels.last().up_to.amount().as_str()),
            _ => true,
        };
        in_band && in_levels
    }

    fn publisher(&self, provider: &Id) -> Option<&'a Id> {
        self.hops
            .iter()
            .find(|h| &h.provider == provider)
            .and_then(|h| h.tuple.origin.publisher())
    }

    fn over_ceiling(&self, operations: &[&Operation]) -> Option<DropReason> {
        let placed = |o: &Operation, asset| {
            if &o.from.asset == asset {
                big(o.amount_in.amount().as_str())
            } else if &o.to.asset == asset {
                big(o.amount_out.as_str())
            } else {
                BigUint::ZERO
            }
        };
        let held = |at: &dyn Fn(&Id, &Id) -> bool, asset| -> BigUint {
            self.snapshot
                .figures
                .iter()
                .filter_map(|f| match &f.value {
                    FigureValue::Exposure {
                        provider,
                        publisher,
                        asset: a,
                        in_flight,
                    } if at(provider, publisher) && a == asset => Some(big(in_flight.as_str())),
                    _ => None,
                })
                .sum()
        };
        for c in &self.bounds.provider_ceilings {
            let value: BigUint = operations
                .iter()
                .filter(|o| o.provider == c.provider)
                .map(|o| placed(o, &c.asset))
                .sum();
            if value > BigUint::ZERO
                && value + held(&|p, _| p == &c.provider, &c.asset) > big(c.amount.as_str())
            {
                return Some(DropReason::OverCeiling {
                    scope: CeilingScope::Provider,
                    party: c.provider.clone(),
                    asset: c.asset.clone(),
                });
            }
        }
        for c in &self.bounds.publisher_ceilings {
            let value: BigUint = operations
                .iter()
                .filter(|o| self.publisher(&o.provider) == Some(&c.publisher))
                .map(|o| placed(o, &c.asset))
                .sum();
            if value > BigUint::ZERO
                && value + held(&|_, q| q == &c.publisher, &c.asset) > big(c.amount.as_str())
            {
                return Some(DropReason::OverCeiling {
                    scope: CeilingScope::Publisher,
                    party: c.publisher.clone(),
                    asset: c.asset.clone(),
                });
            }
        }
        None
    }

    /// Whether the path comes back to a node and asset it held with more than it held there, the two counted in
    /// whatever units: compared as `now × 10^then_decimals` against `then × 10^now_decimals`.
    fn profitable_cycle(steps: &[Step]) -> bool {
        let first = &steps[0].operation;
        let held: Vec<(&Endpoint, BigUint)> =
            std::iter::once((&first.from, big(first.amount_in.amount().as_str())))
                .chain(
                    steps
                        .iter()
                        .map(|s| (&s.operation.to, big(s.operation.amount_out.as_str()))),
                )
                .collect();
        let ten = |d: u8| BigUint::from(10u8).pow(u32::from(d));
        held.iter().enumerate().any(|(j, (at, now))| {
            held[..j].iter().any(|(was, then)| {
                was.node == at.node
                    && was.asset.id == at.asset.id
                    && now * ten(was.asset.decimals) > then * ten(at.asset.decimals)
            })
        })
    }

    /// Why a path is dropped, before ranking, or `None`.
    fn fault(&self, found: &Found) -> Option<DropReason> {
        let operations: Vec<&Operation> = found.steps.iter().map(|s| &s.operation).collect();
        let cited: Vec<&Figure> = operations
            .iter()
            .flat_map(|o| o.figures.iter())
            .map(|id| self.figure(id))
            .collect();
        let excluded = operations.iter().any(|o| {
            self.bounds.excluded_providers.contains(&o.provider)
                || self
                    .bounds
                    .excluded_tuples
                    .iter()
                    .any(|t| t.provider == o.provider && t.from == o.from && t.to == o.to)
        });
        if excluded {
            return Some(DropReason::ExcludedByPolicy {});
        }
        if let Some(f) = cited.iter().find(|f| self.statement_is_void(f)) {
            return Some(DropReason::VoidStatement {
                figure: f.figure_id.clone(),
            });
        }
        if let Some(f) = cited.iter().find(|f| !f.is_within_validity()) {
            return Some(DropReason::StaleFigure {
                figure: f.figure_id.clone(),
            });
        }
        if let Some(f) = cited
            .iter()
            .find(|f| matches!(&f.value, FigureValue::DryRun { result, .. } if refused(&result.statement)))
        {
            return Some(DropReason::DryRunRefused {
                figure: f.figure_id.clone(),
            });
        }
        if let Some(f) = cited.iter().find(|f| {
            matches!(&f.value, FigureValue::Eligibility { judgement, .. } if judgement.statement.answer == JudgementAnswer::No)
        }) {
            return Some(DropReason::Ineligible {
                figure: f.figure_id.clone(),
            });
        }
        for (o, operation) in operations.iter().enumerate() {
            let hop = self
                .hops
                .iter()
                .find(|h| operation.figures.first() == &h.tuple.figure_id)
                .expect("the first figure is the tuple");
            if !self.within_band(hop, operation) {
                return Some(DropReason::OutsideBand {
                    part: 0,
                    operation: o as u32,
                });
            }
        }
        if let Some(reason) = self.over_ceiling(&operations) {
            return Some(reason);
        }
        let last = big(operations
            .last()
            .expect("a path has an operation")
            .amount_out
            .as_str());
        if last == BigUint::ZERO {
            return Some(DropReason::NoPositiveOutcome {});
        }
        if self
            .request
            .floor
            .as_ref()
            .is_some_and(|floor| last < big(floor.as_str()))
        {
            return Some(DropReason::BelowFloor {});
        }
        if FLAW != FOLLOWS_RATE_CYCLES && Self::profitable_cycle(&found.steps) {
            return Some(DropReason::SuspectRateCycle {});
        }
        if self.choice()
            && FLAW != OUTRANKS_UNCONFIRMED
            && let Some(o) = found.steps.iter().position(|s| !s.confirmed)
        {
            return Some(DropReason::Unconfirmed {
                part: 0,
                operation: o as u32,
            });
        }
        None
    }

    /// The order of two paths under the objective: better first.
    fn compare(&self, a: &Found, b: &Found) -> Ordering {
        let arrives = |f: &Found| {
            big(f
                .steps
                .last()
                .expect("an operation")
                .operation
                .amount_out
                .as_str())
        };
        let settles = |f: &Found| -> u128 {
            f.steps
                .iter()
                .map(|s| {
                    let quoted = s
                        .operation
                        .figures
                        .iter()
                        .map(|id| self.figure(id))
                        .filter(|f| f.is_within_validity() && !self.statement_is_void(f))
                        .filter_map(|f| match &f.value {
                            FigureValue::FreshQuote { quote, .. } => {
                                accepted_terms(&quote.statement).map(|(.., seconds)| seconds)
                            }
                            _ => None,
                        })
                        .max();
                    let priced = match &self.figure(&s.operation.figures[1]).value {
                        FigureValue::Price {
                            settlement_seconds, ..
                        } => settlement_seconds.get(),
                        _ => unreachable!("the second figure is the price"),
                    };
                    u128::from(quoted.unwrap_or(priced))
                })
                .sum()
        };
        match self.request.objective {
            Objective::FastestSettlement {} if FLAW != IGNORES_OBJECTIVE => {
                settles(a).cmp(&settles(b))
            }
            _ => arrives(b).cmp(&arrives(a)),
        }
    }

    fn record(self) -> DecisionRecord {
        let mut found = Vec::new();
        let origin = self.request.from.clone();
        self.extend(
            &origin,
            &big(self.request.amount.amount().as_str()),
            &mut Vec::new(),
            &mut Vec::new(),
            &mut found,
        );
        found.sort_by(|a, b| a.tuples.cmp(&b.tuples));
        let width = found.len().to_string().len().max(3);
        let id = |i: usize| Id::new(format!("c{:0width$}", i + 1)).expect("an identifier");
        let mut dropped: Vec<Option<DropReason>> = found.iter().map(|f| self.fault(f)).collect();
        let mut ranked: Vec<usize> = (0..found.len()).filter(|&i| dropped[i].is_none()).collect();
        ranked.sort_by(|&a, &b| {
            self.compare(&found[a], &found[b]).then_with(|| {
                let by_id = id(a).as_str().as_bytes().cmp(id(b).as_str().as_bytes());
                if FLAW == BREAKS_TIES_DESCENDING {
                    by_id.reverse()
                } else {
                    by_id
                }
            })
        });
        let mut chosen = None;
        if self.choice() {
            let first = ranked
                .iter()
                .copied()
                .find(|&i| found[i].steps.iter().all(|s| s.confirmed));
            match first {
                Some(winner) => {
                    for &i in &ranked {
                        if i != winner {
                            dropped[i] = Some(DropReason::Outranked { by: id(winner) });
                        }
                    }
                    chosen = Some(id(winner));
                }
                None => {
                    for &i in &ranked {
                        let o = found[i]
                            .steps
                            .iter()
                            .position(|s| !s.confirmed)
                            .unwrap_or(0);
                        dropped[i] = Some(DropReason::Unconfirmed {
                            part: 0,
                            operation: o as u32,
                        });
                    }
                }
            }
        }
        let candidates = found
            .into_iter()
            .zip(dropped)
            .enumerate()
            .map(|(i, (f, dropped))| Candidate {
                candidate_id: id(i),
                parts: NonEmpty::new(vec![Part {
                    operations: NonEmpty::new(f.steps.into_iter().map(|s| s.operation).collect())
                        .expect("a path has an operation"),
                }])
                .expect("one part"),
                dropped,
            })
            .collect();
        DecisionRecord {
            router_version: Version::new("reference-0.1.0").expect("a version"),
            request: self.request.clone(),
            snapshot: self.snapshot.clone(),
            bounds: self.bounds.clone(),
            candidates,
            chosen,
        }
    }
}
