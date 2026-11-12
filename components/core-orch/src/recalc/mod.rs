//! Failure and recalculation: a route recalculated only from a node where the value is evidenced.
//!
//! Declared by `T-020-01`; `T-020-10` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! [`Recalculations::recalculate`] recalculates one part of an order, automatically and at the user's cost:
//!
//! 1. **Only from evidenced value** (`REQ-ORD-2`, `INV-4`). The part's state is read from the order Core follows
//!    ([`Segments::order`]), and [`evidenced`] places its value: at the start of its first segment that hasn't
//!    settled, where that segment hasn't started, was reversed in full, or is a deposit that failed, and the segment
//!    before it settled there. A segment in flight (started with no result that counts, failed with its reversal
//!    owed, of unknown outcome) keeps it from being recalculated, so silence, a stopped advertisement or a stale
//!    version never release a route (`THR-12`): none of them is evidence. Nothing is asked and nothing recorded.
//! 2. **Bounded** (`REQ-ORD-4`). At most [`RECALCULATIONS`] recalculations per order; past them the order has no
//!    route left, which [`Recalculation::NoRouteLeft`] answers for `T-020-11` to record (core-internal's README puts
//!    `no_route` there). A route that passes a provider again from the node where that provider's segment didn't
//!    settle is named in [`Recalculated::passed_again`], and the log's inputs show the same ([`passed_again`]).
//! 3. **Routed from where the value sits**, through the rounds (`T-020-06`), with the request's `follows` naming the
//!    choice round it follows: from that endpoint, the whole of what is evidenced there ([`Segments::handed_on`]),
//!    to the part's destination and to asset, under the authorisation's floor. The user pays for it: the new route's
//!    charges come off what it delivers, and the failed provider's segment covers only the value's return to its
//!    origin, since what is evidenced there is moved in full (`REQ-ORD-22`, `INV-2`). No yes is asked: the value
//!    is in the book already.
//! 4. **Inside the authorisation** (`REQ-AUZ-3`, `INV-10`). The router's choice is held by [`check_route`] against
//!    the authorisation, never against its own record: one part, from where the value is evidenced, moving all of
//!    it, ending in a deposit at the authorisation's destination reserve and node, in its to asset, leaving the
//!    customer its floor after the operator's fee. The state machine's own rules
//!    (`recalculation_keeps_destination`, `recalculation_starts_where_value_sits`) are run on it too. Each move on
//!    the new route goes to policy (`F-021`) with the route's own ends ([`route_end`], [`Recalculated::route`]),
//!    and the log holds them to the authorisation (`move-rests-on-its-authorisation`), a third guard. After a
//!    routing that offered no route, or one Core refused, the part is routed no more until `T-020-11` resolves it.
//! 5. **Past the floor, the order waits for the customer** (`REQ-ORD-27`, `AC-12`). A routing whose every candidate
//!    was dropped as below the floor makes the part wait ([`Recalculation::WaitsForCustomer`]), its decision record
//!    showing why. And [`Segments::run_part`], the one way a part runs, checks the floor its
//!    [`Live`](crate::segments::Live) carries at each settled segment, against what is evidenced there and the quotes
//!    still ahead ([`left_after`], `AC-42`). Either way the wait is a recorded `waiting_at_floor` input on the value
//!    the part's last settled segment evidenced, built once and appended until the log takes it, so the order Core
//!    follows waits for the customer with its age ([`Segments::waiting`], state-machine 3.0.0), and a waiting part
//!    takes no move (`T-020-10b`).
//! 6. **Recorded once.** The `route_recalculated` input, citing the choice round's decision record, is built once
//!    and appended until the log takes it; the log admits it only on a `segment_settled` of that part ending where
//!    the route starts (`recalculation-starts-from-evidenced-value`). Only then does the order Core follows move to
//!    the new route ([`Segments::observe`]) and are the old route's holds released, so the value counts once
//!    against the ceilings in flight (`REQ-ORD-8`).

mod rule;

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{
    AppendFailure, Charges, Evidence, GeneralEntry, Log, PersonQuestions,
};
use rutent_contracts::core_internal::{
    Actor, Authorisation, Entry, Outcome, Receipt, RouteEnd, TimeSource, VerifiedEntry,
    VerifiedKind, What, When,
};
use rutent_contracts::router_api::{
    DecisionRecord, DropReason, Endpoint, Id, Objective, PolicyBounds, PositiveAmount, Request,
    Round, Router,
};
use rutent_contracts::state_machine::{
    self as stm, Observed, RecordedInput, SegmentPlan, SegmentState, Source, SourceKind, Standing,
    StateMachine, Subject,
};
use rutent_prov::catalogue::Catalogue;
use rutent_stm::Stm;

use crate::rounds::{
    self, Fresh, History, Issued, Offer, OperatorKey, Providers, Recorded, Routed, Routing,
    RoutingError,
};
use crate::segments::Segments;
use crate::snapshot::{Clock, Exposures, OrchestrationLog, Price, Releases};

pub use rule::{
    NotRecalculable, PassedAgain, Rejected, check_route, evidenced, holds_floor, left_after,
    passed_again, route_end,
};

/// How many times one order's routes may be recalculated (`REQ-ORD-4`). Past it, the order has no route left. A
/// reading, reversible.
pub const RECALCULATIONS: usize = 3;

/// How many fresh identifiers are drawn for a segment before the source is taken as broken.
const DRAWS: usize = 4;

/// How many times in all an append the log can't take now is tried.
const APPEND_TRIES: usize = 3;

/// What a recalculation reaches. The assembly builds it; Core's catalogue, clock, exposures and releases are those
/// the rounds and the segments use.
pub struct Parts<'a> {
    /// The catalogue: the rounds' snapshots read it.
    pub catalogue: &'a mut Catalogue,
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Core's record of value in flight: the old route's holds are released once the new one is recorded.
    pub exposures: &'a mut Exposures,
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
    /// The providers' modules, which the rounds ask.
    pub providers: &'a mut dyn Providers,
    /// The operator key.
    pub operator_key: &'a mut dyn OperatorKey,
    /// Fresh identifiers.
    pub fresh: &'a mut dyn Fresh,
    /// The transactions Core asked under.
    pub issued: &'a mut Issued,
    /// The segments Core handed over, and the orders it follows.
    pub segments: &'a mut Segments,
}

/// One recalculation to try.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// The order, as the rounds route it: whose work it is, its origin reserve, its inputs and needs.
    pub order: &'a rounds::Order,
    /// The part to recalculate.
    pub part: &'a Id,
    /// The authorisation the order's value moves under: its destination, to asset and floor bound the new route
    /// (`REQ-AUZ-3`).
    pub authorisation: &'a Authorisation,
    /// What the router optimises for.
    pub objective: &'a Objective,
    /// The bounds the router runs under. A recalculation routes its part as one route, so its `max_parts` is 1.
    pub bounds: &'a PolicyBounds,
    /// The prices Core holds.
    pub prices: &'a [Price],
    /// Core, acting on its own account: who writes the input.
    pub core: &'a Actor,
    /// The operator, in whose name Core signs its questions and records.
    pub operator: &'a Actor,
}

/// A recalculation recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recalculated {
    /// The `route_recalculated` input, as recorded.
    pub input: RecordedInput,
    /// The receipt of its entry.
    pub receipt: Receipt,
    /// The choice round that chose the new route: its record, which the input cites.
    pub record: DecisionRecord,
    /// The id Core gave each segment of the new route, in order.
    pub segments: Vec<Id>,
    /// What each move on the new route ends at, read from the route ([`route_end`]): its last deposit's reserve and
    /// node, the asset it delivers, and what it leaves the customer after the operator's fee; the account is the
    /// authorisation's. Each move carries it to policy, and the log holds it to the authorisation (`REQ-AUZ-3`).
    pub route: RouteEnd,
    /// Where the value was evidenced, and so where the new route starts.
    pub from: Endpoint,
    /// What was evidenced there, all of which the new route moves.
    pub amount: PositiveAmount,
    /// The providers the new route passes again without the value having moved on (`REQ-ORD-4`).
    pub passed_again: Vec<PassedAgain>,
    /// How many times the order's routes have been recalculated, this one included.
    pub recalculated: usize,
}

/// What a recalculation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recalculation {
    /// Not now: the value is evidenced at no node. Nothing was asked or recorded.
    NotNow(NotRecalculable),
    /// The order's recalculations are at their bound: it has no route left (`REQ-ORD-4`, `REQ-ORD-28`). Nothing was
    /// asked or recorded; `T-020-11` records `no_route` and the stuck state.
    NoRouteLeft {
        /// How many recalculations the order had.
        recalculated: usize,
    },
    /// Every candidate the routing considered leaves the customer below its floor: the part waits for the customer
    /// (`REQ-ORD-27`). The routing's decision record shows it; nothing else was recorded.
    WaitsForCustomer {
        /// The deciding round's record.
        record: Box<Recorded>,
    },
    /// The routing offered no route, for another reason than the floor: the order has no route left for now, for
    /// `T-020-11` to record.
    NoRoute {
        /// The deciding round's record.
        record: Box<Recorded>,
    },
    /// Core refused the router's choice: nothing was recorded but the routing's own records.
    Rejected {
        /// Why.
        why: Rejected,
        /// The deciding round's record.
        record: Box<Recorded>,
    },
    /// The part's route was recalculated.
    Recalculated(Box<Recalculated>),
    /// The input was recorded, but the order Core follows didn't take it: no hold was released and nothing was
    /// counted. The log holds it; the order's state is the one to settle (`T-020-11`).
    NotFollowed {
        /// The receipt of its entry.
        receipt: Receipt,
    },
}

/// Why a recalculation couldn't be tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecalcError {
    /// Core doesn't follow the order for recalculation ([`Recalculations::follow`]).
    NotFollowed,
    /// The authorisation isn't the order's, or names another destination or to asset than the part's: Core's own
    /// records disagree, and nothing is routed.
    NotItsAuthorisation,
    /// The routing stopped.
    Routing(RoutingError),
    /// The log refused or failed the input's append: it is kept, and the next try appends it unchanged.
    Append(AppendFailure),
    /// The source of fresh identifiers repeats ones the order used.
    NoFreshSegment,
    /// An identifier Core holds isn't one the state machine's input can carry.
    Id(&'static str),
    /// Core's clock reads no time a timestamp can hold.
    Clock,
    /// The part's wait at its floor couldn't be recorded (`T-020-10b`).
    Wait(crate::segments::SegmentsError),
}

impl From<AppendFailure> for RecalcError {
    fn from(failure: AppendFailure) -> Self {
        RecalcError::Append(failure)
    }
}

/// A recalculation built, kept until the log takes its input.
#[derive(Debug, Clone)]
struct Pending {
    entry: GeneralEntry,
    recalculated: Recalculated,
    retired: Vec<Id>,
    tried: Vec<Tried>,
}

/// A segment handed over from a node, that never settled.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tried {
    part: stm::Id,
    from: Endpoint,
    provider: stm::Id,
    segment: stm::Id,
}

/// What Core keeps of an order it may recalculate.
#[derive(Debug, Clone)]
struct Followed {
    /// The choice round's request the next recalculation follows.
    follows: Id,
    /// How many recalculations were recorded.
    recalculated: usize,
    /// Every segment of the order handed over from a node, that never settled, as recalculations retired them.
    tried: Vec<Tried>,
    /// A recalculation built for a part, its input not yet appended.
    pending: BTreeMap<String, Pending>,
    /// The parts whose routing left the customer below its floor, until their `waiting_at_floor` is recorded: the
    /// deciding round's record, so a retry records the wait without routing again (`T-020-10b`).
    waiting: BTreeMap<String, Box<Recorded>>,
    /// The parts whose last routing offered no route, or one Core refused: no route left until `T-020-11` resolves
    /// them, so they are routed no more (`REQ-ORD-4`).
    no_route: BTreeMap<String, ()>,
}

/// Core's record of the orders it may recalculate, kept across orders by the assembly, which the production assembly
/// keeps durably (`F-030`), as it does [`Segments`].
#[derive(Debug, Clone, Default)]
pub struct Recalculations {
    orders: BTreeMap<String, Followed>,
}

impl Recalculations {
    /// Nothing followed yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Follows `order`, whose route was chosen by the choice round of request `follows`: its first recalculation
    /// follows that round.
    pub fn follow(&mut self, order: &Id, follows: &Id) {
        self.orders
            .entry(order.as_str().to_owned())
            .or_insert_with(|| Followed {
                follows: follows.clone(),
                recalculated: 0,
                tried: Vec::new(),
                pending: BTreeMap::new(),
                waiting: BTreeMap::new(),
                no_route: BTreeMap::new(),
            });
    }

    /// Has `order`'s next recalculation follow the choice round of request `follows`, the one that chose the route
    /// the customer resumed a waiting part on (`T-020-13`), so each round chains to the one before (`REQ-AUD-4`).
    /// The customer's choice isn't a recalculation: the count stays.
    pub fn followed_by(&mut self, order: &Id, follows: &Id) {
        if let Some(followed) = self.orders.get_mut(order.as_str()) {
            followed.follows = follows.clone();
        }
    }

    /// [`Recalculations::follow`], its count rebuilt from `inputs`, the order's recorded inputs as the log holds
    /// them, so a restart keeps the bound (`REQ-ORD-4`, `F-030`).
    pub fn follow_recorded(&mut self, order: &Id, follows: &Id, inputs: &[RecordedInput]) {
        self.follow(order, follows);
        let recorded = inputs
            .iter()
            .filter(|i| {
                i.subject.order.as_str() == order.as_str()
                    && matches!(i.observed, Observed::RouteRecalculated { .. })
            })
            .count();
        if let Some(followed) = self.orders.get_mut(order.as_str()) {
            followed.recalculated = followed.recalculated.max(recorded);
        }
    }

    /// How many times `order`'s routes have been recalculated.
    pub fn recalculated(&self, order: &Id) -> usize {
        self.orders
            .get(order.as_str())
            .map_or(0, |f| f.recalculated)
    }

    /// Recalculates `ask.part` of its order (see the module's docs), with router `R`, chaining each routing's
    /// records to `history`.
    pub fn recalculate<R: Router>(
        &mut self,
        parts: &mut Parts<'_>,
        ask: Ask<'_>,
        history: &mut History,
    ) -> Result<Recalculation, RecalcError> {
        let order_id = &ask.order.order;
        let followed = self
            .orders
            .get_mut(order_id.as_str())
            .ok_or(RecalcError::NotFollowed)?;
        // A recalculation built and not yet taken is appended as built: it is routed and recorded once.
        if let Some(pending) = followed.pending.get(ask.part.as_str()).cloned() {
            return Self::record(followed, parts, ask.part, pending);
        }
        // A part with no route left is routed no more: colluding providers can't make Core ask again and again
        // (`REQ-ORD-4`, `THR-28`).
        if followed.no_route.contains_key(ask.part.as_str()) {
            return Ok(Recalculation::NoRouteLeft {
                recalculated: followed.recalculated,
            });
        }
        // A wait at the floor built and not yet taken is recorded as built: the part is routed once.
        if let Some(record) = followed.waiting.get(ask.part.as_str()).cloned() {
            return Self::wait(followed, parts, ask, record);
        }
        let Some(order) = parts.segments.order(order_id).cloned() else {
            return Ok(Recalculation::NotNow(NotRecalculable::Unknown));
        };
        let Some(part) = order
            .parts
            .iter()
            .find(|p| p.part_id.as_str() == ask.part.as_str())
        else {
            return Ok(Recalculation::NotNow(NotRecalculable::Unknown));
        };
        // Only from where the value is evidenced (`REQ-ORD-2`, `INV-4`).
        let (at, settled) = match evidenced(part) {
            Ok(evidenced) => evidenced,
            Err(why) => return Ok(Recalculation::NotNow(why)),
        };
        // A segment handed over with no final outcome may have its effect in the log, though the state machine
        // still has it pending: no route goes around it (`INV-4`).
        if let Some(why) = handed_over(parts.segments, order_id, part) {
            return Ok(Recalculation::NotNow(why));
        }
        let settled_id =
            Id::new(settled.segment_id.as_str()).ok_or(RecalcError::Id("a segment id"))?;
        let Some(amount) = parts
            .segments
            .handed_on(order_id, &settled_id)
            .and_then(|a| PositiveAmount::new(a.as_str()))
        else {
            return Ok(Recalculation::NotNow(NotRecalculable::NoSettledEnd));
        };
        // Bounded in count (`REQ-ORD-4`).
        // Those waiting on the log count too, so a split order's parts can't pass the bound together.
        if followed.recalculated + followed.pending.len() >= RECALCULATIONS {
            return Ok(Recalculation::NoRouteLeft {
                recalculated: followed.recalculated,
            });
        }
        let authorisation = ask.authorisation;
        let keeps = authorisation.order == *order_id
            && part.destination.provider.as_str() == authorisation.destination.reserve.as_str()
            && part.destination.to.node
                == rutent_contracts::provider_role::NodeRef::Outer(
                    authorisation.destination.node.clone(),
                )
            && part.destination.to.asset == authorisation.to_asset;
        if !keeps {
            return Err(RecalcError::NotItsAuthorisation);
        }
        // Routed from where the value sits, all of it, to the part's destination, under the floor.
        let request = Request {
            request_id: parts.fresh.id(),
            round: Round::Shortlist {},
            from: at.clone(),
            to: part.destination.to.clone(),
            amount: amount.clone(),
            objective: *ask.objective,
            floor: authorisation.floor.as_ref().map(|f| f.amount().clone()),
            follows: Some(followed.follows.clone()),
        };
        let mut bounds = ask.bounds.clone();
        bounds.max_parts = std::num::NonZeroU8::MIN;
        let routed = {
            let mut rounds_parts = rounds::Parts {
                catalogue: &mut *parts.catalogue,
                clock: parts.clock,
                exposures: &*parts.exposures,
                releases: parts.releases,
                log: &mut *parts.log,
                evidence: &mut *parts.evidence,
                charges: &mut *parts.charges,
                persons: &mut *parts.persons,
                providers: &mut *parts.providers,
                operator_key: &mut *parts.operator_key,
                fresh: &mut *parts.fresh,
                issued: &mut *parts.issued,
            };
            rounds::route::<R>(
                &mut rounds_parts,
                Routing {
                    request: &request,
                    bounds: &bounds,
                    prices: ask.prices,
                    order: ask.order,
                    core: ask.core,
                    operator: ask.operator,
                },
                history,
            )
            .map_err(RecalcError::Routing)?
        };
        let Routed {
            shortlist,
            mut choices,
            offer,
            ..
        } = routed;
        let deciding = Box::new(choices.pop().unwrap_or(shortlist));
        let chosen = match (&offer, deciding.record.chosen()) {
            (Offer::Offered { .. }, Some(chosen)) => chosen.clone(),
            _ => {
                if every_candidate_below_floor(&deciding.record) {
                    // Past the floor the part waits for the customer, recorded as a state-machine input on the
                    // value evidenced where it sits (`waiting_at_floor`, `REQ-ORD-27`, `T-020-10b`).
                    followed
                        .waiting
                        .insert(ask.part.as_str().to_owned(), deciding.clone());
                    return Self::wait(followed, parts, ask, deciding);
                }
                followed.no_route.insert(ask.part.as_str().to_owned(), ());
                return Ok(Recalculation::NoRoute { record: deciding });
            }
        };
        // Inside the authorisation, read from it and never from the record (`INV-10`, `REQ-AUZ-3`).
        let part_id = ask.part.as_str().to_owned();
        let mut rejected = |why| {
            followed.no_route.insert(part_id.clone(), ());
            Ok(Recalculation::Rejected {
                why,
                record: deciding.clone(),
            })
        };
        if let Err(why) = check_route(
            &deciding.record.bounds.operator_fee,
            chosen.parts.as_slice(),
            at,
            &amount,
            authorisation,
        ) {
            return rejected(why);
        }
        let route_part = chosen.parts.first();
        // Fresh segment ids: none the order ever had (`Order::check`, `reuses_segment_id`).
        let mut used: Vec<String> = order
            .parts
            .iter()
            .flat_map(|p| {
                p.segments
                    .iter()
                    .map(|s| s.segment_id.as_str().to_owned())
                    .chain(p.retired_segments.iter().map(|s| s.as_str().to_owned()))
            })
            .collect();
        let mut plan = Vec::new();
        let mut ids = Vec::new();
        for operation in route_part.operations.as_slice() {
            let id = fresh_segment(parts.fresh, &used)?;
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
        if !stm::recalculation_keeps_destination(part, &plan) {
            return rejected(Rejected::ChangesDestination);
        }
        if !stm::recalculation_starts_where_value_sits(part, &plan) {
            return rejected(Rejected::StartsElsewhere);
        }
        let now = parts.clock.now().timestamp().ok_or(RecalcError::Clock)?;
        let input = RecordedInput {
            record_id: stm::RecordId::new(parts.fresh.id().as_str())
                .ok_or(RecalcError::Id("a record id"))?,
            recorded_at: stm::Timestamp::new(now.as_str()).ok_or(RecalcError::Clock)?,
            time_source: stm_id("core_clock")?,
            source: Source {
                kind: SourceKind::Orchestrator,
                id: stm_id(ask.core.acts.as_str())?,
            },
            standing: Standing::Verified,
            subject: Subject {
                order: stm_id(order_id.as_str())?,
                part: Some(stm_id(ask.part.as_str())?),
                segment: None,
            },
            observed: Observed::RouteRecalculated {
                decision_record: stm_id(deciding.record.record_id().as_str())?,
                segments: plan.clone(),
            },
        };
        let after = Stm::transition(&order, &input);
        if after.last_input.outcome != (stm::Outcome::Applied {}) {
            return rejected(Rejected::NotApplicable(after.last_input.outcome));
        }
        // The providers passed again from this node, by what was handed over from it and never settled.
        let index = part
            .segments
            .iter()
            .position(|s| s.state != SegmentState::Settled)
            .unwrap_or(part.segments.len());
        let retiring: Vec<&stm::Segment> = part.segments[index..].iter().collect();
        let newly_tried: Vec<Tried> = retiring
            .iter()
            .filter(|s| s.state != SegmentState::Pending)
            .map(|s| Tried {
                part: part.part_id.clone(),
                from: s.from.clone(),
                provider: s.provider.clone(),
                segment: s.segment_id.clone(),
            })
            .collect();
        let passed = passed_again_from(
            followed.tried.iter().chain(&newly_tried),
            &part.part_id,
            at,
            &plan,
        );
        let Some(route) = route_end(
            route_part,
            &deciding.record.bounds.operator_fee,
            authorisation,
        ) else {
            return rejected(Rejected::ChangesDestination);
        };
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: ask.core.clone(),
            what: What::Input {
                input: input.clone(),
            },
            when: When {
                at: now,
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Observed {},
        });
        entry.check().map_err(AppendFailure::Refused)?;
        let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
        let retired = retiring
            .iter()
            .filter_map(|s| Id::new(s.segment_id.as_str()))
            .collect();
        let pending = Pending {
            entry,
            recalculated: Recalculated {
                input,
                receipt: deciding.receipt.clone(),
                record: deciding.record.clone(),
                segments: ids,
                route,
                from: at.clone(),
                amount,
                passed_again: passed,
                recalculated: 0,
            },
            retired,
            tried: newly_tried,
        };
        followed
            .pending
            .insert(ask.part.as_str().to_owned(), pending.clone());
        Self::record(followed, parts, ask.part, pending)
    }

    /// Records the part's wait at its floor, the routing's `record` deciding it. A log that can't take it now leaves it
    /// kept, recorded on the next call without routing again; one the log refuses, or one the order Core follows no
    /// longer takes (the part read again moved on, or is stuck), is dropped, and the part is routed afresh next time
    /// (`T-020-10b`).
    fn wait(
        followed: &mut Followed,
        parts: &mut Parts<'_>,
        ask: Ask<'_>,
        record: Box<Recorded>,
    ) -> Result<Recalculation, RecalcError> {
        let order = &ask.order.order;
        match parts.segments.wait_at_floor(
            parts.log,
            parts.clock,
            parts.fresh,
            ask.core,
            order,
            ask.part,
        ) {
            Ok(_) => {
                followed.waiting.remove(ask.part.as_str());
                Ok(Recalculation::WaitsForCustomer { record })
            }
            Err(crate::segments::SegmentsError::Append(AppendFailure::Unavailable)) => {
                Err(RecalcError::Append(AppendFailure::Unavailable))
            }
            Err(crate::segments::SegmentsError::WaitNotTaken(_)) => {
                followed.waiting.remove(ask.part.as_str());
                let why = parts
                    .segments
                    .order(order)
                    .and_then(|o| {
                        o.parts
                            .iter()
                            .find(|p| p.part_id.as_str() == ask.part.as_str())
                    })
                    .and_then(|p| evidenced(p).err())
                    .unwrap_or(NotRecalculable::NoSettledEnd);
                Ok(Recalculation::NotNow(why))
            }
            Err(crate::segments::SegmentsError::Append(refused)) => {
                followed.waiting.remove(ask.part.as_str());
                Err(RecalcError::Append(refused))
            }
            Err(other) => {
                followed.waiting.remove(ask.part.as_str());
                Err(RecalcError::Wait(other))
            }
        }
    }

    /// Appends a recalculation's input, built once, and once the log takes it moves the order Core follows to the
    /// new route and releases the old route's holds.
    fn record(
        followed: &mut Followed,
        parts: &mut Parts<'_>,
        part: &Id,
        pending: Pending,
    ) -> Result<Recalculation, RecalcError> {
        // The part is read again first: a recalculation built before it moved on is no recalculation now.
        let order = Id::new(pending.recalculated.input.subject.order.as_str())
            .ok_or(RecalcError::Id("an order id"))?;
        let stale = match parts.segments.order(&order) {
            None => Some(Recalculation::NotNow(NotRecalculable::Unknown)),
            Some(followed_order) => {
                let now = followed_order
                    .parts
                    .iter()
                    .find(|p| p.part_id.as_str() == part.as_str());
                match now.map(|p| (evidenced(p), p)) {
                    None => Some(Recalculation::NotNow(NotRecalculable::Unknown)),
                    Some((Err(why), _)) => Some(Recalculation::NotNow(why)),
                    Some((Ok(_), p)) => match handed_over(parts.segments, &order, p) {
                        Some(why) => Some(Recalculation::NotNow(why)),
                        None => {
                            let after =
                                Stm::transition(followed_order, &pending.recalculated.input);
                            (after.last_input.outcome != (stm::Outcome::Applied {}))
                                .then_some(Recalculation::NotNow(NotRecalculable::NoSettledEnd))
                        }
                    },
                }
            }
        };
        if let Some(stale) = stale {
            followed.pending.remove(part.as_str());
            return Ok(stale);
        }
        // Only a log that can't take it now leaves it kept: one the log refuses is no recalculation.
        let receipt = match append(parts.log, &pending.entry) {
            Ok(receipt) => receipt,
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            Err(refused) => {
                followed.pending.remove(part.as_str());
                return Err(refused.into());
            }
        };
        followed.pending.remove(part.as_str());
        let mut recalculated = pending.recalculated;
        recalculated.receipt = receipt;
        if !parts.segments.observe(parts.clock, &recalculated.input) {
            // Recorded, but the order Core follows didn't take it: no hold is released and nothing is counted.
            return Ok(Recalculation::NotFollowed {
                receipt: recalculated.receipt,
            });
        }
        for segment in &pending.retired {
            parts
                .exposures
                .release(&recalculated.record_order(), segment);
        }
        followed.recalculated += 1;
        recalculated.recalculated = followed.recalculated;
        followed.follows = recalculated.record.request.request_id.clone();
        followed.tried.extend(pending.tried);
        Ok(Recalculation::Recalculated(Box::new(recalculated)))
    }
}

impl Recalculated {
    /// The order the input is about.
    fn record_order(&self) -> Id {
        Id::new(self.input.subject.order.as_str()).expect("an order id is an id")
    }
}

/// The first segment of `part` that hasn't settled and that `segments` handed over with no final outcome, as why
/// the part isn't recalculated.
fn handed_over(segments: &Segments, order: &Id, part: &stm::Part) -> Option<NotRecalculable> {
    part.segments
        .iter()
        .filter(|s| s.state != SegmentState::Settled)
        .find(|s| Id::new(s.segment_id.as_str()).is_some_and(|id| segments.handed_over(order, &id)))
        .map(|s| NotRecalculable::HandedOver {
            segment: s.segment_id.clone(),
        })
}

/// Whether the record considered a candidate and dropped every one as below the floor (`REQ-ORD-27`).
fn every_candidate_below_floor(record: &DecisionRecord) -> bool {
    let candidates = record.candidates();
    !candidates.is_empty()
        && candidates
            .iter()
            .all(|c| matches!(c.dropped, Some(DropReason::BelowFloor {})))
}

/// [`passed_again`] over what Core kept of the segments handed over and never settled.
fn passed_again_from<'a>(
    tried: impl Iterator<Item = &'a Tried> + Clone,
    part: &stm::Id,
    at: &Endpoint,
    route: &[SegmentPlan],
) -> Vec<PassedAgain> {
    let mut passed: Vec<PassedAgain> = Vec::new();
    for again in route {
        if passed.iter().any(|p| p.provider == again.provider) {
            continue;
        }
        let before: Vec<stm::Id> = tried
            .clone()
            .filter(|t| &t.part == part && t.provider == again.provider && t.from == *at)
            .map(|t| t.segment.clone())
            .collect();
        if !before.is_empty() {
            passed.push(PassedAgain {
                provider: again.provider.clone(),
                before,
            });
        }
    }
    passed
}

/// A fresh segment id: none the order used.
fn fresh_segment(fresh: &mut dyn Fresh, used: &[String]) -> Result<Id, RecalcError> {
    for _ in 0..DRAWS {
        let id = fresh.id();
        if !used.iter().any(|u| u == id.as_str()) && stm::Id::new(id.as_str()).is_some() {
            return Ok(id);
        }
    }
    Err(RecalcError::NoFreshSegment)
}

fn stm_id(text: &str) -> Result<stm::Id, RecalcError> {
    stm::Id::new(text).ok_or(RecalcError::Id("an id"))
}

/// Appends `entry`, trying again while the log can't take it now: the log appends an equal entry once.
fn append(log: &mut dyn Log, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
    let mut last = AppendFailure::Unavailable;
    for _ in 0..APPEND_TRIES {
        match log.append(entry) {
            Ok(receipt) => return Ok(receipt),
            Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
            Err(refused) => return Err(refused),
        }
    }
    Err(last)
}
