//! The shortlist (`REQ-CAT-5`, `REQ-CAT-14`, `REQ-QTE-13`, `DEC-8`): what Core asks between the two rounds, and of
//! whom.
//!
//! The router never calls a provider. Its shortlist round's record keeps the candidates it doesn't drop, and
//! [`asks`] reads from that record alone the questions each of their providers is asked: for every operation of a
//! kept candidate, its provider's fresh quote (a route or exchange) or dry run (a withdrawal or deposit) at the
//! amount that reaches it, and eligibility for its tuple. No other provider is asked anything. Core puts the
//! questions, and the answers come back as recorded figures in the choice round's snapshot, so the choice round is
//! the same pure function of its inputs ([`Asks::admits`] says which answers may enter). There a refused dry run
//! drops its option, and another provider serving the same ends, asked beside it, is chosen if it confirms
//! (`AC-3`).
//!
//! Each question names only its provider's own tuple, the two ends and the amount entering it: no candidate, no
//! request, no rival's tuple or figure (`REQ-CAT-5`). And a provider is asked about each of its tuples once, at one
//! amount: where two kept paths reach its operation with different amounts, it is asked at the amount of the path
//! the search ranks first, so it can't read from two amounts that a rival came before it, or what that rival
//! charges. The other path's operation stays unconfirmed in the choice round until Core asks again (`F-020`).
//!
//! A shortlist round that keeps nothing asks nothing: no path arrives with a positive balance within the bounds,
//! and the routing is refused before any provider hears of it (`REQ-QTE-13`).

use std::collections::BTreeMap;

use rutent_contracts::router_api::{
    DecisionRecord, Endpoint, Figure, FigureValue, Id, OperationKind, PositiveAmount, Round,
};

use std::cmp::Ordering;

use num_bigint::BigUint;
use rutent_contracts::router_api::{Candidate, Objective, Operation};

use crate::price::whole;
use crate::search::{accepted, refuses};

/// What a question asks its provider for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Wanted {
    /// The provider's fresh quote, its dry-run answer as a transfer or conversion quote: for a route or an
    /// exchange.
    FreshQuote,
    /// The reserve's dry run: for a withdrawal or a deposit.
    DryRun,
}

impl Wanted {
    /// What confirms an operation of this kind in the choice round.
    pub fn for_kind(kind: OperationKind) -> Self {
        match kind {
            OperationKind::Route | OperationKind::Exchange => Wanted::FreshQuote,
            OperationKind::Withdrawal | OperationKind::Deposit => Wanted::DryRun,
        }
    }
}

/// One question to one shortlisted provider, about one operation: only what the step needs (`REQ-CAT-5`). Each
/// question, taken alone, names no rival's tuple or figure; that the set a provider receives reveals no rival either
/// is kept by asking each tuple at one amount only (see [`asks`]).
///
/// Eligibility is checked for the same tuple, for the parties its provider's rules name (`REQ-CPL-6`). The
/// snapshot names no class or verifier for a tuple, so which judgement to seek is the catalogue's to say; the
/// question names the tuple it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// The tuple asked about, by its figure id in the shortlist round's snapshot.
    pub tuple: Id,
    /// The operation the tuple performs.
    pub kind: OperationKind,
    /// Where value enters it, in its input unit.
    pub from: Endpoint,
    /// Where value leaves it, in its output unit.
    pub to: Endpoint,
    /// What reaches it, after the fees before it, in its input unit (`REQ-QTE-12`).
    pub amount_in: PositiveAmount,
    /// What the provider is asked for.
    pub wants: Wanted,
}

/// Why nothing is asked from a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The record is a choice round's. Only a shortlist round's candidates are a shortlist.
    NotAShortlistRound,
    /// The shortlist round kept no candidate: none arrives with a positive balance within the bounds
    /// (`REQ-QTE-13`), so the routing is refused and no provider is asked.
    NoRoute,
}

/// The questions a shortlist round's record leads to, by provider (`REQ-CAT-5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asks {
    /// Each provider, keyed by its id's text, with its questions, ordered by tuple id, one per tuple.
    by_provider: BTreeMap<String, (Id, Vec<Question>)>,
}

/// One provider's questions, keyed by tuple id, while they are gathered.
type ByTuple = BTreeMap<String, Question>;

/// The questions a shortlist round's record leads to: for each tuple an operation of a kept candidate takes, one
/// question to its provider at the amount entering it (`REQ-CAT-5`, `DEC-8`).
///
/// Where kept candidates take one tuple at different amounts, the question is at the amount on the candidate ranked
/// first: by the request's objective, as the choice round ranks, then by `candidate_id`. So the
/// best route is the one that can be confirmed at once.
///
/// Pure: it reads the record alone, so the same record always gives the same questions, whatever the order of its
/// snapshot's figures.
pub fn asks(record: &DecisionRecord) -> Result<Asks, Refusal> {
    if !matches!(record.request.round, Round::Shortlist {}) {
        return Err(Refusal::NotAShortlistRound);
    }
    let kept = record.shortlist();
    if kept.is_empty() {
        return Err(Refusal::NoRoute);
    }
    let mut keyed: BTreeMap<String, (Id, ByTuple)> = BTreeMap::new();
    for operation in ranked(record, kept) {
        // An operation's first figure is its tuple (`Operation::figures`, as the search states it).
        let tuple = operation.figures.first().clone();
        keyed
            .entry(operation.provider.as_str().to_owned())
            .or_insert_with(|| (operation.provider.clone(), BTreeMap::new()))
            .1
            .entry(tuple.as_str().to_owned())
            .or_insert_with(|| Question {
                tuple,
                kind: operation.kind,
                from: operation.from.clone(),
                to: operation.to.clone(),
                amount_in: operation.amount_in.clone(),
                wants: Wanted::for_kind(operation.kind),
            });
    }
    Ok(Asks {
        by_provider: keyed
            .into_iter()
            .map(|(key, (provider, questions))| {
                (key, (provider, questions.into_values().collect()))
            })
            .collect(),
    })
}

impl Asks {
    /// The providers asked, in byte order of their ids.
    pub fn providers(&self) -> impl Iterator<Item = &Id> {
        self.by_provider.values().map(|(provider, _)| provider)
    }

    /// What `provider` is asked: its own questions only, empty for a provider off the shortlist.
    pub fn of(&self, provider: &Id) -> &[Question] {
        self.by_provider
            .get(provider.as_str())
            .map(|(_, questions)| questions.as_slice())
            .unwrap_or_default()
    }

    /// Whether `figure` may enter the choice round's snapshot. An advertised figure or an exposure always may. A
    /// provider's answer may only if it answers a question: made by the provider asked, about the tuple asked, at
    /// the amount asked, in the form asked. A question that wants a fresh quote is answered by the accepted quote, or
    /// by a refused dry run, which is how a ramp or exchange refuses (a fresh quote is always accepted; the contract
    /// refuses any other). A question that wants a dry run is answered by a dry run, accepted or refused. An
    /// eligibility judgement may enter only if it is for a tuple some provider was asked about.
    pub fn admits(&self, figure: &Figure) -> bool {
        let answers = |tuple: &Id, amount: &str, fits: &dyn Fn(Wanted) -> bool| {
            figure.origin.provider().is_some_and(|p| {
                self.of(p).iter().any(|q| {
                    &q.tuple == tuple && q.amount_in.amount().as_str() == amount && fits(q.wants)
                })
            })
        };
        match &figure.value {
            FigureValue::FreshQuote { tuple, quote } => accepted(&quote.statement)
                .is_some_and(|a| answers(tuple, a.amount_in, &|w| w == Wanted::FreshQuote)),
            FigureValue::DryRun {
                tuple,
                amount_in,
                result,
            } => {
                let refusal = refuses(&result.statement);
                answers(tuple, amount_in.amount().as_str(), &|w| {
                    w == Wanted::DryRun || refusal
                })
            }
            FigureValue::Eligibility { tuple, .. } => self
                .by_provider
                .values()
                .flat_map(|(_, questions)| questions)
                .any(|q| &q.tuple == tuple),
            FigureValue::ReserveTuple { .. }
            | FigureValue::ExchangeTuple { .. }
            | FigureValue::RampTuple { .. }
            | FigureValue::Price { .. }
            | FigureValue::Exposure { .. } => true,
        }
    }
}

/// The kept candidates' operations, the candidates ranked by the request's objective as the choice round ranks
/// them, ties going to the lowest `candidate_id`. Lowest total cost, the default: the most value at the
/// destination. Fastest settlement: the least declared settlement time, a part's the sum of its operations' prices'
/// seconds and a split's its slowest part's; a shortlist round holds no fresh quote to state another.
fn ranked<'a>(record: &'a DecisionRecord, mut kept: Vec<&'a Candidate>) -> Vec<&'a Operation> {
    let price_seconds = |operation: &Operation| -> u128 {
        operation
            .figures
            .iter()
            .find_map(|id| {
                record
                    .snapshot
                    .figures
                    .iter()
                    .find(|f| &f.figure_id == id)
                    .and_then(|f| match &f.value {
                        FigureValue::Price {
                            settlement_seconds, ..
                        } => Some(u128::from(settlement_seconds.get())),
                        _ => None,
                    })
            })
            .unwrap_or_default()
    };
    let arrives = |c: &Candidate| -> BigUint {
        c.parts
            .iter()
            .map(|p| whole(p.operations.last().amount_out.as_str()).unwrap_or_default())
            .sum()
    };
    let settles = |c: &Candidate| -> u128 {
        c.parts
            .iter()
            .map(|p| p.operations.iter().map(price_seconds).sum::<u128>())
            .max()
            .unwrap_or_default()
    };
    let by_objective = |a: &Candidate, b: &Candidate| -> Ordering {
        match record.request.objective {
            Objective::FastestSettlement {} => settles(a).cmp(&settles(b)),
            _ => arrives(b).cmp(&arrives(a)),
        }
    };
    kept.sort_by(|a, b| {
        by_objective(a, b).then_with(|| a.candidate_id.as_str().cmp(b.candidate_id.as_str()))
    });
    kept.into_iter()
        .flat_map(|c| c.parts.iter())
        .flat_map(|p| p.operations.iter())
        .collect()
}
