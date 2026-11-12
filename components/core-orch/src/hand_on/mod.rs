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
//!    the customer's new destination is held to the new authorisation ([`check_route`]), then to the state
//!    machine's rules ([`destination_change`]: it changes the destination and starts where the value sits), and
//!    recorded as `destination_changed`, citing the choice round's decision record and the customer's acceptance.
//!    The order Core follows takes it, and the value's rest hold goes: the new route's segments hold it as they run.
//! 3. **Handed on to a linked order** ([`HandOn::hand_on`]). The linked order's route starts at the inner node where
//!    the value sits, with no withdrawal: its route is opened ([`Stm::open`]) and held to the first order by the
//!    start rule across the two orders, `handed_on_route_starts_where_value_sits`, before anything is recorded
//!    (`INV-4`). The linked order's acceptance keeps the original customer as requester (`REQ-ORD-13`). Then the
//!    first order's `handed_on`, naming the linked order and its acceptance, and the linked order's `route_chosen`,
//!    naming `handed_on_from`, are recorded in that order, so the two records point to each other
//!    (`handed-on-route-points-back`). Each is built once and appended until the log takes it.
//! 4. **The holding account's record proves the start** ([`HandOn::prove_start`], `REQ-ORD-40`, `INV-5`). No
//!    segment of a route handed on is handed over until the holding account's own record of the withdrawal, made by
//!    the linked order's starting reserve, at the route's first node, of the first order's value and of all the part
//!    held, is held for it ([`proves_start`]); the segments refuse it before then
//!    ([`SegmentsError::StartUnproven`](crate::segments::SegmentsError::StartUnproven)).
//!
//! 5. **A wait at the floor, resumed** ([`HandOn::resume`], `REQ-ORD-27`). A part waiting for the customer resumes
//!    on a route to its own destination under the customer's new terms (a lower floor, say), which the customer
//!    accepted, held to them and recorded as `route_recalculated`; it changes no destination (`INV-10`).
//!
//! **The operator's hand-on** ([`Basis::Resolution`]). The operator hands a stuck order's value on only as a stuck
//! resolution, decided and moved under `REQ-AUZ-4` ([`Stuck`](crate::stuck::Stuck)); once its `resolve_stuck`
//! effect's result is done, this module records the part's `handed_on` and the linked order's route the same way
//! (`stuck-resolved-follows-a-done-resolution`).
//!
//! Every value-moving effect of the linked order, or of the new route, stays under policy's decision and its own
//! authorisation, as every move does (`F-021`).

mod rule;

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Authorisations, GeneralEntry};
use rutent_contracts::core_internal::{
    Action, Actor, Authorisation, Decision, Entry, Outcome, Receipt, Resolution, RouteEnd,
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
use crate::stuck::is_receipt_of;

pub use rule::{
    Accepted, Refused, Unproven, acceptance_of, after_failed_final_deposit, destination_change,
    new_terms, proves_start, requester_of,
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
    /// The new authorisation isn't for this order, or doesn't speak for the part.
    NotItsAuthorisation,
    /// No done `resolve_stuck` effect handing this part on to the linked order is in the log
    /// (`stuck-resolved-follows-a-done-resolution`).
    NoDoneResolution,
    /// The holding account's record doesn't prove the route's start (`REQ-ORD-40`).
    Unproven(Unproven),
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
/// waiting part (the gate `T-021-04` leaves to orchestration): `current` are the terms in force, `requester` the
/// order's customer as the log holds it ([`requester_of`]). Refused, with nothing asked, otherwise.
pub fn authorise(
    segments: &Segments,
    authorisations: &mut dyn Authorisations,
    current: &Authorisation,
    new: &Authorisation,
    requester: &Id,
) -> Result<Decision, HandOnError> {
    let order = segments
        .order(&new.order)
        .ok_or(HandOnError::Refused(Refused::Unknown))?;
    new_terms(order, current, new, requester)?;
    Ok(authorisations.authorise(new)?)
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
    /// The customer's new authorisation, in force for the part ([`authorise`]): the new route is held to it.
    pub authorisation: &'a Authorisation,
    /// The log's entries, as the log holds them: where the acceptance is read from.
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

/// A part waiting for the customer, resumed on a route to its own destination under the customer's new terms.
#[derive(Debug, Clone, Copy)]
pub struct Resume<'a> {
    /// The order.
    pub order: &'a Id,
    /// The part waiting for the customer.
    pub part: &'a Id,
    /// The choice round's record of the route from where the value sits, routed under the new terms' floor.
    pub record: &'a DecisionRecord,
    /// The customer's new authorisation, in force for the part ([`authorise`]): the route is held to it.
    pub authorisation: &'a Authorisation,
    /// The customer's acceptance of the new terms, which the log holds: of this order, accepting the new
    /// authorisation, which every move on the route rests on (`move-rests-on-its-orders-acceptance`, `REQ-ORD-34`).
    pub acceptance: &'a Id,
    /// The log's entries, as the log holds them: where the acceptance is read from.
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

/// A destination change built, kept until the log takes it.
#[derive(Debug, Clone)]
struct PendingChange {
    step: Step,
    changed: Changed,
    /// The segments whose holds go once it is taken: the settled one the value rests under, and those retired.
    released: Vec<Id>,
}

/// A resumption built, kept until the log takes it.
#[derive(Debug, Clone)]
struct PendingResume {
    step: Step,
    resumed: Resumed,
    released: Vec<Id>,
}

/// A hand-on built, kept until the log takes both its inputs.
#[derive(Debug, Clone)]
struct PendingHandOn {
    handed_on: (Step, RecordedInput),
    route_chosen: (Step, RecordedInput),
    at: Endpoint,
    released: Vec<Id>,
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
    linked: BTreeMap<String, Linked>,
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
        let authorisation = change.authorisation;
        let speaks_for = authorisation
            .parts
            .as_ref()
            .is_none_or(|ps| ps.iter().any(|p| p.as_str() == change.part.as_str()));
        if authorisation.order != *change.order || !speaks_for {
            return Err(HandOnError::NotItsAuthorisation);
        }
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
    /// (`REQ-ORD-27`, `AC-12`). The router's choice is held to the new authorisation ([`check_route`]): from where
    /// the value sits, all of it, to the authorisation's destination and to asset, leaving its floor; and to the
    /// state machine's rules, as a `route_recalculated`, citing the choice round's decision record. It changes no
    /// destination (`INV-10`). Built once and appended until the log takes it; the order Core follows then takes
    /// it, and the next recalculation follows that round ([`Recalculations::followed_by`](crate::recalc::Recalculations::followed_by)).
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
        let authorisation = resume.authorisation;
        let speaks_for = authorisation
            .parts
            .as_ref()
            .is_none_or(|ps| ps.iter().any(|p| p.as_str() == resume.part.as_str()));
        if authorisation.order != *resume.order || !speaks_for {
            return Err(HandOnError::NotItsAuthorisation);
        }
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
    /// linked order's `route_chosen` naming `handed_on_from`, each built once and appended until the log takes it.
    /// A hand-on built and not yet taken whole is appended as built, and nothing is checked again.
    pub fn hand_on(
        &mut self,
        parts: &mut Parts<'_>,
        handing: Handing<'_>,
    ) -> Result<HandedOn, HandOnError> {
        let at = key(handing.order, handing.part);
        if !self.hand_ons.contains_key(&at) {
            let built = Self::build_hand_on(parts, &handing)?;
            self.hand_ons.insert(at.clone(), built);
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
        // Once the first order is handed on, the linked order's route is kept until the log takes it: the value is
        // never left handed on to an order that wasn't opened.
        let route_chosen_receipt = pending.route_chosen.0.append(parts.log)?;
        let pending = self.hand_ons.remove(&at).ok_or(HandOnError::Unknown)?;
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
        match handing.basis {
            // The customer's own choice resolves a part waiting for it; a stuck part is the operator's.
            Basis::Customer if part.state != OrderState::WaitingForCustomer => {
                return Err(Refused::NotWaiting(part.state).into());
            }
            Basis::Customer => {}
            Basis::Resolution { effect } => {
                if part.state != OrderState::Stuck {
                    return Err(Refused::NotWaiting(part.state).into());
                }
                done_hand_on(handing.recorded, effect, handing)?;
            }
        }
        // The linked order's acceptance keeps the original customer as requester (`REQ-ORD-13`).
        let requester = requester_of(&entries, handing.order).ok_or(HandOnError::Unknown)?;
        match acceptance_of(&entries, handing.acceptance) {
            Some(accepted) if accepted.order != handing.linked => {
                return Err(HandOnError::NotItsAcceptance);
            }
            Some(accepted) if accepted.requester != requester => {
                return Err(HandOnError::NotTheRequester);
            }
            Some(_) => {}
            None => return Err(HandOnError::NotItsAcceptance),
        }
        let (at, settled, _) = at_rest(parts.segments, handing.order, part)?;
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
            source_kind(handing.by),
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
        let mut released = vec![settled];
        released.extend(retiring(part));
        Ok(PendingHandOn {
            handed_on: (handed_on_step, handed_on),
            route_chosen: (route_chosen_step, route_chosen),
            at,
            released,
        })
    }

    /// Holds `record`, the holding account's own record of its withdrawal for `linked`, a route handed on, to the
    /// linked order's start ([`proves_start`], `REQ-ORD-40`): made by `reserve`, the linked order's starting reserve,
    /// at its route's first node, in its asset, of the first order's value, and of all the part held. Once proven,
    /// the segments hand the linked order's segments over; before, they refuse them.
    pub fn prove_start(
        &mut self,
        segments: &mut Segments,
        linked: &Id,
        reserve: &Id,
        record: &WithdrawalRecord,
    ) -> Result<(), HandOnError> {
        if !self.linked.contains_key(linked.as_str()) {
            let order = segments.order(linked).ok_or(HandOnError::Unknown)?;
            let from = order
                .handed_on_from
                .as_ref()
                .and_then(|f| Id::new(f.as_str()))
                .ok_or(HandOnError::Unknown)?;
            let first_segment = order
                .parts
                .first()
                .and_then(|p| p.segments.first())
                .ok_or(HandOnError::Unknown)?;
            let first = SegmentPlan {
                segment_id: first_segment.segment_id.clone(),
                provider: first_segment.provider.clone(),
                kind: first_segment.kind,
                from: first_segment.from.clone(),
                to: first_segment.to.clone(),
            };
            let handed = segments.order(&from).ok_or(HandOnError::Unknown)?;
            let part = handed
                .parts
                .iter()
                .find(|p| {
                    p.state == OrderState::HandedOn
                        && p.linked_order.as_ref().map(|l| l.as_str()) == Some(linked.as_str())
                })
                .ok_or(HandOnError::Unknown)?;
            let settled = part
                .segments
                .iter()
                .rev()
                .find(|s| s.state == SegmentState::Settled)
                .and_then(|s| Id::new(s.segment_id.as_str()))
                .ok_or(HandOnError::Unknown)?;
            let amount = segments
                .handed_on(&from, &settled)
                .cloned()
                .ok_or(HandOnError::Unknown)?;
            self.linked.insert(
                linked.as_str().to_owned(),
                Linked {
                    from,
                    amount,
                    reserve: reserve.clone(),
                    first,
                },
            );
        }
        let held = self
            .linked
            .get(linked.as_str())
            .ok_or(HandOnError::Unknown)?;
        proves_start(record, &held.reserve, &held.first, &held.from, &held.amount)
            .map_err(HandOnError::Unproven)?;
        segments.prove_start(linked);
        Ok(())
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

/// Whether the log holds, at `effect`, a done `resolve_stuck` effect of `handing`'s order handing the value on to
/// its linked order, decided for its part (`stuck-resolved-follows-a-done-resolution`).
fn done_hand_on(
    recorded: &[(Receipt, Entry)],
    effect: &Receipt,
    handing: &Handing<'_>,
) -> Result<(), HandOnError> {
    let verified = |e: &Entry| match e {
        Entry::Verified(v) => Some(v.clone()),
        Entry::Stated(_) => None,
    };
    let carries_out = recorded
        .iter()
        .find(|(r, _)| r == effect)
        .and_then(|(_, e)| verified(e))
        .and_then(|v| match v.what {
            What::Effect {
                action:
                    Action::ResolveStuck {
                        order,
                        carries_out,
                        resolution: Resolution::HandOn { linked_order, .. },
                        ..
                    },
                ..
            } if order == *handing.order && linked_order == *handing.linked => Some(carries_out),
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
        Ok(())
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

/// The state machine's source for an input stated by `by`.
fn source_of(by: &Actor) -> Result<Source, HandOnError> {
    Ok(Source {
        kind: source_kind(by),
        id: stm_id(by.acts.as_str())?,
    })
}

fn source_kind(by: &Actor) -> SourceKind {
    match by.role {
        rutent_contracts::core_internal::Role::Operator => SourceKind::Operator,
        rutent_contracts::core_internal::Role::Core => SourceKind::Orchestrator,
        _ => SourceKind::Customer,
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
