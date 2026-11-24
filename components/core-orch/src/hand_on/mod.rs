//! A failed final deposit and a route handed on.
//!
//! Declared by `T-020-01`; `T-020-13` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! A failed final deposit never rolls the order back (`REQ-ORD-43`): the value stays at the node the deposit left
//! it at, and the first answer is a recalculation to the same destination, [`Recalculations`](crate::recalc)'s
//! (`REQ-ORD-22`). With no route to it left, `T-020-11` records the state machine's `no_route`, which has the part
//! wait for the customer there. Only from there does this module act, on the customer's choice, which the customer
//! accepts (`REQ-ORD-34`, `REQ-AUZ-3`):
//!
//! 1. **New terms only from a waiting part** ([`new_terms`], [`authorise`]). Policy holds who gives new terms, not
//!    when (`T-021-04`'s hand-off): Core asks policy to register the customer's new authorisation only where every
//!    part it speaks for waits for the customer, the customer gives it under an id of its own, and, where it moves
//!    the destination or the to asset, each of those parts waits at a final deposit that failed (`INV-10`). A new
//!    floor alone may resolve any wait. A split order's other part goes on under its own terms.
//! 2. **Another destination, from where the value sits** ([`HandOn::change_destination`]). The router's choice to
//!    the customer's new destination is held to the customer's new authorisation as the log holds it in force for
//!    the part ([`fresh_terms`]), never the caller's ([`check_route`]), then to the state
//!    machine's rules ([`destination_change`]: it changes the destination and starts where the value sits), and
//!    recorded as `destination_changed`, citing the choice round's decision record and the customer's acceptance.
//!    The order Core follows takes it, and the value's rest hold goes: the new route's segments hold it as they run.
//! 3. **Handed on to a linked order** ([`HandOn::hand_on`]). The linked order's route starts at the inner node where
//!    the value sits, with no withdrawal: its route is opened ([`Stm::open`]) and held to the first order by the
//!    start rule across the two orders, `handed_on_route_starts_where_value_sits`, before anything is recorded
//!    (`INV-4`). The customer's hand-on is the customer's own word, on its given acceptance of the linked order,
//!    which keeps the original customer as requester (`REQ-ORD-13`), and only after a failed final deposit
//!    (`INV-10`). Everything the linked order's `route_chosen` needs is checked first: the order not open, its
//!    decision record in the log, its plan within its acceptance. Then the first order's `handed_on`, naming the
//!    linked order and its acceptance, and the linked order's `route_chosen`, naming `handed_on_from`, are recorded
//!    in that order, so the two records point to each other (`handed-on-route-points-back`). Each is appended as
//!    built while the log can't take it now; a `route_chosen` the log refuses outright after the `handed_on` went in
//!    is recorded as the linked order's refusal ([`HandOnError::LinkedRefused`]).
//! 4. **The holding account's record proves the start** ([`HandOn::prove_start`], `REQ-ORD-40`, `INV-5`). No
//!    segment of a route handed on is handed over until the holding account's own record of the withdrawal, made by
//!    the linked order's starting reserve (from Core's records, never the caller's), at the route's first node, of the
//!    first order's value and of all the part held, is held for it ([`proves_start`]), one record for one start; the
//!    segments refuse it before then, and refuse an inner-start route of an order they don't follow
//!    ([`SegmentsError::StartUnproven`](crate::segments::SegmentsError::StartUnproven)).
//!
//! 5. **A wait at the floor, resumed** ([`HandOn::resume`], `REQ-ORD-27`). A part waiting for the customer resumes
//!    on a route to its own destination under the customer's new terms (a lower floor, say), which the customer
//!    accepted, held to them and recorded as `route_recalculated`; it changes no destination (`INV-10`).
//!
//! **The operator's hand-on** ([`Basis::Resolution`]). The operator hands a stuck order's value on only as a stuck
//! resolution, decided and moved under `REQ-AUZ-4` ([`Stuck`]); once its `resolve_stuck`
//! effect's result is done, this module records the part's `handed_on` and the linked order's route the same way
//! (`stuck-resolved-follows-a-done-resolution`).
//!
//! Every value-moving effect of the linked order, or of the new route, stays under policy's decision and its own
//! authorisation, as every move does (`F-021`).
//!
//! **In memory only.** The pending changes, resumes and hand-ons, the refusals recorded for linked orders, the linked
//! orders' starts and the spent records live in [`HandOn`]; after a restart a linked order's start can't be proven
//! again and its segments are refused, failing closed. Keeping them durably is `F-030`'s.

mod rule;

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Authorisations, GeneralEntry};
use rutent_contracts::core_internal::{
    Action, Actor, Authorisation, Decision, Entry, Outcome, Receipt, Resolution, Role, RouteEnd,
    TimeSource, VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::router_api::{DecisionRecord, Endpoint, Id, PositiveAmount};
use rutent_contracts::state_machine::{
    self as stm, Observed, OrderState, PartPlan, RecordedInput, SegmentPlan, SegmentState, Source,
    SourceKind, Standing, StateMachine, Subject,
};
use rutent_prov::holding::wallet::WithdrawalRecord;
use rutent_stm::{HandOnRefusal, Stm, handed_on_route_starts_where_value_sits};

use crate::recalc::{Rejected, check_route, route_end};
use crate::rounds::Fresh;
use crate::segments::Segments;
use crate::snapshot::{Clock, Exposures, OrchestrationLog};
use crate::stuck::{Stuck, StuckError, is_receipt_of};

pub use rule::{
    Accepted, InForce, Refused, Unproven, acceptance_of, after_failed_final_deposit,
    destination_change, fresh_terms, in_force, new_terms, proves_start, requester_of, waited_from,
};

/// How many times in all an append the log can't take now is tried.
const APPEND_TRIES: usize = 3;

/// How many fresh identifiers are drawn for a segment before the source is taken as broken.
const DRAWS: usize = 4;

/// What the hand-on reaches. The assembly builds it; Core's clock, segments and exposures are those the rest of
/// Core uses.
pub struct Parts<'a> {
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Orchestration's handle on the log.
    pub log: &'a mut dyn OrchestrationLog,
    /// Fresh identifiers.
    pub fresh: &'a mut dyn Fresh,
    /// The segments Core handed over, and the orders it follows.
    pub segments: &'a mut Segments,
    /// Core's record of value in flight and at rest.
    pub exposures: &'a mut Exposures,
    /// Core's stuck record, whose ends for each part are followed again once a new destination is taken
    /// ([`Stuck::follow`]), so its ends check is never stale.
    pub stuck: &'a mut Stuck,
}

/// Why the hand-on did nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandOnError {
    /// Core's own rules refuse it: nothing was asked or recorded.
    Refused(Refused),
    /// The router's choice for the customer's new destination isn't inside the new authorisation, or the state
    /// machine wouldn't take it: nothing was recorded.
    Rejected(Rejected),
    /// The linked order's route doesn't start where the first order's value sits, or the two orders aren't linked
    /// (`INV-4`, `REQ-ORD-43`): nothing was recorded.
    StartRule(HandOnRefusal),
    /// The linked order's route isn't a route handed on the state machine opens: it starts with a withdrawal, or at
    /// an outer node, or breaks another of its rules.
    NotAHandedOnRoute,
    /// The acceptance isn't of the linked order (or of this order, for a new destination), or the log holds none.
    NotItsAcceptance,
    /// The linked order's acceptance names another requester than the first order's (`REQ-ORD-13`).
    NotTheRequester,
    /// The linked order's acceptance isn't the customer's own: given, in the customer's session, by the customer or
    /// its agent (`REQ-ORD-43`, `REQ-ORD-34`).
    NotTheCustomersAcceptance,
    /// The log holds no decision record of the linked order under the id its route cites
    /// (`input-cites-its-decision-record`): nothing was recorded.
    NoDecisionRecord,
    /// The linked order's parts aren't those its acceptance has a yes for, in order, each from the reserve its first
    /// segment runs at (`chosen-route-rests-on-its-orders-acceptance`): nothing was recorded.
    PlanOutsideItsAcceptance,
    /// The linked order is taken already: Core follows it, the log holds its `route_chosen` or a `handed_on` naming
    /// it, or another hand-on pending here names it. An order is opened once (`order-is-opened-once`).
    AlreadyOpened,
    /// A hand-on of this part is pending, built for another linked order (`pending`): a retry names that one. The
    /// first order's `handed_on` may be in the log already, so what was built is never recorded under another's id
    /// (`INV-4`); nothing was recorded.
    PendingForAnother {
        /// The linked order the pending hand-on was built for.
        pending: Id,
    },
    /// The log refused the linked order's `route_chosen` outright after the first order's `handed_on` went in: the
    /// refusal is recorded as the linked order's, so the part isn't left handed on to an order that silently never
    /// opened. The state machine has no input that takes a hand-on back: an item for the contract's author.
    LinkedRefused {
        /// The linked order.
        linked: Id,
        /// The receipt of its refusal's entry.
        refusal: Receipt,
    },
    /// The log holds no authorisation of the customer's in force for the part, permitted since it began to wait
    /// ([`fresh_terms`]), or the parts new terms speak for have different terms in force.
    NotItsAuthorisation,
    /// The operator's hand-on isn't stated by the operator.
    NotTheOperator,
    /// No done `resolve_stuck` effect handing this part on to the linked order is in the log
    /// (`stuck-resolved-follows-a-done-resolution`).
    NoDoneResolution,
    /// The holding account's record doesn't prove the route's start (`REQ-ORD-40`).
    Unproven(Unproven),
    /// Core's stuck record couldn't follow the order's ends again after its new terms ([`Stuck::follow`]). Where this
    /// answers a destination change, its input was recorded and the order Core follows took it.
    Follow(StuckError),
    /// Core follows no such order, or holds no amount at rest for the part.
    Unknown,
    /// The log refused or failed an append. One it can't take now is kept and appended as built by the next call.
    Append(AppendFailure),
    /// The source of fresh identifiers repeats ones the order used.
    NoFreshSegment,
    /// An identifier Core holds isn't one the state machine's input can carry.
    Id(&'static str),
    /// Core's clock reads no time a timestamp can hold.
    Clock,
}

impl From<Refused> for HandOnError {
    fn from(refused: Refused) -> Self {
        HandOnError::Refused(refused)
    }
}

impl From<AppendFailure> for HandOnError {
    fn from(failure: AppendFailure) -> Self {
        HandOnError::Append(failure)
    }
}

/// Asks policy to register `new`, the customer's new terms for `order`, only where [`new_terms`] holds them to a
/// waiting part (the gate `T-021-04` leaves to orchestration). The terms in force and the order's customer are read
/// from `recorded`, the log's entries as the log holds them, never from the caller ([`in_force`],
/// [`requester_of`]): every part `new` speaks for must have the same terms in force. Refused, with nothing asked,
/// otherwise. A caller that isn't the order's customer is told [`Refused::NotTheCustomer`] before anything of the
/// order, whether it exists or not.
///
/// Once policy permits terms that keep the destination (a new floor), `stuck` follows the order's ends again over
/// the log's entries read then, so its ends check is never stale (`T-020-11b`). Terms that move the destination are
/// followed once the order Core follows takes the change ([`HandOn::change_destination`]): until then the part's
/// destination is the old one, which `Stuck::follow` holds the authorisation to.
pub fn authorise(
    segments: &Segments,
    stuck: &mut Stuck,
    authorisations: &mut dyn Authorisations,
    new: &Authorisation,
    recorded: &mut dyn FnMut() -> Vec<Entry>,
) -> Result<Decision, HandOnError> {
    // The caller is the order's customer first, as the log's acceptance names it: policy can't be asked before the
    // gate holds the terms to a waiting part, so a caller that isn't the customer, of an order that exists or not, is
    // told one refusal, and only the customer is told more (`REQ-AUZ-7`).
    let entries = recorded();
    let requester = requester_of(&entries, &new.order)
        .filter(|requester| {
            new.given_by.role == Role::Customer && **requester == new.given_by.party
        })
        .ok_or(HandOnError::Refused(Refused::NotTheCustomer))?;
    let order = segments
        .order(&new.order)
        .ok_or(HandOnError::Refused(Refused::Unknown))?;
    let mut current: Option<&Authorisation> = None;
    for part in &order.parts {
        let speaks = new
            .parts
            .as_ref()
            .is_none_or(|ps| ps.iter().any(|p| p.as_str() == part.part_id.as_str()));
        if !speaks {
            continue;
        }
        let held = in_force(&entries, &new.order, part.part_id.as_str())
            .ok_or(HandOnError::NotItsAuthorisation)?
            .authorisation;
        if current.is_some_and(|c| c.authorisation != held.authorisation) {
            return Err(HandOnError::NotItsAuthorisation);
        }
        current = Some(held);
    }
    let current = current.ok_or(HandOnError::Refused(Refused::NotItsAuthorisation))?;
    new_terms(order, current, new, requester)?;
    let keeps_ends = new.destination == current.destination && new.to_asset == current.to_asset;
    let decision = authorisations.authorise(new)?;
    if decision.permit().is_some() && keeps_ends {
        stuck
            .follow(segments, &recorded(), &new.order)
            .map_err(HandOnError::Follow)?;
    }
    Ok(decision)
}

/// The customer's new destination for one part.
#[derive(Debug, Clone, Copy)]
pub struct Change<'a> {
    /// The order.
    pub order: &'a Id,
    /// The part waiting for the customer.
    pub part: &'a Id,
    /// The choice round's record of the route to the new destination, from where the value sits.
    pub record: &'a DecisionRecord,
    /// The customer's acceptance of the new destination, which the log holds: of this order, accepting the new
    /// authorisation.
    pub acceptance: &'a Id,
    /// The log's entries, as the log holds them: where the customer's new authorisation, in force for the part
    /// ([`fresh_terms`]), and the acceptance are read from. The new route is held to that authorisation.
    pub recorded: &'a [Entry],
    /// The customer, whose choice the input records.
    pub customer: &'a Actor,
    /// Core, acting on its own account: who writes the entry.
    pub core: &'a Actor,
}

/// A destination changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// The `destination_changed` input, as recorded.
    pub input: RecordedInput,
    /// The receipt of its entry.
    pub receipt: Receipt,
    /// The id Core gave each segment of the new route, in order.
    pub segments: Vec<Id>,
    /// What each move on the new route ends at, read from the route ([`route_end`]): policy and the log hold it to
    /// the customer's new authorisation (`move-rests-on-its-authorisation`).
    pub route: RouteEnd,
    /// Where the value sat, and so where the new route starts.
    pub from: Endpoint,
    /// What sat there, all of which the new route moves.
    pub amount: PositiveAmount,
}

/// A part handed on whose linked order the log refused: the refusal's receipt, and the part's `hand_on_taken_back`,
/// appended as built until the log takes it.
#[derive(Debug, Clone)]
struct Stranded {
    linked: Id,
    refusal: Receipt,
    taken_back: (Step, RecordedInput),
}

/// A route handed on's start, as the holding account recorded it.
#[derive(Debug, Clone, Copy)]
pub struct Start<'a> {
    /// The linked order whose route it starts.
    pub linked: &'a Id,
    /// The holding account's own record of its withdrawal, as its module answered it.
    pub record: &'a WithdrawalRecord,
    /// The receipt of the holding account's stated entry of that withdrawal, which the log holds verified: the
    /// record the `holding_withdrawal` cites (`holding-withdrawal-rests-on-its-record`).
    pub statement: &'a Receipt,
    /// Core, acting on its own account: who writes the `holding_withdrawal`.
    pub core: &'a Actor,
    /// The party whose private node the route starts at, where the node is declared private (the catalogue's word,
    /// which the assembly reads): the record is then its publisher's word, labelled as such, and the log holds the
    /// label to the party the record's maker states for (`REQ-ORD-40`, `holding-withdrawal-rests-on-its-record`).
    /// `None` at a public node, where the record is the operator's own module's.
    pub publisher: Option<&'a Id>,
}

/// A part waiting for the customer, resumed on a route to its own destination under the customer's new terms.
#[derive(Debug, Clone, Copy)]
pub struct Resume<'a> {
    /// The order.
    pub order: &'a Id,
    /// The part waiting for the customer.
    pub part: &'a Id,
    /// The choice round's record of the route from where the value sits, routed under the new terms' floor.
    pub record: &'a DecisionRecord,
    /// The customer's acceptance of the new terms, which the log holds: of this order, accepting the new
    /// authorisation, which every move on the route rests on (`move-rests-on-its-orders-acceptance`, `REQ-ORD-34`).
    pub acceptance: &'a Id,
    /// The log's entries, as the log holds them: where the customer's fresh terms, in force for the part
    /// ([`fresh_terms`]), and the acceptance are read from. The route is held to those terms.
    pub recorded: &'a [Entry],
    /// Core, acting on its own account: who writes the input and its entry.
    pub core: &'a Actor,
}

/// A waiting part resumed.
pub type Resumed = Changed;

/// On whose word a part is handed on.
#[derive(Debug, Clone, Copy)]
pub enum Basis<'a> {
    /// The customer's own choice for a part waiting for it: its acceptance of the linked order is the whole of it
    /// (`REQ-ORD-43`).
    Customer,
    /// The operator's, for a stuck part, as a stuck resolution under `REQ-AUZ-4`: the receipt of the `resolve_stuck`
    /// effect that handed the value on, whose result is done.
    Resolution {
        /// The effect's receipt.
        effect: &'a Receipt,
    },
}

/// One part handed on to a linked order.
#[derive(Debug, Clone, Copy)]
pub struct Handing<'a> {
    /// The order handed on.
    pub order: &'a Id,
    /// Its part whose value is handed on.
    pub part: &'a Id,
    /// The linked order.
    pub linked: &'a Id,
    /// The linked order's route, as its decision record's id and its parts' plans, each starting where the value
    /// sits, with no withdrawal.
    pub decision_record: &'a Id,
    /// The linked order's parts.
    pub plan: &'a [PartPlan],
    /// The customer's acceptance of the linked order, which the log holds.
    pub acceptance: &'a Id,
    /// On whose word.
    pub basis: Basis<'a>,
    /// The log's entries with their receipts, as the log holds them: where the acceptances, and an operator's
    /// resolution, are read from.
    pub recorded: &'a [(Receipt, Entry)],
    /// Who states the linked order's route: the customer, or the operator.
    pub by: &'a Actor,
    /// Core, acting on its own account: who writes the entries.
    pub core: &'a Actor,
}

/// A part handed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandedOn {
    /// The first order's `handed_on` input, as recorded.
    pub handed_on: RecordedInput,
    /// The receipt of its entry.
    pub handed_on_receipt: Receipt,
    /// The linked order's `route_chosen`, naming `handed_on_from`, as recorded.
    pub route_chosen: RecordedInput,
    /// The receipt of its entry.
    pub route_chosen_receipt: Receipt,
    /// Where the value sits, where the linked order's route starts.
    pub at: Endpoint,
}

/// A start built and not yet taken, with what it was built from: a retry passes the same record and statement, or is
/// refused.
#[derive(Debug, Clone)]
struct KeptStart {
    step: Step,
    record: WithdrawalRecord,
    statement: Receipt,
    publisher: Option<Id>,
}

/// An entry built once, and its receipt once the log took it.
#[derive(Debug, Clone)]
struct Step {
    entry: GeneralEntry,
    receipt: Option<Receipt>,
}

impl Step {
    fn append(&mut self, log: &mut dyn OrchestrationLog) -> Result<Receipt, AppendFailure> {
        if let Some(receipt) = &self.receipt {
            return Ok(receipt.clone());
        }
        let mut last = AppendFailure::Unavailable;
        for _ in 0..APPEND_TRIES {
            match log.append(&self.entry) {
                Ok(receipt) if is_receipt_of(&receipt, &self.entry) => {
                    self.receipt = Some(receipt.clone());
                    return Ok(receipt);
                }
                Ok(_) | Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
                Err(refused) => return Err(refused),
            }
        }
        Err(last)
    }
}

/// A destination change built: appended as built while the log can't take it now, dropped where it refuses it.
#[derive(Debug, Clone)]
struct PendingChange {
    step: Step,
    changed: Changed,
    /// The segments whose holds go once it is taken: the settled one the value rests under, and those retired.
    released: Vec<Id>,
}

/// A resumption built: appended as built while the log can't take it now, dropped where it refuses it.
#[derive(Debug, Clone)]
struct PendingResume {
    step: Step,
    resumed: Resumed,
    released: Vec<Id>,
}

/// A hand-on built: appended as built while the log can't take it now; its `handed_on` dropped where the log refuses
/// it, its `route_chosen` recorded as the linked order's refusal where the log refuses that.
#[derive(Debug, Clone)]
struct PendingHandOn {
    handed_on: (Step, RecordedInput),
    route_chosen: (Step, RecordedInput),
    /// The linked order's refusal, appended only where the log refuses its `route_chosen` outright.
    refusal: Step,
    at: Endpoint,
    released: Vec<Id>,
    /// The linked order's start, as Core's records name it.
    linked: Linked,
}

/// A linked order's start: the first order, the part's value, and the route's first segment.
#[derive(Debug, Clone)]
struct Linked {
    from: Id,
    amount: rutent_contracts::router_api::Amount,
    reserve: Id,
    first: SegmentPlan,
}

/// Core's record of the parts it resolves on the customer's choice and the orders it hands value on to, which the
/// production assembly keeps durably (`F-030`), as it does [`Segments`].
#[derive(Debug, Clone, Default)]
pub struct HandOn {
    changes: BTreeMap<(String, String), PendingChange>,
    hand_ons: BTreeMap<(String, String), PendingHandOn>,
    resumes: BTreeMap<(String, String), PendingResume>,
    /// The parts handed on whose linked order the log refused, with that refusal's receipt.
    stranded: BTreeMap<(String, String), Stranded>,
    /// The starts of routes handed on built and not yet taken by the log, by linked order.
    starts: BTreeMap<String, KeptStart>,
    /// The holding accounts' records spent, by provider and reference, with the linked order each proved.
    spent: BTreeMap<(String, String), String>,
    /// The starts the log took, by linked order: a linked order has one start, so a second, different record or
    /// statement is refused, and the same entry again is answered with its receipt.
    started: BTreeMap<String, Started>,
    linked: BTreeMap<String, Linked>,
}

/// A linked order's start as the log took it.
#[derive(Debug, Clone)]
struct Started {
    record: WithdrawalRecord,
    statement: Receipt,
    publisher: Option<Id>,
    receipt: Receipt,
}

impl HandOn {
    /// Nothing handed on yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the customer's new destination for a waiting part (see the module's docs). A change built and not
    /// yet taken is appended as built, and nothing is routed or checked again.
    pub fn change_destination(
        &mut self,
        parts: &mut Parts<'_>,
        change: Change<'_>,
    ) -> Result<Changed, HandOnError> {
        let at = key(change.order, change.part);
        if !self.changes.contains_key(&at) {
            let built = Self::build_change(parts, &change)?;
            self.changes.insert(at.clone(), built);
        }
        let pending = self.changes.get_mut(&at).ok_or(HandOnError::Unknown)?;
        let receipt = match pending.step.append(parts.log) {
            Ok(receipt) => receipt,
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            Err(refused) => {
                self.changes.remove(&at);
                return Err(refused.into());
            }
        };
        let pending = self.changes.remove(&at).ok_or(HandOnError::Unknown)?;
        let mut changed = pending.changed;
        changed.receipt = receipt;
        if parts.segments.observe(parts.clock, &changed.input) {
            for segment in &pending.released {
                parts.exposures.release(change.order, segment);
            }
            // The part's destination is the customer's now: Core's stuck record follows its new ends
            // (`T-020-11b`), read from the log's entries, the new authorisation's among them.
            parts
                .stuck
                .follow(parts.segments, change.recorded, change.order)
                .map_err(HandOnError::Follow)?;
        }
        Ok(changed)
    }

    fn build_change(
        parts: &mut Parts<'_>,
        change: &Change<'_>,
    ) -> Result<PendingChange, HandOnError> {
        let order = parts
            .segments
            .order(change.order)
            .cloned()
            .ok_or(Refused::Unknown)?;
        let part = order
            .parts
            .iter()
            .find(|p| p.part_id.as_str() == change.part.as_str())
            .ok_or(Refused::Unknown)?;
        if part.state != OrderState::WaitingForCustomer {
            return Err(Refused::NotWaiting(part.state).into());
        }
        // The customer's fresh terms, as the log holds them in force for the part, never the caller's
        // (`REQ-AUZ-3`): the terms the part waited under are no answer.
        let authorisation = fresh_terms(change.recorded, change.order, change.part.as_str())
            .ok_or(HandOnError::NotItsAuthorisation)?;
        let (from, settled, amount) = at_rest(parts.segments, change.order, part)?;
        let (plan, ids, route) = plan_route(
            parts.fresh,
            &order,
            change.record,
            &from,
            &amount,
            authorisation,
        )?;
        // The state machine's rules: another destination, from where the value sits (`INV-4`, `INV-10`).
        destination_change(part, &plan)?;
        let input = Self::part_input(
            parts,
            change.order,
            change.part,
            change.customer,
            SourceKind::Customer,
            Observed::DestinationChanged {
                decision_record: stm_id(change.record.record_id().as_str())?,
                acceptance: stm_id(change.acceptance.as_str())?,
                segments: plan,
            },
        )?;
        let after = Stm::transition(&order, &input);
        if after.last_input.outcome != (stm::Outcome::Applied {}) {
            return Err(HandOnError::Rejected(Rejected::NotApplicable(
                after.last_input.outcome,
            )));
        }
        // The customer's acceptance of the new destination: of this order, accepting the new authorisation
        // (`changed-destination-rests-on-its-acceptance`, `REQ-ORD-34`).
        match acceptance_of(change.recorded, change.acceptance) {
            Some(accepted)
                if accepted.order == change.order
                    && *accepted.authorisation == authorisation.authorisation => {}
            _ => return Err(HandOnError::NotItsAcceptance),
        }
        let step = built(parts, change.core, &input)?;
        let mut released = vec![settled];
        released.extend(retiring(part));
        Ok(PendingChange {
            step,
            changed: Changed {
                input,
                receipt: placeholder(),
                segments: ids,
                route,
                from,
                amount,
            },
            released,
        })
    }

    /// Resumes a part waiting for the customer on a route to its own destination, under the customer's new terms in
    /// force for it (a lower floor, say; [`authorise`]): the waits at the floor `T-020-10b` records end this way
    /// (`REQ-ORD-27`, `AC-12`). The router's choice is held to the customer's fresh terms as the log holds them in
    /// force for the part ([`fresh_terms`], [`check_route`]): from where the value sits, all of it, to their
    /// destination and to asset, leaving their floor; and to the state machine's rules, as a `route_recalculated`,
    /// citing the choice round's decision record, once the log holds the customer's acceptance of those terms. It
    /// changes no destination (`INV-10`). Built once, appended as built while the log can't take it now and dropped
    /// where it refuses it; the order Core follows then takes it, and the next recalculation follows that round
    /// ([`Recalculations::followed_by`](crate::recalc::Recalculations::followed_by)).
    pub fn resume(
        &mut self,
        parts: &mut Parts<'_>,
        resume: Resume<'_>,
    ) -> Result<Resumed, HandOnError> {
        let at = key(resume.order, resume.part);
        if !self.resumes.contains_key(&at) {
            let built = Self::build_resume(parts, &resume)?;
            self.resumes.insert(at.clone(), built);
        }
        let pending = self.resumes.get_mut(&at).ok_or(HandOnError::Unknown)?;
        let receipt = match pending.step.append(parts.log) {
            Ok(receipt) => receipt,
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            Err(refused) => {
                self.resumes.remove(&at);
                return Err(refused.into());
            }
        };
        let pending = self.resumes.remove(&at).ok_or(HandOnError::Unknown)?;
        let mut resumed = pending.resumed;
        resumed.receipt = receipt;
        if parts.segments.observe(parts.clock, &resumed.input) {
            for segment in &pending.released {
                parts.exposures.release(resume.order, segment);
            }
        }
        Ok(resumed)
    }

    fn build_resume(
        parts: &mut Parts<'_>,
        resume: &Resume<'_>,
    ) -> Result<PendingResume, HandOnError> {
        let order = parts
            .segments
            .order(resume.order)
            .cloned()
            .ok_or(Refused::Unknown)?;
        let part = order
            .parts
            .iter()
            .find(|p| p.part_id.as_str() == resume.part.as_str())
            .ok_or(Refused::Unknown)?;
        if part.state != OrderState::WaitingForCustomer {
            return Err(Refused::NotWaiting(part.state).into());
        }
        // The customer's fresh terms, as the log holds them in force for the part, never the caller's
        // (`REQ-AUZ-3`): the terms the part waited under are no answer.
        let authorisation = fresh_terms(resume.recorded, resume.order, resume.part.as_str())
            .ok_or(HandOnError::NotItsAuthorisation)?;
        let (from, settled, amount) = at_rest(parts.segments, resume.order, part)?;
        let (plan, ids, route) = plan_route(
            parts.fresh,
            &order,
            resume.record,
            &from,
            &amount,
            authorisation,
        )?;
        if !stm::recalculation_keeps_destination(part, &plan) {
            return Err(HandOnError::Rejected(Rejected::ChangesDestination));
        }
        if !stm::recalculation_starts_where_value_sits(part, &plan) {
            return Err(HandOnError::Rejected(Rejected::StartsElsewhere));
        }
        let input = Self::part_input(
            parts,
            resume.order,
            resume.part,
            resume.core,
            SourceKind::Orchestrator,
            Observed::RouteRecalculated {
                decision_record: stm_id(resume.record.record_id().as_str())?,
                segments: plan,
            },
        )?;
        let after = Stm::transition(&order, &input);
        if after.last_input.outcome != (stm::Outcome::Applied {}) {
            return Err(HandOnError::Rejected(Rejected::NotApplicable(
                after.last_input.outcome,
            )));
        }
        match acceptance_of(resume.recorded, resume.acceptance) {
            Some(accepted)
                if accepted.order == resume.order
                    && *accepted.authorisation == authorisation.authorisation => {}
            _ => return Err(HandOnError::NotItsAcceptance),
        }
        let step = built(parts, resume.core, &input)?;
        let mut released = vec![settled];
        released.extend(retiring(part));
        Ok(PendingResume {
            step,
            resumed: Resumed {
                input,
                receipt: placeholder(),
                segments: ids,
                route,
                from,
                amount,
            },
            released,
        })
    }

    /// Hands a part's value on to a linked order (see the module's docs): the first order's `handed_on`, then the
    /// linked order's `route_chosen` naming `handed_on_from`. Everything the `route_chosen` needs is checked before
    /// the first is appended. A hand-on built is appended as built while the log can't take it now, and nothing is
    /// checked again; one whose `handed_on` the log refuses is dropped. Where the log refuses the `route_chosen`
    /// outright after the `handed_on` went in, the linked order's refusal is recorded once and is the answer from
    /// then ([`HandOnError::LinkedRefused`]).
    pub fn hand_on(
        &mut self,
        parts: &mut Parts<'_>,
        handing: Handing<'_>,
    ) -> Result<HandedOn, HandOnError> {
        let at = key(handing.order, handing.part);
        if self.stranded.contains_key(&at) {
            return self.take_back(parts, &at);
        }
        if !self.hand_ons.contains_key(&at) {
            let built = Self::build_hand_on(parts, &handing)?;
            self.hand_ons.insert(at.clone(), built);
        }
        // Before the first record, and again before each retry of it: the linked order is free, in the log and
        // among the hand-ons pending here, so the `route_chosen` that follows is the linked order's first
        // (`order-is-opened-once`). A hand-on whose linked order was taken meanwhile is dropped, nothing recorded.
        // One whose own `handed_on` the log took already, its answer lost, is past that point: it goes on to the
        // `route_chosen`, so Core and the log never part on whether the part was handed on.
        if self.hand_ons.get(&at).is_some_and(|p| {
            p.handed_on.0.receipt.is_none() && !in_the_log(handing.recorded, &p.handed_on.1)
        }) && let Err(taken) = self.linked_is_free(parts.segments, &handing, &at)
        {
            self.hand_ons.remove(&at);
            return Err(taken);
        }
        // A hand-on pending was built for one linked order, its `handed_on` perhaps in the log already: a retry names
        // that one. It is never recorded under another's id (`INV-4`); the caller isn't told another was handed on.
        let pending = self.hand_ons.get(&at).ok_or(HandOnError::Unknown)?;
        if pending.route_chosen.1.subject.order.as_str() != handing.linked.as_str() {
            return Err(HandOnError::PendingForAnother {
                pending: Id::new(pending.route_chosen.1.subject.order.as_str())
                    .ok_or(HandOnError::Id("a linked order"))?,
            });
        }
        let pending = self.hand_ons.get_mut(&at).ok_or(HandOnError::Unknown)?;
        // The first order's record first: the linked order's points back to it (`handed-on-route-points-back`).
        let handed_on_receipt = match pending.handed_on.0.append(parts.log) {
            Ok(receipt) => receipt,
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            Err(refused) => {
                self.hand_ons.remove(&at);
                return Err(refused.into());
            }
        };
        // Once the first order is handed on, the linked order's route is appended as built while the log can't take
        // it now. One the log refuses outright is the linked order's refusal, recorded, so the part handed on isn't
        // left with no record of why its linked order never opened.
        let route_chosen_receipt = match pending.route_chosen.0.append(parts.log) {
            Ok(receipt) => receipt,
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            Err(_) => {
                let refusal = pending.refusal.append(parts.log)?;
                let pending = self.hand_ons.remove(&at).ok_or(HandOnError::Unknown)?;
                let _ = parts.segments.observe(parts.clock, &pending.handed_on.1);
                // The hand-on is taken back on the linked order's refusal: the part waits again where its final
                // deposit failed, or else is stuck (`hand-on-taken-back-rests-on-the-linked-orders-refusal`).
                let input = Self::part_input(
                    parts,
                    handing.order,
                    handing.part,
                    handing.core,
                    SourceKind::Orchestrator,
                    Observed::HandOnTakenBack {
                        linked_order: stm_id(handing.linked.as_str())?,
                    },
                )?;
                let step = built(parts, handing.core, &input)?;
                self.stranded.insert(
                    at.clone(),
                    Stranded {
                        linked: handing.linked.clone(),
                        refusal,
                        taken_back: (step, input),
                    },
                );
                return self.take_back(parts, &at);
            }
        };
        let pending = self.hand_ons.remove(&at).ok_or(HandOnError::Unknown)?;
        self.linked
            .insert(handing.linked.as_str().to_owned(), pending.linked.clone());
        let (handed_on, route_chosen) = (pending.handed_on.1, pending.route_chosen.1);
        if parts.segments.observe(parts.clock, &handed_on) {
            for segment in &pending.released {
                parts.exposures.release(handing.order, segment);
            }
        }
        parts.segments.follow(parts.clock, &route_chosen);
        Ok(HandedOn {
            handed_on,
            handed_on_receipt,
            route_chosen,
            route_chosen_receipt,
            at: pending.at,
        })
    }

    /// Appends the `hand_on_taken_back` of the part at `at`, whose linked order the log refused, as built, and once
    /// the log takes it has the order Core follows take it; answers the refusal ([`HandOnError::LinkedRefused`]).
    fn take_back(
        &mut self,
        parts: &mut Parts<'_>,
        at: &(String, String),
    ) -> Result<HandedOn, HandOnError> {
        let stranded = self.stranded.get_mut(at).ok_or(HandOnError::Unknown)?;
        match stranded.taken_back.0.append(parts.log) {
            Ok(_) => {
                let stranded = self.stranded.remove(at).ok_or(HandOnError::Unknown)?;
                let _ = parts.segments.observe(parts.clock, &stranded.taken_back.1);
                Err(HandOnError::LinkedRefused {
                    linked: stranded.linked,
                    refusal: stranded.refusal,
                })
            }
            Err(AppendFailure::Unavailable) => Err(AppendFailure::Unavailable.into()),
            Err(refused) => {
                self.stranded.remove(at);
                Err(refused.into())
            }
        }
    }

    /// Whether `handing`'s linked order is free for the hand-on at `at`: Core doesn't follow it, the log holds no
    /// `route_chosen` of it and no `handed_on` naming it but this hand-on's own, and no other hand-on pending here
    /// names it. After a restart the log is what tells, since Core's records may follow none of it.
    fn linked_is_free(
        &self,
        segments: &Segments,
        handing: &Handing<'_>,
        at: &(String, String),
    ) -> Result<(), HandOnError> {
        let linked = handing.linked.as_str();
        let own = self.hand_ons.get(at).map(|p| &p.handed_on.1.record_id);
        let pending_elsewhere = self
            .hand_ons
            .iter()
            .any(|(k, p)| k != at && p.route_chosen.1.subject.order.as_str() == linked);
        let in_the_log = handing.recorded.iter().any(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Input { input } => match &input.observed {
                    Observed::RouteChosen { .. } => input.subject.order.as_str() == linked,
                    Observed::HandedOn { linked_order, .. } => {
                        linked_order.as_str() == linked && Some(&input.record_id) != own
                    }
                    _ => false,
                },
                _ => false,
            },
            Entry::Stated(_) => false,
        });
        if segments.order(handing.linked).is_some() || pending_elsewhere || in_the_log {
            return Err(HandOnError::AlreadyOpened);
        }
        Ok(())
    }

    fn build_hand_on(
        parts: &mut Parts<'_>,
        handing: &Handing<'_>,
    ) -> Result<PendingHandOn, HandOnError> {
        let first = parts
            .segments
            .order(handing.order)
            .cloned()
            .ok_or(Refused::Unknown)?;
        let part = first
            .parts
            .iter()
            .find(|p| p.part_id.as_str() == handing.part.as_str())
            .ok_or(Refused::Unknown)?;
        let entries: Vec<Entry> = handing.recorded.iter().map(|(_, e)| e.clone()).collect();
        let requester = requester_of(&entries, handing.order).ok_or(HandOnError::Unknown)?;
        let (at, settled, amount) = at_rest(parts.segments, handing.order, part)?;
        // The linked order's starting reserve, the holding account's provider, from Core's records, never the
        // caller's: the reserve the customer's linked route starts at, or the provider the operator's resolution
        // names as holding the value.
        let reserve = match handing.basis {
            Basis::Customer => {
                // The customer's own word: the order's requester, acting as its customer.
                if handing.by.role != Role::Customer || handing.by.party != *requester {
                    return Err(Refused::NotTheCustomer.into());
                }
                // A part waiting for it; a stuck part is the operator's.
                if part.state != OrderState::WaitingForCustomer {
                    return Err(Refused::NotWaiting(part.state).into());
                }
                // A hand-on moves the value to another order's destination: only after a failed final deposit
                // (`INV-10`), as a new destination.
                if !after_failed_final_deposit(part) {
                    return Err(Refused::NoFailedDeposit.into());
                }
                handing
                    .plan
                    .first()
                    .and_then(|p| p.segments.first())
                    .and_then(|s| Id::new(s.provider.as_str()))
                    .ok_or(HandOnError::NotAHandedOnRoute)?
            }
            Basis::Resolution { effect } => {
                if handing.by.role != Role::Operator {
                    return Err(HandOnError::NotTheOperator);
                }
                if part.state != OrderState::Stuck {
                    return Err(Refused::NotWaiting(part.state).into());
                }
                done_hand_on(handing.recorded, effect, handing, &at, &amount)?
            }
        };
        // The linked order is opened once (`order-is-opened-once`).
        if parts.segments.order(handing.linked).is_some() {
            return Err(HandOnError::AlreadyOpened);
        }
        let accepted = linked_accepted(
            &entries,
            requester,
            handing.linked,
            handing.decision_record,
            handing.acceptance,
            handing.plan,
        )?;
        let now = now(parts.clock)?;
        let route_chosen = RecordedInput {
            record_id: stm::RecordId::new(parts.fresh.id().as_str())
                .ok_or(HandOnError::Id("a record id"))?,
            recorded_at: stm::Timestamp::new(now.as_str()).ok_or(HandOnError::Clock)?,
            time_source: stm_id("core_clock")?,
            source: source_of(handing.by)?,
            standing: Standing::Stated,
            subject: Subject {
                order: stm_id(handing.linked.as_str())?,
                part: None,
                segment: None,
            },
            observed: Observed::RouteChosen {
                decision_record: stm_id(handing.decision_record.as_str())?,
                parts: handing.plan.to_vec(),
                handed_on_from: Some(stm_id(handing.order.as_str())?),
            },
        };
        // The linked order opens as a route handed on: from an inner node, with no withdrawal.
        let second = Stm::open(&route_chosen).ok_or(HandOnError::NotAHandedOnRoute)?;
        // The start rule across the two orders: every part of the linked order starts where this part's value sits
        // (`INV-4`).
        handed_on_route_starts_where_value_sits(&first, &second).map_err(HandOnError::StartRule)?;
        if second
            .parts
            .iter()
            .any(|p| p.segments.first().map(|s| &s.from) != Some(&at))
        {
            return Err(HandOnError::StartRule(
                HandOnRefusal::StartsAwayFromTheValue,
            ));
        }
        let handed_on = Self::part_input(
            parts,
            handing.order,
            handing.part,
            handing.by,
            source_kind(handing.by)?,
            Observed::HandedOn {
                linked_order: stm_id(handing.linked.as_str())?,
                acceptance: stm_id(handing.acceptance.as_str())?,
            },
        )?;
        let after = Stm::transition(&first, &handed_on);
        if after.last_input.outcome != (stm::Outcome::Applied {}) {
            return Err(HandOnError::Rejected(Rejected::NotApplicable(
                after.last_input.outcome,
            )));
        }
        let handed_on_step = built(parts, handing.core, &handed_on)?;
        let route_chosen_step = built(parts, handing.core, &route_chosen)?;
        let refusal = built_what(
            parts,
            handing.core,
            // The linked order's own request, the one its acceptance was opened under; its route isn't one the log
            // takes now, the closest class there is (no class says the log refused it).
            What::OrderRefusal {
                request: accepted.request.clone(),
                order: Some(handing.linked.clone()),
                class: rutent_contracts::core_internal::OrderRefusalClass::NoRouteRightNow,
            },
        )?;
        let first_segment = handing
            .plan
            .first()
            .and_then(|p| p.segments.first())
            .cloned()
            .ok_or(HandOnError::NotAHandedOnRoute)?;
        let mut released = vec![settled];
        released.extend(retiring(part));
        Ok(PendingHandOn {
            handed_on: (handed_on_step, handed_on),
            route_chosen: (route_chosen_step, route_chosen),
            refusal,
            at,
            released,
            linked: Linked {
                from: handing.order.clone(),
                amount: amount.amount().clone(),
                reserve,
                first: first_segment,
            },
        })
    }

    /// Holds `record`, the holding account's own record of its withdrawal for `linked`, a route handed on, to the
    /// linked order's start as Core recorded it when it handed the value on ([`proves_start`], `REQ-ORD-40`): made
    /// by the linked order's starting reserve (the reserve its route starts at, or the provider the operator's
    /// resolution named), at its route's first node, in its asset, of the first order's value, and of all the part
    /// held. One record proves one start: its reference, once spent, proves no other linked order. Once proven, the
    /// segments hand the linked order's segments over; before, they refuse them. A linked order this record of
    /// Core's didn't hand on (after a restart, say) is [`HandOnError::Unknown`]: `F-030`'s.
    pub fn prove_start(
        &mut self,
        parts: &mut Parts<'_>,
        start: Start<'_>,
    ) -> Result<Receipt, HandOnError> {
        let (linked, record) = (start.linked, start.record);
        // A linked order has one start: the entry the log took, retried, is answered with its receipt; another
        // record or statement of it is refused, nothing recorded (`one-start-per-linked-order`).
        if let Some(taken) = self.started.get(linked.as_str()) {
            if &taken.record == record
                && &taken.statement == start.statement
                && taken.publisher.as_ref() == start.publisher
            {
                return Ok(taken.receipt.clone());
            }
            return Err(HandOnError::Unproven(Unproven::AlreadyStarted));
        }
        // A start built and not yet taken is appended as built.
        if let Some(kept) = self.starts.get_mut(linked.as_str()) {
            if &kept.record != record
                || &kept.statement != start.statement
                || kept.publisher.as_ref() != start.publisher
            {
                return Err(HandOnError::Unproven(Unproven::AnotherStart));
            }
            let receipt = Self::record_start(parts, &mut kept.step, linked)?;
            let kept = self.starts.remove(linked.as_str());
            if let Some(kept) = kept {
                self.started.insert(
                    linked.as_str().to_owned(),
                    Started {
                        record: kept.record,
                        statement: kept.statement,
                        publisher: kept.publisher,
                        receipt: receipt.clone(),
                    },
                );
            }
            return Ok(receipt);
        }
        let held = self
            .linked
            .get(linked.as_str())
            .ok_or(HandOnError::Unknown)?;
        proves_start(record, &held.reserve, &held.first, &held.from, &held.amount)
            .map_err(HandOnError::Unproven)?;
        let spent = (record.by.clone(), record.reference.clone());
        match self.spent.get(&spent) {
            Some(proved) if proved != linked.as_str() => {
                return Err(HandOnError::Unproven(Unproven::ReferenceSpent));
            }
            _ => {}
        }
        // The log's record of the start, on the holding account's own statement of it: the linked order's first move
        // rests on it (`handed-on-route-starts-on-its-holding-record`), and one statement starts one route
        // (`one-start-per-holding-record`).
        // The node the route handed on starts at: an inner node, where the holding account holds the value.
        let node = match &held.first.from.node {
            rutent_contracts::provider_role::NodeRef::Inner(node) => Id::new(node.as_str()),
            rutent_contracts::provider_role::NodeRef::Outer(_) => None,
        }
        .ok_or(HandOnError::NotAHandedOnRoute)?;
        let mut step = built_what(
            parts,
            start.core,
            What::HoldingWithdrawal {
                order: linked.clone(),
                handed_on_from: held.from.clone(),
                holder: held.reserve.clone(),
                node,
                record: start.statement.clone(),
                publisher: start.publisher.cloned(),
            },
        )?;
        let taken = Self::record_start(parts, &mut step, linked);
        match &taken {
            Ok(receipt) => {
                self.spent.insert(spent, linked.as_str().to_owned());
                self.started.insert(
                    linked.as_str().to_owned(),
                    Started {
                        record: record.clone(),
                        statement: start.statement.clone(),
                        publisher: start.publisher.cloned(),
                        receipt: receipt.clone(),
                    },
                );
            }
            Err(HandOnError::Append(AppendFailure::Unavailable)) => {
                self.spent.insert(spent, linked.as_str().to_owned());
                self.starts.insert(
                    linked.as_str().to_owned(),
                    KeptStart {
                        step,
                        record: record.clone(),
                        statement: start.statement.clone(),
                        publisher: start.publisher.cloned(),
                    },
                );
            }
            Err(_) => {}
        }
        taken
    }

    /// Rebuilds, from the log's `entries`, the starts a restarted Core's `segments` forget: each route handed on
    /// whose `holding_withdrawal` the log holds is proven again, so its segments are handed over without the
    /// holding account being asked again (`REQ-ORD-43`, `REQ-ORD-40`). Returns how many it proved.
    ///
    /// Fail closed: the record counts only as Core's own entry, for a linked order the log opened by a
    /// `route_chosen` that names the order it was handed on from, with a `handed_on` of that order naming it, from
    /// an inner node that is the node the record names, whose every part starts at that node; the record's holder is
    /// the one the operator's resolution named (the node's holding account, which may run no segment of the route),
    /// or, for a hand-on the customer made, the route's first provider; and only once for the order. Anything else proves nothing, and the order's segments are refused as
    /// before. The record's statement, and that one statement starts one route, are the log's own rows.
    pub fn recover_starts(segments: &mut Segments, entries: &[Entry]) -> usize {
        let mut proven = 0;
        let mut seen: Vec<&str> = Vec::new();
        for entry in entries {
            let Entry::Verified(v) = entry else {
                continue;
            };
            let What::HoldingWithdrawal {
                order,
                handed_on_from,
                holder,
                node,
                ..
            } = &v.what
            else {
                continue;
            };
            if v.who.role != Role::Core
                || !matches!(v.outcome, Outcome::Observed {})
                || seen.contains(&order.as_str())
            {
                continue;
            }
            // The hand-on this record follows, and the effect it followed: the last `resolve_stuck` effect handing the
            // order on to this linked order before the `handed_on` input, which is the one whose result was done (a
            // part is resolved once, so none follows a done one; an earlier failed or unknown effect, perhaps naming
            // another provider, doesn't count).
            let handed_at = entries.iter().position(|e| {
                matches!(e, Entry::Verified(v) if matches!(&v.what, What::Input { input }
                    if input.subject.order.as_str() == handed_on_from.as_str()
                        && matches!(&input.observed, Observed::HandedOn { linked_order, .. }
                            if linked_order.as_str() == order.as_str())))
            });
            // The holder the operator's resolution named, where the hand-on was an operator's: the node's holding
            // account, which may run no segment of the route (`T-020-33`). A hand-on with none was the customer's,
            // and its starting reserve is its route's first provider.
            let named = entries[..handed_at.unwrap_or(0)]
                .iter()
                .rev()
                .find_map(|e| match e {
                    Entry::Verified(v) => match &v.what {
                        What::Effect {
                            action:
                                Action::ResolveStuck {
                                    order: resolved,
                                    resolution:
                                        Resolution::HandOn {
                                            by, linked_order, ..
                                        },
                                    ..
                                },
                            ..
                        } if resolved.as_str() == handed_on_from.as_str()
                            && linked_order.as_str() == order.as_str() =>
                        {
                            Some(by)
                        }
                        _ => None,
                    },
                    Entry::Stated(_) => None,
                });
            let opened = entries.iter().any(|e| {
                matches!(e, Entry::Verified(v) if matches!(&v.what, What::Input { input }
                    if input.subject.order.as_str() == order.as_str()
                        && matches!(&input.observed, Observed::RouteChosen { parts, handed_on_from: Some(from), .. }
                            if from.as_str() == handed_on_from.as_str()
                                && !parts.is_empty()
                                && parts.iter().all(|part| part.segments.first().is_some_and(|s|
                                    matches!(&s.from.node,
                                        rutent_contracts::provider_role::NodeRef::Inner(n) if n.as_str() == node.as_str())
                                        && Self::starts_with_holder(s.provider.as_str(), named.map(Id::as_str), holder.as_str())))
                        )))
            });
            let handed = handed_at.is_some();
            if opened && handed {
                seen.push(order.as_str());
                segments.prove_start(order);
                proven += 1;
            }
        }
        proven
    }

    /// Whether a route handed on may start with a segment of `first`, given the `holder` a `holding_withdrawal` names: where
    /// the operator's resolution named a holder (`named`), the holder is that one and no other, and the route's first
    /// provider runs the first move whoever it is; where none did, the hand-on was the customer's and the holder is the
    /// route's first provider (`T-020-33`).
    fn starts_with_holder(first: &str, named: Option<&str>, holder: &str) -> bool {
        match named {
            Some(by) => by == holder,
            None => first == holder,
        }
    }

    /// Appends a start's `holding_withdrawal`, built once, and once the log takes it has the segments hand the
    /// linked order's segments over. One the log refuses opens nothing.
    fn record_start(
        parts: &mut Parts<'_>,
        step: &mut Step,
        linked: &Id,
    ) -> Result<Receipt, HandOnError> {
        let receipt = step.append(parts.log)?;
        parts.segments.prove_start(linked);
        Ok(receipt)
    }

    /// Builds a part's input from `by`, as `kind`.
    fn part_input(
        parts: &mut Parts<'_>,
        order: &Id,
        part: &Id,
        by: &Actor,
        kind: SourceKind,
        observed: Observed,
    ) -> Result<RecordedInput, HandOnError> {
        let now = now(parts.clock)?;
        Ok(RecordedInput {
            record_id: stm::RecordId::new(parts.fresh.id().as_str())
                .ok_or(HandOnError::Id("a record id"))?,
            recorded_at: stm::Timestamp::new(now.as_str()).ok_or(HandOnError::Clock)?,
            time_source: stm_id("core_clock")?,
            source: Source {
                kind,
                id: stm_id(by.acts.as_str())?,
            },
            standing: Standing::Stated,
            subject: Subject {
                order: stm_id(order.as_str())?,
                part: Some(stm_id(part.as_str())?),
                segment: None,
            },
            observed,
        })
    }
}

/// The router's choice in `record`, held to `authorisation` ([`check_route`]) from `from`, moving `amount`: its plan,
/// under fresh segment ids none of `order`'s segments had, those ids, and what each move on it ends at
/// ([`route_end`]).
fn plan_route(
    fresh: &mut dyn Fresh,
    order: &stm::Order,
    record: &DecisionRecord,
    from: &Endpoint,
    amount: &PositiveAmount,
    authorisation: &Authorisation,
) -> Result<(Vec<SegmentPlan>, Vec<Id>, RouteEnd), HandOnError> {
    let chosen = record
        .chosen()
        .ok_or(HandOnError::Rejected(Rejected::Split))?;
    // Inside the customer's new authorisation, read from it and never from the record (`REQ-AUZ-3`).
    let fee = &record.bounds.operator_fee;
    check_route(fee, chosen.parts.as_slice(), from, amount, authorisation)
        .map_err(HandOnError::Rejected)?;
    let route_part = chosen.parts.first();
    let mut used = used_ids(order);
    let (mut plan, mut ids) = (Vec::new(), Vec::new());
    for operation in route_part.operations.as_slice() {
        let id = fresh_segment(fresh, &used)?;
        used.push(id.as_str().to_owned());
        plan.push(SegmentPlan {
            segment_id: stm_id(id.as_str())?,
            provider: stm_id(operation.provider.as_str())?,
            kind: operation.kind,
            from: operation.from.clone(),
            to: operation.to.clone(),
        });
        ids.push(id);
    }
    let route = route_end(route_part, fee, authorisation)
        .ok_or(HandOnError::Rejected(Rejected::ChangesDestination))?;
    Ok((plan, ids, route))
}

/// Where `part`'s value rests, the settled segment that took it there, and what that segment handed on: all of
/// which the customer's new route, or the linked order, moves.
fn at_rest(
    segments: &Segments,
    order: &Id,
    part: &stm::Part,
) -> Result<(Endpoint, Id, PositiveAmount), HandOnError> {
    let at = stm::value_at(part).ok_or(HandOnError::Unknown)?.clone();
    let index = part
        .segments
        .iter()
        .position(|s| s.state != SegmentState::Settled)
        .ok_or(HandOnError::Unknown)?;
    let settled = index
        .checked_sub(1)
        .and_then(|i| part.segments.get(i))
        .filter(|s| s.to == at)
        .ok_or(HandOnError::Unknown)?;
    let settled = Id::new(settled.segment_id.as_str()).ok_or(HandOnError::Id("a segment id"))?;
    let amount = segments
        .handed_on(order, &settled)
        .and_then(|a| PositiveAmount::new(a.as_str()))
        .ok_or(HandOnError::Unknown)?;
    Ok((at, settled, amount))
}

/// Holds a linked order's route to what the log records of it, before anything is recorded or moved: its route cites
/// a decision record of its own (`input-cites-its-decision-record`); its acceptance is the customer's own, of the
/// linked order, keeping the original customer `requester` (`REQ-ORD-13`, `REQ-ORD-34`), with one yes per part of the
/// plan, in order, each from the reserve its first segment runs at (`chosen-route-rests-on-its-orders-acceptance`).
/// The production mover runs it before the holding account is asked anything (`T-020-17`).
pub(crate) fn linked_accepted<'a>(
    entries: &'a [Entry],
    requester: &Id,
    linked: &Id,
    record_id: &Id,
    acceptance: &Id,
    plan: &[PartPlan],
) -> Result<Accepted<'a>, HandOnError> {
    // Its route cites a decision record of its own (`input-cites-its-decision-record`).
    let recorded_route = entries.iter().any(|e| {
        matches!(e, Entry::Verified(v) if matches!(&v.what,
                What::DecisionRecord { decision_record, order, .. }
                    if decision_record == record_id && order == linked))
    });
    if !recorded_route {
        return Err(HandOnError::NoDecisionRecord);
    }
    // Its acceptance is the customer's own, of the linked order, keeping the original customer as requester
    // (`REQ-ORD-13`, `REQ-ORD-34`), with one yes per part of the plan, in order, each from the reserve its first
    // segment runs at (`chosen-route-rests-on-its-orders-acceptance`).
    let accepted = match acceptance_of(entries, acceptance) {
        Some(accepted) if accepted.order != linked => {
            return Err(HandOnError::NotItsAcceptance);
        }
        Some(accepted) if accepted.requester != requester => {
            return Err(HandOnError::NotTheRequester);
        }
        Some(accepted) if !accepted.customers_own() => {
            return Err(HandOnError::NotTheCustomersAcceptance);
        }
        Some(accepted) => accepted,
        None => return Err(HandOnError::NotItsAcceptance),
    };
    let fits = accepted.yeses.len() == plan.len()
        && accepted.yeses.iter().zip(plan).all(|(yes, plan)| {
            yes.part.as_str() == plan.part_id.as_str()
                && plan
                    .segments
                    .first()
                    .is_some_and(|s| s.provider.as_str() == yes.origin_reserve.as_str())
        });
    if !fits {
        return Err(HandOnError::PlanOutsideItsAcceptance);
    }
    Ok(accepted)
}

/// Where `part` of `order` holds its value, and how much of it, as Core's own records of the order's settled segments
/// say (`T-020-17`): what the production mover holds a hand-on's route to before the holding account is asked
/// anything.
pub(crate) fn resting(
    segments: &Segments,
    order: &Id,
    part: &Id,
) -> Option<(Endpoint, PositiveAmount)> {
    let followed = segments.order(order)?;
    let part = followed
        .parts
        .iter()
        .find(|p| p.part_id.as_str() == part.as_str())?;
    at_rest(segments, order, part)
        .ok()
        .map(|(at, _, amount)| (at, amount))
}

/// Whether `recorded`, the log's entries, holds `input` already, by its record id.
fn in_the_log(recorded: &[(Receipt, Entry)], input: &RecordedInput) -> bool {
    recorded.iter().any(|(_, e)| {
        matches!(e, Entry::Verified(v) if matches!(&v.what,
            What::Input { input: held } if held.record_id == input.record_id))
    })
}

/// The segments of `part` a new route retires: from its first that hasn't settled.
fn retiring(part: &stm::Part) -> Vec<Id> {
    let index = part
        .segments
        .iter()
        .position(|s| s.state != SegmentState::Settled)
        .unwrap_or(part.segments.len());
    part.segments[index..]
        .iter()
        .filter_map(|s| Id::new(s.segment_id.as_str()))
        .collect()
}

/// The provider holding the value, as the log's done `resolve_stuck` effect at `effect` names it: an effect of
/// `handing`'s order handing the value on to its linked order, decided for its part, moving the part's whole value
/// in the asset it sits in at `at`, and whose result is done (`stuck-resolved-follows-a-done-resolution`).
fn done_hand_on(
    recorded: &[(Receipt, Entry)],
    effect: &Receipt,
    handing: &Handing<'_>,
    at: &Endpoint,
    amount: &PositiveAmount,
) -> Result<Id, HandOnError> {
    let verified = |e: &Entry| match e {
        Entry::Verified(v) => Some(v.clone()),
        Entry::Stated(_) => None,
    };
    let (carries_out, by) = recorded
        .iter()
        .find(|(r, _)| r == effect)
        .and_then(|(_, e)| verified(e))
        .and_then(|v| match v.what {
            What::Effect {
                action:
                    Action::ResolveStuck {
                        order,
                        carries_out,
                        resolution:
                            Resolution::HandOn {
                                by,
                                linked_order,
                                asset,
                                amount: moved,
                            },
                        ..
                    },
                ..
            } if order == *handing.order
                && linked_order == *handing.linked
                && asset == at.asset
                && moved == *amount =>
            {
                Some((carries_out, by))
            }
            _ => None,
        })
        .ok_or(HandOnError::NoDoneResolution)?;
    let decided_for_part = recorded.iter().any(|(_, e)| {
        verified(e).is_some_and(|v| {
            matches!(&v.what, What::StuckDecide { request, decision }
                if request.request == carries_out.request
                    && *decision == carries_out.decision
                    && request.part == *handing.part)
        })
    });
    let done = recorded.iter().any(|(_, e)| {
        verified(e).is_some_and(|v| {
            matches!(&v.what, What::Result { effect: e } if e == effect)
                && matches!(v.outcome, Outcome::Done {})
        })
    });
    if decided_for_part && done {
        Ok(by)
    } else {
        Err(HandOnError::NoDoneResolution)
    }
}

/// Builds `input` as Core's verified entry, checked.
fn built(parts: &Parts<'_>, core: &Actor, input: &RecordedInput) -> Result<Step, HandOnError> {
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: core.clone(),
        what: What::Input {
            input: input.clone(),
        },
        when: When {
            at: now(parts.clock)?,
            source: TimeSource::CoreClock,
        },
        outcome: Outcome::Observed {},
    });
    entry.check().map_err(AppendFailure::Refused)?;
    let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
    Ok(Step {
        entry,
        receipt: None,
    })
}

/// Builds `what` as Core's verified entry, checked.
fn built_what(parts: &Parts<'_>, core: &Actor, what: What) -> Result<Step, HandOnError> {
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: core.clone(),
        what,
        when: When {
            at: now(parts.clock)?,
            source: TimeSource::CoreClock,
        },
        outcome: Outcome::Observed {},
    });
    entry.check().map_err(AppendFailure::Refused)?;
    let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
    Ok(Step {
        entry,
        receipt: None,
    })
}

/// The state machine's source for an input stated by `by`.
fn source_of(by: &Actor) -> Result<Source, HandOnError> {
    Ok(Source {
        kind: source_kind(by)?,
        id: stm_id(by.acts.as_str())?,
    })
}

/// The source kind of `by`'s word: only the customer's role is the customer's, the operator's the operator's, and
/// Core's the orchestrator's; any other role states no hand-on.
fn source_kind(by: &Actor) -> Result<SourceKind, HandOnError> {
    match by.role {
        Role::Customer => Ok(SourceKind::Customer),
        Role::Operator => Ok(SourceKind::Operator),
        Role::Core => Ok(SourceKind::Orchestrator),
        _ => Err(Refused::NotTheCustomer.into()),
    }
}

/// Every segment id `order` ever had.
fn used_ids(order: &stm::Order) -> Vec<String> {
    order
        .parts
        .iter()
        .flat_map(|p| {
            p.segments
                .iter()
                .map(|s| s.segment_id.as_str().to_owned())
                .chain(p.retired_segments.iter().map(|s| s.as_str().to_owned()))
        })
        .collect()
}

/// A fresh segment id: none the order used.
fn fresh_segment(fresh: &mut dyn Fresh, used: &[String]) -> Result<Id, HandOnError> {
    for _ in 0..DRAWS {
        let id = fresh.id();
        if !used.iter().any(|u| u == id.as_str()) && stm::Id::new(id.as_str()).is_some() {
            return Ok(id);
        }
    }
    Err(HandOnError::NoFreshSegment)
}

fn key(order: &Id, part: &Id) -> (String, String) {
    (order.as_str().to_owned(), part.as_str().to_owned())
}

fn stm_id(text: &str) -> Result<stm::Id, HandOnError> {
    stm::Id::new(text).ok_or(HandOnError::Id("an id"))
}

fn now(clock: &dyn Clock) -> Result<rutent_contracts::router_api::Timestamp, HandOnError> {
    clock.now().timestamp().ok_or(HandOnError::Clock)
}

/// A receipt standing until the log's is taken.
fn placeholder() -> Receipt {
    Receipt {
        log: Id::new("unappended").expect("an id"),
        index: rutent_contracts::provider_role::SafeInteger::try_from(0u64).expect("zero"),
        entry: serde_json::from_value(serde_json::json!({
            "algorithm": "sha-256",
            "value": "00".repeat(32),
        }))
        .expect("a digest"),
    }
}
