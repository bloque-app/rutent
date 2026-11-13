//! The hop-limited exhaustive search over a snapshot (ADR-0006, `DEC-8`).
//!
//! From the request's origin, every path of at most `max_hops` segments, each tuple at most once in a path, is
//! followed with the actual amount: each segment is evaluated on what the one before delivered, after its fees and
//! in its own unit, so its band and its price's levels are judged on that amount (`REQ-QTE-12`). Each operation's
//! terms come from a usable accepted answer of its provider at that amount where the snapshot holds one, and else
//! from its advertised price, so the route's price is its segments' prices composed (`REQ-QTE-8`). Either way an
//! operation states what arrives: an answer under a tuple that reports `gross` includes its charge in what a
//! withdrawal, deposit or transfer gives out, so the operation states that less the charge (`REQ-CAT-11`,
//! `T-011-07`), and an advertised price takes its fee out before anything is given out.

use std::collections::BTreeMap;

use num_bigint::BigUint;
use rutent_contracts::provider_role::{
    ConversionOutcome, Fee, ReserveMovementOutcome, ResultBody, TransferOutcome, VoidedBy,
};
use rutent_contracts::router_api::{
    Amount, CatalogueSnapshot, Endpoint, Figure, FigureValue, NonEmpty, Operation, PolicyBounds,
    PositiveAmount, Request, Round, Timestamp,
};

use crate::network::{Segment, segments};
use crate::price::{evaluate, whole};

/// A provider's accepted dry-run answer, a fresh quote included.
pub(crate) struct Answer<'a> {
    pub(crate) amount_in: &'a str,
    pub(crate) fee: &'a Fee,
    pub(crate) amount_out: &'a str,
    pub(crate) settlement_seconds: u64,
}

/// The accepted answer a result states, or `None` for a refusal or any other result.
pub(crate) fn accepted(body: &ResultBody) -> Option<Answer<'_>> {
    fn answer<'b>(
        amount_in: &'b str,
        fee: &'b Fee,
        amount_out: &'b str,
        seconds: u64,
    ) -> Answer<'b> {
        Answer {
            amount_in,
            fee,
            amount_out,
            settlement_seconds: seconds,
        }
    }
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            ReserveMovementOutcome::DryRunAccepted(a) => Some(answer(
                &a.amount_in,
                &a.fee,
                &a.amount_out,
                a.settlement_seconds.get(),
            )),
            _ => None,
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::DryRunAccepted(q) => Some(answer(
                &q.amount_in,
                &q.fee,
                &q.amount_out,
                q.settlement_seconds.get(),
            )),
            _ => None,
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::DryRunAccepted(q) => Some(answer(
                &q.amount_in,
                &q.fee,
                &q.amount_out,
                q.settlement_seconds.get(),
            )),
            _ => None,
        },
        _ => None,
    }
}

/// Whether a result refuses.
pub(crate) fn refuses(body: &ResultBody) -> bool {
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => {
            matches!(r.outcome, ReserveMovementOutcome::Refused(_))
        }
        ResultBody::Transfer(r) => matches!(r.outcome, TransferOutcome::Refused(_)),
        ResultBody::Conversion(r) => matches!(r.outcome, ConversionOutcome::Refused(_)),
        _ => false,
    }
}

/// The accepted answer a statement figure carries, if it carries one.
pub(crate) fn answer_of(figure: &Figure) -> Option<Answer<'_>> {
    match &figure.value {
        FigureValue::FreshQuote { quote, .. } => accepted(&quote.statement),
        FigureValue::DryRun { result, .. } => accepted(&result.statement),
        _ => None,
    }
}

/// One operation as evaluated, whether an answer of its provider confirmed its terms, and whether it can't be
/// stated.
pub(crate) struct Step {
    pub(crate) operation: Operation,
    pub(crate) confirmed: bool,
    pub(crate) unstatable: Option<Cut>,
}

/// Why a path ends at its last operation, nothing after it being statable (router-api's cut-short reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cut {
    /// Its net is above its price's last level, which serves it nothing (`beyond_levels`).
    BeyondLevels,
    /// Its tuple has no price in the snapshot (`no_price`).
    NoPrice,
    /// Its price's fee is above the amount that reaches it (`fee_above_amount`).
    FeeAboveAmount,
    /// It delivers nothing, short of the destination (`nothing_delivered`).
    NothingDelivered,
}

/// A path the search found: the segments it takes, by index, and their operations.
pub(crate) struct Path {
    pub(crate) segments: Vec<usize>,
    pub(crate) steps: Vec<Step>,
    /// Cut short at its last operation, which can't be followed.
    pub(crate) cut: Option<Cut>,
}

/// What the router reads, indexed once.
pub(crate) struct Search<'a> {
    pub(crate) request: &'a Request,
    snapshot: &'a CatalogueSnapshot,
    pub(crate) bounds: &'a PolicyBounds,
    pub(crate) segments: Vec<Segment<'a>>,
    figures: BTreeMap<&'a str, &'a Figure>,
    /// The statements about each tuple, by its figure id, in the snapshot's order.
    statements: BTreeMap<&'a str, Vec<&'a Figure>>,
}

impl<'a> Search<'a> {
    pub(crate) fn new(
        request: &'a Request,
        snapshot: &'a CatalogueSnapshot,
        bounds: &'a PolicyBounds,
    ) -> Self {
        let mut statements: BTreeMap<&str, Vec<&Figure>> = BTreeMap::new();
        for figure in &snapshot.figures {
            let about = match &figure.value {
                FigureValue::FreshQuote { tuple, .. }
                | FigureValue::DryRun { tuple, .. }
                | FigureValue::Eligibility { tuple, .. } => tuple,
                _ => continue,
            };
            statements.entry(about.as_str()).or_default().push(figure);
        }
        Search {
            request,
            snapshot,
            bounds,
            segments: segments(snapshot),
            figures: snapshot
                .figures
                .iter()
                .map(|f| (f.figure_id.as_str(), f))
                .collect(),
            statements,
        }
    }

    /// The snapshot's in-flight exposures, from the operator's records.
    pub(crate) fn exposures(&self) -> impl Iterator<Item = &'a Figure> + use<'a, '_> {
        self.snapshot
            .figures
            .iter()
            .filter(|f| matches!(f.value, FigureValue::Exposure { .. }))
    }

    /// A figure the search cited, by id.
    pub(crate) fn figure(&self, id: &str) -> Option<&'a Figure> {
        self.figures.get(id).copied()
    }

    pub(crate) fn is_choice(&self) -> bool {
        matches!(self.request.round, Round::Choice { .. })
    }

    /// Whether a statement is void: its envelope names another deployment or protocol version than the bounds'
    /// (`REQ-PRT-1`). Other figures are never void.
    pub(crate) fn is_void(&self, figure: &Figure) -> bool {
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

    /// Whether a statement may bind an operation: within its validity (`REQ-QTE-4`), not void, and, for an
    /// accepted answer in a choice round, made at or after the shortlist round's `taken_at`. A refusal or a
    /// judgement binds whenever it was made.
    pub(crate) fn is_usable(&self, figure: &Figure) -> bool {
        let made_at = match &figure.value {
            FigureValue::FreshQuote { quote, .. } => Some(quote.made_at.as_str()),
            FigureValue::DryRun { result, .. } if accepted(&result.statement).is_some() => {
                Some(result.made_at.as_str())
            }
            _ => None,
        };
        let in_time = match (&self.request.round, made_at) {
            (Round::Choice { refines }, Some(made_at)) => Timestamp::new(made_at)
                .is_some_and(|t| t.unix_nanos() >= refines.taken_at.unix_nanos()),
            _ => true,
        };
        figure.is_within_validity() && !self.is_void(figure) && in_time
    }

    /// A usable fresh quote about `operation`'s tuple, accepted for another amount than the one entering it and
    /// voided by `amount_changes`: the quote the choice round names when it drops the operation's candidate as
    /// `quote_voided` (`REQ-QTE-1`). The first in the snapshot's order.
    pub(crate) fn voided_quote(&self, operation: &Operation) -> Option<&'a Figure> {
        let asked = operation.amount_in.amount().as_str();
        self.statements
            .get(operation.figures.first().as_str())?
            .iter()
            .copied()
            .find(|f| {
                let FigureValue::FreshQuote { quote, .. } = &f.value else {
                    return false;
                };
                let voided_by = match &quote.statement {
                    ResultBody::Transfer(r) => match &r.outcome {
                        TransferOutcome::DryRunAccepted(q) => &q.voided_by,
                        _ => return false,
                    },
                    ResultBody::Conversion(r) => match &r.outcome {
                        ConversionOutcome::DryRunAccepted(q) => &q.voided_by,
                        _ => return false,
                    },
                    _ => return false,
                };
                self.is_usable(f)
                    && voided_by.contains(&VoidedBy::AmountChanges)
                    && accepted(&quote.statement).is_some_and(|a| a.amount_in != asked)
            })
    }

    /// Every statement about a segment's tuple that bears on `asked` entering it: each fresh quote and dry run
    /// for that amount, and each eligibility judgement.
    fn statements_at(&self, segment: &Segment<'_>, asked: &str) -> Vec<&'a Figure> {
        let Some(about) = self.statements.get(segment.tuple.figure_id.as_str()) else {
            return Vec::new();
        };
        about
            .iter()
            .copied()
            .filter(|f| match &f.value {
                FigureValue::FreshQuote { quote, .. } => {
                    accepted(&quote.statement).is_some_and(|a| a.amount_in == asked)
                }
                FigureValue::DryRun { amount_in, .. } => amount_in.amount().as_str() == asked,
                FigureValue::Eligibility { .. } => true,
                _ => false,
            })
            .collect()
    }

    /// The operation through `segment` on `amount` entering it. Where none can be stated, because its tuple has no
    /// price or its price's fee is above what enters (and no usable answer binds it), the step is marked so, with
    /// the shape router-api 3.0.0 gives it: a fee of `0` with no price cited, or a fee of all that enters; either
    /// way it delivers `0`. `None` only for an amount that isn't positive, which no path reaches.
    fn step(&self, segment: &Segment<'_>, amount: &BigUint) -> Option<Step> {
        let amount_in = PositiveAmount::new(amount.to_string())?;
        let asked = amount_in.amount().as_str();
        let statements = self.statements_at(segment, asked);
        // A usable answer at this amount binds: the provider's own terms, in the segment's input asset and at
        // most what enters. A figure past its validity binds nothing (`REQ-QTE-4`). The operation states what
        // arrives, as router-api's `Operation::stated_out` reads the answer: under a tuple that reports `gross`, a
        // withdrawal's, deposit's or transfer's `amount_out` less the charge it includes, in its out unit; else the
        // answer's `amount_out` as it stands (`REQ-CAT-11`). So the next segment, the ranking, the floor, the
        // ceilings and the rate-cycle check all read what arrives.
        let confirmed = statements
            .iter()
            .filter(|f| self.is_usable(f))
            .find_map(|f| {
                let answer = answer_of(f)?;
                let fee = whole(&answer.fee.amount)?;
                (answer.fee.asset == segment.from.asset.id && &fee <= amount).then_some(())?;
                let out = Self::stated_out(segment, &amount_in, &answer)?;
                Some((answer.fee.clone(), Amount::new(out)?))
            });
        let fee_of = |amount: String| Fee {
            asset: segment.from.asset.id.clone(),
            amount,
            billed_separately: None,
        };
        let (fee, amount_out, is_confirmed, unstatable) = match (confirmed, segment.price) {
            // An operation cites one price: with none, it can't be stated, whatever answers there are.
            (_, None) => (
                fee_of("0".to_owned()),
                Amount::zero(),
                false,
                Some(Cut::NoPrice),
            ),
            (Some((fee, out)), Some(_)) => (fee, out, true, None),
            (None, Some((_, formula))) => match evaluate(
                formula,
                segment.rate,
                amount,
                &segment.from.asset,
                &segment.to.asset,
            ) {
                Some(priced) => (
                    fee_of(priced.fee.to_string()),
                    Amount::new(priced.amount_out.to_string())?,
                    false,
                    None,
                ),
                None => (
                    fee_of(amount.to_string()),
                    Amount::zero(),
                    false,
                    Some(Cut::FeeAboveAmount),
                ),
            },
        };
        // Cite every usable statement, as the contract asks. One that is past its validity or void is cited only
        // where no usable answer covers the operation: there the operation stands on it and is dropped for it. An
        // operation that can't be stated is dropped for that, so it cites the usable ones only.
        let mut figures = vec![segment.tuple.figure_id.clone()];
        figures.extend(segment.price.map(|(price, _)| price.figure_id.clone()));
        figures.extend(
            statements
                .iter()
                .filter(|f| (!is_confirmed && unstatable.is_none()) || self.is_usable(f))
                .map(|f| f.figure_id.clone()),
        );
        Some(Step {
            operation: Operation {
                kind: segment.kind,
                provider: segment.provider.clone(),
                from: segment.from.clone(),
                to: segment.to.clone(),
                amount_in,
                fee,
                amount_out,
                figures: NonEmpty::new(figures)?,
                // Named once the part is whole, by `calls::name`: they depend on its other segments.
                calls: Vec::new(),
            },
            confirmed: is_confirmed,
            unstatable,
        })
    }

    /// What an operation through `segment` on `amount_in` states as its `amount_out` for `answer`: router-api's
    /// [`Operation::stated_out`], so the router states exactly what the contract confirms. `None` where the answer's
    /// charge is above the amount it gives out under `gross`, which then confirms nothing.
    fn stated_out(
        segment: &Segment<'_>,
        amount_in: &PositiveAmount,
        answer: &Answer<'_>,
    ) -> Option<String> {
        let operation = Operation {
            kind: segment.kind,
            provider: segment.provider.clone(),
            from: segment.from.clone(),
            to: segment.to.clone(),
            amount_in: amount_in.clone(),
            fee: answer.fee.clone(),
            amount_out: Amount::zero(),
            figures: NonEmpty::new(vec![segment.tuple.figure_id.clone()])?,
            calls: Vec::new(),
        };
        operation.stated_out(segment.tuple, answer.amount_out, answer.fee)
    }

    /// Whether an unconfirmed step's net is above its price's last level, so its price serves it nothing.
    fn beyond_levels(segment: &Segment<'_>, step: &Step) -> bool {
        if step.confirmed {
            return false;
        }
        let net = whole(step.operation.amount_in.amount().as_str()).unwrap_or_default()
            - whole(&step.operation.fee.amount).unwrap_or_default();
        crate::price::deepest(segment.rate).is_some_and(|last| net > last)
    }

    /// Every path from the request's origin, with its amount.
    pub(crate) fn paths(&self) -> Vec<Path> {
        let mut found = Vec::new();
        let amount = whole(self.request.amount.amount().as_str()).unwrap_or_default();
        self.extend(
            &self.request.from,
            &amount,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut found,
        );
        found
    }

    fn extend(
        &self,
        at: &Endpoint,
        amount: &BigUint,
        taken: &mut Vec<usize>,
        steps: &mut Vec<Step>,
        found: &mut Vec<Path>,
    ) {
        for (i, segment) in self.segments.iter().enumerate() {
            if taken.contains(&i) || &segment.from != at {
                continue;
            }
            let Some(step) = self.step(segment, amount) else {
                continue;
            };
            let arrives = whole(step.operation.amount_out.as_str()).unwrap_or_default();
            let at_destination = segment.to == self.request.to;
            // A step that can't be stated ends its path wherever it is: nothing after it can be stated, so it
            // adds one candidate per prefix the search already walks, and never chains.
            let cut = step.unstatable.or_else(|| {
                if at_destination {
                    None
                } else if Self::beyond_levels(segment, &step) {
                    Some(Cut::BeyondLevels)
                } else {
                    // Short of the destination with nothing: no operation after it could take in anything.
                    (arrives == BigUint::ZERO).then_some(Cut::NothingDelivered)
                }
            });
            taken.push(i);
            steps.push(step);
            if at_destination || cut.is_some() {
                found.push(Path {
                    segments: taken.clone(),
                    steps: steps
                        .iter()
                        .map(|s| Step {
                            operation: s.operation.clone(),
                            confirmed: s.confirmed,
                            unstatable: s.unstatable,
                        })
                        .collect(),
                    cut,
                });
            }
            if cut.is_none()
                && steps.len() < usize::from(self.bounds.max_hops.get())
                && arrives > BigUint::ZERO
            {
                self.extend(&segment.to, &arrives, taken, steps, found);
            }
            steps.pop();
            taken.pop();
        }
    }
}
