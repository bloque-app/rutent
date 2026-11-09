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
//! Each provider's questions name only its own tuple, the two ends and the amount entering it: no candidate, no
//! request, no rival's tuple or figure, so no provider learns a rival's quote or that a rival was asked
//! (`REQ-CAT-5`).
//!
//! A shortlist round that keeps nothing asks nothing: no path arrives with a positive balance within the bounds,
//! and the routing is refused before any provider hears of it (`REQ-QTE-13`).

use std::collections::BTreeMap;

use rutent_contracts::router_api::{
    DecisionRecord, Endpoint, Figure, FigureValue, Id, OperationKind, PositiveAmount, Round,
};

use crate::search::accepted;

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

/// One question to one shortlisted provider, about one operation: only what the step needs (`REQ-CAT-5`).
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
    /// Each provider, keyed by its id's text, with its questions, ordered by tuple id and then amount, each once.
    by_provider: BTreeMap<String, (Id, Vec<Question>)>,
}

/// One provider's questions, keyed by tuple id and amount in, while they are gathered.
type ByTupleAndAmount = BTreeMap<(String, String), Question>;

/// The questions a shortlist round's record leads to: for each operation of each kept candidate, one question to
/// its provider about its tuple at its amount in, each asked once (`REQ-CAT-5`, `DEC-8`).
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
    let mut keyed: BTreeMap<String, (Id, ByTupleAndAmount)> = BTreeMap::new();
    for operation in kept
        .iter()
        .flat_map(|c| c.parts.iter())
        .flat_map(|p| p.operations.iter())
    {
        // An operation's first figure is its tuple (`Operation::figures`, as the search states it).
        let tuple = operation.figures.first().clone();
        let amount = operation.amount_in.amount().as_str().to_owned();
        keyed
            .entry(operation.provider.as_str().to_owned())
            .or_insert_with(|| (operation.provider.clone(), BTreeMap::new()))
            .1
            .entry((tuple.as_str().to_owned(), amount))
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
    /// fresh quote or dry run may only if it answers a question: made by the provider asked, about the tuple asked,
    /// at the amount asked. An eligibility judgement may only if it is for a tuple some provider was asked about.
    pub fn admits(&self, figure: &Figure) -> bool {
        let answers = |tuple: &Id, amount: &str| {
            figure.origin.provider().is_some_and(|p| {
                self.of(p)
                    .iter()
                    .any(|q| &q.tuple == tuple && q.amount_in.amount().as_str() == amount)
            })
        };
        match &figure.value {
            FigureValue::FreshQuote { tuple, quote } => {
                accepted(&quote.statement).is_some_and(|a| answers(tuple, a.amount_in))
            }
            FigureValue::DryRun {
                tuple, amount_in, ..
            } => answers(tuple, amount_in.amount().as_str()),
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
