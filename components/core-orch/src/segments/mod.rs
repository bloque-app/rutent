//! Handing segments to the platform and taking their results: each segment begun and ended on the platform, its calls
//! named, and the platform's events, the proxy's uses and each segment record handed to the log.
//!
//! Declared by `T-020-01`; `T-020-08` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! [`Segments::run_part`] runs one part of an order's chosen route, segment after segment:
//!
//! 1. **The route guard first.** The part is checked by `T-020-05`'s [`check_part`] before any segment of it is
//!    handed over; a refusal is recorded through [`record_route_refusal`] as `no_route_right_now`, and nothing is
//!    begun (`REQ-ORD-16`, `REQ-ORD-36`, `REQ-ORD-37`).
//! 2. **A decision, then its effect.** Each segment's operation is decided by policy (`Decisions::decide`, a
//!    `move_value`), and only a permit goes on: its `effect` entry is appended, and its receipt held, before
//!    anything is handed over, so a failure between the record and the effect leaves no effect (`REQ-AUZ-1`,
//!    `INV-7`). Each effect is acted on once for its receipt, however often its append was retried, since the log
//!    admits an identical effect again; a segment is handed over once (`INV-8`).
//! 3. **Held in flight.** The segment's value is held in [`Exposures`] from just before its effect until the
//!    segment is accounted for, so the ceilings in flight count what is live (`REQ-ORD-8`). A settled segment and
//!    a cancelled withdrawal release it; a failed segment, one of unknown outcome and one with no result counted
//!    keep it, for the reversal or the stuck order's resolution to release (`T-020-11`, `T-020-12`).
//! 4. **On the platform.** The segment begins on the platform under a fresh transaction Core assigns, and its calls
//!    are named in the platform's routes from the chosen record (`RouteCalls::name_segment`), so a cross-module
//!    call runs only where the live segment names it (`REQ-MDL-21`, `REQ-MDL-9`). The operation, signed with the
//!    operator key, is appended to the log about its order, segment and transaction before it is handed over, so the
//!    answer binds to it (`REQ-ORD-32`, `REQ-ORD-38`).
//! 5. **Everything handed to the log.** After each operation the platform's events and the proxy's uses are drained
//!    and appended, and when the segment ends its record is too (`REQ-MDL-8`, `REQ-MDL-6`, `REQ-MDL-5`). An append
//!    that fails keeps them, to append again whole, never drained twice.
//! 6. **A result counts only if it is the segment's.** The result's bytes are kept and appended as the provider's
//!    stated entry, and it counts only when attributable to the segment's provider, answering its transaction, with
//!    amounts that hold (`judge`, `REQ-ORD-1`, `INV-1`). One that doesn't count is no result: no outcome is recorded
//!    on it, and the segment waits.
//! 7. **One final outcome per segment.** A result that counts gives the effect's `result` entry and the segment's
//!    input: settled, on the evidence `T-020-09` grades ([`Grading`]); failed; or outcome unknown, only on the
//!    provider's own word or the platform's record that the module passed something out (`REQ-ORD-1`). A
//!    withdrawal that fails cancels the part with nothing moved, and no later segment is handed over
//!    (`REQ-ORD-40`). A provider waiting on the person is the provider's statement, labelled so (`REQ-ORD-26`).
//! 8. **What the next segment moves** is what the one before gave out by its result that counted, never the quote, so
//!    a shortfall arriving at a node is the earlier segment's and not the next provider's failure (`REQ-ORD-21`).
//!    Each settled segment's actual time is recorded beside the one its price declared (`REQ-QTE-3`).
//!
//! An order's state and its age are readable by whoever policy lets read it ([`Segments::status`], `REQ-ORD-5`).
//!
//! **A part stuck** (`T-020-11`'s seams, additive): [`Segments::observe`] takes a part's `no_route` and
//! `stuck_declared` too, so a stuck or waiting part reads so; [`Segments::run_part`] refuses a stuck part whole
//! ([`SegmentsError::Stuck`]), so nothing of it is handed over or carried out again while its resolution may move
//! its value, though what a segment carried out already gave back is still recorded, and a value it settled at an
//! inner node while the part is stuck is held there at rest; [`Segments::act_on`] holds a
//! resolution's effect to the one register of effects acted on; [`Segments::follow_recorded`] rebuilds an order's
//! state from the log; and [`Segments::stuck`], [`Segments::carried_since`] and [`Segments::settlements`] read what
//! the stuck clock runs on.
//!
//! **A part recalculated** (`T-020-10`'s seams, the one change to this module outside its owning row): a record whose
//! request `follows` another is a recalculation, so its part is checked by [`check_recalculated_part`], which starts
//! it at the inner node where the value is evidenced rather than with a withdrawal; only a withdrawal's failure
//! cancels the part, so a recalculated route's first segment fails as any later one does; a route recalculated
//! elsewhere reaches the order Core follows through [`Segments::observe`]; [`Segments::run_part`] checks the part's floor at each settled segment, holding it there below it, handing nothing more over
//! (`REQ-ORD-27`); a segment the followed order retired is never handed over again; and [`Segments::order`],
//! [`Segments::handed_on`], [`Segments::handed_over`], [`Segments::retired`] and [`Segments::waiting`] read where
//! the value is, what arrived there and what may still move.

mod drain;
mod journal;
mod judge;
mod ports;
mod sign;

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::{
    AppendFailure, Decisions, Evidence, GeneralEntry, Log, Reads,
};
use rutent_contracts::core_internal::{
    About, Action, Actor, ActorKind, Answer, CatalogueChange, DecisionRequest, Effect, Entry,
    OrderRefusalClass, Outcome, Permit, ReadAnswer, ReadCheck, ReadOutcome, Receipt, Role,
    RouteEnd, VerifiedEntry, What,
};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{
    Operation as ProviderOperation, Refusal, SafeInteger, Statement,
};
use rutent_contracts::router_api::{Amount, DecisionRecord, Id, PositiveAmount, Timestamp};
use rutent_contracts::state_machine::{
    self as stm, Observed, OrderState, RecordedInput, Source, SourceKind, Standing, StateMachine,
    Subject,
};
use rutent_prov::catalogue::{Catalogue, Publisher};
use rutent_prov::module::lifecycle::LifecycleRefusal;
use rutent_prov::module::types::UnknownOutcome;
use rutent_stm::Stm;

use crate::recalc::{holds_floor, left_after};
use crate::rounds::{Fresh, Issued, OperatorKey};
use crate::snapshot::{
    Clock, Exposures, HoldError, OrchestrationLog, Reading, RouteRefused, check_part,
    check_recalculated_part, record_route_refusal,
};

pub use drain::Uncarried;
pub use journal::{APPEND_TRIES, Unappended};
pub use judge::{Counted, NotCounted, Quoted, judge};
pub use ports::{Answered, Grading, Modules, NoProxy, ProvidersWord, ToGrade, Uses};
pub use sign::Unwritten;

/// How many fresh identifiers are drawn for a transaction before the source is taken as broken.
const DRAWS: usize = 4;

/// What the segments reach. The assembly builds it; Core's clock, catalogue and exposures are those the snapshots are
/// formed from.
pub struct Parts<'a> {
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// The catalogue: a result is attributed by its keys.
    pub catalogue: &'a Catalogue,
    /// Orchestration's handle on the log.
    pub log: &'a mut dyn OrchestrationLog,
    /// The evidence store.
    pub evidence: &'a mut dyn Evidence,
    /// Policy's one decision call.
    pub decisions: &'a mut dyn Decisions,
    /// The module platform.
    pub modules: &'a mut dyn Modules,
    /// The credential proxy's records of use.
    pub uses: &'a mut dyn Uses,
    /// The grading of a settled segment's evidence (`T-020-09`).
    pub grading: &'a mut dyn Grading,
    /// The operator key, which signs each operation Core hands over.
    pub operator_key: &'a mut dyn OperatorKey,
    /// Fresh identifiers: transactions, requests, drains, records and evidence.
    pub fresh: &'a mut dyn Fresh,
    /// The transactions the routing rounds asked under: no segment's is one of them.
    pub issued: &'a Issued,
    /// Core's record of value in flight.
    pub exposures: &'a mut Exposures,
}

/// One part of an order's chosen route, as Core runs it.
#[derive(Debug, Clone, Copy)]
pub struct Live<'a> {
    /// The order.
    pub order: &'a Id,
    /// The part's id in the order's plan.
    pub part: &'a Id,
    /// The id Core gave each of the part's segments in the order's plan, in the route's order.
    pub segments: &'a [Id],
    /// The router's recorded decision that chose the route.
    pub record: &'a DecisionRecord,
    /// The part's place in the chosen candidate.
    pub at: usize,
    /// The authorisation the order's value moves under (`REQ-AUZ-3`).
    pub authorisation: &'a Id,
    /// The order's origin reserve.
    pub origin_reserve: &'a Id,
    /// What the route ends at, as it stands.
    pub route: &'a RouteEnd,
    /// Who asks policy for each move: Core carrying the user's authorisation onward, or the operator acting for the
    /// user (`REQ-AUZ-9`).
    pub caller: &'a Actor,
    /// Core, acting on its own account: who writes Core's own entries.
    pub core: &'a Actor,
    /// The operator, in whose name Core signs each operation: its `acts` is their maker.
    pub operator: &'a Actor,
    /// The user's session, which a withdrawal is asked in.
    pub session: Option<&'a Id>,
    /// The order's inputs, by name; an operation carries only those its tuple's terms require.
    pub inputs: &'a BTreeMap<String, String>,
    /// The least the customer takes at the destination, as its authorisation names it: checked at each settled
    /// segment against what is evidenced there and the quotes still ahead, before the next is handed over
    /// (`REQ-ORD-27`, `AC-42`, `T-020-10`).
    pub floor: Option<&'a PositiveAmount>,
}

/// A provider's statement, labelled as its word: what Core passes on of it is its maker's, never rutent's
/// (`REQ-ORD-26`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidersStatement {
    /// The provider whose word it is.
    pub maker: Id,
    /// The receipt of its stated entry.
    pub statement: Receipt,
    /// Its bytes, as received and as the platform checked them against the result's schema (`REQ-MDL-12`).
    pub bytes: Vec<u8>,
}

/// Who says a segment's outcome is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownBy {
    /// The provider, in a result that counts: its stated entry.
    Provider(Receipt),
    /// The platform: the module passed something out, then the operation ended with no answer (`OutcomeUnknown`).
    /// What it passed out is in the drain handed over with it.
    Platform(PlatformRefusal),
}

/// How one segment was accounted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentOutcome {
    /// Policy didn't permit it: nothing was recorded as an effect, and nothing moved.
    NotPermitted(Answer),
    /// It wasn't handed over, or the platform refused it before its module passed anything out: nothing moved.
    NotHandedOver(PlatformRefusal),
    /// It settled, on the evidence its `segment_settled` cites; what it gave out is what the next segment moves.
    Settled {
        /// What arrived at its destination net of its charge, as its tuple's terms report it: what the next segment
        /// moves.
        amount_out: Amount,
        /// The evidence it settled on.
        evidence: Id,
    },
    /// It failed: its reversal is owed, and its value stays held.
    Failed,
    /// The provider refused it, finally or until a known time: it is failed, and its value stays held.
    Refused(Refusal),
    /// The part's withdrawal failed or was refused, with nothing moved: the part ends, cancelled, and no later
    /// segment is handed over (`REQ-ORD-40`).
    WithdrawalCancelled,
    /// Its outcome is unknown, on stated evidence.
    Unknown(UnknownBy),
    /// The provider waits on the person: its statement, labelled as its word.
    WaitingOnThePerson(ProvidersStatement),
    /// Its module answered with nothing that counts: no result, and the segment waits.
    NoResult(NotCounted),
}

/// One segment, accounted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accounted {
    /// The segment.
    pub segment: Id,
    /// The transaction it ran under, once one was assigned.
    pub transaction: Option<Id>,
    /// The receipt of its effect's entry, once recorded.
    pub effect: Option<Receipt>,
    /// How it was accounted for.
    pub outcome: SegmentOutcome,
}

/// What running one part did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// The part broke the route guard: refused before any segment was handed over, the refusal recorded.
    Refused {
        /// Why.
        refused: RouteRefused,
        /// The receipt of its `order_refusal` entry.
        receipt: Receipt,
    },
    /// Its segments, in order, up to the one it stopped at.
    Segments(Vec<Accounted>),
}

/// Where a part waits for the customer past its floor (`REQ-ORD-27`, `T-020-10`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waits {
    /// The settled segment it waits after, where its value is evidenced.
    pub after: Id,
    /// What is evidenced there.
    pub evidenced: PositiveAmount,
    /// What the quotes still ahead would leave the customer after the operator's fee; `None` where Core couldn't
    /// count it, which holds no floor.
    pub left: Option<Amount>,
}

/// A settled segment's actual settlement time, as its `settlement` entry recorded it (`REQ-QTE-3`, `T-020-11`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    /// The order.
    pub order: Id,
    /// The segment.
    pub segment: Id,
    /// Its provider.
    pub provider: Id,
    /// How long it took, in seconds.
    pub actual_seconds: u64,
    /// The time it declared with its quote, in seconds.
    pub declared_seconds: u64,
    /// The receipt of its `settlement` entry.
    pub receipt: Receipt,
}

/// Why a segment couldn't be run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentsError {
    /// The log or the evidence store refused or failed an append. Before the effect's receipt, nothing moved; after
    /// it, everything the segment gave back is kept, and running the part again resumes it from where it stopped.
    Append(AppendFailure),
    /// No operation could be written or signed for the segment.
    Unwritten(Unwritten),
    /// The part names no chosen operation at this place, or the plan's segments don't match its operations.
    Plan(&'static str),
    /// The source of fresh identifiers repeats ones Core used.
    NoFreshTransaction,
    /// The segment was handed over already (`INV-8`).
    AlreadyHandedOver(Id),
    /// The order Core follows retired the segment, its route recalculated: it is never handed over or taken on
    /// again (`INV-4`, `T-020-10`).
    Retired(Id),
    /// The part is stuck, and no segment of it was carried out with something still to record: nothing of it is
    /// handed over or carried out again, and only its resolution under policy moves its value (`REQ-ORD-28`,
    /// `REQ-AUZ-4`, `T-020-11`).
    Stuck(Id),
    /// The effect was acted on already (`INV-8`).
    AlreadyActed(Receipt),
    /// The segment's value couldn't be held in flight.
    Hold(HoldError),
    /// A drain names something the contract can't carry; it is kept whole, not dropped.
    Uncarried(Uncarried),
}

impl From<Uncarried> for SegmentsError {
    fn from(uncarried: Uncarried) -> Self {
        SegmentsError::Uncarried(uncarried)
    }
}

impl From<AppendFailure> for SegmentsError {
    fn from(failure: AppendFailure) -> Self {
        SegmentsError::Append(failure)
    }
}

/// An order's state and its age, as a reader policy allowed reads it (`REQ-ORD-5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The order.
    pub order: Id,
    /// The order's state.
    pub state: OrderState,
    /// Each part's state, by its id.
    pub parts: Vec<(Id, OrderState)>,
    /// When the order reached its state, by Core's clock.
    pub since: Timestamp,
    /// How long it has been in it, in whole seconds, by Core's clock.
    pub age_seconds: u64,
}

/// Why an order's state wasn't read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusError {
    /// Policy refused the read, and recorded it.
    Refused(ReadAnswer),
    /// Policy couldn't record the check, so it answered nothing.
    Append(AppendFailure),
    /// Core tracks no such order.
    Unknown,
}

/// A promotion to carry out: the release staged at `provider` that `version` names, asked by `caller`.
#[derive(Debug, Clone, Copy)]
pub struct Promotion<'a> {
    /// The provider.
    pub provider: &'a Id,
    /// The staged version to put in force.
    pub version: &'a Id,
    /// Who asks: the operator's login, which policy decides and the platform records as `by`.
    pub caller: &'a Actor,
    /// Core, which writes the effect and its result.
    pub core: &'a Actor,
}

/// Why a promotion wasn't carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromoteRefused {
    /// The provider has nothing staged.
    NothingStaged,
    /// The version asked for isn't the one staged now: a permit to promote one release promotes no other. Nothing
    /// was asked of policy.
    NotTheStagedVersion {
        /// The version staged.
        staged: String,
    },
    /// Policy didn't permit it: no effect was recorded.
    NotPermitted(Answer),
    /// The log refused or failed an append: before the effect's receipt nothing changed.
    Append(AppendFailure),
    /// The platform refused: the effect's result is `failed`, and nothing changed.
    Lifecycle(LifecycleRefusal),
}

impl From<AppendFailure> for PromoteRefused {
    fn from(failure: AppendFailure) -> Self {
        PromoteRefused::Append(failure)
    }
}

/// One step of a segment's record: the entry, built once and appended again unchanged until the log gives its
/// receipt, so a retried step is one entry (`INV-8`).
#[derive(Debug, Clone)]
struct Step {
    entry: GeneralEntry,
    receipt: Option<Receipt>,
}

/// A segment from its permit until its outcome is final: everything it needs to go on from where it stopped, and
/// everything it gave back, kept before any step that can fail.
#[derive(Debug, Clone)]
struct InFlight {
    place: usize,
    transaction: Id,
    provider: Id,
    statement: Statement<ProviderOperation>,
    effect: Option<Effect>,
    begun: bool,
    carried: bool,
    answered: Option<Answered>,
    started: Option<Reading>,
    /// What the platform reported of an operation that ended with no answer, which core-internal has no item for:
    /// kept here, with the segment.
    unknown: Vec<UnknownOutcome>,
    steps: BTreeMap<&'static str, Step>,
    evidence: Option<(Id, Timestamp)>,
    ended: bool,
    outcome: Option<SegmentOutcome>,
    /// The platform's refusal to begin it, kept before its result is appended.
    begin_refused: Option<PlatformRefusal>,
    /// The first verdict on what it gave back, kept: a later try reads the same, whatever changed since.
    verdict: Option<Result<Counted, NotCounted>>,
    /// The drains taken for it, by id: their errors are its own.
    drains: Vec<Id>,
    /// The drain of its record, which reaches the log before its result.
    record: Option<Id>,
}

/// One drain, kept whole until each of its items is appended.
#[derive(Debug, Clone)]
struct Drain {
    id: Id,
    at: Timestamp,
    who: Actor,
    raw: drain::Raw,
    appended: usize,
}

/// An order whose state Core follows, and when it reached it.
#[derive(Debug, Clone)]
struct Tracked {
    order: stm::Order,
    since: Reading,
}

/// Core's record of the segments it handed over, kept across orders by the assembly: the effects acted on, the
/// transactions assigned, the drains not yet appended, each segment from its permit to its final outcome, and the
/// orders followed. The production assembly keeps it durably (`F-030`): a fresh one knows nothing of the effects acted
/// on before it, so after a restart only the kept record stops a second `move_value` on a segment.
#[derive(Debug, Clone, Default)]
pub struct Segments {
    acted: BTreeSet<String>,
    transactions: BTreeSet<String>,
    undelivered: Vec<Drain>,
    flights: BTreeMap<(String, String), InFlight>,
    orders: BTreeMap<String, Tracked>,
    /// The parts that wait for the customer past their floor, by order and part.
    waiting: BTreeMap<(String, String), Waits>,
}

/// A segment's final outcome: one after which nothing more is recorded on it here.
fn is_final(outcome: &SegmentOutcome) -> bool {
    matches!(
        outcome,
        SegmentOutcome::NotHandedOver(_)
            | SegmentOutcome::Settled { .. }
            | SegmentOutcome::Failed
            | SegmentOutcome::Refused(_)
            | SegmentOutcome::WithdrawalCancelled
    )
}

impl Segments {
    /// Nothing handed over yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many drains wait to be appended, whole or in part.
    pub fn undelivered(&self) -> usize {
        self.undelivered.len()
    }

    /// Whether the effect at `receipt` was acted on.
    pub fn acted_on(&self, receipt: &Receipt) -> bool {
        self.acted.contains(&key(receipt))
    }

    /// Marks the effect at `receipt` acted on, in the one register every effect Core carries out is held to, a stuck
    /// order's resolution among them: false where it was acted on already, by a segment or a resolution, and so
    /// isn't acted on again (`INV-8`, `T-020-11`).
    pub fn act_on(&mut self, receipt: &Receipt) -> bool {
        self.acted.insert(key(receipt))
    }

    /// Follows the order `inputs` open, rebuilt from the log: its `route_chosen` and every input recorded about it
    /// since, in order, as a restart finds them (`F-030`, `T-020-11`). The effects acted on and the segments in
    /// flight are not rebuilt; only the order's state is. False if they open no order.
    pub fn follow_recorded(&mut self, clock: &dyn Clock, inputs: &[RecordedInput]) -> bool {
        let Some((first, rest)) = inputs.split_first() else {
            return false;
        };
        let Some(mut order) = Stm::open(first) else {
            return false;
        };
        for input in rest {
            if input.subject.order.as_str() == order.order_id.as_str() {
                order = Stm::transition(&order, input);
            }
        }
        self.orders.insert(
            order.order_id.as_str().to_owned(),
            Tracked {
                order,
                since: clock.now(),
            },
        );
        true
    }

    /// The transaction a segment of `order` in flight runs under, until its outcome is final.
    pub fn in_flight(&self, order: &Id, segment: &Id) -> Option<&Id> {
        self.flights
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))
            .filter(|f| f.outcome.as_ref().is_none_or(|o| !is_final(o)))
            .map(|f| &f.transaction)
    }

    /// What the platform reported of a segment's operation that ended with no answer (`Events::unknown`), which the
    /// log has no item for yet: kept with the segment.
    pub fn unknown_outcomes(&self, order: &Id, segment: &Id) -> &[UnknownOutcome] {
        self.flights
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))
            .map_or(&[], |f| f.unknown.as_slice())
    }

    /// Appends again, in order, every drain an append failed for, each from its first item not yet appended. A drain
    /// that fails stays where it is, kept whole, and the others are still tried, so one drain the log can't take
    /// holds up no other: none is dropped and none is drained again (`REQ-MDL-8`). The first failure is answered as
    /// the error it is, every time it is tried: an item the contract can't carry, or one the log refuses.
    pub fn flush(&mut self, log: &mut dyn Log) -> Result<(), SegmentsError> {
        match self.flush_all(log).into_iter().next() {
            Some((_, failure)) => Err(failure),
            None => Ok(()),
        }
    }

    /// [`Segments::flush`], answering each drain that failed with its error.
    fn flush_all(&mut self, log: &mut dyn Log) -> Vec<(Id, SegmentsError)> {
        let mut failed = Vec::new();
        let mut kept = Vec::new();
        for mut drain in std::mem::take(&mut self.undelivered) {
            if let Err(failure) = Self::append_drain(log, &mut drain) {
                failed.push((drain.id.clone(), failure));
                kept.push(drain);
            }
        }
        self.undelivered = kept;
        failed
    }

    /// Appends one drain from its first item not yet appended.
    fn append_drain(log: &mut dyn Log, drain: &mut Drain) -> Result<(), SegmentsError> {
        let items = drain.raw.items()?;
        let whats = drain::entries(items, &drain.id)?;
        for what in whats.into_iter().skip(drain.appended) {
            let entry = journal::verified(&drain.who, what, &drain.at, Outcome::Observed {});
            journal::append(log, entry)?;
            drain.appended += 1;
        }
        Ok(())
    }

    /// Where `part` of `order` waits for the customer past its floor, if it does (`REQ-ORD-27`).
    pub fn waiting(&self, order: &Id, part: &Id) -> Option<&Waits> {
        self.waiting
            .get(&(order.as_str().to_owned(), part.as_str().to_owned()))
    }

    /// Whether the order Core follows retired `segment`: a recalculation replaced it (`T-020-10`).
    pub fn retired(&self, order: &Id, segment: &Id) -> bool {
        self.orders.get(order.as_str()).is_some_and(|t| {
            t.order.parts.iter().any(|p| {
                p.retired_segments
                    .iter()
                    .any(|r| r.as_str() == segment.as_str())
            })
        })
    }

    /// Whether `segment` of `order` was handed over and has no final outcome: from its permit on, its effect may be
    /// in the log and its value about to move, whatever the state machine says of it yet (`INV-4`, `T-020-10`).
    pub fn handed_over(&self, order: &Id, segment: &Id) -> bool {
        self.in_flight(order, segment).is_some()
    }

    /// The order Core follows, as the inputs recorded so far left it: where each part's value sits
    /// (`T-020-10`).
    pub fn order(&self, order: &Id) -> Option<&stm::Order> {
        self.orders.get(order.as_str()).map(|t| &t.order)
    }

    /// What `segment` of `order` gave out by its result that counted, net of its charge as its tuple reports it,
    /// once it settled: what is evidenced at its end, and what the segment after it moves (`REQ-ORD-21`,
    /// `T-020-10`).
    pub fn handed_on(&self, order: &Id, segment: &Id) -> Option<&Amount> {
        let flight = self
            .flights
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))?;
        match &flight.outcome {
            Some(SegmentOutcome::Settled { amount_out, .. }) => Some(amount_out),
            _ => None,
        }
    }

    /// Moves the order Core follows by `input`, a part's input recorded outside the segments: a
    /// `route_recalculated` (`T-020-10`), so the inputs the segments record next apply to its new route, or a
    /// `no_route` or `stuck_declared` (`T-020-11`), so the part's state is read as stuck or waiting. Only those
    /// kinds: every segment's input the segments record themselves. False where it is another kind, Core follows no
    /// such order, or it moved nothing.
    pub fn observe(&mut self, clock: &dyn Clock, input: &RecordedInput) -> bool {
        if !matches!(
            input.observed,
            Observed::RouteRecalculated { .. }
                | Observed::NoRoute { .. }
                | Observed::StuckDeclared { .. }
        ) {
            return false;
        }
        let Some(tracked) = self.orders.get_mut(input.subject.order.as_str()) else {
            return false;
        };
        let next = Stm::transition(&tracked.order, input);
        let applied = next.last_input.outcome == (stm::Outcome::Applied {});
        if applied {
            tracked.order = next;
            tracked.since = clock.now();
        }
        applied
    }

    /// Whether `part` of `order` is stuck in the order Core follows (`T-020-11`).
    pub fn stuck(&self, order: &Id, part: &Id) -> bool {
        self.orders.get(order.as_str()).is_some_and(|t| {
            t.order
                .parts
                .iter()
                .any(|p| p.part_id.as_str() == part.as_str() && p.state == OrderState::Stuck)
        })
    }

    /// The operation Core signed and handed over for `segment` of `order`, as it was appended to the log: what it
    /// named, the user's accounts among its inputs (`T-020-11`, a resolution's origin).
    pub fn sent(&self, order: &Id, segment: &Id) -> Option<&ProviderOperation> {
        self.flights
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))
            .map(|f| &f.statement.statement)
    }

    /// When `segment` of `order` was carried out, while its outcome isn't final: from then its value is held at
    /// its provider, with no result that counts, an outcome unknown, or the person to act (`T-020-11`, the stuck
    /// clock).
    pub fn carried_since(&self, order: &Id, segment: &Id) -> Option<Reading> {
        self.flights
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))
            .filter(|f| f.carried && f.outcome.as_ref().is_none_or(|o| !is_final(o)))
            .and_then(|f| f.started)
    }

    /// Every settled segment's actual settlement time as its `settlement` entry recorded it, with that entry's
    /// receipt, once the log took it: the history the stuck clock's p99 is read from (`REQ-ORD-29`, `T-020-11`).
    pub fn settlements(&self) -> Vec<Settlement> {
        self.flights
            .iter()
            .filter_map(|((order, segment), flight)| {
                let step = flight.steps.get("settlement")?;
                let receipt = step.receipt.clone()?;
                let Entry::Verified(VerifiedEntry {
                    what:
                        What::Settlement {
                            actual_seconds,
                            declared_seconds,
                            ..
                        },
                    ..
                }) = step.entry.entry()
                else {
                    return None;
                };
                Some(Settlement {
                    order: Id::new(order.as_str())?,
                    segment: Id::new(segment.as_str())?,
                    provider: flight.provider.clone(),
                    actual_seconds: u64::from(*actual_seconds),
                    declared_seconds: u64::from(*declared_seconds),
                    receipt,
                })
            })
            .collect()
    }

    /// Follows `chosen`, the order's `route_chosen` input as recorded, so its state and age can be read and the
    /// inputs the segments record move it. False if it opens no order.
    pub fn follow(&mut self, clock: &dyn Clock, chosen: &RecordedInput) -> bool {
        let Some(order) = Stm::open(chosen) else {
            return false;
        };
        self.orders.insert(
            order.order_id.as_str().to_owned(),
            Tracked {
                order,
                since: clock.now(),
            },
        );
        true
    }

    /// An order's state and age, for `reader`, once policy allows the read; the check is recorded either way
    /// (`REQ-ORD-5`, `REQ-AUZ-7`). The read is of the order's state and age alone (`Reads::OrderState`), which the
    /// order's customer and the operator's listed login read without a permission (`T-021-02`); a reader that reads
    /// by an identified permission names it.
    pub fn status(
        &self,
        reads: &mut dyn Reads,
        clock: &dyn Clock,
        fresh: &mut dyn Fresh,
        reader: &Actor,
        permission: Option<&Id>,
        order: &Id,
    ) -> Result<Status, StatusError> {
        let tracked = self
            .orders
            .get(order.as_str())
            .ok_or(StatusError::Unknown)?;
        let now = clock.now();
        let no_time = || StatusError::Append(AppendFailure::Refused(journal::NO_TIME));
        let at = now.timestamp().ok_or_else(no_time)?;
        let answer = reads
            .check_read(&ReadCheck {
                request: fresh.id(),
                reader: reader.clone(),
                reads: rutent_contracts::core_internal::Reads::OrderState {
                    order: order.clone(),
                },
                permission: permission.cloned(),
                at,
            })
            .map_err(StatusError::Append)?;
        if !matches!(answer.outcome, ReadOutcome::Allowed {}) {
            return Err(StatusError::Refused(answer));
        }
        let since = tracked.since.timestamp().ok_or_else(no_time)?;
        Ok(Status {
            order: order.clone(),
            state: tracked.order.state,
            parts: tracked
                .order
                .parts
                .iter()
                .filter_map(|p| Some((Id::new(p.part_id.as_str())?, p.state)))
                .collect(),
            since,
            age_seconds: now.floor().0.saturating_sub(tracked.since.floor().0),
        })
    }

    /// Runs one part of an order's chosen route: the route guard, then each segment in turn, while each settles. A
    /// segment that stopped before its outcome was final is resumed from where it stopped, with what it gave back;
    /// one whose outcome is final is refused as handed over.
    ///
    /// After each segment that settles with a later one to hand over, the part's floor ([`Live::floor`]) is checked
    /// against what that segment gave out and the quotes still ahead (`recalc::left_after`, `REQ-ORD-27`, `AC-42`):
    /// below it, nothing more is handed over and the part waits for the customer ([`Segments::waiting`]), however
    /// often it is run. Every run goes through here, so the check can't be skipped (`T-020-10`).
    pub fn run_part(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
    ) -> Result<Ran, SegmentsError> {
        let chosen = live
            .record
            .chosen()
            .ok_or(SegmentsError::Plan("the record chose no route"))?;
        let part = chosen
            .parts
            .get(live.at)
            .ok_or(SegmentsError::Plan("the chosen route has no part there"))?;
        // A stuck part hands nothing over and carries nothing out: its value moves only on its resolution. What a
        // segment carried out already gave back is still recorded, each step as it was built, so nothing it kept
        // is lost; nothing is asked of its module again (`T-020-11`).
        if self.stuck(live.order, live.part) {
            let mut taken = Vec::new();
            for (place, segment) in live.segments.iter().enumerate() {
                let at = (live.order.as_str().to_owned(), segment.as_str().to_owned());
                let carried = self
                    .flights
                    .get(&at)
                    .is_some_and(|f| f.carried && f.outcome.as_ref().is_none_or(|o| !is_final(o)));
                if !carried {
                    continue;
                }
                let operation = part.operations.get(place).ok_or(SegmentsError::Plan(
                    "the chosen route has no operation there",
                ))?;
                let done = self.advance(parts, live, &at, operation, false)?;
                // A segment that settles here released its hold, its value arrived at the node it reached. While
                // the part is still stuck there, that value rests at the node and is held there, so the ceilings
                // and the part's resolution still count it (`REQ-ORD-8`, `T-020-11`).
                if let SegmentOutcome::Settled { amount_out, .. } = &done.outcome
                    && self.stuck(live.order, live.part)
                    && matches!(
                        operation.to.node,
                        rutent_contracts::provider_role::NodeRef::Inner(_)
                    )
                    && !amount_out.is_zero()
                {
                    match parts.exposures.hold_resting(
                        live.order,
                        segment,
                        &operation.provider,
                        &operation.to.asset,
                        amount_out,
                    ) {
                        Ok(()) | Err(HoldError::AlreadyHeld) => {}
                        Err(other) => return Err(SegmentsError::Hold(other)),
                    }
                }
                taken.push(done);
            }
            if taken.is_empty() {
                return Err(SegmentsError::Stuck(live.part.clone()));
            }
            return Ok(Ran::Segments(taken));
        }
        // The route guard, before any segment is handed over (`REQ-ORD-16`, `REQ-ORD-36`, `REQ-ORD-37`). A
        // recalculated part starts where its value is evidenced, at an inner node (`REQ-ORD-2`).
        let guard = match live.record.request.follows {
            Some(_) => check_recalculated_part(part),
            None => check_part(part),
        };
        if let Err(refused) = guard {
            let receipt = record_route_refusal(
                parts.log,
                parts.clock,
                live.core.clone(),
                live.record.record_id(),
                Some(live.order),
                &refused,
            )?;
            return Ok(Ran::Refused { refused, receipt });
        }
        if part.operations.len() != live.segments.len() {
            return Err(SegmentsError::Plan(
                "the plan's segments are not the route's operations",
            ));
        }
        let mut amount = part.operations[0].amount_in.clone();
        let mut accounted = Vec::new();
        for place in 0..part.operations.len() {
            // A segment whose outcome is final stands as it is: a run that stopped after it goes on from the next
            // one, with what it gave out.
            let done = match self.standing(live, place) {
                Some(done) => done,
                None => self.hand_over(parts, live, place, &amount)?,
            };
            let next = match &done.outcome {
                SegmentOutcome::Settled { amount_out, .. } => {
                    match PositiveAmount::new(amount_out.clone()) {
                        Some(next) => Some(next),
                        None if place + 1 < part.operations.len() => {
                            // Nothing arrived to move on: the part can't go on, and the order is told so. The
                            // refusal is the settled segment's step, kept and appended once, so a run that stopped
                            // before the log took it records it when it goes on.
                            let at = (
                                live.order.as_str().to_owned(),
                                done.segment.as_str().to_owned(),
                            );
                            let when = now(parts.clock)?;
                            self.built_step(parts.log, &at, "refusal", || {
                                Ok(journal::verified(
                                    live.core,
                                    What::OrderRefusal {
                                        request: live.record.record_id().clone(),
                                        order: Some(live.order.clone()),
                                        class: OrderRefusalClass::NoRouteForAmount,
                                    },
                                    &when,
                                    Outcome::Observed {},
                                ))
                            })?;
                            None
                        }
                        None => None,
                    }
                }
                _ => None,
            };
            accounted.push(done);
            // What the next segment moves is what this one gave out, by its result that counted (`REQ-ORD-21`).
            match next {
                Some(next) if place + 1 < part.operations.len() => {
                    let left = left_after(
                        part.operations.as_slice(),
                        &live.record.bounds.operator_fee,
                        place,
                        &next,
                    );
                    if !holds_floor(left.as_ref(), live.floor) {
                        let waits = Waits {
                            after: live.segments[place].clone(),
                            evidenced: next,
                            left,
                        };
                        self.waiting.insert(
                            (
                                live.order.as_str().to_owned(),
                                live.part.as_str().to_owned(),
                            ),
                            waits,
                        );
                        break;
                    }
                    amount = next;
                }
                Some(next) => amount = next,
                None => break,
            }
        }
        Ok(Ran::Segments(accounted))
    }

    /// The segment at `place` of the part, as it stands, where its outcome is final.
    fn standing(&self, live: &Live<'_>, place: usize) -> Option<Accounted> {
        let segment = live.segments.get(place)?;
        let flight = self
            .flights
            .get(&(live.order.as_str().to_owned(), segment.as_str().to_owned()))?;
        let outcome = flight.outcome.clone().filter(is_final)?;
        Some(Accounted {
            segment: segment.clone(),
            transaction: Some(flight.transaction.clone()),
            effect: flight.effect.as_ref().map(|e| e.receipt.clone()),
            outcome,
        })
    }

    /// Hands over the part's segment at `place`, moving `amount`, or resumes it where it stopped, and accounts for it.
    fn hand_over(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        place: usize,
        amount: &PositiveAmount,
    ) -> Result<Accounted, SegmentsError> {
        let chosen = live
            .record
            .chosen()
            .ok_or(SegmentsError::Plan("the record chose no route"))?;
        let operation = chosen
            .parts
            .get(live.at)
            .and_then(|p| p.operations.get(place))
            .ok_or(SegmentsError::Plan(
                "the chosen route has no operation there",
            ))?;
        let segment = live
            .segments
            .get(place)
            .ok_or(SegmentsError::Plan("the plan has no segment there"))?;
        let at = (live.order.as_str().to_owned(), segment.as_str().to_owned());
        // A segment the order Core follows retired is never handed over or taken on again, whatever it kept: its
        // value moves only on the route that replaced it (`INV-4`, `T-020-10`).
        if self.retired(live.order, segment) {
            return Err(SegmentsError::Retired(segment.clone()));
        }
        if let Some(flight) = self.flights.get(&at) {
            if flight.outcome.as_ref().is_some_and(is_final) {
                return Err(SegmentsError::AlreadyHandedOver(segment.clone()));
            }
            return self.advance(parts, live, &at, operation, true);
        }
        let cited = sign::cited(live.record, operation).ok_or(SegmentsError::Plan(
            "the record's snapshot names no tuple for the operation",
        ))?;
        let transaction = self.fresh_transaction(parts)?;
        let inputs: BTreeMap<String, String> = cited
            .terms
            .required_inputs
            .iter()
            .filter_map(|name| Some((name.clone(), live.inputs.get(name)?.clone())))
            .collect();
        let provider_operation = sign::operation(&sign::Moving {
            operation,
            transaction: &transaction,
            amount,
            session: live.session,
            inputs,
        })
        .map_err(SegmentsError::Unwritten)?;

        // Policy decides the move; only a permit goes on (`REQ-AUZ-1`).
        let action = Action::MoveValue {
            order: live.order.clone(),
            segment: segment.clone(),
            provider: operation.provider.clone(),
            origin_reserve: live.origin_reserve.clone(),
            authorisation: live.authorisation.clone(),
            route: live.route.clone(),
        };
        let request = DecisionRequest {
            request: parts.fresh.id(),
            caller: live.caller.clone(),
            action: action.clone(),
            at: now(parts.clock)?,
        };
        let decision = parts.decisions.decide(&request)?;
        let Some(permit) = decision.permit() else {
            return Ok(Accounted {
                segment: segment.clone(),
                transaction: None,
                effect: None,
                outcome: SegmentOutcome::NotPermitted(decision.outcome),
            });
        };

        // The operation, signed. It, the question's entry and the effect's are built once, here, and appended
        // unchanged however often a step is tried again.
        let signed = sign::sign(
            parts.operator_key,
            &live.record.bounds,
            &live.operator.acts,
            &now(parts.clock)?,
            provider_operation,
        )
        .map_err(SegmentsError::Unwritten)?;
        let statement: Statement<ProviderOperation> = serde_json::from_slice(&signed)
            .map_err(|_| SegmentsError::Unwritten(Unwritten::Unsigned))?;
        let about = About {
            order: Some(live.order.clone()),
            segment: Some(segment.clone()),
            transaction: Some(transaction.clone()),
        };
        let question = journal::stated_entry(
            parts.evidence,
            &now(parts.clock)?,
            &signed,
            live.operator.clone(),
            about,
        )
        .map_err(|why| match why {
            Unappended::Append(failure) => SegmentsError::Append(failure),
            Unappended::Unrecordable => SegmentsError::Unwritten(Unwritten::Unsigned),
        })?;
        let effect = journal::verified(
            live.core,
            What::Effect {
                action,
                decision: permit,
            },
            &now(parts.clock)?,
            Outcome::ToTakeEffect {},
        );
        let mut steps = BTreeMap::new();
        for (name, entry) in [("question", question), ("effect", effect)] {
            entry.check().map_err(AppendFailure::Refused)?;
            let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
            steps.insert(
                name,
                Step {
                    entry,
                    receipt: None,
                },
            );
        }
        self.flights.insert(
            at.clone(),
            InFlight {
                place,
                transaction,
                provider: operation.provider.clone(),
                statement,
                effect: None,
                begun: false,
                carried: false,
                answered: None,
                started: None,
                unknown: Vec::new(),
                steps,
                evidence: None,
                ended: false,
                outcome: None,
                begin_refused: None,
                verdict: None,
                drains: Vec::new(),
                record: None,
            },
        );
        self.advance(parts, live, &at, operation, true)
    }

    /// Takes a segment on from where it stopped: its question and effect appended, its value held, the segment
    /// begun, its calls named, its operation carried out once for its effect's receipt, and its outcome accounted
    /// for. A step that fails leaves everything before it kept, to go on from next time.
    fn advance(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        at: &(String, String),
        operation: &rutent_contracts::router_api::Operation,
        hold: bool,
    ) -> Result<Accounted, SegmentsError> {
        let flight = self.flight(at)?;
        let (segment, place) = (id_of(&at.1)?, flight.place);
        let moved = flight.statement.statement.clone();
        // Held in flight from before its effect, at the amount it moves (`REQ-ORD-8`). A hold already there is this
        // segment's, from a try that stopped before.
        let held = held_at(operation, &moved).ok_or(SegmentsError::Plan(
            "the amount moved is one the route's operation can carry",
        ))?;
        // A stuck part's segment takes no hold: it still holds its value, or the part's resolution released it, and
        // a hold taken back would count what it no longer holds (`T-020-11`).
        if hold {
            match parts.exposures.hold(live.order, &segment, &held) {
                Ok(()) | Err(HoldError::AlreadyHeld) => {}
                Err(other) => return Err(SegmentsError::Hold(other)),
            }
        }
        let release = |exposures: &mut Exposures| exposures.release(live.order, &segment);

        if let Err(failure) = self.step(parts.log, at, "question") {
            if self.flight(at)?.effect.is_none() {
                release(parts.exposures);
            }
            return Err(failure);
        }
        // The effect's entry and its receipt, before anything takes effect (`INV-7`).
        let receipt = match self.step(parts.log, at, "effect") {
            Ok(receipt) => receipt,
            Err(failure) => {
                release(parts.exposures);
                return Err(failure);
            }
        };
        let flight = self.flight_mut(at)?;
        let effect = match &flight.effect {
            Some(effect) => effect.clone(),
            None => {
                let Entry::Verified(entry) = flight.steps["effect"].entry.entry() else {
                    return Err(SegmentsError::Plan("an effect is a verified entry"));
                };
                let What::Effect { action, decision } = &entry.what else {
                    return Err(SegmentsError::Plan("an effect's entry records an effect"));
                };
                let effect = effect_of(action.clone(), decision.clone(), receipt);
                flight.effect = Some(effect.clone());
                effect
            }
        };
        let (provider, transaction) = (flight.provider.clone(), flight.transaction.clone());
        let identifier = Identifier::new(transaction.as_str())
            .ok_or(SegmentsError::Plan("a transaction is an identifier"))?;
        let accounted = |outcome| Accounted {
            segment: segment.clone(),
            transaction: Some(transaction.clone()),
            effect: Some(effect.receipt.clone()),
            outcome,
        };

        // A segment the platform wouldn't begin stays refused: its outcome is kept before its result is appended, so
        // a run that stopped there finishes it so, and never begins it under a result that says it wasn't.
        let refused = match flight.begin_refused {
            Some(refusal) => Some(refusal),
            None if !flight.begun => parts.modules.begin_segment(&provider, &identifier).err(),
            None => None,
        };
        if let Some(refusal) = refused {
            self.flight_mut(at)?.begin_refused = Some(refusal);
            let outcome = SegmentOutcome::NotHandedOver(refusal);
            self.result(parts, live, at, Outcome::Unanswered {})?;
            release(parts.exposures);
            self.finish(at, &outcome)?;
            return Ok(accounted(outcome));
        }
        if !self.flight(at)?.begun {
            self.flight_mut(at)?.begun = true;
            // The live segment's calls, from the chosen record: a call it doesn't name is refused (`REQ-MDL-21`). A
            // record that can't name them names none, which fails closed.
            let segment_identifier = Identifier::new(segment.as_str())
                .ok_or(SegmentsError::Plan("a segment is an identifier"))?;
            let _ = parts.modules.name_calls(
                &identifier,
                live.record,
                (live.at, place),
                &segment_identifier,
            );
        }
        self.input(
            parts,
            live,
            at,
            "bound",
            Some(&segment),
            Observed::ProviderBound {},
        )?;

        if !self.flight(at)?.carried {
            // Acted on once for its receipt, whatever retries its append had, and whichever segment it came back
            // for (`INV-8`).
            let started = parts.clock.now();
            let drained_at = now(parts.clock)?;
            if !self.acted.insert(key(&effect.receipt)) {
                return Err(SegmentsError::AlreadyActed(effect.receipt.clone()));
            }
            let statement = self.flight(at)?.statement.clone();
            let answered = parts.modules.carry_out(&effect, &provider, &statement);
            // Everything that came back is kept before any step that can fail.
            let events = parts.modules.take_events();
            let uses = parts.uses.take_uses();
            let flight = self.flight_mut(at)?;
            flight.carried = true;
            flight.started = Some(started);
            flight.answered = Some(answered);
            flight.unknown.extend(events.unknown.iter().cloned());
            let kept = [
                self.keep_drain(parts, live, &drained_at, drain::Raw::Events(events)),
                self.keep_drain(parts, live, &drained_at, drain::Raw::Uses(uses)),
            ];
            self.flight_mut(at)?
                .drains
                .extend(kept.into_iter().flatten());
        }
        self.deliver(parts.log, at)?;
        let outcome = self.account(parts, live, at)?;
        if is_final(&outcome) {
            self.finish(at, &outcome)?;
        } else {
            self.flight_mut(at)?.outcome = Some(outcome.clone());
        }
        Ok(accounted(outcome))
    }

    /// Accounts for what the segment's operation gave back.
    fn account(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        at: &(String, String),
    ) -> Result<SegmentOutcome, SegmentsError> {
        let flight = self.flight(at)?;
        let segment = id_of(&at.1)?;
        let (provider, place) = (flight.provider.clone(), flight.place);
        let answered = flight.answered.clone().ok_or(SegmentsError::Plan(
            "an operation carried out gave something back",
        ))?;
        let bytes = match answered {
            Answered::Refused(PlatformRefusal::OutcomeUnknown) => {
                // The module passed something out, then ended with no answer: the platform's record is the
                // evidence, handed over with it. The segment stays in flight, held.
                self.input(
                    parts,
                    live,
                    at,
                    "unknown",
                    Some(&segment),
                    Observed::SegmentOutcomeUnknown {},
                )?;
                return Ok(SegmentOutcome::Unknown(UnknownBy::Platform(
                    PlatformRefusal::OutcomeUnknown,
                )));
            }
            Answered::Refused(refusal) => {
                // Refused before the module passed anything out: nothing moved.
                self.end(parts, live, at)?;
                self.result(parts, live, at, Outcome::Unanswered {})?;
                parts.exposures.release(live.order, &segment);
                return Ok(SegmentOutcome::NotHandedOver(refusal));
            }
            Answered::Bytes(bytes) => bytes,
        };
        let chosen = live
            .record
            .chosen()
            .ok_or(SegmentsError::Plan("the record chose no route"))?;
        let operation = &chosen.parts[live.at].operations[place];
        let cited = sign::cited(live.record, operation).ok_or(SegmentsError::Plan(
            "the record's snapshot names no tuple for the operation",
        ))?;
        // Judged once: a later try reads the first verdict, so a key retired since can't undo a settled segment.
        let counted = match &self.flight(at)?.verdict {
            Some(verdict) => verdict.clone(),
            None => {
                let verdict = judge(
                    parts.catalogue,
                    parts.clock.now().floor(),
                    &provider,
                    &self.flight(at)?.statement.statement,
                    cited.terms,
                    cited.quoted.as_ref(),
                    &bytes,
                );
                self.flight_mut(at)?.verdict = Some(verdict.clone());
                verdict
            }
        };
        // Only a withdrawal's failure cancels the part: a recalculated route's first segment is no withdrawal.
        let first =
            place == 0 && operation.kind == rutent_contracts::router_api::OperationKind::Withdrawal;
        // A final outcome ends the segment and hands its record over before its result is appended, so the log
        // knows which release it ran on when it attributes the result: a release revoked mid-flight keeps its key
        // only for segments whose record shows they ran on it (`REQ-MDL-20`).
        let ends = matches!(
            counted,
            Ok(Counted::Succeeded { .. }
                | Counted::Failed
                | Counted::Refused(Refusal::Final(_) | Refusal::LiftsAtKnownTime(_)))
        );
        if ends {
            self.end(parts, live, at)?;
        }
        // The result's bytes are kept and appended as the provider's statement, whether or not it counts.
        let stated = match self.stated(parts, at, &bytes) {
            Ok(receipt) => receipt,
            Err(Unappended::Unrecordable) => {
                return Ok(SegmentOutcome::NoResult(NotCounted::Unreadable));
            }
            Err(Unappended::Append(failure)) => return Err(SegmentsError::Append(failure)),
        };
        let counted = match counted {
            Ok(counted) => counted,
            Err(why) => return Ok(SegmentOutcome::NoResult(why)),
        };
        match counted {
            Counted::Succeeded { handed_on, .. } => {
                let amount_out = handed_on;
                let amount_out = Amount::new(amount_out)
                    .ok_or(SegmentsError::Plan("a counted result states an amount"))?;
                let flight = self.flight_mut(at)?;
                let (evidence, graded_at) = match &flight.evidence {
                    Some(graded) => graded.clone(),
                    None => {
                        let graded = (parts.fresh.id(), now(parts.clock)?);
                        flight.evidence = Some(graded.clone());
                        graded
                    }
                };
                let evidence = parts.grading.grade(
                    parts.log,
                    &ToGrade {
                        evidence: &evidence,
                        order: live.order,
                        segment: &segment,
                        provider: &provider,
                        result: &stated,
                        core: live.core,
                        at: &graded_at,
                    },
                )?;
                let state_evidence = stm::Id::new(evidence.as_str())
                    .ok_or(SegmentsError::Plan("an evidence id is an id"))?;
                self.input(
                    parts,
                    live,
                    at,
                    "settled",
                    Some(&segment),
                    Observed::SegmentSettled {
                        evidence: state_evidence,
                    },
                )?;
                self.result(parts, live, at, Outcome::Done {})?;
                if let Some(declared) = cited.declared_seconds {
                    self.settlement(parts, live, at, declared)?;
                }
                parts.exposures.release(live.order, &segment);
                Ok(SegmentOutcome::Settled {
                    amount_out,
                    evidence,
                })
            }
            Counted::Failed
            | Counted::Refused(Refusal::Final(_) | Refusal::LiftsAtKnownTime(_))
                if first =>
            {
                // The withdrawal failed: nothing moved, the reserve cancels it, and the part ends (`REQ-ORD-40`).
                self.result(parts, live, at, Outcome::Failed {})?;
                self.input(
                    parts,
                    live,
                    at,
                    "cancelled",
                    None,
                    Observed::WithdrawalCancelled {},
                )?;
                parts.exposures.release(live.order, &segment);
                Ok(SegmentOutcome::WithdrawalCancelled)
            }
            Counted::Failed => {
                self.result(parts, live, at, Outcome::Failed {})?;
                self.input(
                    parts,
                    live,
                    at,
                    "failed",
                    Some(&segment),
                    Observed::SegmentFailed {},
                )?;
                Ok(SegmentOutcome::Failed)
            }
            Counted::Refused(Refusal::WaitsOnPerson(_)) => {
                // The provider sends the person to approve: its word, labelled as its statement (`REQ-ORD-26`).
                Ok(SegmentOutcome::WaitingOnThePerson(ProvidersStatement {
                    maker: provider,
                    statement: stated,
                    bytes,
                }))
            }
            Counted::Refused(refusal) => {
                self.result(parts, live, at, Outcome::Failed {})?;
                self.input(
                    parts,
                    live,
                    at,
                    "failed",
                    Some(&segment),
                    Observed::SegmentFailed {},
                )?;
                Ok(SegmentOutcome::Refused(refusal))
            }
            Counted::Unknown => {
                self.input(
                    parts,
                    live,
                    at,
                    "unknown",
                    Some(&segment),
                    Observed::SegmentOutcomeUnknown {},
                )?;
                Ok(SegmentOutcome::Unknown(UnknownBy::Provider(stated)))
            }
        }
    }

    /// Puts `promotion.provider`'s staged release in force, as policy decides, by the decision's caller: only the
    /// version staged now, so a permit to promote one release promotes no other (`REQ-AUZ-6`, `REQ-MDL-4`,
    /// ADR-0014). Core asks the decision itself, records the effect before it calls the platform, and records its
    /// result after; an effect is marked acted on only once the platform put the release in force.
    pub fn promote(
        &mut self,
        log: &mut dyn Log,
        decisions: &mut dyn Decisions,
        modules: &mut dyn Modules,
        clock: &dyn Clock,
        fresh: &mut dyn Fresh,
        promotion: &Promotion<'_>,
    ) -> Result<String, PromoteRefused> {
        let staged = modules
            .staged(promotion.provider)
            .ok_or(PromoteRefused::NothingStaged)?;
        if staged != promotion.version.as_str() {
            return Err(PromoteRefused::NotTheStagedVersion { staged });
        }
        let action = Action::ChangeCatalogue {
            change: CatalogueChange::Promote,
            provider: promotion.provider.clone(),
            version: Some(promotion.version.clone()),
            tuple: None,
            advertising_keys: Vec::new(),
            statement_key: None,
        };
        let request = DecisionRequest {
            request: fresh.id(),
            caller: promotion.caller.clone(),
            action: action.clone(),
            at: now(clock)?,
        };
        let decision = decisions.decide(&request)?;
        let Some(permit) = decision.permit() else {
            return Err(PromoteRefused::NotPermitted(decision.outcome));
        };
        let receipt = journal::append(
            log,
            journal::verified(
                promotion.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &now(clock)?,
                Outcome::ToTakeEffect {},
            ),
        )?;
        let effect = Effect {
            action,
            decision: permit,
            receipt,
        };
        // Compared again right before the platform is asked, which puts in force whatever is staged.
        let staged_now = modules.staged(promotion.provider);
        let promoted = match staged_now.as_deref() == Some(promotion.version.as_str()) {
            true => carry_promotion(modules, &effect, &request.caller.acts, clock),
            false => Err(PromoteRefused::NotTheStagedVersion {
                staged: staged_now.unwrap_or_default(),
            }),
        };
        let outcome = match promoted {
            Ok(_) => Outcome::Done {},
            Err(_) => Outcome::Failed {},
        };
        journal::append(
            log,
            journal::verified(
                promotion.core,
                What::Result {
                    effect: effect.receipt.clone(),
                },
                &now(clock)?,
                outcome,
            ),
        )?;
        if promoted.is_ok() {
            self.acted.insert(key(&effect.receipt));
        }
        promoted
    }

    fn flight(&self, at: &(String, String)) -> Result<&InFlight, SegmentsError> {
        self.flights
            .get(at)
            .ok_or(SegmentsError::Plan("the segment is in flight"))
    }

    fn flight_mut(&mut self, at: &(String, String)) -> Result<&mut InFlight, SegmentsError> {
        self.flights
            .get_mut(at)
            .ok_or(SegmentsError::Plan("the segment is in flight"))
    }

    /// Marks a segment's outcome final.
    fn finish(
        &mut self,
        at: &(String, String),
        outcome: &SegmentOutcome,
    ) -> Result<(), SegmentsError> {
        self.flight_mut(at)?.outcome = Some(outcome.clone());
        Ok(())
    }

    /// Appends the segment's step `name`, built already, unless its receipt is held.
    fn step(
        &mut self,
        log: &mut dyn Log,
        at: &(String, String),
        name: &'static str,
    ) -> Result<Receipt, SegmentsError> {
        let step = self
            .flight_mut(at)?
            .steps
            .get_mut(name)
            .ok_or(SegmentsError::Plan("a step is built before it is appended"))?;
        if let Some(receipt) = &step.receipt {
            return Ok(receipt.clone());
        }
        let receipt = journal::append_general(log, &step.entry)?;
        step.receipt = Some(receipt.clone());
        Ok(receipt)
    }

    /// Builds the segment's step `name` once, with `build`, and appends it. True with the receipt where this call
    /// is the one that got it.
    fn built_step(
        &mut self,
        log: &mut dyn Log,
        at: &(String, String),
        name: &'static str,
        build: impl FnOnce() -> Result<Entry, SegmentsError>,
    ) -> Result<(Receipt, bool), SegmentsError> {
        let flight = self.flight_mut(at)?;
        if let Some(Step {
            receipt: Some(receipt),
            ..
        }) = flight.steps.get(name)
        {
            return Ok((receipt.clone(), false));
        }
        if !flight.steps.contains_key(name) {
            let entry = build()?;
            entry.check().map_err(AppendFailure::Refused)?;
            let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
            flight.steps.insert(
                name,
                Step {
                    entry,
                    receipt: None,
                },
            );
        }
        Ok((self.step(log, at, name)?, true))
    }

    /// The result's stated entry, its bytes kept once.
    fn stated(
        &mut self,
        parts: &mut Parts<'_>,
        at: &(String, String),
        bytes: &[u8],
    ) -> Result<Receipt, Unappended> {
        let flight = self.flights.get_mut(at).ok_or(Unappended::Unrecordable)?;
        if !flight.steps.contains_key("stated") {
            let about = About {
                order: Some(id_of(&at.0).map_err(|_| Unappended::Unrecordable)?),
                segment: Some(id_of(&at.1).map_err(|_| Unappended::Unrecordable)?),
                transaction: Some(flight.transaction.clone()),
            };
            let received_at = now(parts.clock).map_err(Unappended::Append)?;
            let entry = journal::stated_entry(
                parts.evidence,
                &received_at,
                bytes,
                maker(parts.catalogue, bytes),
                about,
            )?;
            entry
                .check()
                .map_err(|v| Unappended::Append(AppendFailure::Refused(v)))?;
            let entry = GeneralEntry::try_from(entry)
                .map_err(|v| Unappended::Append(AppendFailure::Refused(v)))?;
            flight.steps.insert(
                "stated",
                Step {
                    entry,
                    receipt: None,
                },
            );
        }
        self.step(parts.log, at, "stated").map_err(|e| match e {
            SegmentsError::Append(failure) => Unappended::Append(failure),
            _ => Unappended::Unrecordable,
        })
    }

    /// A fresh transaction: never one the rounds asked under, nor one a segment ran under.
    fn fresh_transaction(&mut self, parts: &mut Parts<'_>) -> Result<Id, SegmentsError> {
        for _ in 0..DRAWS {
            let id = parts.fresh.id();
            if !parts.issued.contains(id.as_str())
                && Identifier::new(id.as_str()).is_some()
                && self.transactions.insert(id.as_str().to_owned())
            {
                return Ok(id);
            }
        }
        Err(SegmentsError::NoFreshTransaction)
    }

    /// Keeps one drain whole, to append; an empty one is nothing to keep.
    fn keep_drain(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        at: &Timestamp,
        raw: drain::Raw,
    ) -> Option<Id> {
        if raw.is_empty() {
            return None;
        }
        let id = parts.fresh.id();
        self.undelivered.push(Drain {
            id: id.clone(),
            at: at.clone(),
            who: live.core.clone(),
            raw,
            appended: 0,
        });
        Some(id)
    }

    /// Appends the drains kept. Another segment's drain that fails holds nothing up here, and the log not taking one
    /// now holds nothing up either: they wait, kept. One of this segment's own that the contract can't carry, or the
    /// log refuses, is answered as the error it is.
    fn deliver(&mut self, log: &mut dyn Log, at: &(String, String)) -> Result<(), SegmentsError> {
        let own = self.flight(at)?.drains.clone();
        for (id, failure) in self.flush_all(log) {
            let unavailable = matches!(failure, SegmentsError::Append(AppendFailure::Unavailable));
            if own.contains(&id) && !unavailable {
                return Err(failure);
            }
        }
        Ok(())
    }

    /// Ends a segment on the platform, once, keeps its record to hand over, hands it over, and ends its calls.
    fn end(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        at: &(String, String),
    ) -> Result<(), SegmentsError> {
        let flight = self.flight(at)?;
        if !flight.ended {
            let transaction = Identifier::new(flight.transaction.as_str())
                .ok_or(SegmentsError::Plan("a transaction is an identifier"))?;
            let record = parts.modules.end_segment(&transaction);
            parts.modules.end_calls(&transaction);
            self.flight_mut(at)?.ended = true;
            if let Some(record) = record {
                let drained_at = now(parts.clock)?;
                let kept = self.keep_drain(parts, live, &drained_at, drain::Raw::Record(record));
                self.flight_mut(at)?.record = kept;
            }
        }
        // Its record reaches the log before its result: the result waits for it, and for nothing else.
        let record = self.flight(at)?.record.clone();
        let failed = self.flush_all(parts.log);
        match failed
            .into_iter()
            .find(|(id, _)| Some(id) == record.as_ref())
        {
            Some((_, failure)) => Err(failure),
            None => Ok(()),
        }
    }

    /// The effect's one `result` entry.
    fn result(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        at: &(String, String),
        outcome: Outcome,
    ) -> Result<Receipt, SegmentsError> {
        let effect = self
            .flight(at)?
            .effect
            .as_ref()
            .map(|e| e.receipt.clone())
            .ok_or(SegmentsError::Plan("a result follows its effect"))?;
        let when = now(parts.clock)?;
        // The step is the outcome's own: an entry built for one outcome is never appended for another.
        let step = match outcome {
            Outcome::Done {} => "result: done",
            Outcome::Failed {} => "result: failed",
            Outcome::Unanswered {} => "result: unanswered",
            _ => {
                return Err(SegmentsError::Plan(
                    "an effect's result is done, failed or unanswered",
                ));
            }
        };
        let (receipt, _) = self.built_step(parts.log, at, step, || {
            Ok(journal::verified(
                live.core,
                What::Result { effect },
                &when,
                outcome,
            ))
        })?;
        Ok(receipt)
    }

    /// The segment's declared and actual settlement times (`REQ-QTE-3`).
    fn settlement(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        at: &(String, String),
        declared: u64,
    ) -> Result<Receipt, SegmentsError> {
        let flight = self.flight(at)?;
        let started = flight
            .started
            .ok_or(SegmentsError::Plan("a settled segment started"))?;
        let provider = flight.provider.clone();
        let segment = id_of(&at.1)?;
        let ended = parts.clock.now();
        let when = now(parts.clock)?;
        let actual = ended.ceil().0.saturating_sub(started.floor().0);
        let seconds = |s: u64| {
            SafeInteger::try_from(s)
                .map_err(|_| SegmentsError::Plan("a settlement time is a safe integer"))
        };
        let (declared, actual) = (seconds(declared)?, seconds(actual)?);
        let (receipt, _) = self.built_step(parts.log, at, "settlement", || {
            Ok(journal::verified(
                live.core,
                What::Settlement {
                    order: live.order.clone(),
                    segment,
                    provider,
                    declared_seconds: declared,
                    actual_seconds: actual,
                },
                &when,
                Outcome::Observed {},
            ))
        })?;
        Ok(receipt)
    }

    /// Appends one state-machine input about the part, or one of its segments, as the segment's step `name`, and
    /// moves the order Core follows, once.
    fn input(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        at: &(String, String),
        name: &'static str,
        segment: Option<&Id>,
        observed: Observed,
    ) -> Result<Receipt, SegmentsError> {
        let reading = parts.clock.now();
        let when = reading
            .timestamp()
            .ok_or(AppendFailure::Refused(journal::NO_TIME))?;
        let record_id = parts.fresh.id();
        let mut built = None;
        let (receipt, now_appended) = self.built_step(parts.log, at, name, || {
            let state_id = |text: &str| stm::Id::new(text).ok_or(SegmentsError::Plan("an id"));
            let standing = match observed {
                Observed::ProviderBound {} => Standing::Verified,
                _ => Standing::Stated,
            };
            let input = RecordedInput {
                record_id: stm::RecordId::new(record_id.as_str())
                    .ok_or(SegmentsError::Plan("a record id"))?,
                recorded_at: stm::Timestamp::new(when.as_str())
                    .ok_or(AppendFailure::Refused(journal::NO_TIME))?,
                time_source: state_id("core_clock")?,
                source: Source {
                    kind: SourceKind::Orchestrator,
                    id: state_id(live.core.acts.as_str())?,
                },
                standing,
                subject: Subject {
                    order: state_id(live.order.as_str())?,
                    part: Some(state_id(live.part.as_str())?),
                    segment: segment.map(|s| state_id(s.as_str())).transpose()?,
                },
                observed,
            };
            built = Some(input.clone());
            Ok(journal::verified(
                live.core,
                What::Input { input },
                &when,
                Outcome::Observed {},
            ))
        })?;
        if now_appended {
            let input = match built {
                Some(input) => input,
                None => match self.flight(at)?.steps[name].entry.entry() {
                    Entry::Verified(VerifiedEntry {
                        what: What::Input { input },
                        ..
                    }) => input.clone(),
                    _ => return Ok(receipt),
                },
            };
            if let Some(tracked) = self.orders.get_mut(live.order.as_str()) {
                let next = Stm::transition(&tracked.order, &input);
                let moved = next.state != tracked.order.state
                    || next
                        .parts
                        .iter()
                        .zip(&tracked.order.parts)
                        .any(|(a, b)| a.state != b.state);
                tracked.order = next;
                if moved {
                    tracked.since = reading;
                }
            }
        }
        Ok(receipt)
    }
}

/// The effect its entry recorded, on the permit it rests on, at its entry's receipt.
fn effect_of(action: Action, decision: Permit, receipt: Receipt) -> Effect {
    Effect {
        action,
        decision,
        receipt,
    }
}

/// Puts in force the staged release `effect` permits, by `by`, the permitting decision's caller.
fn carry_promotion(
    modules: &mut dyn Modules,
    effect: &Effect,
    by: &Id,
    clock: &dyn Clock,
) -> Result<String, PromoteRefused> {
    let Action::ChangeCatalogue { provider, .. } = &effect.action else {
        return Err(PromoteRefused::NothingStaged);
    };
    modules
        .promote(effect, provider, by, clock.now().floor())
        .map_err(PromoteRefused::Lifecycle)
}

/// The route's operation as it is held in flight: at the amount the segment moves, and, where it changes asset, what
/// it gives out at that amount, by the route's own rate, rounded up so the hold never counts less.
fn held_at(
    operation: &rutent_contracts::router_api::Operation,
    moved: &ProviderOperation,
) -> Option<rutent_contracts::router_api::Operation> {
    let amount = match moved {
        ProviderOperation::Withdrawal(o) => &o.amount,
        ProviderOperation::Deposit(o) => &o.amount,
        ProviderOperation::Transfer(o) => &o.amount,
        ProviderOperation::Conversion(o) => &o.amount,
        _ => return None,
    };
    let mut held = operation.clone();
    held.amount_in = PositiveAmount::new(Amount::new(amount.clone())?)?;
    if operation.to.asset != operation.from.asset {
        let whole = |a: &str| a.parse::<u128>().ok();
        let quoted_in = whole(operation.amount_in.amount().as_str())?;
        let quoted_out = whole(operation.amount_out.as_str())?;
        let out = quoted_out
            .checked_mul(whole(amount)?)?
            .checked_add(quoted_in - 1)?
            .checked_div(quoted_in)?;
        held.amount_out = Amount::new(out.to_string())?;
    } else {
        held.amount_out = Amount::new(amount.clone())?;
    }
    Some(held)
}

/// The maker a result's bytes claim, as its stated entry names it: its provider, for its publisher.
fn maker(catalogue: &Catalogue, bytes: &[u8]) -> Actor {
    let acts = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v["maker"].as_str().and_then(Id::new))
        .unwrap_or_else(|| Id::new("unknown").expect("an id"));
    let party = match catalogue.publishers().publisher(acts.as_str()) {
        Some(Publisher::Operator) => Id::new("operator"),
        Some(Publisher::Party(publisher)) => Id::new(publisher),
        None => None,
    }
    .unwrap_or_else(|| acts.clone());
    Actor {
        party,
        acts,
        role: Role::Provider,
        kind: ActorKind::Component,
    }
}

fn id_of(text: &str) -> Result<Id, SegmentsError> {
    Id::new(text).ok_or(SegmentsError::Plan("an id"))
}

/// A receipt as a key: its log, its place and its digest.
fn key(receipt: &Receipt) -> String {
    serde_json::to_string(receipt).unwrap_or_default()
}

/// Core's clock, as a timestamp.
fn now(clock: &dyn Clock) -> Result<Timestamp, AppendFailure> {
    clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(journal::NO_TIME))
}
