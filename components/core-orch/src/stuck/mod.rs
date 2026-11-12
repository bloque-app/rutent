//! Stuck orders: with no route left the order is stuck and its processing recorded.
//!
//! Declared by `T-020-01`; `T-020-11` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! [`Stuck`] is Core's record of the parts it declared stuck, which the production assembly keeps durably
//! (`F-030`), as it does [`Segments`]. Five things reach it:
//!
//! 1. **No route left** ([`Stuck::no_route`], `REQ-ORD-28`). A recalculation that leaves the part no route
//!    ([`Recalculation::NoRouteLeft`], `NoRoute` or `Rejected`, `T-020-10`) is recorded as the state machine's
//!    `no_route` input, citing the decision record it rests on; nothing else records one. The state machine places
//!    the part ([`after_no_route`](rutent_contracts::state_machine::after_no_route)): stuck, or waiting for the
//!    customer where the value sits at the node a failed final deposit left it at (`REQ-ORD-43`). Nothing is rolled
//!    back: no reversal is asked, no hold released and nothing handed over, and a stuck part is never handed over
//!    again ([`SegmentsError::Stuck`](crate::segments::SegmentsError::Stuck)) nor recalculated
//!    ([`NotRecalculable::Held`](crate::recalc::NotRecalculable::Held)). The value resting at the node is held
//!    there ([`Exposures::hold_resting`]), so the ceilings in flight count it.
//! 2. **Over time** ([`Stuck::watch`], `REQ-ORD-29`). A segment carried out with no final outcome (no result that
//!    counts, an outcome unknown, or the person to act) is timed from when it was carried out. Past twice the p99
//!    of its tuple's latest [`WINDOW`] settlements, or twice its advertised time until the tuple has [`HISTORY`]
//!    settlements, the figures are
//!    recorded (`stuck_call`, `REQ-ORD-6`) and then the part declared stuck (`stuck_declared`, `over_time`), and the
//!    declaration is handed back as the operator's warning ([`StuckCalled`]). Exactly twice is not over.
//! 3. **A material change in a provider's p99** ([`Stuck::learn`], `REQ-ORD-6`, `THR-24`). Each settled
//!    segment's actual time joins its tuple's history once. The first p99, formed at [`HISTORY`] settlements, is
//!    compared with the advertised time; each later one, over the latest [`WINDOW`], with the one last raised or
//!    first formed, so it can fall as well as rise. A move of [`MATERIAL_BPS`] of what it is compared with, either
//!    way, is raised to the operator (`Operator::raise`, a `settlement_time` difference citing the settlements it
//!    rests on), and the raise is recorded with whether it was delivered. Detection never holds up the clock.
//! 4. **Seen by value and age** ([`Stuck::stuck_values`], `REQ-OPS-2`): every stuck order's value in each asset,
//!    what its parts hold in flight or at rest, less what resolutions moved, ranked largest first within each asset,
//!    then oldest. The customer reads its order's state through [`Segments::status`]; the
//!    provider reads its own segment's through [`seen_by_provider`], each on a read policy allows
//!    (`REQ-ORD-6`).
//! 5. **Resolved in two steps** ([`Stuck::decide`], [`Stuck::carry_out`], `REQ-AUZ-4`, ADR-0011). One actor
//!    decides the outcome and names its evidence; policy decides that step. Another actor, of another accountable
//!    party, moves the value; policy decides that step too, and only its permit backs the `resolve_stuck` effect.
//!    Core holds both rules itself whatever policy answers: it carries out a move only for a decision it holds,
//!    permitted and answering its own request, of a part it declared stuck and that is still unresolved, and only
//!    where the mover is neither of the decider's party nor its login. A resolution moves no more than the part
//!    still holds, through a provider of the part, to the order's destination or back to its origin, both read
//!    from Core's records ([`Stuck::follow`]), never from the caller. One move of a part at a time: while one is in
//!    progress, or its outcome unknown, nothing more is decided or moved for the part, and a move is finished only
//!    by its own mover. Each decision is spent by the move that carried it out, whatever its outcome. The effect is
//!    acted on once for its receipt, in the one register the segments keep too (`INV-8`), and only on a receipt
//!    that is its own; the part is resolved once all its value moved.
//!
//! **Gaps, each named in `F-020`'s notes.** Policy's `StuckOrders` is unbuilt (`T-021-06`): it answers not known,
//! so nothing moves until it is built, and it holds no rule comparing the two actors yet, nor the roles a decider
//! and a mover may hold. state-machine 2.1.0 has no input that ends a part a resolution delivered, so the part stays
//! `stuck` there and Core holds it resolved (`T-020-01f` is adding one). The order's authorisation and its payer's
//! account reach [`Stuck::follow`] from the assembly until policy registers authorisations (`T-021-04`). No row
//! owns the production [`Resolutions`] yet: `hand_on` is `T-020-13`'s, and `deliver` needs a new row.

mod journal;
mod timing;
mod view;

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::{
    AppendFailure, Delivery, Operator, Reads, StuckOrders,
};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, Decided, Decision, Destination, Difference, Effect, Entry, Outcome,
    Permit, Raise, ReadCheck, ReadOutcome, Receipt, Resolution, StuckBasis, StuckDecide, StuckMove,
    What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    Denomination, NodeRef, Operation as ProviderOperation, OuterNode, SafeInteger,
};
use rutent_contracts::router_api::{
    Amount, DecisionRecord, Endpoint, Id, OperationKind, Timestamp,
};
use rutent_contracts::state_machine::{
    self as stm, Observed, OrderState, RecordedInput, SegmentState, Source, SourceKind, Standing,
    StateMachine, StuckReason, Subject,
};
use rutent_router::quote::declared_times;
use rutent_stm::Stm;

use crate::recalc::Recalculation;
use crate::rounds::Fresh;
use crate::segments::{Segments, StatusError};
use crate::snapshot::{Clock, Exposures, HoldError, OrchestrationLog, Reading};

pub use journal::{APPEND_TRIES, is_receipt_of};
use journal::{NO_TIME, Step};
pub use timing::{Change, HISTORY, History, MATERIAL_BPS, WINDOW, material, over_time, p99};
pub use view::{StuckValue, rank};

/// What the stuck orders reach. The assembly builds it; Core's clock, segments and exposures are those the rest of
/// Core uses.
pub struct Parts<'a> {
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Orchestration's handle on the log.
    pub log: &'a mut dyn OrchestrationLog,
    /// Fresh identifiers: records, raises and requests.
    pub fresh: &'a mut dyn Fresh,
    /// The segments Core handed over, and the orders it follows.
    pub segments: &'a mut Segments,
    /// Core's record of value in flight: a stuck order is marked there, and a resolution releases what it held.
    pub exposures: &'a mut Exposures,
}

/// A part a recalculation left with no route, to record (`REQ-ORD-28`).
#[derive(Debug, Clone, Copy)]
pub struct NoRouteLeft<'a> {
    /// The order.
    pub order: &'a Id,
    /// The part.
    pub part: &'a Id,
    /// What the recalculation answered: only `NoRouteLeft`, `NoRoute` and `Rejected` leave no route.
    pub outcome: &'a Recalculation,
    /// The order's last decision record, which the input cites where the outcome names none (past the bound,
    /// nothing was routed): the one the part's route rests on.
    pub last_record: &'a Id,
    /// Core, acting on its own account: who writes the input.
    pub core: &'a Actor,
}

/// A part to time (`REQ-ORD-29`): the route it runs, as [`Live`](crate::segments::Live) names it.
#[derive(Debug, Clone, Copy)]
pub struct Watching<'a> {
    /// The order.
    pub order: &'a Id,
    /// The part.
    pub part: &'a Id,
    /// The id Core gave each of the part's segments, in the route's order.
    pub segments: &'a [Id],
    /// The router's recorded decision that chose the route: its advertised times.
    pub record: &'a DecisionRecord,
    /// The part's place in the chosen candidate.
    pub at: usize,
    /// Core, acting on its own account: who writes the records.
    pub core: &'a Actor,
}

/// A part declared stuck over time: the warning the operator is handed, with the figures behind it
/// (`REQ-ORD-29`, `REQ-ORD-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StuckCalled {
    /// The order.
    pub order: Id,
    /// The part.
    pub part: Id,
    /// The segment whose funds took too long.
    pub segment: Id,
    /// Its provider, which holds the funds.
    pub provider: Id,
    /// What the clock ran on.
    pub basis: StuckBasis,
    /// The figure it ran on, in seconds: the p99, or the advertised time.
    pub figure_seconds: u64,
    /// How long the funds had been held, in seconds.
    pub held_seconds: u64,
    /// The receipt of the `stuck_call` entry.
    pub call: Receipt,
    /// The receipt of the `stuck_declared` input.
    pub input: Receipt,
}

/// What timing a part found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
    /// No segment of the part is over its time.
    Running,
    /// The part was declared stuck now: the warning.
    Stuck(Box<StuckCalled>),
    /// The part was stuck already.
    AlreadyStuck,
}

/// A part's `no_route` recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoRouteRecorded {
    /// The state the part went to: stuck, or waiting for the customer (`REQ-ORD-43`).
    pub state: OrderState,
    /// The receipt of the input's entry.
    pub receipt: Receipt,
}

/// A material change in a tuple's p99 raised to the operator (`REQ-ORD-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The raise.
    pub raise: Raise,
    /// Whether it reached the operator.
    pub delivered: bool,
    /// The receipt of its entry.
    pub receipt: Receipt,
}

/// Why something stuck couldn't be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StuckError {
    /// The recalculation's answer leaves a route: nothing is recorded.
    RouteLeft,
    /// Core follows no such order, or the order has no such part.
    Unknown,
    /// The part is stuck already.
    AlreadyStuck,
    /// The state machine wouldn't take the input from the part as it stands: nothing is recorded.
    NotApplicable(stm::Outcome),
    /// The record it would rest on isn't one the route names.
    Plan(&'static str),
    /// The log refused or failed an append. One the log can't take now is kept, and the next call appends it as
    /// built.
    Append(AppendFailure),
    /// An identifier Core holds isn't one the state machine's input can carry.
    Id(&'static str),
    /// The authorisation isn't the order's as Core follows it: another destination, or another origin reserve, or
    /// an order whose first segment isn't a withdrawal from an outer node.
    NotItsAuthorisation,
}

impl From<AppendFailure> for StuckError {
    fn from(failure: AppendFailure) -> Self {
        StuckError::Append(failure)
    }
}

/// A decide step to ask (`REQ-AUZ-4`).
#[derive(Debug, Clone, Copy)]
pub struct Deciding<'a> {
    /// The stuck order.
    pub order: &'a Id,
    /// Its stuck part.
    pub part: &'a Id,
    /// Who decides.
    pub decider: &'a Actor,
    /// The outcome decided.
    pub resolution: &'a Resolution,
    /// The evidence relied on, by digest in the evidence store: at least one, each once.
    pub evidence: &'a [Digest],
}

/// A move's outcome, once unknown, as reported and confirmed by two actors, each naming its evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettledOutcome {
    /// The decision the move carried out.
    pub decision: Decided,
    /// The outcome recorded.
    pub outcome: Resolved,
    /// Who reported it: the move's mover.
    pub reported_by: Actor,
    /// The evidence the report named.
    pub reported_with: Id,
    /// Who confirmed it.
    pub confirmed_by: Actor,
    /// The evidence the confirmation named.
    pub confirmed_with: Id,
}

/// A reported outcome to confirm (`REQ-AUZ-4`).
#[derive(Debug, Clone, Copy)]
pub struct Confirming<'a> {
    /// The stuck order.
    pub order: &'a Id,
    /// The decide step's decision the move carried out.
    pub carries_out: &'a Decided,
    /// Who confirms the outcome: of another party and login than the mover.
    pub confirmer: &'a Actor,
    /// The outcome the confirmer read in the report it confirms.
    pub outcome: Resolved,
    /// The version of the report it confirms ([`Moved::AwaitsConfirmation`]).
    pub report: u64,
    /// The evidence the report it confirms named.
    pub reported_with: &'a Id,
    /// The evidence the confirmation relies on, by an opaque reference: the provider's stated result for the
    /// effect's transaction, say.
    pub evidence: &'a Id,
    /// Core, which writes the result.
    pub core: &'a Actor,
}

/// A move step to carry out (`REQ-AUZ-4`).
#[derive(Debug, Clone, Copy)]
pub struct Moving<'a> {
    /// The stuck order.
    pub order: &'a Id,
    /// The decide step's decision it carries out.
    pub carries_out: &'a Decided,
    /// Who moves the value.
    pub mover: &'a Actor,
    /// Core, which writes the effect and its result.
    pub core: &'a Actor,
}

/// Why a step of a resolution was refused before anything moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Core declared no such part stuck, or the part isn't stuck in the order Core follows.
    NotStuck,
    /// The part was resolved already: its value moved on a resolution.
    Resolved,
    /// The decision names no evidence, or names an item twice.
    NoEvidence,
    /// The resolution delivers elsewhere than the order's destination or its origin.
    ElsewhereThanItsEnds,
    /// Core holds no destination and origin for the order ([`Stuck::follow`]): nothing is decided for it.
    NoEnds,
    /// The resolution moves another asset, more than the part still holds, or through a provider that isn't the
    /// part's.
    NotItsValue,
    /// A move of the part is in progress: nothing more is decided or moved for it until it ends.
    MoveInProgress,
    /// A move of the part ended with its outcome unknown: the part stays held, and nothing more is decided or moved
    /// for it until the outcome is known.
    OutcomeUnknown,
    /// Policy's answer is to another request than the one Core sent: it is kept for nothing.
    NotItsAnswer,
    /// The move names no decide step Core holds permitted for this order: one never asked, refused, not known, or
    /// of another order (`REQ-AUZ-4`).
    NoDecision,
    /// The mover acts for the decider's accountable party: one party would both decide and move (`REQ-AUZ-4`,
    /// ADR-0011).
    SameParty,
    /// The mover is the decider's own login, under another stated party: one actor would both decide and move
    /// (`REQ-AUZ-4`).
    SameLogin,
    /// A move in progress is taken on again only by the mover policy permitted it to (`REQ-AUZ-4`).
    NotTheMover,
    /// A confirmation names another report than the mover's current one: another outcome, version or evidence.
    /// Nothing is recorded.
    NotTheReport,
    /// The effect was acted on already (`INV-8`).
    AlreadyActed(Receipt),
    /// The log or policy refused or failed an append. Nothing moved; where an effect was built, it is kept, and the
    /// next call goes on from it.
    Append(AppendFailure),
}

impl From<AppendFailure> for ResolveError {
    fn from(failure: AppendFailure) -> Self {
        ResolveError::Append(failure)
    }
}

/// How a resolution's move was carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    /// The value moved as the decision named.
    Done,
    /// It didn't: nothing moved, and the part's value stays held. Its decision is spent.
    Failed,
    /// Whether it moved is unknown: the part stays held, its decision spent, and nothing more is decided or moved
    /// for it until the outcome is known. No result is recorded yet.
    Unknown,
}

/// What a move step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Moved {
    /// Policy didn't permit it: nothing was recorded as an effect, and nothing moved.
    NotPermitted(Answer),
    /// Its mover reported the outcome of a move of unknown outcome: nothing is recorded and the part stays held until
    /// an actor of another party and login confirms it ([`Stuck::confirm_outcome`]), naming this report.
    AwaitsConfirmation {
        /// The report's version: each report of the move counts one more, and a confirmation names the one it
        /// confirms.
        report: u64,
    },
    /// The effect was recorded and carried out.
    CarriedOut {
        /// The receipt of the `resolve_stuck` effect's entry.
        effect: Receipt,
        /// How it went.
        resolved: Resolved,
        /// The receipt of its `result` entry; none while its outcome is unknown.
        result: Option<Receipt>,
    },
}

/// What carries a stuck order's resolution out: the value moved as the effect's action names it, by the provider it
/// names. It holds the [`Effect`] it carries out, whose entry the log admitted only on a stuck move's permit
/// (`REQ-AUZ-1`, `REQ-AUZ-4`).
pub trait Resolutions {
    /// Moves the value `effect` resolves.
    fn resolve(&mut self, effect: &Effect) -> Resolved;
}

/// No way to carry a resolution out yet: each fails, so nothing moves and the part's value stays held. The
/// production one, which writes the move to the platform, has no row yet: `hand_on` is `T-020-13`'s, `deliver` a
/// new row's.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbuilt;

impl Resolutions for Unbuilt {
    fn resolve(&mut self, _effect: &Effect) -> Resolved {
        Resolved::Failed
    }
}

/// What the provider of a segment of a stuck order reads of it (`REQ-ORD-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderView {
    /// The order.
    pub order: Id,
    /// The part the segment is of.
    pub part: Id,
    /// The segment.
    pub segment: Id,
    /// The part's state.
    pub part_state: OrderState,
    /// The segment's state.
    pub segment_state: SegmentState,
}

/// A part Core declared stuck, or left waiting by `no_route`.
#[derive(Debug, Clone)]
struct Declared {
    state: OrderState,
    since: Reading,
    /// The part's segments, and the providers they run at.
    segments: Vec<(Id, Id)>,
    /// What its resolutions moved so far, by asset.
    moved: Vec<(Denomination, Amount)>,
    /// The move of the part in progress, by the request its decision answered.
    in_motion: Option<String>,
    resolved: bool,
}

/// The figures behind a call over time, before the log gave their receipts.
#[derive(Debug, Clone)]
struct Call {
    segment: Id,
    provider: Id,
    basis: StuckBasis,
    figure_seconds: u64,
    held_seconds: u64,
}

/// A part's declaration built, kept until the log takes it.
#[derive(Debug, Clone)]
struct Pending {
    call: Option<(Step, Call)>,
    input: Step,
    recorded: RecordedInput,
}

/// A decide step policy permitted, kept for its move.
#[derive(Debug, Clone)]
struct Kept {
    request: StuckDecide,
    part: Id,
    decision: Id,
}

/// An order's two ends, from Core's records: the destination its authorisation names, and the origin its
/// withdrawal took the value from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ends {
    destination: Destination,
    origin_reserve: Id,
    origin_node: OuterNode,
    /// The order's withdrawal, whose signed operation names the origin's accounts.
    withdrawal: Id,
}

impl Ends {
    /// Whether a resolution may deliver to `to`: the destination exactly, or the origin's reserve and node at an
    /// account the withdrawal Core signed named. A withdrawal Core never signed names none.
    fn takes(&self, segments: &Segments, order: &Id, to: &Destination) -> bool {
        if *to == self.destination {
            return true;
        }
        to.reserve == self.origin_reserve
            && to.node == self.origin_node
            && segments
                .sent(order, &self.withdrawal)
                .and_then(withdrawal_inputs)
                .is_some_and(|inputs| names_the_payer(inputs, to.account.as_str()))
    }
}

/// The input a withdrawal names the payer's account by, the account the value came from: the name provider-role's
/// fixtures and the simulated providers use. provider-role names no input itself (a tuple's terms list the ones
/// its operations require), so a contract-named key for it would be a provider-role item.
pub const PAYER_ACCOUNT: &str = "payer.bank_account";

/// Whether `account` is the payer's account a withdrawal's `inputs` name: only that input, never another account
/// the withdrawal carries, a beneficiary's say.
fn names_the_payer(inputs: &std::collections::BTreeMap<String, String>, account: &str) -> bool {
    inputs.get(PAYER_ACCOUNT).is_some_and(|a| a == account)
}

/// The inputs a withdrawal Core signed carries; any other operation names no origin account.
fn withdrawal_inputs(
    operation: &ProviderOperation,
) -> Option<&std::collections::BTreeMap<String, String>> {
    match operation {
        ProviderOperation::Withdrawal(o) => Some(&o.inputs),
        _ => None,
    }
}

/// A move from its permit until its result is recorded.
#[derive(Debug, Clone)]
struct InMotion {
    order: Id,
    part: Id,
    /// Who moves it: only that mover takes the move on again.
    mover: Actor,
    /// The outcome its mover reported, after it was unknown, with the evidence named and the report's version:
    /// recorded, the part resolved or freed, only once another actor confirms that very report. A later report
    /// replaces it.
    reported: Option<(Resolved, Id, u64)>,
    /// How many reports its mover made.
    reports: u64,
    /// Why the move was dropped before it acted, its effect recorded: its result is recorded `failed`, since it
    /// never took effect, and then the move answers this.
    dropped: Option<ResolveError>,
    effect: Step,
    action: Action,
    permit: Permit,
    resolved: Option<Resolved>,
    result: Option<Step>,
}

/// The settlement history, by tuple, and the baselines its p99 changes are measured from.
#[derive(Debug, Clone, Default)]
struct Timings {
    seen: BTreeSet<(String, String)>,
    /// Each tuple's settlement history.
    tuples: BTreeMap<String, History>,
    /// The settlement each tuple's baseline rests on.
    baseline: BTreeMap<String, Receipt>,
    /// Raises delivered whose entry the log hasn't taken yet: appended as built, never raised again.
    raises: Vec<(Step, Raise, bool)>,
}

/// Core's record of stuck orders, kept across orders by the assembly, which the production assembly keeps durably
/// (`F-030`), as it does [`Segments`].
#[derive(Debug, Clone, Default)]
pub struct Stuck {
    declared: BTreeMap<(String, String), Declared>,
    pending: BTreeMap<(String, String), Pending>,
    timings: Timings,
    kept: BTreeMap<String, Kept>,
    moving: BTreeMap<String, InMotion>,
    /// Each order's destination and origin, from Core's records ([`Stuck::follow`]).
    ends: BTreeMap<String, Ends>,
    /// Each move of unknown outcome since confirmed, by the request its decision answered.
    outcomes: BTreeMap<String, SettledOutcome>,
}

impl Stuck {
    /// Nothing declared yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether Core declared `part` of `order` stuck and it is still unresolved.
    pub fn is_stuck(&self, order: &Id, part: &Id) -> bool {
        self.declared
            .get(&key(order, part))
            .is_some_and(|d| d.state == OrderState::Stuck && !d.resolved)
    }

    /// Records the `no_route` input of a part a recalculation left no route (see the module's docs). Built once and
    /// appended until the log takes it: a call that finds one kept appends it as built, once the part still takes
    /// it.
    pub fn no_route(
        &mut self,
        parts: &mut Parts<'_>,
        ask: NoRouteLeft<'_>,
    ) -> Result<NoRouteRecorded, StuckError> {
        let at = key(ask.order, ask.part);
        if self.pending.get(&at).is_some_and(|p| p.call.is_some()) {
            return Err(StuckError::AlreadyStuck);
        }
        if !self.pending.contains_key(&at) {
            let record = match ask.outcome {
                Recalculation::NoRouteLeft { .. } => ask.last_record.clone(),
                Recalculation::NoRoute { record } | Recalculation::Rejected { record, .. } => {
                    record.record.record_id().clone()
                }
                _ => return Err(StuckError::RouteLeft),
            };
            let observed = Observed::NoRoute {
                decision_record: stm_id(record.as_str())?,
            };
            let input = Self::part_input(parts, ask.order, ask.part, ask.core, observed)?;
            let entry = journal::built(
                ask.core,
                What::Input {
                    input: input.clone(),
                },
                &now(parts.clock)?,
                Outcome::Observed {},
            )?;
            self.pending.insert(
                at.clone(),
                Pending {
                    call: None,
                    input: Step::new(entry),
                    recorded: input,
                },
            );
        }
        let receipt = self.record_pending(parts, &at)?;
        let state = self
            .declared
            .get(&at)
            .map_or(OrderState::Stuck, |d| d.state);
        Ok(NoRouteRecorded { state, receipt })
    }

    /// Times the part (see the module's docs): learns the settlement history first, raising each material change in
    /// a tuple's p99 to `operator`, then declares the part stuck at its first segment whose funds are over their
    /// time. A declaration the log couldn't take is appended as built on the next call.
    pub fn watch(
        &mut self,
        parts: &mut Parts<'_>,
        operator: &mut dyn Operator,
        watching: Watching<'_>,
    ) -> Result<Watch, StuckError> {
        // Detection never holds up the clock: a raise the log can't take now waits for the next call, and `learn`
        // answers its failure when called on its own (`REQ-ORD-9`'s "only detects").
        let _ = self.learn(parts, operator, watching.core);
        let at = key(watching.order, watching.part);
        match self.pending.get(&at).map(|p| p.call.is_some()) {
            Some(true) => return self.finish_watch(parts, &at),
            // A `no_route` the log hasn't taken yet is appended as built, never overwritten.
            Some(false) => {
                self.record_pending(parts, &at)?;
            }
            None => {}
        }
        if parts.segments.stuck(watching.order, watching.part) {
            return Ok(Watch::AlreadyStuck);
        }
        let chosen = watching
            .record
            .chosen()
            .ok_or(StuckError::Plan("the record chose no route"))?;
        let route = chosen
            .parts
            .get(watching.at)
            .ok_or(StuckError::Plan("the chosen route has no part there"))?;
        let advertised = declared_times(watching.record, chosen);
        let now = parts.clock.now();
        for (place, segment) in watching.segments.iter().enumerate() {
            let Some(since) = parts.segments.carried_since(watching.order, segment) else {
                continue;
            };
            let operation = route
                .operations
                .get(place)
                .ok_or(StuckError::Plan("the route has no operation there"))?;
            let tuple = tuple_key(&operation.provider, &operation.from, &operation.to);
            let declared = advertised
                .iter()
                .find(|t| t.part as usize == watching.at && t.operation as usize == place)
                .map_or(u64::MAX, |t| t.declared_seconds);
            let figure = match self.timings.tuples.get(&tuple) {
                Some(history) => history.figure(declared),
                None => History::new().figure(declared),
            };
            let Some((basis, figure)) = figure else {
                continue;
            };
            let held = now.floor().0.saturating_sub(since.floor().0);
            if !over_time(figure, held) {
                continue;
            }
            let when = now_at(now)?;
            let call = Call {
                segment: segment.clone(),
                provider: operation.provider.clone(),
                basis,
                figure_seconds: figure,
                held_seconds: held,
            };
            let call_entry = journal::built(
                watching.core,
                What::StuckCall {
                    order: watching.order.clone(),
                    part: watching.part.clone(),
                    provider: operation.provider.clone(),
                    basis,
                    figure_seconds: safe(figure)?,
                    held_seconds: safe(held)?,
                },
                &when,
                Outcome::Observed {},
            )?;
            let observed = Observed::StuckDeclared {
                reason: StuckReason::OverTime,
            };
            let input = Self::part_input(
                parts,
                watching.order,
                watching.part,
                watching.core,
                observed,
            )?;
            let input_entry = journal::built(
                watching.core,
                What::Input {
                    input: input.clone(),
                },
                &when,
                Outcome::Observed {},
            )?;
            self.pending.insert(
                at.clone(),
                Pending {
                    call: Some((Step::new(call_entry), call)),
                    input: Step::new(input_entry),
                    recorded: input,
                },
            );
            return self.finish_watch(parts, &at);
        }
        Ok(Watch::Running)
    }

    /// Learns each settled segment's time once, in the order the log took them, into its tuple's history, and
    /// raises to `operator` each material change in a tuple's p99 from its baseline (`REQ-ORD-6`). A raise whose
    /// entry the log couldn't take is appended as built on the next call, and never raised again.
    pub fn learn(
        &mut self,
        parts: &mut Parts<'_>,
        operator: &mut dyn Operator,
        core: &Actor,
    ) -> Result<Vec<Raised>, StuckError> {
        let mut raised = Vec::new();
        let mut settled = parts.segments.settlements();
        settled.sort_by_key(|s| u64::from(s.receipt.index));
        for s in settled {
            let seen = (s.order.as_str().to_owned(), s.segment.as_str().to_owned());
            if self.timings.seen.contains(&seen) {
                continue;
            }
            let Some(segment) = parts.segments.order(&s.order).and_then(|o| {
                o.parts
                    .iter()
                    .flat_map(|p| &p.segments)
                    .find(|g| g.segment_id.as_str() == s.segment.as_str())
                    .cloned()
            }) else {
                continue;
            };
            self.timings.seen.insert(seen);
            let tuple = tuple_key(&s.provider, &segment.from, &segment.to);
            let history = self.timings.tuples.entry(tuple.clone()).or_default();
            let Some(change) = history.add(s.actual_seconds, s.declared_seconds) else {
                if !self.timings.baseline.contains_key(&tuple) && history.figure(u64::MAX).is_some()
                {
                    self.timings.baseline.insert(tuple, s.receipt.clone());
                }
                continue;
            };
            // The first p99 rests on the settlement that formed it; a later change on the baseline's too.
            let mut cites = Vec::new();
            if !change.formed
                && let Some(rests_on) = self.timings.baseline.get(&tuple)
            {
                cites.push(rests_on.clone());
            }
            if !cites.contains(&s.receipt) {
                cites.push(s.receipt.clone());
            }
            let raise = Raise {
                raise: parts.fresh.id(),
                difference: Difference::SettlementTime {
                    provider: s.provider.clone(),
                    before_seconds: safe(change.before)?,
                    now_seconds: safe(change.now)?,
                },
                cites,
                at: now_at(parts.clock.now())?,
            };
            let delivered = matches!(operator.raise(&raise), Delivery::Delivered);
            let outcome = if delivered {
                Outcome::Done {}
            } else {
                Outcome::Failed {}
            };
            let entry = journal::built(
                core,
                What::Raise {
                    raise: raise.clone(),
                },
                &raise.at,
                outcome,
            )?;
            self.timings
                .raises
                .push((Step::new(entry), raise, delivered));
            self.timings.baseline.insert(tuple, s.receipt.clone());
        }
        // Every raise delivered is recorded: one the log can't take now waits, one it refuses is dropped, and the
        // others go on.
        let mut failure = None;
        let mut kept = Vec::new();
        for (mut step, raise, delivered) in std::mem::take(&mut self.timings.raises) {
            match step.append(parts.log) {
                Ok(receipt) => raised.push(Raised {
                    raise,
                    delivered,
                    receipt,
                }),
                Err(AppendFailure::Unavailable) => {
                    failure = failure.or(Some(AppendFailure::Unavailable));
                    kept.push((step, raise, delivered));
                }
                Err(refused) => failure = failure.or(Some(refused)),
            }
        }
        self.timings.raises = kept;
        match failure {
            Some(why) => Err(why.into()),
            None => Ok(raised),
        }
    }

    /// Every stuck order's value in each asset, as the operator sees it: ranked largest first within each asset,
    /// then oldest (`REQ-OPS-2`, [`rank`]). A part's value is what it holds, in flight or resting at the node it is
    /// evidenced at ([`Exposures::stuck`]), less what its resolutions moved; a resolved part counts no more.
    pub fn stuck_values(&self, exposures: &Exposures, clock: &dyn Clock) -> Vec<StuckValue> {
        let now = clock.now().floor().0;
        let mut values: BTreeMap<(String, String, u8), StuckValue> = BTreeMap::new();
        for ((order, _), declared) in &self.declared {
            if declared.state != OrderState::Stuck || declared.resolved {
                continue;
            }
            let Some(order_id) = Id::new(order.as_str()) else {
                continue;
            };
            let age = now.saturating_sub(declared.since.floor().0);
            for (asset, amount) in remaining(exposures, &order_id, declared) {
                let slot = values
                    .entry((order.clone(), asset.id.clone(), asset.decimals))
                    .or_insert_with(|| StuckValue {
                        order: order_id.clone(),
                        asset: asset.clone(),
                        amount: Amount::zero(),
                        age_seconds: 0,
                    });
                if let Some(sum) = slot.amount.checked_add(&amount) {
                    slot.amount = sum;
                }
                slot.age_seconds = slot.age_seconds.max(age);
            }
        }
        rank(values.into_values().collect())
    }

    /// Follows `order` for its resolution: the two places a resolution may deliver its value to, read from Core's
    /// own records, never from a caller (`THR-77`). The destination, its account included, is the order's latest
    /// permitted authorisation in `recorded`, the log's entries as the log holds them (the `authorise` entries
    /// `T-021-04`'s engine reads), held to the destination of every part of the order Core follows. The origin is the
    /// reserve and outer node the order's withdrawal took the value from, held to that authorisation's origin
    /// reserve; its account is checked at each decision against the payer's account the withdrawal Core signed
    /// named ([`PAYER_ACCOUNT`], `Segments::sent`), since the log holds the operation only by its commitment. The
    /// ends first followed stand: following again with others is refused ([`StuckError::NotItsAuthorisation`]).
    ///
    /// `recorded` is taken as the log's: this holds only where the assembly passes the log's own entries, never a
    /// caller's, until Core reads authorisations from policy's register (`T-021-04`). One authorisation per order,
    /// its latest, with one destination for every part, is this row's reading; a per-part authorisation
    /// (`T-020-01f`) is work for its merge.
    pub fn follow(
        &mut self,
        segments: &Segments,
        recorded: &[Entry],
        order: &Id,
    ) -> Result<(), StuckError> {
        let followed = segments.order(order).ok_or(StuckError::Unknown)?;
        let authorisation = recorded
            .iter()
            .rev()
            .find_map(|entry| match entry {
                Entry::Verified(v) if matches!(v.outcome, Outcome::Permitted {}) => match &v.what {
                    What::Authorise { authorisation, .. } if authorisation.order == *order => {
                        Some(authorisation)
                    }
                    _ => None,
                },
                _ => None,
            })
            .ok_or(StuckError::NotItsAuthorisation)?;
        let destination = &authorisation.destination;
        let to_destination = followed.parts.iter().all(|p| {
            p.destination.provider.as_str() == destination.reserve.as_str()
                && p.destination.to.node == NodeRef::Outer(destination.node.clone())
        });
        let first = followed
            .parts
            .first()
            .and_then(|p| p.segments.first())
            .ok_or(StuckError::Unknown)?;
        let origin_node = match &first.from.node {
            NodeRef::Outer(node) if first.kind == OperationKind::Withdrawal => node.clone(),
            _ => return Err(StuckError::NotItsAuthorisation),
        };
        if !to_destination || first.provider.as_str() != authorisation.origin_reserve.as_str() {
            return Err(StuckError::NotItsAuthorisation);
        }
        let ends = Ends {
            destination: destination.clone(),
            origin_reserve: authorisation.origin_reserve.clone(),
            origin_node,
            withdrawal: Id::new(first.segment_id.as_str()).ok_or(StuckError::Id("a segment id"))?,
        };
        // The ends first followed stand: following again with the same is nothing, and with others is refused.
        match self.ends.get(order.as_str()) {
            Some(held) if *held == ends => Ok(()),
            Some(_) => Err(StuckError::NotItsAuthorisation),
            None => {
                self.ends.insert(order.as_str().to_owned(), ends);
                Ok(())
            }
        }
    }

    /// The decide step (see the module's docs): Core's own checks first, then policy's decision, recorded by
    /// policy. A permitting decision answering Core's own request is kept for its move; any other is answered and
    /// kept for nothing.
    pub fn decide(
        &mut self,
        parts: &mut Parts<'_>,
        policy: &mut dyn StuckOrders,
        deciding: Deciding<'_>,
    ) -> Result<Decision, ResolveError> {
        let declared = self.unresolved(parts.segments, deciding.order, deciding.part)?;
        // While a move of the part is in progress, or its outcome unknown, nothing more is decided for it.
        self.idle(declared)?;
        let ends = self
            .ends
            .get(deciding.order.as_str())
            .ok_or(ResolveError::NoEnds)?;
        let request = StuckDecide {
            request: parts.fresh.id(),
            order: deciding.order.clone(),
            decider: deciding.decider.clone(),
            resolution: deciding.resolution.clone(),
            evidence: deciding.evidence.to_vec(),
            at: now(parts.clock)?,
        };
        if request.check().is_err() {
            return Err(ResolveError::NoEvidence);
        }
        if let Resolution::Deliver { to, .. } = deciding.resolution
            && !ends.takes(parts.segments, deciding.order, to)
        {
            return Err(ResolveError::ElsewhereThanItsEnds);
        }
        within(
            parts.exposures,
            deciding.order,
            declared,
            deciding.resolution,
        )?;
        let decision = policy.decide_stuck(&request)?;
        if decision.request != request.request {
            return Err(ResolveError::NotItsAnswer);
        }
        if decision.permit().is_some() {
            self.kept.insert(
                request.request.as_str().to_owned(),
                Kept {
                    request,
                    part: deciding.part.clone(),
                    decision: decision.decision.clone(),
                },
            );
        }
        Ok(decision)
    }

    /// The move step (see the module's docs): Core's own checks, then policy's decision, then the `resolve_stuck`
    /// effect on its permit, carried out once by `resolutions`, and its result. A move that stopped after its effect
    /// was built goes on from there, asking policy nothing again, for its own mover and order only.
    pub fn carry_out(
        &mut self,
        parts: &mut Parts<'_>,
        policy: &mut dyn StuckOrders,
        resolutions: &mut dyn Resolutions,
        moving: Moving<'_>,
    ) -> Result<Moved, ResolveError> {
        let at = moving.carries_out.request.as_str().to_owned();
        if let Some(motion) = self.moving.get(&at) {
            // Only the mover whose move policy permitted takes it on again, for its order: a move that stopped
            // after its effect was built is no one else's to finish (`REQ-AUZ-4`).
            if motion.order != *moving.order {
                return Err(ResolveError::NoDecision);
            }
            if motion.mover != *moving.mover {
                return Err(ResolveError::NotTheMover);
            }
            return self.advance(parts, resolutions, moving.core, &at);
        }
        let kept = self
            .kept
            .get(&at)
            .filter(|k| {
                k.decision == moving.carries_out.decision && k.request.order == *moving.order
            })
            .cloned()
            .ok_or(ResolveError::NoDecision)?;
        // Another accountable party, and another login: one person with logins under two stated parties is one
        // actor (`REQ-AUZ-4`, ADR-0011).
        if kept.request.decider.party == moving.mover.party {
            return Err(ResolveError::SameParty);
        }
        if kept.request.decider.acts == moving.mover.acts {
            return Err(ResolveError::SameLogin);
        }
        let declared = self.unresolved(parts.segments, moving.order, &kept.part)?;
        self.idle(declared)?;
        // What it moves is held to what the part still holds now, after every move since it was decided.
        within(
            parts.exposures,
            moving.order,
            declared,
            &kept.request.resolution,
        )?;
        let request = StuckMove {
            request: parts.fresh.id(),
            order: moving.order.clone(),
            carries_out: moving.carries_out.clone(),
            mover: moving.mover.clone(),
            at: now(parts.clock)?,
        };
        let decision = policy.move_stuck(&request)?;
        if decision.request != request.request {
            return Err(ResolveError::NotItsAnswer);
        }
        let Some(permit) = decision.permit() else {
            return Ok(Moved::NotPermitted(decision.outcome));
        };
        let action = Action::ResolveStuck {
            order: moving.order.clone(),
            carries_out: moving.carries_out.clone(),
            resolution: kept.request.resolution.clone(),
        };
        let entry = journal::built(
            moving.core,
            What::Effect {
                action: action.clone(),
                decision: permit.clone(),
            },
            &now(parts.clock)?,
            Outcome::ToTakeEffect {},
        )?;
        let part = key(moving.order, &kept.part);
        if let Some(declared) = self.declared.get_mut(&part) {
            declared.in_motion = Some(at.clone());
        }
        self.moving.insert(
            at.clone(),
            InMotion {
                order: moving.order.clone(),
                part: kept.part,
                mover: moving.mover.clone(),
                reported: None,
                reports: 0,
                dropped: None,
                effect: Step::new(entry),
                action,
                permit,
                resolved: None,
                result: None,
            },
        );
        self.advance(parts, resolutions, moving.core, &at)
    }

    /// Reports the outcome, now known, of a move whose outcome was unknown, done or failed, as its own mover for its
    /// own order, naming the evidence it relies on by an opaque reference. A report settles nothing: done would
    /// resolve the part and failed free it, each on the mover's word alone, which a mover colluding with the provider
    /// holding the funds could misuse (`THR-70`). It is recorded only once another actor confirms it
    /// ([`Stuck::confirm_outcome`]); a later report replaces it until then. Nothing is carried out again. Who learns
    /// the outcome from the provider's later word is the production [`Resolutions`]'s; the provider's stated result
    /// for the effect's transaction, as evidence Core checks, is the real answer, that row's or `T-020-12`'s.
    pub fn known_outcome(
        &mut self,
        moving: Moving<'_>,
        outcome: Resolved,
        evidence: &Id,
    ) -> Result<Moved, ResolveError> {
        let at = moving.carries_out.request.as_str().to_owned();
        let motion = self
            .moving
            .get_mut(&at)
            .filter(|m| m.order == *moving.order && m.resolved == Some(Resolved::Unknown))
            .ok_or(ResolveError::NoDecision)?;
        if motion.mover != *moving.mover {
            return Err(ResolveError::NotTheMover);
        }
        if outcome == Resolved::Unknown {
            return Err(ResolveError::OutcomeUnknown);
        }
        motion.reports = motion.reports.saturating_add(1);
        motion.reported = Some((outcome, evidence.clone(), motion.reports));
        Ok(Moved::AwaitsConfirmation {
            report: motion.reports,
        })
    }

    /// Confirms the outcome a move's mover reported ([`Stuck::known_outcome`]), naming the report it confirms (its
    /// outcome, version and evidence, which must all be the current report's, so the mover can't change the outcome
    /// after the confirmer committed) and the evidence relied on: only by an actor of another party and another login
    /// than the mover, as a decision and its move are (`REQ-AUZ-4`,
    /// ADR-0011; the decider may). Once confirmed, the outcome is recorded as the effect's result, once, with who
    /// reported and confirmed it and their evidence kept ([`Stuck::outcome_of`]): done counts what moved, and the part
    /// resolves once all of it moved; failed frees the part for a new decision. The roles a confirmer may hold (no
    /// customer, not the desk of the provider holding the funds, `THR-70`) are `T-021-06`'s to rule; the log's record
    /// of who reported and confirmed needs a contract kind (queued with the contract's author).
    pub fn confirm_outcome(
        &mut self,
        parts: &mut Parts<'_>,
        confirming: Confirming<'_>,
    ) -> Result<Moved, ResolveError> {
        let at = confirming.carries_out.request.as_str().to_owned();
        let motion = self
            .moving
            .get_mut(&at)
            .filter(|m| m.order == *confirming.order && m.resolved == Some(Resolved::Unknown))
            .ok_or(ResolveError::NoDecision)?;
        let Some((outcome, reported_with, report)) = motion.reported.clone() else {
            return Err(ResolveError::NoDecision);
        };
        if motion.mover.party == confirming.confirmer.party {
            return Err(ResolveError::SameParty);
        }
        if motion.mover.acts == confirming.confirmer.acts {
            return Err(ResolveError::SameLogin);
        }
        // The confirmer confirms the report it read, never one the mover made since: its outcome, version and
        // evidence must all be the current report's, or nothing is recorded.
        if (
            confirming.outcome,
            confirming.report,
            confirming.reported_with,
        ) != (outcome, report, &reported_with)
        {
            return Err(ResolveError::NotTheReport);
        }
        motion.resolved = Some(outcome);
        if outcome == Resolved::Done
            && let Some((asset, amount)) = moved_by(&motion.action)
            && let Some(declared) = self.declared.get_mut(&key(&motion.order, &motion.part))
        {
            add_moved(&mut declared.moved, &asset, &amount);
        }
        self.outcomes.insert(
            at.clone(),
            SettledOutcome {
                decision: confirming.carries_out.clone(),
                outcome,
                reported_by: motion.mover.clone(),
                reported_with,
                confirmed_by: confirming.confirmer.clone(),
                confirmed_with: confirming.evidence.clone(),
            },
        );
        self.advance(parts, &mut Unbuilt, confirming.core, &at)
    }

    /// The outcome of a move whose outcome was unknown, once confirmed: who reported and who confirmed it, each with
    /// the evidence it named.
    pub fn outcome_of(&self, decided: &Decided) -> Option<&SettledOutcome> {
        self.outcomes
            .get(decided.request.as_str())
            .filter(|o| o.decision == *decided)
    }

    /// Takes a move on from where it stopped: its effect appended, acted on once, its result recorded, and the
    /// part's value released once all of it moved.
    fn advance(
        &mut self,
        parts: &mut Parts<'_>,
        resolutions: &mut dyn Resolutions,
        core: &Actor,
        at: &str,
    ) -> Result<Moved, ResolveError> {
        let motion = self.moving.get_mut(at).ok_or(ResolveError::NoDecision)?;
        let receipt = match motion.effect.append(parts.log) {
            Ok(receipt) => receipt,
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            // A refused effect backs nothing: nothing moved and the permit is unused. The move is dropped, the part
            // freed, and its decision spent, so a new decision and move are asked (`INV-7`).
            Err(refused) => {
                let part = key(&motion.order, &motion.part);
                if let Some(decided) = carried_out_of(&motion.action) {
                    self.kept.remove(decided.request.as_str());
                }
                self.moving.remove(at);
                if let Some(declared) = self.declared.get_mut(&part) {
                    declared.in_motion = None;
                }
                return Err(refused.into());
            }
        };
        let part = key(&motion.order, &motion.part);
        let resolved = match motion.resolved {
            Some(resolved) => resolved,
            None => {
                // The part is read again before anything moves: one that left `stuck` while the move stalled
                // (resolved, or completed by a result recorded since), or holds less than the move names now, is
                // moved no more. The move is dropped and its decision spent (`INV-4`); its effect, recorded already,
                // gets its `failed` result, so no effect is left open.
                let (order, part_id) = (motion.order.clone(), motion.part.clone());
                let resolution = match &motion.action {
                    Action::ResolveStuck { resolution, .. } => resolution.clone(),
                    _ => return Err(ResolveError::NoDecision),
                };
                let still = self
                    .unresolved(parts.segments, &order, &part_id)
                    .and_then(|declared| within(parts.exposures, &order, declared, &resolution));
                if let Err(why) = still {
                    let motion = self.moving.get_mut(at).ok_or(ResolveError::NoDecision)?;
                    if let Some(decided) = carried_out_of(&motion.action) {
                        self.kept.remove(decided.request.as_str());
                    }
                    motion.resolved = Some(Resolved::Failed);
                    motion.dropped = Some(why);
                    Resolved::Failed
                } else {
                    // One register of effects acted on, the segments' and the resolutions' (`INV-8`).
                    if !parts.segments.act_on(&receipt) {
                        return Err(ResolveError::AlreadyActed(receipt));
                    }
                    let motion = self.moving.get_mut(at).ok_or(ResolveError::NoDecision)?;
                    let effect = effect_of(
                        motion.action.clone(),
                        motion.permit.clone(),
                        receipt.clone(),
                    );
                    let resolved = resolutions.resolve(&effect);
                    motion.resolved = Some(resolved);
                    // Its decision is spent, whatever came of it: a new move needs a new decision.
                    let decided = carried_out_of(&motion.action);
                    if let Some(decided) = decided {
                        self.kept.remove(decided.request.as_str());
                    }
                    if resolved == Resolved::Done
                        && let Some((asset, amount)) = moved_by(&self.moving[at].action)
                        && let Some(declared) = self.declared.get_mut(&part)
                    {
                        add_moved(&mut declared.moved, &asset, &amount);
                    }
                    resolved
                }
            }
        };
        let outcome = match resolved {
            Resolved::Done => Outcome::Done {},
            Resolved::Failed => Outcome::Failed {},
            // An outcome unknown has no result yet: the part stays held, and nothing more moves for it until its
            // outcome is known (`REQ-ORD-1`).
            Resolved::Unknown => {
                return Ok(Moved::CarriedOut {
                    effect: receipt,
                    resolved,
                    result: None,
                });
            }
        };
        let motion = self.moving.get_mut(at).ok_or(ResolveError::NoDecision)?;
        if motion.result.is_none() {
            let entry = journal::built(
                core,
                What::Result {
                    effect: receipt.clone(),
                },
                &now(parts.clock)?,
                outcome,
            )?;
            motion.result = Some(Step::new(entry));
        }
        let result = motion
            .result
            .as_mut()
            .ok_or(ResolveError::NoDecision)?
            .append(parts.log)?;
        let motion = self.moving.remove(at).ok_or(ResolveError::NoDecision)?;
        if let Some(declared) = self.declared.get_mut(&part) {
            declared.in_motion = None;
        }
        if let Some(why) = motion.dropped {
            return Err(why);
        }
        if resolved == Resolved::Done {
            let order = motion.order.clone();
            let all_moved = self
                .declared
                .get(&part)
                .is_some_and(|d| remaining(parts.exposures, &order, d).is_empty());
            if all_moved {
                self.release_part(parts.exposures, parts.segments, &order, &motion.part);
            }
        }
        Ok(Moved::CarriedOut {
            effect: receipt,
            resolved,
            result: Some(result),
        })
    }

    /// Resolves a part all of whose value moved: its holds released, every decision kept for it spent, any move of
    /// it that never acted dropped, and the order's stuck mark released once the order holds nothing more. Another
    /// part, stuck or still in flight, keeps its holds and the mark, which over-reports a stuck exposure rather than
    /// drop a live one (`REQ-ORD-8`).
    fn release_part(
        &mut self,
        exposures: &mut Exposures,
        segments: &mut Segments,
        order: &Id,
        part: &Id,
    ) {
        let at = key(order, part);
        // The segments take no rest hold for it any more: it holds nothing, though the state machine still reads it
        // stuck until its resolution's input lands (`T-020-01f`).
        segments.mark_resolved(order, part);
        if let Some(declared) = self.declared.get_mut(&at) {
            declared.resolved = true;
            declared.in_motion = None;
            for (segment, _) in &declared.segments {
                exposures.release(order, segment);
            }
        }
        self.kept
            .retain(|_, k| key(&k.request.order, &k.part) != at);
        self.moving
            .retain(|_, m| key(&m.order, &m.part) != at || m.resolved.is_some());
        let unresolved = self
            .declared
            .iter()
            .any(|((o, _), d)| o == order.as_str() && d.state == OrderState::Stuck && !d.resolved);
        let holds = exposures.held().any(|h| &h.order == order);
        if !unresolved && !holds {
            exposures.release_order(order);
        }
    }

    /// The part as Core declared it, stuck and unresolved, and stuck in the order Core follows.
    fn unresolved(
        &self,
        segments: &Segments,
        order: &Id,
        part: &Id,
    ) -> Result<&Declared, ResolveError> {
        let declared = self
            .declared
            .get(&key(order, part))
            .filter(|d| d.state == OrderState::Stuck)
            .ok_or(ResolveError::NotStuck)?;
        if declared.resolved {
            return Err(ResolveError::Resolved);
        }
        if !segments.stuck(order, part) {
            return Err(ResolveError::NotStuck);
        }
        Ok(declared)
    }

    /// Whether no move of the part is in progress: one whose outcome is unknown holds the part until it is known.
    fn idle(&self, declared: &Declared) -> Result<(), ResolveError> {
        match declared.in_motion.as_ref().and_then(|m| self.moving.get(m)) {
            None => Ok(()),
            Some(m) if m.resolved == Some(Resolved::Unknown) => Err(ResolveError::OutcomeUnknown),
            Some(_) => Err(ResolveError::MoveInProgress),
        }
    }

    /// Appends a part's kept declaration and finishes it.
    fn finish_watch(
        &mut self,
        parts: &mut Parts<'_>,
        at: &(String, String),
    ) -> Result<Watch, StuckError> {
        let (step, call) = self
            .pending
            .get_mut(at)
            .and_then(|p| p.call.as_mut())
            .ok_or(StuckError::Plan("a declaration over time has its call"))?;
        // The figures reach the log before the declaration that rests on them (`REQ-ORD-6`).
        let call_receipt = step.append(parts.log)?;
        let call = call.clone();
        let input = self.record_pending(parts, at)?;
        Ok(Watch::Stuck(Box::new(StuckCalled {
            order: Id::new(at.0.as_str()).ok_or(StuckError::Id("an order id"))?,
            part: Id::new(at.1.as_str()).ok_or(StuckError::Id("a part id"))?,
            segment: call.segment,
            provider: call.provider,
            basis: call.basis,
            figure_seconds: call.figure_seconds,
            held_seconds: call.held_seconds,
            call: call_receipt,
            input,
        })))
    }

    /// Appends a part's kept input, once the part still takes it, and once the log took it moves the order Core
    /// follows and marks the order stuck.
    fn record_pending(
        &mut self,
        parts: &mut Parts<'_>,
        at: &(String, String),
    ) -> Result<Receipt, StuckError> {
        let pending = self.pending.get(at).cloned().ok_or(StuckError::Unknown)?;
        let order = Id::new(at.0.as_str()).ok_or(StuckError::Id("an order id"))?;
        let part = Id::new(at.1.as_str()).ok_or(StuckError::Id("a part id"))?;
        // The part is read again first: an input built before it moved on is no input now.
        if pending.input.receipt.is_none() {
            let followed = parts.segments.order(&order).ok_or(StuckError::Unknown)?;
            let after = Stm::transition(followed, &pending.recorded);
            if after.last_input.outcome != (stm::Outcome::Applied {}) {
                self.pending.remove(at);
                return Err(StuckError::NotApplicable(after.last_input.outcome));
            }
        }
        let receipt = match self.pending.get_mut(at) {
            Some(p) => match p.input.append(parts.log) {
                Ok(receipt) => receipt,
                Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
                Err(refused) => {
                    self.pending.remove(at);
                    return Err(refused.into());
                }
            },
            None => return Err(StuckError::Unknown),
        };
        self.pending.remove(at);
        let resting = resting(parts.segments, parts.exposures, &order, &part);
        if !parts.segments.observe(parts.clock, &pending.recorded) {
            return Err(StuckError::NotApplicable(stm::Outcome::Applied {}));
        }
        let followed = parts.segments.order(&order).ok_or(StuckError::Unknown)?;
        let now_part = followed
            .parts
            .iter()
            .find(|p| p.part_id.as_str() == part.as_str())
            .ok_or(StuckError::Unknown)?;
        let segments = now_part
            .segments
            .iter()
            .filter_map(|s| {
                Some((
                    Id::new(s.segment_id.as_str())?,
                    Id::new(s.provider.as_str())?,
                ))
            })
            .collect();
        let state = now_part.state;
        if state == OrderState::Stuck {
            // What rests at the node is held there, so the ceilings in flight and the stuck view count it.
            if let Some((segment, provider, asset, amount)) = resting {
                match parts
                    .exposures
                    .hold_resting(&order, &segment, &provider, &asset, &amount)
                {
                    Ok(()) | Err(HoldError::AlreadyHeld) => {}
                    Err(HoldError::Overflow) => {
                        return Err(StuckError::Plan(
                            "the value resting there overflows its provider's",
                        ));
                    }
                }
            }
            parts.exposures.mark_stuck(&order);
        }
        self.declared.insert(
            at.clone(),
            Declared {
                state,
                since: parts.clock.now(),
                segments,
                moved: Vec::new(),
                in_motion: None,
                resolved: false,
            },
        );
        Ok(receipt)
    }

    /// One input about a part, by Core, built and checked to apply to the part as Core follows it.
    fn part_input(
        parts: &mut Parts<'_>,
        order: &Id,
        part: &Id,
        core: &Actor,
        observed: Observed,
    ) -> Result<RecordedInput, StuckError> {
        let followed = parts.segments.order(order).ok_or(StuckError::Unknown)?;
        let current = followed
            .parts
            .iter()
            .find(|p| p.part_id.as_str() == part.as_str())
            .ok_or(StuckError::Unknown)?;
        if current.state == OrderState::Stuck {
            return Err(StuckError::AlreadyStuck);
        }
        let when = now(parts.clock)?;
        let input = RecordedInput {
            record_id: stm::RecordId::new(parts.fresh.id().as_str())
                .ok_or(StuckError::Id("a record id"))?,
            recorded_at: stm::Timestamp::new(when.as_str())
                .ok_or(StuckError::Append(AppendFailure::Refused(NO_TIME)))?,
            time_source: stm_id("core_clock")?,
            source: Source {
                kind: SourceKind::Orchestrator,
                id: stm_id(core.acts.as_str())?,
            },
            standing: Standing::Verified,
            subject: Subject {
                order: stm_id(order.as_str())?,
                part: Some(stm_id(part.as_str())?),
                segment: None,
            },
            observed,
        };
        let followed = parts.segments.order(order).ok_or(StuckError::Unknown)?;
        let after = Stm::transition(followed, &input);
        if after.last_input.outcome != (stm::Outcome::Applied {}) {
            return Err(StuckError::NotApplicable(after.last_input.outcome));
        }
        Ok(input)
    }
}

/// What `reader`, a provider, reads of `segment` of `order`: its state and its part's, on a read of that segment
/// policy allows, recorded either way (`REQ-ORD-6`, `REQ-AUZ-7`). The read names the segment's provider as the
/// order Core follows has it, never the reader's word, and policy holds the reader to it.
pub fn seen_by_provider(
    segments: &Segments,
    reads: &mut dyn Reads,
    clock: &dyn Clock,
    fresh: &mut dyn Fresh,
    reader: &Actor,
    order: &Id,
    segment: &Id,
) -> Result<ProviderView, StatusError> {
    let followed = segments.order(order).ok_or(StatusError::Unknown)?;
    let (part, seg) = followed
        .parts
        .iter()
        .find_map(|p| {
            p.segments
                .iter()
                .find(|s| s.segment_id.as_str() == segment.as_str())
                .map(|s| (p, s))
        })
        .ok_or(StatusError::Unknown)?;
    let provider = Id::new(seg.provider.as_str()).ok_or(StatusError::Unknown)?;
    let at = clock
        .now()
        .timestamp()
        .ok_or(StatusError::Append(AppendFailure::Refused(NO_TIME)))?;
    let answer = reads
        .check_read(&ReadCheck {
            request: fresh.id(),
            reader: reader.clone(),
            reads: rutent_contracts::core_internal::Reads::Segment {
                order: order.clone(),
                segment: segment.clone(),
                provider,
            },
            permission: None,
            at,
        })
        .map_err(StatusError::Append)?;
    if !matches!(answer.outcome, ReadOutcome::Allowed {}) {
        return Err(StatusError::Refused(answer));
    }
    Ok(ProviderView {
        order: order.clone(),
        part: Id::new(part.part_id.as_str()).ok_or(StatusError::Unknown)?,
        segment: segment.clone(),
        part_state: part.state,
        segment_state: seg.state,
    })
}

/// What rests at the node `part`'s value is evidenced at, where no segment of the part holds anything in flight:
/// what its last settled segment handed on there, in the asset it arrived in, under that segment and at its
/// provider.
fn resting(
    segments: &Segments,
    exposures: &Exposures,
    order: &Id,
    part: &Id,
) -> Option<(Id, Id, Denomination, Amount)> {
    let followed = segments.order(order)?;
    let part = followed
        .parts
        .iter()
        .find(|p| p.part_id.as_str() == part.as_str())?;
    let held = exposures.held().any(|h| {
        &h.order == order
            && part
                .segments
                .iter()
                .any(|s| s.segment_id.as_str() == h.segment.as_str())
    });
    if held {
        return None;
    }
    let index = part
        .segments
        .iter()
        .position(|s| s.state != SegmentState::Settled)
        .unwrap_or(part.segments.len());
    let settled = part.segments.get(index.checked_sub(1)?)?;
    let segment = Id::new(settled.segment_id.as_str())?;
    let amount = segments.handed_on(order, &segment)?.clone();
    let provider = Id::new(settled.provider.as_str())?;
    Some((segment, provider, settled.to.asset.clone(), amount))
}

/// What a declared part still holds, by asset: what it holds in flight or resting at its node, less what its
/// resolutions moved. Only the assets it holds something of.
fn remaining(
    exposures: &Exposures,
    order: &Id,
    declared: &Declared,
) -> Vec<(Denomination, Amount)> {
    let mut held: Vec<(Denomination, Amount)> = Vec::new();
    for h in exposures.stuck() {
        if &h.order == order && declared.segments.iter().any(|(s, _)| s == &h.segment) {
            add_moved(&mut held, &h.asset, &h.amount);
        }
    }
    held.into_iter()
        .filter_map(|(asset, amount)| {
            let moved = declared
                .moved
                .iter()
                .find(|(a, _)| a == &asset)
                .map_or_else(Amount::zero, |(_, m)| m.clone());
            let left = amount.checked_sub(&moved).unwrap_or_else(Amount::zero);
            (!left.is_zero()).then_some((asset, left))
        })
        .collect()
}

/// Whether `resolution` moves the part's own value: through a provider of the part, in an asset it holds, no more
/// than it still holds there (`THR-77`).
fn within(
    exposures: &Exposures,
    order: &Id,
    declared: &Declared,
    resolution: &Resolution,
) -> Result<(), ResolveError> {
    let (by, asset, amount) = match resolution {
        Resolution::Deliver {
            by, asset, amount, ..
        }
        | Resolution::HandOn {
            by, asset, amount, ..
        } => (by, asset, amount),
    };
    let left = remaining(exposures, order, declared)
        .into_iter()
        .find(|(a, _)| a == asset)
        .map_or_else(Amount::zero, |(_, l)| l);
    if !declared.segments.iter().any(|(_, p)| p == by) || amount.amount() > &left {
        return Err(ResolveError::NotItsValue);
    }
    Ok(())
}

/// The decision a `resolve_stuck` action carries out.
fn carried_out_of(action: &Action) -> Option<Decided> {
    match action {
        Action::ResolveStuck { carries_out, .. } => Some(carries_out.clone()),
        _ => None,
    }
}

/// What a `resolve_stuck` action moves: its asset and amount.
fn moved_by(action: &Action) -> Option<(Denomination, Amount)> {
    match action {
        Action::ResolveStuck {
            resolution:
                Resolution::Deliver { asset, amount, .. } | Resolution::HandOn { asset, amount, .. },
            ..
        } => Some((asset.clone(), amount.amount().clone())),
        _ => None,
    }
}

/// Adds `amount` of `asset` to `sums`.
fn add_moved(sums: &mut Vec<(Denomination, Amount)>, asset: &Denomination, amount: &Amount) {
    match sums.iter_mut().find(|(a, _)| a == asset) {
        Some((_, sum)) => {
            if let Some(total) = sum.checked_add(amount) {
                *sum = total;
            }
        }
        None => sums.push((asset.clone(), amount.clone())),
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

fn key(order: &Id, part: &Id) -> (String, String) {
    (order.as_str().to_owned(), part.as_str().to_owned())
}

/// A tuple's key: its provider and the endpoints it runs between.
fn tuple_key(provider: &Id, from: &Endpoint, to: &Endpoint) -> String {
    let endpoint = |e: &Endpoint| serde_json::to_string(e).unwrap_or_default();
    format!("{}|{}|{}", provider.as_str(), endpoint(from), endpoint(to))
}

fn stm_id(text: &str) -> Result<stm::Id, StuckError> {
    stm::Id::new(text).ok_or(StuckError::Id("an id"))
}

fn safe(seconds: u64) -> Result<SafeInteger, StuckError> {
    SafeInteger::try_from(seconds)
        .map_err(|_| StuckError::Plan("a time in seconds is a safe integer"))
}

fn now(clock: &dyn Clock) -> Result<Timestamp, AppendFailure> {
    now_at(clock.now())
}

fn now_at(reading: Reading) -> Result<Timestamp, AppendFailure> {
    reading.timestamp().ok_or(AppendFailure::Refused(NO_TIME))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct At(u64);

    impl Clock for At {
        fn now(&self) -> Reading {
            Reading::at(self.0)
        }
    }

    fn cop() -> Denomination {
        Denomination {
            id: "COP".into(),
            decimals: 2,
        }
    }

    fn id(text: &str) -> Id {
        Id::new(text).unwrap()
    }

    fn declared(
        stuck: &mut Stuck,
        exposures: &mut Exposures,
        order: &str,
        since: u64,
        amount: &str,
    ) {
        let segment = id(&format!("{order}-s1"));
        exposures
            .hold_resting(
                &id(order),
                &segment,
                &id("reserve"),
                &cop(),
                &Amount::new(amount).unwrap(),
            )
            .unwrap();
        exposures.mark_stuck(&id(order));
        stuck.declared.insert(
            (order.to_owned(), "p1".to_owned()),
            Declared {
                state: OrderState::Stuck,
                since: Reading::at(since),
                segments: vec![(segment, id("reserve"))],
                moved: Vec::new(),
                in_motion: None,
                resolved: false,
            },
        );
    }

    /// Fifty small orders stuck for an hour and one large one stuck a minute ago: the large one comes first, then
    /// the small ones oldest first, each at its value; a resolved order and one waiting for the customer are not
    /// on the view.
    // verifies: REQ-OPS-2
    #[test]
    fn one_large_stuck_order_is_seen_among_many_small() {
        let (mut stuck, mut exposures) = (Stuck::new(), Exposures::new());
        for n in 0..50 {
            declared(
                &mut stuck,
                &mut exposures,
                &format!("small-{n:02}"),
                1_000 + n,
                "1000",
            );
        }
        declared(&mut stuck, &mut exposures, "large", 4_540, "500000000");
        declared(&mut stuck, &mut exposures, "resolved", 0, "900000000");
        if let Some(d) = stuck
            .declared
            .get_mut(&("resolved".to_owned(), "p1".to_owned()))
        {
            d.resolved = true;
        }
        declared(&mut stuck, &mut exposures, "waiting", 0, "800000000");
        if let Some(d) = stuck
            .declared
            .get_mut(&("waiting".to_owned(), "p1".to_owned()))
        {
            d.state = OrderState::WaitingForCustomer;
        }
        let seen = stuck.stuck_values(&exposures, &At(4_600));
        assert_eq!(seen.len(), 51);
        assert_eq!(
            seen[0],
            StuckValue {
                order: id("large"),
                asset: cop(),
                amount: Amount::new("500000000").unwrap(),
                age_seconds: 60,
            }
        );
        assert_eq!(seen[1].order.as_str(), "small-00");
        assert_eq!(seen[1].age_seconds, 3_600);
        assert_eq!(seen[50].order.as_str(), "small-49");
        assert!(
            seen.iter()
                .all(|v| v.order.as_str() != "resolved" && v.order.as_str() != "waiting")
        );
    }

    /// A split order's first part is stuck and resolved; its second was never declared stuck and still holds its
    /// segment in flight. Resolving the first releases its own hold and keeps the second's, and the order's stuck
    /// mark with it, so a live hold is never dropped from the ceilings.
    // verifies: REQ-ORD-8, REQ-AUZ-4
    #[test]
    fn resolving_one_part_keeps_another_parts_live_hold() {
        let (mut stuck, mut exposures) = (Stuck::new(), Exposures::new());
        declared(&mut stuck, &mut exposures, "split", 0, "600");
        exposures
            .hold_resting(
                &id("split"),
                &id("p2-s1"),
                &id("reserve"),
                &cop(),
                &Amount::new("400").unwrap(),
            )
            .unwrap();
        stuck.release_part(
            &mut exposures,
            &mut Segments::new(),
            &id("split"),
            &id("p1"),
        );
        let left: Vec<_> = exposures
            .held()
            .map(|h| (h.segment.as_str().to_owned(), h.amount.clone()))
            .collect();
        assert_eq!(
            left,
            vec![("p2-s1".to_owned(), Amount::new("400").unwrap())]
        );
        assert_eq!(exposures.stuck().len(), 1, "the order's stuck mark stays");
        assert!(stuck.stuck_values(&exposures, &At(10)).is_empty());
    }

    /// A withdrawal whose tuple's terms carry the beneficiary's account beside the payer's: only the payer's is the
    /// origin a resolution may return the value to.
    // verifies: REQ-AUZ-4
    #[test]
    fn only_the_payers_account_is_the_origins() {
        let inputs = std::collections::BTreeMap::from([
            (PAYER_ACCOUNT.to_owned(), "acct-1".to_owned()),
            ("beneficiary.bank_account".to_owned(), "acct-2".to_owned()),
        ]);
        assert!(names_the_payer(&inputs, "acct-1"));
        assert!(!names_the_payer(&inputs, "acct-2"));
        assert!(!names_the_payer(
            &std::collections::BTreeMap::new(),
            "acct-1"
        ));
    }
}
