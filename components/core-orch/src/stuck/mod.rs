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
//!    ([`NotRecalculable::Held`](crate::recalc::NotRecalculable::Held)).
//! 2. **Over time** ([`Stuck::watch`], `REQ-ORD-29`). A segment carried out with no final outcome (no result that
//!    counts, an outcome unknown, or the person to act) is timed from when it was carried out. Past twice the p99
//!    of its tuple's settlements, or twice its advertised time until the tuple has [`HISTORY`], the figures are
//!    recorded (`stuck_call`, `REQ-ORD-6`) and then the part declared stuck (`stuck_declared`, `over_time`), and the
//!    declaration is handed back as the operator's warning ([`StuckCalled`]). Exactly twice is not over.
//! 3. **A material change in a provider's p99** ([`Stuck::learn`], `REQ-ORD-6`, `THR-24`). Each settled
//!    segment's actual time joins its tuple's history once; when the p99 moves by [`MATERIAL_BPS`] of the one last
//!    taken as its baseline, either way, it is raised to the operator (`Operator::raise`, a
//!    `settlement_time` difference citing the settlements it rests on), and the raise is recorded with whether it
//!    was delivered.
//! 4. **Seen by value and age** ([`Stuck::stuck_values`], `REQ-OPS-2`): every stuck order's value in each asset,
//!    what its parts hold in flight and what rests at the node its value is evidenced at, ranked largest first
//!    within each asset, then oldest. The customer reads its order's state through [`Segments::status`]; the
//!    provider reads its own segment's through [`seen_by_provider`], each on a read policy allows
//!    (`REQ-ORD-6`).
//! 5. **Resolved in two steps** ([`Stuck::decide`], [`Stuck::carry_out`], `REQ-AUZ-4`, ADR-0011). One actor
//!    decides the outcome and names its evidence; policy decides that step. Another actor, of another accountable
//!    party, moves the value; policy decides that step too, and only its permit backs the `resolve_stuck` effect.
//!    Core holds both rules itself whatever policy answers: it carries out a move only for a decision it holds,
//!    permitted, of a part it declared stuck and that is still unresolved, and only where the mover's party isn't
//!    the decider's. A resolution moves no more than the part holds, through a provider of the part, to the order's
//!    destination or back to its origin. The effect is acted on once for its receipt (`INV-8`), and the part is
//!    resolved once.
//!
//! **Gaps, each named in `F-020`'s notes.** Policy's `StuckOrders` is unbuilt (`T-021-06`): it answers not known,
//! so nothing moves until it is built, and it holds no rule comparing the two actors yet. state-machine 2.1.0 has no
//! input that ends a part a resolution delivered, so the part stays `stuck` there and Core holds it resolved. The
//! production [`Resolutions`] that carries a resolution out on the platform is a later row's.

mod journal;
mod timing;
mod view;

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::{
    AppendFailure, Delivery, Operator, Reads, StuckOrders,
};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, Decided, Decision, Destination, Difference, Effect, Outcome, Permit,
    Raise, ReadCheck, ReadOutcome, Receipt, Resolution, StuckBasis, StuckDecide, StuckMove, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{Denomination, SafeInteger};
use rutent_contracts::router_api::{Amount, DecisionRecord, Endpoint, Id, Timestamp};
use rutent_contracts::state_machine::{
    self as stm, Observed, OrderState, RecordedInput, SegmentState, Source, SourceKind, Standing,
    StateMachine, StuckReason, Subject,
};
use rutent_router::quote::declared_times;
use rutent_stm::Stm;

use crate::recalc::Recalculation;
use crate::rounds::Fresh;
use crate::segments::{Segments, StatusError};
use crate::snapshot::{Clock, Exposures, OrchestrationLog, Reading};

pub use journal::APPEND_TRIES;
use journal::{NO_TIME, Step};
pub use timing::{HISTORY, MATERIAL_BPS, clock_figure, material, over_time, p99};
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
}

impl From<AppendFailure> for StuckError {
    fn from(failure: AppendFailure) -> Self {
        StuckError::Append(failure)
    }
}

/// The two places a resolution may deliver a stuck order's value to, from Core's own records: the order's
/// destination, as its authorisation names it, and its origin, the account the value came from.
#[derive(Debug, Clone, Copy)]
pub struct Ends<'a> {
    /// The order's destination.
    pub destination: &'a Destination,
    /// The order's origin.
    pub origin: &'a Destination,
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
    /// Where the value may go.
    pub ends: Ends<'a>,
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
    /// The resolution moves another asset, more than the part holds, or through a provider that isn't the part's.
    NotItsValue,
    /// The move names no decide step Core holds permitted for this order: one never asked, refused, not known, or
    /// of another order (`REQ-AUZ-4`).
    NoDecision,
    /// The mover acts for the decider's accountable party: one party would both decide and move (`REQ-AUZ-4`,
    /// ADR-0011).
    SameParty,
    /// A move in progress is taken on again only by the mover policy permitted it to (`REQ-AUZ-4`).
    NotTheMover,
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
    /// It didn't: nothing moved, and the part's value stays held.
    Failed,
}

/// What a move step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Moved {
    /// Policy didn't permit it: nothing was recorded as an effect, and nothing moved.
    NotPermitted(Answer),
    /// The effect was recorded and carried out.
    CarriedOut {
        /// The receipt of the `resolve_stuck` effect's entry.
        effect: Receipt,
        /// How it went.
        resolved: Resolved,
        /// The receipt of its `result` entry.
        result: Receipt,
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
/// production one, which writes the move to the platform, is a later row's.
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
    /// What rests at the node the part's value is evidenced at, which no hold counts.
    resting: Option<(Denomination, Amount)>,
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

/// A move from its permit until its result is recorded.
#[derive(Debug, Clone)]
struct InMotion {
    order: Id,
    part: Id,
    /// Who moves it: only that mover takes the move on again.
    mover: Actor,
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
    /// Each tuple's settlement times, in seconds, in the order the log took them.
    samples: BTreeMap<String, Vec<u64>>,
    baseline: BTreeMap<String, (u64, Receipt)>,
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
    acted: BTreeSet<String>,
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
        if self.pending.get(&at).is_some_and(|p| p.call.is_some()) {
            return self.finish_watch(parts, &at);
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
            let history: Vec<u64> = self
                .timings
                .samples
                .get(&tuple)
                .cloned()
                .unwrap_or_default();
            let declared = advertised
                .iter()
                .find(|t| t.part as usize == watching.at && t.operation as usize == place)
                .map_or(u64::MAX, |t| t.declared_seconds);
            let Some((basis, figure)) = clock_figure(&history, declared) else {
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
            let samples = self.timings.samples.entry(tuple.clone()).or_default();
            samples.push(s.actual_seconds);
            if samples.len() < HISTORY {
                continue;
            }
            let Some(now) = p99(samples) else { continue };
            match self.timings.baseline.get(&tuple).cloned() {
                None => {
                    self.timings
                        .baseline
                        .insert(tuple, (now, s.receipt.clone()));
                }
                Some((before, rests_on)) if material(before, now) => {
                    let mut cites = vec![rests_on];
                    if !cites.contains(&s.receipt) {
                        cites.push(s.receipt.clone());
                    }
                    let raise = Raise {
                        raise: parts.fresh.id(),
                        difference: Difference::SettlementTime {
                            provider: s.provider.clone(),
                            before_seconds: safe(before)?,
                            now_seconds: safe(now)?,
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
                    self.timings
                        .baseline
                        .insert(tuple, (now, s.receipt.clone()));
                }
                Some(_) => {}
            }
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
    /// then oldest (`REQ-OPS-2`, [`rank`]). A part's value is what it holds in flight ([`Exposures::stuck`]) and
    /// what rests at the node it is evidenced at; a resolved part counts no more.
    pub fn stuck_values(&self, exposures: &Exposures, clock: &dyn Clock) -> Vec<StuckValue> {
        let now = clock.now().floor().0;
        let held = exposures.stuck();
        let mut values: BTreeMap<(String, String, u8), StuckValue> = BTreeMap::new();
        for ((order, _), declared) in &self.declared {
            if declared.state != OrderState::Stuck || declared.resolved {
                continue;
            }
            let Some(order_id) = Id::new(order.as_str()) else {
                continue;
            };
            let age = now.saturating_sub(declared.since.floor().0);
            let mut add = |asset: &Denomination, amount: &Amount| {
                let slot = values
                    .entry((order.clone(), asset.id.clone(), asset.decimals))
                    .or_insert_with(|| StuckValue {
                        order: order_id.clone(),
                        asset: asset.clone(),
                        amount: Amount::zero(),
                        age_seconds: 0,
                    });
                if let Some(sum) = slot.amount.checked_add(amount) {
                    slot.amount = sum;
                }
                slot.age_seconds = slot.age_seconds.max(age);
            };
            for h in held.iter().filter(|h| {
                h.order.as_str() == order.as_str()
                    && declared.segments.iter().any(|(s, _)| s == &h.segment)
            }) {
                add(&h.asset, &h.amount);
            }
            if let Some((asset, amount)) = &declared.resting {
                add(asset, amount);
            }
        }
        rank(values.into_values().collect())
    }

    /// The decide step (see the module's docs): Core's own checks first, then policy's decision, recorded by
    /// policy. A permitting decision is kept for its move; any other is answered and kept for nothing.
    pub fn decide(
        &mut self,
        parts: &mut Parts<'_>,
        policy: &mut dyn StuckOrders,
        deciding: Deciding<'_>,
    ) -> Result<Decision, ResolveError> {
        let declared = self.unresolved(parts.segments, deciding.order, deciding.part)?;
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
        let (by, asset, amount) = match deciding.resolution {
            Resolution::Deliver {
                by,
                to,
                asset,
                amount,
            } => {
                if to != deciding.ends.destination && to != deciding.ends.origin {
                    return Err(ResolveError::ElsewhereThanItsEnds);
                }
                (by, asset, amount)
            }
            Resolution::HandOn {
                by, asset, amount, ..
            } => (by, asset, amount),
        };
        let holds = self.value_of(parts.exposures, deciding.order, declared, asset);
        if !declared.segments.iter().any(|(_, p)| p == by) || amount.amount() > &holds {
            return Err(ResolveError::NotItsValue);
        }
        let decision = policy.decide_stuck(&request)?;
        if decision.permit().is_some() {
            self.kept.insert(
                decision.request.as_str().to_owned(),
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
    /// was built goes on from there, asking policy nothing again.
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
        if kept.request.decider.party == moving.mover.party {
            return Err(ResolveError::SameParty);
        }
        self.unresolved(parts.segments, moving.order, &kept.part)?;
        let request = StuckMove {
            request: parts.fresh.id(),
            order: moving.order.clone(),
            carries_out: moving.carries_out.clone(),
            mover: moving.mover.clone(),
            at: now(parts.clock)?,
        };
        let decision = policy.move_stuck(&request)?;
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
        self.moving.insert(
            at.clone(),
            InMotion {
                order: moving.order.clone(),
                part: kept.part,
                mover: moving.mover.clone(),
                effect: Step::new(entry),
                action,
                permit,
                resolved: None,
                result: None,
            },
        );
        self.advance(parts, resolutions, moving.core, &at)
    }

    /// Takes a move on from where it stopped: its effect appended, acted on once, its result recorded, and the
    /// part's value released once it moved.
    fn advance(
        &mut self,
        parts: &mut Parts<'_>,
        resolutions: &mut dyn Resolutions,
        core: &Actor,
        at: &str,
    ) -> Result<Moved, ResolveError> {
        let motion = self.moving.get_mut(at).ok_or(ResolveError::NoDecision)?;
        let receipt = motion.effect.append(parts.log)?;
        let prior = motion.resolved;
        let resolved = match prior {
            Some(resolved) => resolved,
            None => {
                let effect = effect_of(
                    motion.action.clone(),
                    motion.permit.clone(),
                    receipt.clone(),
                );
                if !self.acted.insert(receipt_key(&receipt)) {
                    return Err(ResolveError::AlreadyActed(receipt));
                }
                let resolved = resolutions.resolve(&effect);
                let motion = self.moving.get_mut(at).ok_or(ResolveError::NoDecision)?;
                motion.resolved = Some(resolved);
                resolved
            }
        };
        let motion = self.moving.get_mut(at).ok_or(ResolveError::NoDecision)?;
        if motion.result.is_none() {
            let outcome = match resolved {
                Resolved::Done => Outcome::Done {},
                Resolved::Failed => Outcome::Failed {},
            };
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
        if resolved == Resolved::Done {
            let part = key(&motion.order, &motion.part);
            if let Some(declared) = self.declared.get_mut(&part) {
                declared.resolved = true;
                for (segment, _) in &declared.segments {
                    parts.exposures.release(&motion.order, segment);
                }
                declared.resting = None;
            }
            // The order's stuck mark goes only once it holds nothing more: another part, stuck or still in flight,
            // keeps its holds and the mark, which over-reports a stuck exposure rather than drop a live one
            // (`REQ-ORD-8`).
            let unresolved = self.declared.iter().any(|((o, _), d)| {
                o == motion.order.as_str() && d.state == OrderState::Stuck && !d.resolved
            });
            let holds = parts.exposures.held().any(|h| h.order == motion.order);
            if !unresolved && !holds {
                parts.exposures.release_order(&motion.order);
            }
            // Every other decision kept for the part is spent: it is resolved once.
            self.kept
                .retain(|_, k| key(&k.request.order, &k.part) != part);
        }
        Ok(Moved::CarriedOut {
            effect: receipt,
            resolved,
            result,
        })
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

    /// What a declared part holds in `asset`: in flight at its providers, and resting at its node.
    fn value_of(
        &self,
        exposures: &Exposures,
        order: &Id,
        declared: &Declared,
        asset: &Denomination,
    ) -> Amount {
        let mut sum = Amount::zero();
        for h in exposures.stuck() {
            if &h.order == order
                && &h.asset == asset
                && declared.segments.iter().any(|(s, _)| s == &h.segment)
            {
                sum = sum.checked_add(&h.amount).unwrap_or(sum);
            }
        }
        if let Some((a, amount)) = &declared.resting
            && a == asset
        {
            sum = sum.checked_add(amount).unwrap_or(sum);
        }
        sum
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
            parts.exposures.mark_stuck(&order);
        }
        self.declared.insert(
            at.clone(),
            Declared {
                state,
                since: parts.clock.now(),
                segments,
                resting: (state == OrderState::Stuck).then_some(resting).flatten(),
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
/// what its last settled segment handed on there, in the asset it arrived in.
fn resting(
    segments: &Segments,
    exposures: &Exposures,
    order: &Id,
    part: &Id,
) -> Option<(Denomination, Amount)> {
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
    let amount = segments.handed_on(order, &Id::new(settled.segment_id.as_str())?)?;
    Some((settled.to.asset.clone(), amount.clone()))
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

fn receipt_key(receipt: &Receipt) -> String {
    format!("{}:{}", receipt.log.as_str(), u64::from(receipt.index))
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
    use crate::snapshot::Reading;

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

    fn declared(stuck: &mut Stuck, order: &str, since: u64, amount: &str) {
        stuck.declared.insert(
            (order.to_owned(), "p1".to_owned()),
            Declared {
                state: OrderState::Stuck,
                since: Reading::at(since),
                segments: vec![(id(&format!("{order}-s1")), id("reserve"))],
                resting: Some((cop(), Amount::new(amount).unwrap())),
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
        let mut stuck = Stuck::new();
        for n in 0..50 {
            declared(&mut stuck, &format!("small-{n:02}"), 1_000 + n, "1000");
        }
        declared(&mut stuck, "large", 4_540, "500000000");
        declared(&mut stuck, "resolved", 0, "900000000");
        if let Some(d) = stuck
            .declared
            .get_mut(&("resolved".to_owned(), "p1".to_owned()))
        {
            d.resolved = true;
        }
        declared(&mut stuck, "waiting", 0, "800000000");
        if let Some(d) = stuck
            .declared
            .get_mut(&("waiting".to_owned(), "p1".to_owned()))
        {
            d.state = OrderState::WaitingForCustomer;
        }
        let seen = stuck.stuck_values(&Exposures::new(), &At(4_600));
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
}
