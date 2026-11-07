//! The router suite (`T-004-02`, `DEC-5`): the choice is recomputed from the decision record alone, the same
//! inputs give the same output, and a changed objective can change the choice (`AC-2`, `REQ-AUD-4`,
//! `REQ-QTE-11`).
//!
//! # Recomputing a record
//!
//! [`recompute`] takes a [`DecisionRecord`] and nothing else: no router, no provider, no clock. Everything it needs
//! is in the record (its request, snapshot and bounds verbatim, `REQ-AUD-4`), so a choice it confirms was
//! recomputed with no provider call. It runs the contract's own [`DecisionRecord::check`] first, then the checks the
//! contract leaves to this suite, each reported on its own ([`Check`]):
//!
//! - in a shortlist round the snapshot holds advertised figures only;
//! - every operation's fee and `amount_out` follow from the figures it cites;
//! - no kept or outranked candidate follows a profitable rate cycle, and `suspect_rate_cycle` names only one that
//!   does;
//! - every `outranked` candidate would itself be a valid choice;
//! - the ranking follows the request's objective, ties going to the lowest `candidate_id`.
//!
//! # Running a router
//!
//! [`run`] drives a [`Router`] over the suite's own [`cases`] and reports each cell ([`Cell`]): two calls give
//! byte-identical records, the record keeps its inputs and passes the contract, it recomputes, it chooses the
//! route the case expects, routing the record's own inputs gives the record again, and the AC-2 pair's two
//! objectives choose differently.
//!
//! # What the suite can't see
//!
//! It recomputes the choice among the candidates a record lists; it can't tell that a router left out a better
//! one. The cases' expected routes catch that for the suite's own inputs only.

mod cases;
mod price;

pub use cases::{Case, Expected, cases};

use std::cmp::Ordering;

use num_bigint::BigUint;
use rutent_contracts::provider_role::{
    ConversionOutcome, Fee, ReserveMovementOutcome, ResultBody, TransferOutcome,
};
use rutent_contracts::router_api::{
    Candidate, DecisionRecord, DropReason, Endpoint, Figure, FigureValue, Id, Objective, Operation,
    Round, Router, Timestamp,
};

/// One check the suite runs on a decision record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Check {
    /// The record passes the contract's own [`DecisionRecord::check`]. When it doesn't, no other check runs.
    Contract,
    /// In a shortlist round the snapshot holds advertised figures only: no fresh quote, dry run or eligibility
    /// judgement, which only the choice round asks for (`REQ-CAT-5`).
    ShortlistReadsAdvertisedFigures,
    /// Every operation's fee and `amount_out` follow from the figures it cites (`REQ-QTE-5`, `REQ-QTE-12`): from
    /// every usable accepted answer it cites for its amount in, or else from its price.
    AmountsFollowTheirFigures,
    /// No kept or outranked candidate follows a profitable rate cycle, and a `suspect_rate_cycle` reason names a
    /// candidate that does (ADR-0006).
    RateCyclesAreNotFollowed,
    /// Every candidate dropped as `outranked` would itself pass the contract as the choice (or as kept, in a
    /// shortlist round): it was beaten, not faulty.
    OutrankedCandidatesAreEligible,
    /// The kept candidates rank ahead of every outranked one by the objective and then `candidate_id`; each
    /// outranked candidate's `by` is kept or outranked and ranks ahead of it; a choice round with such a candidate
    /// chooses (`REQ-QTE-11`).
    RankingFollowsTheObjective,
}

impl Check {
    /// The check's name, as reports print it.
    pub fn name(self) -> &'static str {
        match self {
            Check::Contract => "contract",
            Check::ShortlistReadsAdvertisedFigures => "shortlist-reads-advertised-figures",
            Check::AmountsFollowTheirFigures => "amounts-follow-their-figures",
            Check::RateCyclesAreNotFollowed => "rate-cycles-are-not-followed",
            Check::OutrankedCandidatesAreEligible => "outranked-candidates-are-eligible",
            Check::RankingFollowsTheObjective => "ranking-follows-the-objective",
        }
    }
}

/// One way a record fails a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The check it fails.
    pub check: Check,
    /// The candidate it is about, if one.
    pub candidate: Option<Id>,
    /// What doesn't follow.
    pub detail: String,
}

/// What [`recompute`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recomputed {
    /// Every finding, in the order the checks run.
    pub findings: Vec<Finding>,
    /// The choice recomputed from the record: in a choice round, the candidate ranked first among those kept or
    /// outranked; `None` in a shortlist round, with no such candidate, or when the contract check fails.
    pub choice: Option<Id>,
}

impl Recomputed {
    /// Whether the record passes every check.
    pub fn passes(&self) -> bool {
        self.findings.is_empty()
    }

    /// The checks the record fails, each once, in order.
    pub fn failed_checks(&self) -> Vec<Check> {
        let mut checks: Vec<Check> = self.findings.iter().map(|f| f.check).collect();
        checks.sort();
        checks.dedup();
        checks
    }
}

/// Recomputes a decision record's choice from the record alone, and reports every way the record doesn't
/// follow from its own inputs.
pub fn recompute(record: &DecisionRecord) -> Recomputed {
    let mut findings = Vec::new();
    if let Err(violation) = record.check() {
        findings.push(Finding {
            check: Check::Contract,
            candidate: None,
            detail: violation.to_string(),
        });
        return Recomputed {
            findings,
            choice: None,
        };
    }
    let reader = Reader { record };
    reader.shortlist_reads_advertised_figures(&mut findings);
    reader.amounts_follow_their_figures(&mut findings);
    reader.rate_cycles_are_not_followed(&mut findings);
    reader.outranked_candidates_are_eligible(&mut findings);
    let choice = reader.ranking_follows_the_objective(&mut findings);
    Recomputed { findings, choice }
}

/// A provider's accepted dry-run answer, a fresh quote included, as the suite reads it.
struct Answer<'a> {
    amount_in: &'a str,
    amount_out: &'a str,
    fee: &'a Fee,
    settlement_seconds: u64,
}

fn accepted(body: &ResultBody) -> Option<Answer<'_>> {
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            ReserveMovementOutcome::DryRunAccepted(a) => Some(Answer {
                amount_in: &a.amount_in,
                amount_out: &a.amount_out,
                fee: &a.fee,
                settlement_seconds: a.settlement_seconds.get(),
            }),
            _ => None,
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::DryRunAccepted(q) => Some(Answer {
                amount_in: &q.amount_in,
                amount_out: &q.amount_out,
                fee: &q.fee,
                settlement_seconds: q.settlement_seconds.get(),
            }),
            _ => None,
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::DryRunAccepted(q) => Some(Answer {
                amount_in: &q.amount_in,
                amount_out: &q.amount_out,
                fee: &q.fee,
                settlement_seconds: q.settlement_seconds.get(),
            }),
            _ => None,
        },
        _ => None,
    }
}

/// When a statement figure's envelope says it was made; `None` for a figure that isn't a statement.
fn statement_made_at(figure: &Figure) -> Option<&str> {
    match &figure.value {
        FigureValue::FreshQuote { quote, .. } => Some(&quote.made_at),
        FigureValue::DryRun { result, .. } => Some(&result.made_at),
        FigureValue::Eligibility { judgement, .. } => Some(&judgement.made_at),
        _ => None,
    }
}

fn is_tuple(figure: &Figure) -> bool {
    matches!(
        figure.value,
        FigureValue::ReserveTuple { .. }
            | FigureValue::ExchangeTuple { .. }
            | FigureValue::RampTuple { .. }
    )
}

fn number(text: &str) -> BigUint {
    price::whole(text).expect("the contract checked it is an amount")
}

/// The figures one operation cites, as the contract has already bound them: one tuple, one price for it, and the
/// rest naming that tuple. Since router-api 0.6.0 the operation cites every usable statement about its tuple at its
/// amount in, and a fresh quote only at its amount in.
struct Cited<'a> {
    tuple: &'a Figure,
    price: &'a Figure,
    /// The statements it cites that are usable: within their validity and not void at the bounds' deployment and
    /// protocol version. Only these bind the operation's terms and time. This filter, and the amount filter in
    /// `settlement_seconds`, are backups that the contract makes unreachable for a ranked candidate: since
    /// router-api 0.6.0 it refuses a fresh quote at another amount anywhere but on the candidate it drops, and a
    /// kept candidate citing a stale or void statement; the eligibility check holds an outranked one to the same.
    usable: Vec<&'a Figure>,
    amount_in: &'a str,
}

impl Cited<'_> {
    /// The usable accepted answers it cites for its tuple and for its amount in, fresh quotes included, with
    /// their figure ids.
    fn answers(&self) -> Vec<(&Id, Answer<'_>)> {
        self.usable
            .iter()
            .filter_map(|f| match &f.value {
                FigureValue::FreshQuote { tuple, quote } if tuple == &self.tuple.figure_id => {
                    accepted(&quote.statement).map(|a| (&f.figure_id, a))
                }
                FigureValue::DryRun { tuple, result, .. } if tuple == &self.tuple.figure_id => {
                    accepted(&result.statement).map(|a| (&f.figure_id, a))
                }
                _ => None,
            })
            .filter(|(_, a)| a.amount_in == self.amount_in)
            .collect()
    }

    /// The declared settlement seconds the objective counts: from the usable fresh quote it cites for its amount
    /// in, or else from its price (`Objective::FastestSettlement`). A quote at another amount never counts. Usable
    /// quotes for one amount agree on their terms (the contract), not on their time; if several are cited, the
    /// slowest counts.
    fn settlement_seconds(&self) -> u64 {
        let quoted = self
            .usable
            .iter()
            .filter_map(|f| match &f.value {
                FigureValue::FreshQuote { tuple, quote } if tuple == &self.tuple.figure_id => {
                    accepted(&quote.statement)
                        .filter(|a| a.amount_in == self.amount_in)
                        .map(|a| a.settlement_seconds)
                }
                _ => None,
            })
            .max();
        quoted.unwrap_or_else(|| match &self.price.value {
            FigureValue::Price {
                settlement_seconds, ..
            } => settlement_seconds.get(),
            _ => unreachable!("a price figure"),
        })
    }
}

/// How a candidate ranks under an objective.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Standing {
    /// `lowest_total_cost`: the sum of the parts' last `amount_out`; more is better.
    Arrives(BigUint),
    /// `fastest_settlement`: the slowest part's time, a part's being the sum of its operations' declared
    /// settlement seconds; less is better. Summed in 128 bits, so 255 operations of 2^53 − 1 seconds still fit.
    Settles(u128),
}

struct Reader<'a> {
    record: &'a DecisionRecord,
}

impl<'a> Reader<'a> {
    fn figure(&self, id: &Id) -> &'a Figure {
        self.record
            .snapshot
            .figures
            .iter()
            .find(|f| &f.figure_id == id)
            .expect("the contract checked every cited figure is in the snapshot")
    }

    /// Whether a statement figure is usable here: within its validity and not void (`REQ-QTE-4`, `REQ-PRT-1`), and,
    /// for an accepted answer in a choice round, made at or after the shortlist round's `taken_at` (`REQ-CAT-5`).
    fn is_usable(&self, figure: &Figure) -> bool {
        let (deployment, version) = (
            &self.record.bounds.deployment,
            self.record.bounds.protocol_version.as_str(),
        );
        let in_context = match &figure.value {
            FigureValue::FreshQuote { quote, .. } => {
                quote.check_context(deployment, version).is_ok()
            }
            FigureValue::DryRun { result, .. } => result.check_context(deployment, version).is_ok(),
            FigureValue::Eligibility { judgement, .. } => {
                judgement.check_context(deployment, version).is_ok()
            }
            _ => true,
        };
        // In a choice round, an accepted answer made before the shortlist round's `taken_at` is not usable either;
        // a refusal or a judgement binds whenever it was made.
        let answer = match &figure.value {
            FigureValue::FreshQuote { quote, .. } => accepted(&quote.statement).is_some(),
            FigureValue::DryRun { result, .. } => accepted(&result.statement).is_some(),
            _ => false,
        };
        let after_the_shortlist = match (&self.record.request.round, statement_made_at(figure)) {
            _ if !answer => true,
            (Round::Choice { refines }, Some(made_at)) => Timestamp::new(made_at)
                .is_some_and(|t| t.unix_nanos() >= refines.taken_at.unix_nanos()),
            _ => true,
        };
        figure.is_within_validity() && in_context && after_the_shortlist
    }

    fn cited(&self, operation: &'a Operation) -> Cited<'a> {
        let all: Vec<&Figure> = operation.figures.iter().map(|id| self.figure(id)).collect();
        let tuple = *all
            .iter()
            .find(|f| is_tuple(f))
            .expect("the contract checked one tuple is cited");
        let price = *all
            .iter()
            .find(|f| matches!(f.value, FigureValue::Price { .. }))
            .expect("the contract checked one price is cited");
        let usable = all.iter().copied().filter(|f| self.is_usable(f)).collect();
        Cited {
            tuple,
            price,
            usable,
            amount_in: operation.amount_in.amount().as_str(),
        }
    }

    fn operations(candidate: &Candidate) -> impl Iterator<Item = (usize, usize, &Operation)> {
        candidate.parts.iter().enumerate().flat_map(|(p, part)| {
            part.operations
                .iter()
                .enumerate()
                .map(move |(o, operation)| (p, o, operation))
        })
    }

    fn shortlist_reads_advertised_figures(&self, findings: &mut Vec<Finding>) {
        if !matches!(self.record.request.round, Round::Shortlist {}) {
            return;
        }
        for figure in &self.record.snapshot.figures {
            if matches!(
                figure.value,
                FigureValue::FreshQuote { .. }
                    | FigureValue::DryRun { .. }
                    | FigureValue::Eligibility { .. }
            ) {
                findings.push(Finding {
                    check: Check::ShortlistReadsAdvertisedFigures,
                    candidate: None,
                    detail: format!(
                        "the shortlist round's snapshot holds statement {}, which only the choice round asks for",
                        figure.figure_id
                    ),
                });
            }
        }
    }

    /// Every operation's fee and amount out, recomputed. An operation that cites usable accepted answers for its
    /// amount in states each one's fee and amount out: a provider's confirmed terms bind over its advertised price,
    /// as the contract's confirmation rule already requires of the choice. Any other operation states what its
    /// price gives on its amount in, between its two units.
    fn amounts_follow_their_figures(&self, findings: &mut Vec<Finding>) {
        for candidate in &self.record.candidates {
            for (p, o, operation) in Self::operations(candidate) {
                let mut fail = |detail: String| {
                    findings.push(Finding {
                        check: Check::AmountsFollowTheirFigures,
                        candidate: Some(candidate.candidate_id.clone()),
                        detail: format!("part {p}, operation {o}: {detail}"),
                    });
                };
                let cited = self.cited(operation);
                let amount_in = operation.amount_in.amount().as_str();
                let answers = cited.answers();
                if !answers.is_empty() {
                    for (id, answer) in answers {
                        if answer.fee != &operation.fee
                            || answer.amount_out != operation.amount_out.as_str()
                        {
                            fail(format!(
                                "states a fee of {} and {} out, but answer {id} gives {} and {}",
                                operation.fee.amount,
                                operation.amount_out,
                                answer.fee.amount,
                                answer.amount_out
                            ));
                        }
                    }
                    continue;
                }
                let FigureValue::Price { fee, rate, .. } = &cited.price.value else {
                    unreachable!("a price figure");
                };
                let Some(terms) = price::evaluate(
                    fee,
                    rate.as_ref(),
                    &number(amount_in),
                    &operation.from.asset,
                    &operation.to.asset,
                ) else {
                    fail(format!(
                        "price {} charges more than the {amount_in} that enters",
                        cited.price.figure_id
                    ));
                    continue;
                };
                let stated_fee = price::whole(&operation.fee.amount);
                if stated_fee.as_ref() != Some(&terms.fee) {
                    fail(format!(
                        "states a fee of {}, but price {} charges {}",
                        operation.fee.amount, cited.price.figure_id, terms.fee
                    ));
                }
                if number(operation.amount_out.as_str()) != terms.amount_out {
                    fail(format!(
                        "states {} out, but price {} gives {}",
                        operation.amount_out, cited.price.figure_id, terms.amount_out
                    ));
                }
            }
        }
    }

    /// Whether a part returns to a place it held value at before (the same node and asset) with strictly more
    /// than it held there: a rate cycle implying free profit, possibly stale or false, never followed (ADR-0006).
    ///
    /// The two holdings may be counted in different units of the asset, so they are compared as whole numbers
    /// over a common unit: `a` units of `10^-da` against `b` units of `10^-db` is `a × 10^db` against `b × 10^da`.
    fn follows_a_profitable_cycle(candidate: &Candidate) -> bool {
        let worth_more = |(now, at): (&BigUint, &Endpoint), (then, was): (&BigUint, &Endpoint)| {
            at.node == was.node
                && at.asset.id == was.asset.id
                && now * BigUint::from(10u8).pow(u32::from(was.asset.decimals))
                    > then * BigUint::from(10u8).pow(u32::from(at.asset.decimals))
        };
        candidate.parts.iter().any(|part| {
            let first = part.operations.first();
            let mut held = vec![(number(first.amount_in.amount().as_str()), &first.from)];
            for operation in part.operations.iter() {
                let arrived = number(operation.amount_out.as_str());
                if held
                    .iter()
                    .any(|(amount, was)| worth_more((&arrived, &operation.to), (amount, was)))
                {
                    return true;
                }
                held.push((arrived, &operation.to));
            }
            false
        })
    }

    fn rate_cycles_are_not_followed(&self, findings: &mut Vec<Finding>) {
        for candidate in &self.record.candidates {
            let cycles = Self::follows_a_profitable_cycle(candidate);
            let detail = match &candidate.dropped {
                Some(DropReason::SuspectRateCycle {}) if !cycles => {
                    "is dropped as a suspect rate cycle, but no part returns anywhere with more than it held"
                }
                None | Some(DropReason::Outranked { .. }) if cycles => {
                    "is kept or outranked, but a part returns to where it was with more than it held there"
                }
                _ => continue,
            };
            findings.push(Finding {
                check: Check::RateCyclesAreNotFollowed,
                candidate: Some(candidate.candidate_id.clone()),
                detail: detail.to_owned(),
            });
        }
    }

    /// Whether a candidate would pass the contract as the record's choice (or as kept, in a shortlist round): the
    /// record with it undropped, and in a choice round with it chosen and the chosen one outranked by it, passes
    /// [`DecisionRecord::check`]. The contract's tests on a kept candidate then apply to it unchanged.
    fn as_the_choice(&self, id: &Id) -> Result<(), String> {
        let mut trial = self.record.clone();
        let choice = matches!(trial.request.round, Round::Choice { .. });
        let previous = trial.chosen.clone();
        for candidate in &mut trial.candidates {
            if &candidate.candidate_id == id {
                candidate.dropped = None;
            } else if choice && previous.as_ref() == Some(&candidate.candidate_id) {
                candidate.dropped = Some(DropReason::Outranked { by: id.clone() });
            }
        }
        if choice {
            trial.chosen = Some(id.clone());
        }
        trial.check().map_err(|v| v.to_string())
    }

    fn outranked_candidates_are_eligible(&self, findings: &mut Vec<Finding>) {
        for candidate in &self.record.candidates {
            if !matches!(candidate.dropped, Some(DropReason::Outranked { .. })) {
                continue;
            }
            if let Err(why) = self.as_the_choice(&candidate.candidate_id) {
                findings.push(Finding {
                    check: Check::OutrankedCandidatesAreEligible,
                    candidate: Some(candidate.candidate_id.clone()),
                    detail: format!("is outranked, but couldn't be the choice: {why}"),
                });
            }
        }
    }

    fn standing(&self, candidate: &Candidate) -> Option<Standing> {
        match self.record.request.objective {
            Objective::LowestTotalCost {} => Some(Standing::Arrives(
                candidate
                    .parts
                    .iter()
                    .map(|part| number(part.operations.last().amount_out.as_str()))
                    .sum(),
            )),
            Objective::FastestSettlement {} => Some(Standing::Settles(
                candidate
                    .parts
                    .iter()
                    .map(|part| {
                        part.operations
                            .iter()
                            .map(|o| u128::from(self.cited(o).settlement_seconds()))
                            .sum::<u128>()
                    })
                    .max()
                    .unwrap_or(0),
            )),
            _ => None,
        }
    }

    /// Whether `a` comes before `b`: better by the objective, or equal by it and with the lower `candidate_id`,
    /// comparing bytes.
    fn ahead(&self, a: &Candidate, b: &Candidate) -> bool {
        let by_objective = match (self.standing(a), self.standing(b)) {
            (Some(Standing::Arrives(x)), Some(Standing::Arrives(y))) => y.cmp(&x),
            (Some(Standing::Settles(x)), Some(Standing::Settles(y))) => x.cmp(&y),
            _ => Ordering::Equal,
        };
        by_objective.then_with(|| {
            a.candidate_id
                .as_str()
                .as_bytes()
                .cmp(b.candidate_id.as_str().as_bytes())
        }) == Ordering::Less
    }

    fn ranking_follows_the_objective(&self, findings: &mut Vec<Finding>) -> Option<Id> {
        let record = self.record;
        let mut fail = |candidate: Option<&Id>, detail: String| {
            findings.push(Finding {
                check: Check::RankingFollowsTheObjective,
                candidate: candidate.cloned(),
                detail,
            });
        };
        if self.standing_known().is_none() {
            fail(
                None,
                format!(
                    "the objective {:?} isn't one this suite can rank by",
                    record.request.objective
                ),
            );
            return None;
        }
        let kept: Vec<&Candidate> = record
            .candidates
            .iter()
            .filter(|c| c.dropped.is_none())
            .collect();
        let outranked: Vec<(&Candidate, &Id)> = record
            .candidates
            .iter()
            .filter_map(|c| match &c.dropped {
                Some(DropReason::Outranked { by }) => Some((c, by)),
                _ => None,
            })
            .collect();
        for keeper in &kept {
            for (beaten, _) in &outranked {
                if !self.ahead(keeper, beaten) {
                    fail(
                        Some(&beaten.candidate_id),
                        format!(
                            "is outranked, but ranks ahead of {}, which is kept",
                            keeper.candidate_id
                        ),
                    );
                }
            }
        }
        for (beaten, by) in &outranked {
            let winner = kept
                .iter()
                .copied()
                .chain(outranked.iter().map(|(c, _)| *c))
                .find(|c| &c.candidate_id == *by);
            match winner {
                Some(winner) if self.ahead(winner, beaten) => {}
                Some(_) => fail(
                    Some(&beaten.candidate_id),
                    format!("is outranked by {by}, which doesn't rank ahead of it"),
                ),
                None => fail(
                    Some(&beaten.candidate_id),
                    format!("is outranked by {by}, which is neither kept nor outranked itself"),
                ),
            }
        }
        let mut pool: Vec<&Candidate> = kept
            .iter()
            .copied()
            .chain(outranked.iter().map(|(c, _)| *c))
            .collect();
        pool.sort_by(|a, b| {
            if self.ahead(a, b) {
                Ordering::Less
            } else if self.ahead(b, a) {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
        let Round::Choice { .. } = record.request.round else {
            return None;
        };
        // A choice round that chooses none of its pool leaves the pool's first outranked by no one ahead of it,
        // which the checks above report.
        pool.first().map(|c| c.candidate_id.clone())
    }

    fn standing_known(&self) -> Option<()> {
        match self.record.request.objective {
            Objective::LowestTotalCost {} | Objective::FastestSettlement {} => Some(()),
            _ => None,
        }
    }
}

/// One cell the suite runs a router through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cell {
    /// Routing the same request, snapshot and bounds twice gives byte-identical records.
    SameInputsGiveTheSameRecord,
    /// The record holds the request, snapshot and bounds unchanged.
    KeepsItsInputs,
    /// The record survives serialising: its bytes deserialise, which runs the contract's check.
    PassesTheContract,
    /// [`recompute`] finds nothing in the deserialised record.
    RecomputesFromTheRecordAlone,
    /// The record chooses the route the case expects (or none), and its recomputed choice is its own.
    ChoosesTheExpectedRoute,
    /// Routing the record's own request, snapshot and bounds gives the record again, byte for byte.
    ReplaysFromItsOwnInputs,
    /// Across the AC-2 pair, which differ only in their objective, the chosen routes differ (`AC-2`,
    /// `REQ-QTE-11`).
    ObjectiveChangesTheChoice,
}

impl Cell {
    /// The cell's name, as reports print it.
    pub fn name(self) -> &'static str {
        match self {
            Cell::SameInputsGiveTheSameRecord => "same-inputs-give-the-same-record",
            Cell::KeepsItsInputs => "keeps-its-inputs",
            Cell::PassesTheContract => "passes-the-contract",
            Cell::RecomputesFromTheRecordAlone => "recomputes-from-the-record-alone",
            Cell::ChoosesTheExpectedRoute => "chooses-the-expected-route",
            Cell::ReplaysFromItsOwnInputs => "replays-from-its-own-inputs",
            Cell::ObjectiveChangesTheChoice => "objective-changes-the-choice",
        }
    }
}

/// One cell's outcome on one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The case, by name.
    pub case: &'static str,
    /// The cell.
    pub cell: Cell,
    /// `Ok` if it passed, or what went wrong.
    pub result: Result<(), String>,
}

/// What [`run`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Every cell on every case, in order.
    pub outcomes: Vec<Outcome>,
}

impl Report {
    /// The outcomes that failed.
    pub fn failures(&self) -> Vec<&Outcome> {
        self.outcomes.iter().filter(|o| o.result.is_err()).collect()
    }

    /// Whether every cell passed.
    pub fn passes(&self) -> bool {
        self.failures().is_empty()
    }

    /// One line per failed cell, for an assertion message.
    pub fn summary(&self) -> String {
        self.failures()
            .iter()
            .map(|o| {
                format!(
                    "{} / {}: {}",
                    o.case,
                    o.cell.name(),
                    o.result.as_ref().err().map(String::as_str).unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The tuple figures a candidate's operations cite, part by part: its route through the catalogue.
pub fn path(record: &DecisionRecord, candidate: &Candidate) -> Vec<Vec<String>> {
    let reader = Reader { record };
    candidate
        .parts
        .iter()
        .map(|part| {
            part.operations
                .iter()
                .map(|o| reader.cited(o).tuple.figure_id.as_str().to_owned())
                .collect()
        })
        .collect()
}

fn bytes(record: &DecisionRecord) -> Vec<u8> {
    serde_json::to_vec(record).expect("a decision record serialises")
}

fn run_case<R: Router>(case: &Case, outcomes: &mut Vec<Outcome>) -> Option<DecisionRecord> {
    let mut report = |cell: Cell, result: Result<(), String>| {
        outcomes.push(Outcome {
            case: case.name,
            cell,
            result,
        });
    };
    let first = R::route(&case.request, &case.snapshot, &case.bounds);
    let second = R::route(&case.request, &case.snapshot, &case.bounds);
    let written = bytes(&first);
    report(
        Cell::SameInputsGiveTheSameRecord,
        if written == bytes(&second) {
            Ok(())
        } else {
            Err("two calls on the same inputs gave different records".to_owned())
        },
    );
    report(
        Cell::KeepsItsInputs,
        if first.request == case.request
            && first.snapshot == case.snapshot
            && first.bounds == case.bounds
        {
            Ok(())
        } else {
            Err("the record doesn't hold its request, snapshot and bounds unchanged".to_owned())
        },
    );
    let read: DecisionRecord = match serde_json::from_slice(&written) {
        Ok(read) => {
            report(Cell::PassesTheContract, Ok(()));
            read
        }
        Err(e) => {
            report(Cell::PassesTheContract, Err(e.to_string()));
            return None;
        }
    };
    let recomputed = recompute(&read);
    report(
        Cell::RecomputesFromTheRecordAlone,
        if recomputed.passes() {
            Ok(())
        } else {
            Err(recomputed
                .findings
                .iter()
                .map(|f| format!("{}: {:?} {}", f.check.name(), f.candidate, f.detail))
                .collect::<Vec<_>>()
                .join("; "))
        },
    );
    let chosen_path = read.chosen().map(|c| path(&read, c));
    let expected = match &case.expected {
        Expected::Shortlist => None,
        Expected::Route(route) => Some(Some(vec![
            route.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
        ])),
        Expected::NoRoute => Some(None),
    };
    report(
        Cell::ChoosesTheExpectedRoute,
        match expected {
            None if read.chosen.is_none() => Ok(()),
            None => Err("a shortlist round chooses nothing".to_owned()),
            Some(want) if want != chosen_path => Err(format!(
                "chose {chosen_path:?}, but the case expects {want:?}"
            )),
            Some(_) if recomputed.choice != read.chosen => Err(format!(
                "the record chooses {:?}, but recomputes to {:?}",
                read.chosen, recomputed.choice
            )),
            Some(_) => Ok(()),
        },
    );
    let replayed = R::route(&read.request, &read.snapshot, &read.bounds);
    report(
        Cell::ReplaysFromItsOwnInputs,
        if bytes(&replayed) == written {
            Ok(())
        } else {
            Err("routing the record's own inputs gave another record".to_owned())
        },
    );
    Some(read)
}

/// Runs a router through every case and cell of the suite.
pub fn run<R: Router>() -> Report {
    let mut outcomes = Vec::new();
    let mut chosen = Vec::new();
    for case in cases() {
        let record = run_case::<R>(&case, &mut outcomes);
        if case.objective_pair {
            let route = record.as_ref().and_then(|r| r.chosen().map(|c| path(r, c)));
            chosen.push(route);
        }
    }
    let result = match chosen.as_slice() {
        [Some(a), Some(b)] if a != b => Ok(()),
        [a, b] => Err(format!(
            "the two objectives chose {a:?} and {b:?}; they should choose different routes"
        )),
        _ => Err("the suite's cases should hold exactly one objective pair".to_owned()),
    };
    outcomes.push(Outcome {
        case: "ac2-objective-pair",
        cell: Cell::ObjectiveChangesTheChoice,
        result,
    });
    Report { outcomes }
}

#[cfg(test)]
mod tests {
    use super::Reader;
    use rutent_contracts::router_api::Candidate;
    use serde_json::json;

    /// From USDC in 6 decimals to COP and back to USDC in 18 decimals, at one node, arriving with `back`.
    fn round_trip(back: &str) -> Candidate {
        let at = |id: &str, decimals: u8| json!({ "node": "ledger-co", "asset": { "id": id, "decimals": decimals } });
        let exchange = |from, to, amount_in: &str, fee_asset: &str, amount_out: &str| {
            json!({
                "kind": "exchange", "provider": "exchange-loop", "from": from, "to": to,
                "amount_in": amount_in, "fee": { "asset": fee_asset, "amount": "0" },
                "amount_out": amount_out, "figures": ["f-ex"],
            })
        };
        serde_json::from_value(json!({
            "candidate_id": "c1",
            "parts": [{ "operations": [
                exchange(at("USDC", 6), at("COP", 2), "1000000", "USDC", "410000"),
                exchange(at("COP", 2), at("USDC", 18), "410000", "COP", back),
            ] }],
        }))
        .unwrap()
    }

    // verifies: ADR-0006
    #[test]
    fn a_rate_cycle_is_compared_across_units_of_one_asset() {
        // 1 USDC left; 2 USDC, or 1.000000000000000001, come back in units of 10^-18: a cycle.
        assert!(Reader::follows_a_profitable_cycle(&round_trip(
            "2000000000000000000"
        )));
        assert!(Reader::follows_a_profitable_cycle(&round_trip(
            "1000000000000000001"
        )));
        // Exactly 1 USDC, or less, isn't free profit.
        assert!(!Reader::follows_a_profitable_cycle(&round_trip(
            "1000000000000000000"
        )));
        assert!(!Reader::follows_a_profitable_cycle(&round_trip(
            "500000000000000000"
        )));
    }
}
