//! The routing rounds: `route` twice per routing, the shortlist round then the choice round, with each question about a
//! tuple its own transaction.
//!
//! Declared by `T-020-01`; `T-020-06` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! [`route`] runs one routing of one order (router-api's README, "Two rounds"):
//!
//! 1. **The shortlist round.** A snapshot of advertised figures only, formed through [`Snapshots`]; every statement
//!    it refuses recorded through [`record_refused`]; `route` over it; the recorded decision signed with the
//!    operator key and appended as a `decision_record` entry naming the providers it shortlisted and the verifiers
//!    their tuples need, chained to the order's previous decision record ([`History`], `REQ-AUD-4`). A shortlist
//!    that keeps nothing asks no one (`REQ-QTE-13`), and its record names no provider.
//! 2. **The questions.** Only what [`shortlist::asks`] gives: each shortlisted provider about each of its tuples, at
//!    one amount (`REQ-CAT-5`, `cat5-single-amount-asks`), and eligibility only for a shortlisted tuple, for the
//!    parties its rules name, each first put to policy as a question about a person on the shortlisted route
//!    (`REQ-CPL-6`, `REQ-CPL-2`, `F-021`). A judgement enters only from the verifier its need names, about its
//!    subject and class, and a tuple with needs is offered only with a judgement cited for each (`REQ-AUZ-5`).
//!    Before any is sent, all of them are charged to the caller's accountable
//!    party as fanned-out work, and over its bound none is (`REQ-OPS-1`). Each question is its own transaction,
//!    drawn fresh ([`Fresh`]), never one Core asked under before ([`Issued`]) and never naming the order, so
//!    two questions share nothing a provider could link them by. Each is signed with the operator key and appended
//!    to the log, about its order and transaction, before it is sent, through `rounds/journal.rs`, the one adapter to
//!    the log.
//! 3. **The reserve's yes** (`REQ-ORD-32`). A withdrawal's dry run reveals whether funds suffice, so it is a balance
//!    check (`REQ-CAT-4`): it is sent only once the catalogue's `begin_check` lets it, in the order's session, and
//!    its answer is read through `answer_check`. `own_order` is set only where the check is the order's own: a
//!    withdrawal at the order's origin reserve (`REQ-CAT-3`, `F-012`'s notes). Only the origin reserve is asked for
//!    a withdrawal: the routing runs under the caller's bounds with every other reserve's withdrawal tuple excluded,
//!    so the record shows it. A yes counts only as the answer to Core's own question, under the transaction Core
//!    asked it: a yes the user carries names a transaction Core never issued, and is refused wherever it arrives. A
//!    route is offered only on the origin reserve's yes for each of its parts' withdrawals, asked in the user's
//!    session, except on an automatic deposit.
//! 4. **The choice round.** A snapshot adding the answers that name Core's transactions, and nothing else; every
//!    statement it refuses recorded before the record is formed ([`record_refused`], `REQ-AUD-2`), and every other
//!    answer appended with its transaction in its `about`; `route` with `refines` copied from the shortlist record
//!    Core keeps, its request id and its snapshot's `taken_at`, never read from the clock; the record appended and
//!    chained to the one before.
//! 5. **Later rounds.** Where the choice round chooses nothing, the operations it left unconfirmed at a shortlisted
//!    tuple, at an amount that tuple wasn't asked at, are asked again at their own amounts, each its own
//!    transaction, charged as before, and a further choice round refines the same shortlist, at most
//!    [`LATER_ROUNDS`] times. The router drops a candidate whose operation's fresh quote, at another amount, lists
//!    `amount_changes` as `quote_voided`, citing that quote (`REQ-QTE-1`, `F-011`'s notes), and one with no answer at
//!    its amount as `unconfirmed`; both are asked again.
//!
//! **A recalculation** (`T-020-10`'s seams, the one change to this module outside its owning row): a request that
//! `follows` another routes a part from where its value is evidenced, so a part of it that starts with no withdrawal
//! has no yes to ask: its value is in the book already, and no reserve's check is the customer's to answer
//! (`REQ-ORD-2`, `REQ-ORD-22`). Every other part still needs its yes. [`History::recorded`] gives back an order's
//! history as the log holds it.
//!
//! **Core checks the router's record against what it gave it.** A record whose request, snapshot or bounds aren't
//! the round's own is refused before it is recorded or acted on ([`RoutingError::NotItsRecord`]).

mod ask;
mod journal;
mod ports;

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use rutent_contracts::core_internal::calls::{AppendFailure, Charges, Evidence, PersonQuestions};
use rutent_contracts::core_internal::{
    About, Actor, ActorKind, Authority, Charge, ChargeOutcome, Count, PersonOutcome,
    PersonQuestion, Receipt, Role, RoutingRound, Work,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    Denomination, JUDGEMENT_SHAPE, Judgement, NodeRef, ResultBody, SafeInteger, Statement,
};
use rutent_contracts::router_api::{
    CatalogueSnapshot, DecisionRecord, Distinct, DropReason, Endpoint, FigureValue, Id, Operation,
    OperationKind, PolicyBounds, PositiveAmount, Refines, Request, Round, Router, Timestamp,
    TupleKey, Violation,
};
use rutent_prov::catalogue::{Catalogue, CheckRefusal, Publisher, Verdict};
use rutent_router::shortlist::{self, Wanted};

use crate::snapshot::{
    Ask, Asked, Clock, Exposures, FormError, Formed, OrchestrationLog, Price, Received,
    RecordError, Releases, Snapshots, record_refused,
};

pub use ask::Unsigned;
pub use journal::Unappended;
pub use ports::{Fresh, OperatorKey, Providers};

use ask::Question;

/// How many choice rounds may follow the first, each asking again at the amounts the one before left unconfirmed.
pub const LATER_ROUNDS: usize = 3;

/// Whether a user is present to be asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// The user is: the reserve's yes is asked in the user's session, which the catalogue holds (`REQ-CAT-3`).
    User {
        /// The session.
        session: Id,
    },
    /// An automatic deposit started the order and no user is present, so no yes is needed (`REQ-ORD-32`). A
    /// withdrawal's dry run still goes as a check, in the session Core opened for the deposit's account.
    AutomaticDeposit {
        /// The session.
        session: Id,
    },
}

impl Presence {
    fn session(&self) -> &Id {
        match self {
            Presence::User { session } | Presence::AutomaticDeposit { session } => session,
        }
    }
}

/// A judgement a tuple's provider needs before it may be on the order's route: of which verifier, about which
/// person and class, under the relying provider's subject reference (`REQ-CPL-6`, ADR-0012). Asked only when the
/// tuple is shortlisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    /// The relying provider's tuple.
    pub tuple: TupleKey,
    /// The person, by reference, as policy is asked about them.
    pub person: Id,
    /// The verifier the provider's rules name.
    pub verifier: Id,
    /// The person's subject reference, the relying provider's.
    pub subject: String,
    /// The class.
    pub class: String,
}

/// The order a routing is for, as intake gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// The order.
    pub order: Id,
    /// Whose work the routing is: its fanned-out questions are charged to this actor's accountable party.
    pub caller: Actor,
    /// The reserve the order's value leaves from, where the holder's account is.
    pub origin_reserve: Id,
    /// Whether a user is present.
    pub presence: Presence,
    /// The inputs the order carries for tuples that require them, by name; a question carries only those its tuple
    /// requires.
    pub inputs: BTreeMap<String, String>,
    /// The judgements tuples need.
    pub needs: Vec<Need>,
}

/// What Core keeps of an order across its routings: the receipt of its last decision record entry, which the next
/// chains to, and the verifiers asked about a person for it (`REQ-AUD-4`, `REQ-CPL-5`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    previous: Option<Receipt>,
    asked_about_person: Vec<Id>,
}

impl History {
    /// An order's history before its first routing.
    pub fn new() -> Self {
        Self::default()
    }

    /// An order's history as the log recorded it: the receipt of its last `decision_record` entry, and the
    /// verifiers asked about a person for it, each once. For a caller that keeps the history apart from the rounds,
    /// across a restart (`F-030`) or a routing recorded elsewhere, so the next routing chains to it (`REQ-AUD-4`,
    /// `T-020-10`).
    pub fn recorded(previous: Receipt, asked_about_person: Vec<Id>) -> Self {
        let mut asked: Vec<Id> = Vec::new();
        for verifier in asked_about_person {
            if !asked.contains(&verifier) {
                asked.push(verifier);
            }
        }
        History {
            previous: Some(previous),
            asked_about_person: asked,
        }
    }

    /// The receipt of the order's last decision record entry.
    pub fn previous(&self) -> Option<&Receipt> {
        self.previous.as_ref()
    }

    /// The verifiers already asked about a person for the order, each once.
    pub fn asked_about_person(&self) -> &[Id] {
        &self.asked_about_person
    }
}

/// Every transaction Core has asked a question under, across every order: a fresh one is never one of these.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Issued {
    used: BTreeSet<String>,
}

impl Issued {
    /// None issued yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether Core asked under `transaction`.
    pub fn contains(&self, transaction: &str) -> bool {
        self.used.contains(transaction)
    }
}

/// What the rounds reach. The assembly builds it; Core's catalogue, clock, exposures and releases are those
/// [`Snapshots`] is built from.
pub struct Parts<'a> {
    /// The catalogue: snapshots read it, and balance checks go through it.
    pub catalogue: &'a mut Catalogue,
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Core's record of value in flight.
    pub exposures: &'a Exposures,
    /// The releases accepted.
    pub releases: &'a Releases,
    /// Orchestration's handle on the log.
    pub log: &'a mut dyn OrchestrationLog,
    /// The evidence store.
    pub evidence: &'a mut dyn Evidence,
    /// Policy's charges.
    pub charges: &'a mut dyn Charges,
    /// Policy's questions about people.
    pub persons: &'a mut dyn PersonQuestions,
    /// The providers' modules.
    pub providers: &'a mut dyn Providers,
    /// The operator key.
    pub operator_key: &'a mut dyn OperatorKey,
    /// Fresh identifiers.
    pub fresh: &'a mut dyn Fresh,
    /// The transactions Core asked under.
    pub issued: &'a mut Issued,
}

/// One routing to run.
#[derive(Debug, Clone, Copy)]
pub struct Routing<'a> {
    /// What the customer asks for. Its `request_id` and `round` are Core's to set for each round.
    pub request: &'a Request,
    /// The bounds the router runs under.
    pub bounds: &'a PolicyBounds,
    /// The prices Core holds.
    pub prices: &'a [Price],
    /// The order.
    pub order: &'a Order,
    /// Core, acting on its own account: who writes each `decision_record` entry.
    pub core: &'a Actor,
    /// The operator, in whose name Core signs its questions and records: its `acts` is their `maker`, the one the
    /// log's `StatementKeys::operator` names, and its `party` the operator's.
    pub operator: &'a Actor,
}

/// One round's recorded decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The router's record, as it returned it.
    pub record: DecisionRecord,
    /// The record in its envelope, signed with the operator key, as recorded.
    pub recorded: Vec<u8>,
    /// Its digest, which its entry names.
    pub digest: Digest,
    /// The receipt of its `decision_record` entry.
    pub receipt: Receipt,
}

/// One question Core asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// The provider or verifier asked.
    pub provider: Id,
    /// The transaction it was asked under; a judgement's request carries none, so it has none, and its stated
    /// entry is about nothing (a residual, `T-025-05`).
    pub transaction: Option<Id>,
    /// The tuple and amount it was about; a judgement's names the relying tuple and no amount.
    pub tuple: TupleKey,
    /// The amount, for a provider's question.
    pub amount: Option<PositiveAmount>,
    /// After which choice round it was asked: 0 after the shortlist.
    pub batch: usize,
    /// The receipt of its stated entry, appended before it was sent.
    pub receipt: Receipt,
}

/// Why no route was offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotOffered {
    /// The last round chose nothing.
    NoRoute,
    /// The caller's accountable party is at its bound: the questions weren't sent (`REQ-OPS-1`).
    OverBound {
        /// The bound.
        bound: SafeInteger,
    },
    /// The set asked about a person for the order would pass its bound: the order has no route left
    /// (`REQ-CPL-5`).
    PersonQuestionsPastBound,
    /// A part's withdrawal has no yes from the order's origin reserve, asked in the user's session (`REQ-ORD-32`).
    NoReservesYes,
    /// An operation of the choice whose tuple needs a judgement cites none from the verifier the need names, about
    /// its subject and class (`REQ-CPL-6`, `REQ-AUZ-5`).
    NotEligible {
        /// The tuple.
        tuple: TupleKey,
    },
}

/// What a routing offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// A route: the last choice round's chosen candidate.
    Offered {
        /// The candidate.
        candidate: Id,
        /// The receipt of each part's reserve's yes, in part order: what an acceptance cites; empty on an
        /// automatic deposit.
        reserve_yes: Vec<Receipt>,
        /// The user's session the yes was asked in; `None` on an automatic deposit.
        session: Option<Id>,
    },
    /// None, and why.
    NotOffered(NotOffered),
}

/// One routing's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    /// The shortlist round.
    pub shortlist: Recorded,
    /// Each choice round, in order; the last decides the offer.
    pub choices: Vec<Recorded>,
    /// What is offered.
    pub offer: Offer,
    /// Every question Core asked, in order.
    pub sent: Vec<Sent>,
    /// The balance checks the catalogue refused to let out, by reserve: those questions weren't sent.
    pub refused_checks: Vec<(Id, CheckRefusal)>,
}

/// Why a routing stopped. Nothing after the failing step was asked or recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingError {
    /// No snapshot was formed.
    Form(FormError),
    /// The router returned a record that fails its contract.
    Router(Violation),
    /// The router returned a record of another request, snapshot or bounds than the round gave it.
    NotItsRecord,
    /// The bounds with the other reserves' withdrawals excluded fail their contract or their check against the
    /// snapshot.
    Bounds(Violation),
    /// The log, the evidence store or policy refused or failed.
    Log(AppendFailure),
    /// The operator key didn't sign.
    Unsigned,
    /// Core's clock reads no time a timestamp can hold.
    Clock,
    /// A fresh identifier was one Core used before, or names the order.
    Transaction,
    /// A batch's questions are more than a charge can count.
    Uncountable,
}

impl From<AppendFailure> for RoutingError {
    fn from(failure: AppendFailure) -> Self {
        RoutingError::Log(failure)
    }
}

impl From<Unsigned> for RoutingError {
    fn from(_: Unsigned) -> Self {
        RoutingError::Unsigned
    }
}

/// An answer Core received.
struct Answer {
    provider: Id,
    tuple: TupleKey,
    asked: Asked,
    transaction: Option<Id>,
    bytes: Vec<u8>,
    /// Whether it answers Core's own question: a provider's names the transaction Core asked it under; a
    /// judgement is made by the verifier asked, about the subject and class asked.
    ours: bool,
    /// A withdrawal's dry run at the origin reserve the catalogue read as a yes, asked in the user's session.
    yes: bool,
    receipt: Option<Receipt>,
    /// Whether a snapshot's refusal of it was recorded: each refusal is recorded once.
    refusal_recorded: bool,
}

/// One routing's state as it runs.
struct Run<'r, 'p> {
    parts: &'r mut Parts<'p>,
    routing: Routing<'r>,
    /// The bounds every round runs under: the caller's, with the other reserves' withdrawals excluded, once the
    /// shortlist's snapshot shows them.
    bounds: Option<PolicyBounds>,
    answers: Vec<Answer>,
    asked: Vec<(TupleKey, String)>,
    sent: Vec<Sent>,
    refused_checks: Vec<(Id, CheckRefusal)>,
}

/// Runs one routing of one order (see the module's docs), with router `R`.
pub fn route<R: Router>(
    parts: &mut Parts<'_>,
    routing: Routing<'_>,
    history: &mut History,
) -> Result<Routed, RoutingError> {
    Rounds::<R>(PhantomData).run(parts, routing, history)
}

struct Rounds<R>(PhantomData<R>);

impl<R: Router> Rounds<R> {
    fn run(
        &self,
        parts: &mut Parts<'_>,
        routing: Routing<'_>,
        history: &mut History,
    ) -> Result<Routed, RoutingError> {
        let mut run = Run {
            parts,
            routing,
            bounds: None,
            answers: Vec::new(),
            asked: Vec::new(),
            sent: Vec::new(),
            refused_checks: Vec::new(),
        };
        let mut request = routing.request.clone();
        request.request_id = run.parts.fresh.id();
        request.round = Round::Shortlist {};
        let shortlist = self.round(&mut run, &request, RoutingRound::Shortlist, history)?;
        let mut choices = Vec::new();
        let finish = |run: Run<'_, '_>, shortlist, choices, offer| Routed {
            shortlist,
            choices,
            offer,
            sent: run.sent,
            refused_checks: run.refused_checks,
        };
        let Ok(asks) = shortlist::asks(&shortlist.record) else {
            return Ok(finish(
                run,
                shortlist,
                choices,
                Offer::NotOffered(NotOffered::NoRoute),
            ));
        };
        let shortlisted: Vec<Id> = asks.providers().cloned().collect();
        let verifiers = verifiers_of(&shortlist.record, &routing.order.needs);
        let questions: Vec<Question> = asks
            .providers()
            .flat_map(|p| {
                asks.of(p).iter().map(|q| Question {
                    tuple: TupleKey {
                        provider: p.clone(),
                        from: q.from.clone(),
                        to: q.to.clone(),
                    },
                    kind: q.kind,
                    amount_in: q.amount_in.clone(),
                })
            })
            .collect();
        let tuples: Vec<TupleKey> = questions.iter().map(|q| q.tuple.clone()).collect();
        let needs: Vec<&Need> = routing
            .order
            .needs
            .iter()
            .filter(|n| tuples.contains(&n.tuple))
            .collect();

        if let Some(bound) = run.charge(questions.len() + needs.len())? {
            return Ok(finish(
                run,
                shortlist,
                choices,
                Offer::NotOffered(NotOffered::OverBound { bound }),
            ));
        }
        for need in &needs {
            if !run.ask_about_person(need, &shortlist.digest, &shortlisted, &verifiers, history)? {
                return Ok(finish(
                    run,
                    shortlist,
                    choices,
                    Offer::NotOffered(NotOffered::PersonQuestionsPastBound),
                ));
            }
        }
        for question in &questions {
            run.ask(question, &shortlist.record, 0)?;
        }

        let refines = Refines {
            request_id: shortlist.record.request.request_id.clone(),
            taken_at: shortlist.record.snapshot.taken_at.clone(),
        };
        let mut over = None;
        for batch in 1..=LATER_ROUNDS + 1 {
            let mut choice = request.clone();
            choice.request_id = run.parts.fresh.id();
            choice.round = Round::Choice {
                refines: refines.clone(),
            };
            let recorded = self.round(&mut run, &choice, RoutingRound::Choice, history)?;
            let chosen = recorded.record.chosen.is_some();
            let again = unconfirmed(&recorded.record, &tuples, &run.asked);
            choices.push(recorded);
            if chosen || batch > LATER_ROUNDS || again.is_empty() {
                break;
            }
            if let Some(bound) = run.charge(again.len())? {
                over = Some(bound);
                break;
            }
            let last = &choices.last().expect("a choice round").record;
            for question in &again {
                run.ask(question, last, batch)?;
            }
        }
        let offer = match over {
            Some(bound) => Offer::NotOffered(NotOffered::OverBound { bound }),
            None => run.offer(&choices.last().expect("a choice round").record, &needs),
        };
        Ok(finish(run, shortlist, choices, offer))
    }

    /// One round: its snapshot formed, its refused statements recorded and its answers appended, then `route`; the
    /// record checked to be of what the round gave the router, then signed and appended, chained to the order's
    /// previous one.
    fn round(
        &self,
        run: &mut Run<'_, '_>,
        request: &Request,
        round: RoutingRound,
        history: &mut History,
    ) -> Result<Recorded, RoutingError> {
        let statements: Vec<Received> = match round {
            RoutingRound::Shortlist => Vec::new(),
            RoutingRound::Choice => run
                .answers
                .iter()
                .filter(|a| a.ours)
                .map(|a| Received {
                    tuple: a.tuple.clone(),
                    asked: a.asked.clone(),
                    bytes: a.bytes.clone(),
                })
                .collect(),
        };
        let snapshot_id = run.parts.fresh.id();
        let formed = Snapshots::new(
            run.parts.catalogue,
            run.parts.clock,
            run.parts.exposures,
            run.parts.releases,
        )
        .form(&Ask {
            snapshot_id: snapshot_id.as_str(),
            bounds: run.bounds.as_ref().unwrap_or(run.routing.bounds),
            prices: run.routing.prices,
            statements: &statements,
        })
        .map_err(RoutingError::Form)?;
        if run.bounds.is_none() {
            let bounds = origin_only(
                run.routing.bounds,
                &formed.snapshot,
                &run.routing.order.origin_reserve,
            )?;
            run.bounds = Some(bounds);
        }
        let bounds = run.bounds.clone().expect("the routing's bounds");
        run.record_answers(&formed)?;

        let record = R::route(request, &formed.snapshot, &bounds);
        record.check().map_err(RoutingError::Router)?;
        if &record.request != request
            || record.snapshot != formed.snapshot
            || record.bounds != bounds
        {
            return Err(RoutingError::NotItsRecord);
        }
        let maker = &run.routing.operator.acts;
        let made_at = run.now()?;
        let recorded = ask::sign_record(run.parts.operator_key, &record, maker, &made_at)?;
        let digest = ask::digest(&recorded).ok_or(RoutingError::Unsigned)?;
        let (shortlisted, verifiers) = match round {
            RoutingRound::Shortlist => (
                Some(providers_of(&record)),
                verifiers_of(&record, &run.routing.order.needs),
            ),
            RoutingRound::Choice => (None, Vec::new()),
        };
        let receipt = journal::append_record(
            run.parts.log,
            run.parts.clock,
            journal::RecordEntry {
                who: run.routing.core,
                order: &run.routing.order.order,
                record_id: record.record_id(),
                round,
                digest: &digest,
                previous: history.previous.as_ref(),
                shortlisted,
                verifiers,
            },
        )?;
        history.previous = Some(receipt.clone());
        Ok(Recorded {
            record,
            recorded,
            digest,
            receipt,
        })
    }
}

/// The caller's bounds with every withdrawal tuple of the snapshot excluded but the origin reserve's: the yes an
/// order is routed on is its origin reserve's (`REQ-ORD-32`), and no other reserve is asked about the holder's funds.
fn origin_only(
    bounds: &PolicyBounds,
    snapshot: &CatalogueSnapshot,
    origin: &Id,
) -> Result<PolicyBounds, RoutingError> {
    let mut excluded: Vec<TupleKey> = bounds.excluded_tuples.to_vec();
    for figure in &snapshot.figures {
        let FigureValue::ReserveTuple { tuple } = &figure.value else {
            continue;
        };
        let Some(provider) = figure.origin.provider() else {
            continue;
        };
        if provider == origin || !matches!(tuple.origin, NodeRef::Outer(_)) {
            continue;
        }
        let unit = |decimals| Denomination {
            id: tuple.asset.clone(),
            decimals,
        };
        let key = TupleKey {
            provider: provider.clone(),
            from: Endpoint {
                node: tuple.origin.clone(),
                asset: unit(tuple.decimals.origin),
            },
            to: Endpoint {
                node: tuple.destination.clone(),
                asset: unit(tuple.decimals.destination),
            },
        };
        if !excluded.contains(&key) {
            excluded.push(key);
        }
    }
    let mut bounds = bounds.clone();
    bounds.excluded_tuples = Distinct::new(excluded).ok_or(RoutingError::Bounds(Violation(
        "excluded tuples are distinct",
    )))?;
    bounds.check().map_err(RoutingError::Bounds)?;
    bounds
        .check_against(snapshot)
        .map_err(RoutingError::Bounds)?;
    Ok(bounds)
}

impl Run<'_, '_> {
    fn now(&self) -> Result<Timestamp, RoutingError> {
        self.parts
            .clock
            .now()
            .timestamp()
            .ok_or(RoutingError::Clock)
    }

    /// A transaction never asked under, naming nothing of the order.
    fn transaction(&mut self) -> Result<Id, RoutingError> {
        let id = self.parts.fresh.id();
        if self.parts.issued.contains(id.as_str())
            || id.as_str().contains(self.routing.order.order.as_str())
        {
            return Err(RoutingError::Transaction);
        }
        self.parts.issued.used.insert(id.as_str().to_owned());
        Ok(id)
    }

    /// Charges `count` questions to the caller's accountable party; `Some(bound)` where it is over its bound. A
    /// count a charge can't hold is refused, never taken as within.
    fn charge(&mut self, count: usize) -> Result<Option<SafeInteger>, RoutingError> {
        let count = u64::try_from(count)
            .ok()
            .and_then(Count::new)
            .ok_or(RoutingError::Uncountable)?;
        let caller = &self.routing.order.caller;
        let charge = Charge {
            request: self.parts.fresh.id(),
            party: caller.party.clone(),
            caller: caller.clone(),
            work: Work::FanOut,
            count,
            order: Some(self.routing.order.order.clone()),
            at: self.now()?,
        };
        Ok(match self.parts.charges.charge(&charge)?.outcome {
            ChargeOutcome::Within { .. } => None,
            ChargeOutcome::Over { bound } => Some(bound),
        })
    }

    /// Puts a need to policy as a question about a person on the shortlisted route and, where allowed, asks its
    /// verifier. False where the set asked would pass its bound.
    fn ask_about_person(
        &mut self,
        need: &Need,
        shortlist: &Digest,
        shortlisted: &[Id],
        verifiers: &[Id],
        history: &mut History,
    ) -> Result<bool, RoutingError> {
        let order = self.routing.order;
        // The verifier receives the subject, so it is the one asked, recorded and bounded (`REQ-CPL-5`).
        let verifier = need.verifier.clone();
        let question = PersonQuestion {
            request: self.parts.fresh.id(),
            asker: order.caller.clone(),
            person: need.person.clone(),
            order: order.order.clone(),
            authority: Authority::ShortlistedRoute {
                shortlist: shortlist.clone(),
                providers: shortlisted.to_vec(),
                verifiers: verifiers.to_vec(),
            },
            ask: vec![verifier.clone()],
            asked_before: history.asked_about_person.clone(),
            at: self.now()?,
        };
        match self.parts.persons.ask_about_person(&question)?.outcome {
            PersonOutcome::Allowed {} => {}
            PersonOutcome::PastBound { .. } => return Ok(false),
            PersonOutcome::Refused { .. } | PersonOutcome::NotKnown { .. } => return Ok(true),
        }
        if !history.asked_about_person.contains(&verifier) {
            history.asked_about_person.push(verifier);
        }
        // A judgement request carries no transaction, so the question is logged about nothing its body doesn't
        // name, and the verifier's answer is bound by the need it answers alone (a residual, `T-025-05`).
        let operation = ask::judgement(&need.verifier, &need.subject, &need.class);
        let made_at = self.now()?;
        let bytes = ask::sign_question(
            self.parts.operator_key,
            self.bounds.as_ref().unwrap_or(self.routing.bounds),
            &self.routing.operator.acts,
            &made_at,
            operation,
        )?;
        let receipt = self.append_question(&bytes, None)?;
        self.sent.push(Sent {
            provider: need.verifier.clone(),
            transaction: None,
            tuple: need.tuple.clone(),
            amount: None,
            batch: 0,
            receipt,
        });
        if let Some(answer) = self.parts.providers.ask(&need.verifier, &bytes) {
            let ours = serde_json::from_slice::<Statement<Judgement>>(&answer)
                .ok()
                .is_some_and(|j| {
                    j.maker == need.verifier.as_str()
                        && j.statement.subject == need.subject
                        && j.statement.class == need.class
                });
            self.answers.push(Answer {
                provider: need.verifier.clone(),
                tuple: need.tuple.clone(),
                asked: Asked::Eligibility,
                transaction: None,
                bytes: answer,
                ours,
                yes: false,
                receipt: None,
                refusal_recorded: false,
            });
        }
        Ok(true)
    }

    /// Asks one provider one question, under a fresh transaction, logged before it is sent; a withdrawal only at the
    /// order's origin reserve, as a balance check the catalogue lets out.
    fn ask(
        &mut self,
        question: &Question,
        record: &DecisionRecord,
        batch: usize,
    ) -> Result<(), RoutingError> {
        let order = self.routing.order;
        let provider = question.tuple.provider.clone();
        let withdrawal = question.kind == OperationKind::Withdrawal;
        if withdrawal && provider != order.origin_reserve {
            return Ok(());
        }
        self.asked.push((
            question.tuple.clone(),
            question.amount_in.amount().as_str().to_owned(),
        ));
        let transaction = self.transaction()?;
        let inputs = ask::inputs_for(&record.snapshot, &question.tuple, &order.inputs);
        let session = withdrawal.then(|| order.presence.session());
        let Some(operation) = ask::operation(question, &transaction, session, inputs) else {
            return Ok(());
        };
        let ticket = if withdrawal {
            // The holder's own order: a withdrawal at its origin reserve, in its session.
            let own = (provider == order.origin_reserve).then_some(order.order.as_str());
            match self
                .parts
                .catalogue
                .begin_check(&operation, own, self.parts.clock.now().floor())
            {
                Ok(ticket) => Some(ticket),
                Err(refusal) => {
                    self.refused_checks.push((provider, refusal));
                    return Ok(());
                }
            }
        } else {
            None
        };
        let made_at = self.now()?;
        let bytes = ask::sign_question(
            self.parts.operator_key,
            self.bounds.as_ref().unwrap_or(self.routing.bounds),
            &self.routing.operator.acts,
            &made_at,
            operation,
        )?;
        let about = About {
            order: Some(order.order.clone()),
            segment: None,
            transaction: Some(transaction.clone()),
        };
        let receipt = self.append_question(&bytes, Some(about))?;
        self.sent.push(Sent {
            provider: provider.clone(),
            transaction: Some(transaction.clone()),
            tuple: question.tuple.clone(),
            amount: Some(question.amount_in.clone()),
            batch,
            receipt,
        });
        let Some(answer) = self.parts.providers.ask(&provider, &bytes) else {
            return Ok(());
        };
        let ours = ask::transaction_of(&answer).as_deref() == Some(transaction.as_str());
        let mut yes = false;
        if let Some(ticket) = ticket {
            let verdict = serde_json::from_slice::<Statement<ResultBody>>(&answer)
                .ok()
                .map(|result| {
                    self.parts.catalogue.answer_check(
                        &ticket,
                        &result.statement,
                        self.parts.clock.now().floor(),
                    )
                });
            yes = ours
                && verdict == Some(Verdict::Yes)
                && matches!(order.presence, Presence::User { .. });
        }
        let asked = match Wanted::for_kind(question.kind) {
            Wanted::FreshQuote if accepted(&answer) => Asked::FreshQuote,
            _ => Asked::DryRun {
                amount_in: question.amount_in.clone(),
            },
        };
        self.answers.push(Answer {
            provider,
            tuple: question.tuple.clone(),
            asked,
            transaction: Some(transaction),
            bytes: answer,
            ours,
            yes,
            receipt: None,
            refusal_recorded: false,
        });
        Ok(())
    }

    fn append_question(
        &mut self,
        bytes: &[u8],
        about: Option<About>,
    ) -> Result<Receipt, RoutingError> {
        journal::append_stated(
            self.parts.log,
            self.parts.evidence,
            self.parts.clock,
            bytes,
            self.routing.operator.clone(),
            about,
        )
        .map_err(|e| match e {
            Unappended::Append(failure) => RoutingError::Log(failure),
            Unappended::Unrecordable => RoutingError::Unsigned,
        })
    }

    /// Before a round's record is formed: every statement its snapshot refused, recorded once through
    /// [`record_refused`], and every other answer not yet appended, appended with its transaction in its `about`.
    fn record_answers(&mut self, formed: &Formed) -> Result<(), RoutingError> {
        let order = &self.routing.order.order;
        for refused in &formed.refused {
            let answer = self.answers.iter_mut().find(|a| a.bytes == refused.bytes);
            if answer.as_ref().is_some_and(|a| a.refusal_recorded) {
                continue;
            }
            let about = answer.as_ref().and_then(|a| about_answer(order, a));
            match record_refused(
                self.parts.evidence,
                self.parts.log,
                self.parts.clock,
                self.parts.catalogue,
                refused,
                about,
            ) {
                Ok(receipt) => {
                    if let Some(answer) = answer {
                        answer.receipt.get_or_insert(receipt);
                        answer.refusal_recorded = true;
                    }
                }
                Err(RecordError::Unrecordable) => {}
                Err(RecordError::Append(failure)) => return Err(RoutingError::Log(failure)),
            }
        }
        for i in 0..self.answers.len() {
            if self.answers[i].receipt.is_some() {
                continue;
            }
            let answer = &self.answers[i];
            let maker = maker_of(self.parts.catalogue, answer);
            let about = about_answer(order, answer);
            match journal::append_stated(
                self.parts.log,
                self.parts.evidence,
                self.parts.clock,
                &answer.bytes,
                maker,
                about,
            ) {
                Ok(receipt) => self.answers[i].receipt = Some(receipt),
                Err(Unappended::Unrecordable) => {}
                Err(Unappended::Append(failure)) => return Err(RoutingError::Log(failure)),
            }
        }
        Ok(())
    }

    /// What the last choice round offers: its chosen candidate, only on its origin reserve's yes and with every
    /// judgement its tuples need.
    fn offer(&self, record: &DecisionRecord, needs: &[&Need]) -> Offer {
        let Some(chosen) = record.chosen() else {
            return Offer::NotOffered(NotOffered::NoRoute);
        };
        let order = self.routing.order;
        let mut reserve_yes = Vec::new();
        for part in chosen.parts.iter() {
            let first = part.operations.first();
            // A recalculated part's value is already in the book: one that starts with no withdrawal has no yes to
            // ask (`T-020-10`).
            if self.routing.request.follows.is_some() && first.kind != OperationKind::Withdrawal {
                continue;
            }
            let yes = self.answers.iter().find(|a| {
                a.yes
                    && a.provider == first.provider
                    && a.tuple.from == first.from
                    && a.tuple.to == first.to
                    && a.asked
                        == Asked::DryRun {
                            amount_in: first.amount_in.clone(),
                        }
            });
            match (yes.and_then(|a| a.receipt.clone()), &order.presence) {
                (Some(receipt), Presence::User { .. })
                    if first.provider == order.origin_reserve =>
                {
                    reserve_yes.push(receipt);
                }
                (_, Presence::User { .. }) => {
                    return Offer::NotOffered(NotOffered::NoReservesYes);
                }
                (_, Presence::AutomaticDeposit { .. }) => {}
            }
        }
        for operation in chosen.parts.iter().flat_map(|p| p.operations.iter()) {
            let key = TupleKey {
                provider: operation.provider.clone(),
                from: operation.from.clone(),
                to: operation.to.clone(),
            };
            for need in needs.iter().filter(|n| n.tuple == key) {
                if !judged(record, operation, need) {
                    return Offer::NotOffered(NotOffered::NotEligible { tuple: key });
                }
            }
        }
        Offer::Offered {
            candidate: chosen.candidate_id.clone(),
            reserve_yes,
            session: match &order.presence {
                Presence::User { session } => Some(session.clone()),
                Presence::AutomaticDeposit { .. } => None,
            },
        }
    }
}

/// Whether `operation` cites a judgement of its tuple by the verifier `need` names, about its subject and class.
fn judged(record: &DecisionRecord, operation: &Operation, need: &Need) -> bool {
    let tuple = operation.figures.first();
    operation.figures.iter().any(|id| {
        record.snapshot.figures.iter().any(|f| {
            &f.figure_id == id
                && matches!(&f.value, FigureValue::Eligibility { tuple: t, judgement }
                    if t == tuple
                        && judgement.maker == need.verifier.as_str()
                        && judgement.statement.subject == need.subject
                        && judgement.statement.class == need.class)
        })
    })
}

/// What an answer is about: its order and the transaction Core asked it under; a judgement, which names no
/// transaction, about nothing (core-internal's README, "Two kinds of entry").
fn about_answer(order: &Id, answer: &Answer) -> Option<About> {
    answer.transaction.as_ref().map(|transaction| About {
        order: Some(order.clone()),
        segment: None,
        transaction: Some(transaction.clone()),
    })
}

/// Who an answer claims made it: its provider, acting for its publisher, or for the operator where the operator
/// publishes it; a judgement in the verifier's role.
fn maker_of(catalogue: &Catalogue, answer: &Answer) -> Actor {
    let party = match catalogue.publishers().publisher(answer.provider.as_str()) {
        Some(Publisher::Operator) => Id::new("operator"),
        Some(Publisher::Party(publisher)) => Id::new(publisher),
        None => None,
    };
    let judgement = serde_json::from_slice::<serde_json::Value>(&answer.bytes)
        .ok()
        .is_some_and(|v| v.get("shape").and_then(|s| s.as_str()) == Some(JUDGEMENT_SHAPE));
    Actor {
        party: party.unwrap_or_else(|| answer.provider.clone()),
        acts: answer.provider.clone(),
        role: if judgement {
            Role::Verifier
        } else {
            Role::Provider
        },
        kind: ActorKind::Component,
    }
}

/// Whether an answer is an accepted dry run: a quote, where a fresh quote was asked for. Anything else is read as a
/// dry run's answer, a refusal among them, as the router's shortlist admits it.
fn accepted(answer: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(answer)
        .ok()
        .and_then(|v| {
            v.get("statement")?
                .get("outcome")?
                .get("status")?
                .as_str()
                .map(|s| s == "dry_run_accepted")
        })
        .unwrap_or(false)
}

/// The verifiers the tuples of a shortlist round's kept candidates need, by the order's needs, each once.
fn verifiers_of(record: &DecisionRecord, needs: &[Need]) -> Vec<Id> {
    let mut verifiers: Vec<Id> = Vec::new();
    for candidate in record.shortlist() {
        for operation in candidate.parts.iter().flat_map(|p| p.operations.iter()) {
            for need in needs.iter().filter(|n| {
                n.tuple.provider == operation.provider
                    && n.tuple.from == operation.from
                    && n.tuple.to == operation.to
            }) {
                if !verifiers.contains(&need.verifier) {
                    verifiers.push(need.verifier.clone());
                }
            }
        }
    }
    verifiers
}

/// The providers a shortlist round shortlisted, each once, in the order the record names them.
fn providers_of(record: &DecisionRecord) -> Vec<Id> {
    let mut providers: Vec<Id> = Vec::new();
    for candidate in record.shortlist() {
        for operation in candidate.parts.iter().flat_map(|p| p.operations.iter()) {
            if !providers.contains(&operation.provider) {
                providers.push(operation.provider.clone());
            }
        }
    }
    providers
}

/// A choice round's operations to ask again: each of a candidate it dropped as unconfirmed or for a voided quote,
/// at a tuple the shortlist asked about, at an amount that tuple wasn't asked at.
fn unconfirmed(
    record: &DecisionRecord,
    shortlisted: &[TupleKey],
    asked: &[(TupleKey, String)],
) -> Vec<Question> {
    let mut again: Vec<Question> = Vec::new();
    for candidate in record.candidates() {
        if !matches!(
            candidate.dropped,
            Some(DropReason::Unconfirmed { .. } | DropReason::QuoteVoided { .. })
        ) {
            continue;
        }
        for operation in candidate.parts.iter().flat_map(|p| p.operations.iter()) {
            let key = TupleKey {
                provider: operation.provider.clone(),
                from: operation.from.clone(),
                to: operation.to.clone(),
            };
            let amount = operation.amount_in.amount().as_str();
            if !shortlisted.contains(&key)
                || asked.iter().any(|(t, a)| t == &key && a == amount)
                || again
                    .iter()
                    .any(|q| q.tuple == key && q.amount_in == operation.amount_in)
            {
                continue;
            }
            again.push(Question {
                tuple: key,
                kind: operation.kind,
                amount_in: operation.amount_in.clone(),
            });
        }
    }
    again
}
