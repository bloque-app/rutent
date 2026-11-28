//! Core's order run (`T-020-61`, #143's first agreed blocker: no v0 behaviour may have the scenario assembly as its only
//! caller): the first job of every tick, [`Job::Run`](super::Job::Run), Core's own body over the same [`Ports`] as every
//! other job.
//!
//! Each tick, in this order:
//!
//! 1. **What each started order runs on, held** ([`resume`]): every order whose `route_chosen` the log holds and intake
//!    holds nothing to run on for (a Core that reopened, an order filed on an automatic deposit, a linked order a hand-on
//!    started) is built from the log as intake's acceptance builds it (`intake::started_on`): the record read back from
//!    the evidence store and held to the log's digest (the review's L6), the authorisation its acceptance names as policy
//!    holds it in force, the order's own origin, session and inputs. A linked order's authorisation is registered with
//!    policy first, as its acceptance names it. An order a reopened Core holds is never given anything to run on
//!    (`T-020-46`, `T-020-52`: the held gate is Core's).
//! 2. **Each started order's parts, run** ([`Segments::run_part`](crate::segments::Segments::run_part)), each on the
//!    route it runs on now: the last recalculated, read from Core's rebuilt `Segments::routes_ran`, else the first. A
//!    segment begun before Core stopped is carried on first ([`Segments::carry_on`](crate::segments::Segments::carry_on),
//!    item 8). A part with work left ([`Segments::work_left`](crate::segments::Segments::work_left)) is run, and each
//!    segment it left unknown is asked where it stands, once, at a provider that declares a status query (`T-020-47`);
//!    the schedule asks again at its cadence.
//! 3. **Recalculation** (`REQ-ORD-22`: automatic, at the user's cost; `REQ-ORD-4`: bounded): a part with nothing left to
//!    run whose route failed where its value is evidenced is recalculated from there, and the new route runs on the next
//!    tick: one step of a part a tick, so what each step recorded is in the log before the next is taken. A part whose
//!    failed segment owes its reversal isn't recalculated (the review of `T-020-57`), and a route recalculated and not
//!    yet run never is again: Core's run recalculates only a part with nothing left to run whose route failed. Past the bound, or with no route offered, the part is
//!    declared stuck for want of a route (`REQ-ORD-28`), for the operator.
//! 4. **The hand-ons done** (`REQ-ORD-43`, `REQ-ORD-40`): each stuck part whose decided hand-on Core's mover carried out,
//!    its withdrawal from the holding account done on the account's own word, is handed on to its linked order, and the
//!    linked order's start is proven by that word ([`HandOn::hand_on`](crate::hand_on::HandOn::hand_on),
//!    [`HandOn::prove_start`](crate::hand_on::HandOn::prove_start)); the next tick builds what the linked order runs on and
//!    runs it. The withdrawal is the mover's record, or, for a Core that reopened, the log's
//!    ([`crate::hand_on::withdrawn_in_the_log`]).
//!
//! **Fails closed.** A port that can't answer leaves the order's work for the next tick, and the round says why.

use rutent_contracts::core_internal::{
    Action, Actor, Entry, Outcome, Receipt, Resolution, RouteEnd, VerifiedEntry, What,
};
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_contracts::state_machine::{Observed, RecordedInput, SegmentState};
use rutent_router::RouteFinder;

use super::jobs::{Jobs, Ports, current_route, record_entry};
use super::{StatusTried, Unknown};
use crate::hand_on::{Basis, HandedOn, Handing, Start};
use crate::intake::{
    Begun, DESTINATION_INPUT, Placed, Started, accepted_authorisation, held_record, started_on,
};
use crate::recalc::{Ask, Recalculation, route_end};
use crate::reversal::{Asked, At, CarriedReversal, Carrier, Confirmation, Owing, ReversalError};
use crate::rounds::{self, Presence};
use crate::segments::{CarriedOn, CarryOn, Live, Ran};
use crate::stuck::NoRouteLeft;

/// One part of one order, as Core's run took it on a tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartRan {
    /// The order.
    pub order: Id,
    /// The part.
    pub part: Id,
    /// Each segment begun before a restart that the run carried on, and how (item 8).
    pub carried_on: Vec<(Id, CarriedOn)>,
    /// What the part's run came to, where it had work left; `None` where it had none.
    pub ran: Option<Result<Ran, String>>,
    /// The recalculation tried for the part, where nothing of it was left to run and its route failed (`REQ-ORD-22`):
    /// the new route runs on the next tick.
    pub recalculated: Option<Result<Recalculation, String>>,
    /// The part declared stuck for want of a route, where its recalculation left none (`REQ-ORD-28`): the receipt of
    /// its `no_route` input, or why it wasn't recorded.
    pub no_route: Option<Result<Receipt, String>>,
    /// The status questions Core put after the run, about the segments it left unknown (`T-020-47`).
    pub asked: Vec<StatusTried>,
    /// Each failed segment's reversal the run took on this tick (`REQ-ORD-3`, `REQ-ORD-11`, `REQ-ORD-30`).
    pub reversed: Vec<ReversalRan>,
}

/// A failed segment's reversal, as Core's run took it on a tick (`T-020-61`, item 10): recorded owed in the catalogue's
/// list, then, in turn, asked of the failed provider while it carries it and confirmed with the origin reserve, or the
/// node's holder, until it completes (`REQ-ORD-3`, `REQ-ORD-11`, `REQ-ORD-30`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReversalRan {
    /// The failed segment.
    pub segment: Id,
    /// The reversal owed, as the catalogue's list keeps it, or why it wasn't recorded.
    pub owed: Result<Owing, ReversalError>,
    /// The failed provider asked to reverse it, where Core asked this tick.
    pub asked: Option<Result<Asked, ReversalError>>,
    /// The last ask's unknown outcome carried on, where Core carried it on this tick: its status asked, its operation
    /// repeated or its recorded answer read, under the ask's own transaction.
    pub carried_on: Option<Result<CarriedReversal, ReversalError>>,
    /// The confirmation asked this tick, where Core asked one.
    pub confirmed: Option<Result<Confirmation, ReversalError>>,
}

impl ReversalRan {
    /// Whether the reversal completed on this step: the failed provider's answer counted, or the confirmation did.
    pub fn completed(&self) -> bool {
        matches!(&self.asked, Some(Ok(asked)) if asked.completed.is_some())
            || matches!(&self.carried_on, Some(Ok(carried)) if carried.completed.is_some())
            || matches!(&self.confirmed, Some(Ok(Confirmation::Completed(_))))
    }
}

/// A hand-on Core's run recorded, or tried to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandedOnRan {
    /// The order handed on.
    pub order: Id,
    /// The linked order.
    pub linked: Id,
    /// The hand-on, its start proven, or why it wasn't.
    pub handed: Result<HandedOn, String>,
}

/// What Core's run did on a tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Runs {
    /// The hand-ons recorded.
    pub handed_on: Vec<HandedOnRan>,
    /// The orders whose run Core built from the log this tick: what each runs on, now held.
    pub started: Vec<Id>,
    /// Each part of each started order, as the run took it.
    pub parts: Vec<PartRan>,
}

impl Runs {
    /// What the run of `order` came to this tick, each part's in order: what ran, or the first part's error. Empty
    /// where no part of it had work left.
    pub fn ran(&self, order: &Id) -> Result<Vec<Ran>, String> {
        self.parts
            .iter()
            .filter(|p| &p.order == order)
            .filter_map(|p| p.ran.clone())
            .collect()
    }

    /// The hand-on of `order` recorded this tick, where one was.
    pub fn handed_on(&self, order: &Id) -> Option<&Result<HandedOn, String>> {
        self.handed_on
            .iter()
            .find(|h| &h.order == order)
            .map(|h| &h.handed)
    }

    /// The reversals of `order`'s failed segments the run took this tick.
    pub fn reversed(&self, order: &Id) -> Vec<&ReversalRan> {
        self.parts
            .iter()
            .filter(|p| &p.order == order)
            .flat_map(|p| p.reversed.iter())
            .collect()
    }

    /// The recalculation tried for `order` this tick, where one was.
    pub fn recalculated(&self, order: &Id) -> Option<&Result<Recalculation, String>> {
        self.parts
            .iter()
            .filter(|p| &p.order == order)
            .find_map(|p| p.recalculated.as_ref())
    }
}

/// The route a part runs on now.
struct Route {
    record: DecisionRecord,
    segments: Vec<Id>,
    at: usize,
    end: RouteEnd,
}

impl Route {
    fn live<'a>(
        &'a self,
        order: &'a Id,
        part: &'a Id,
        started: &'a Started,
        actors: &'a super::Actors,
        record_entry: Option<&'a Receipt>,
    ) -> Live<'a> {
        Live {
            order,
            part,
            segments: &self.segments,
            record: &self.record,
            at: self.at,
            authorisation: &started.authorisation,
            origin_reserve: &started.origin,
            route: &self.end,
            caller: &actors.core,
            core: &actors.core,
            operator: &actors.operator,
            session: started.session.as_ref(),
            inputs: &started.inputs,
            floor: started.floor.as_ref(),
            record_entry,
        }
    }
}

/// Core's own resume (`T-020-61`, items 1, 2 and 5): what each order the log started runs on, built from the log and
/// held by intake where intake holds nothing for it yet, and the decision record of each route recalculated since, held
/// to the log's digest. An order a reopened Core holds, its inputs unreadable, is given nothing to run on: the held gate
/// is Core's. The deployment calls it once as Core reopens, and each tick runs it first. Answers the orders it built.
pub fn resume(ports: &mut dyn Ports) -> Vec<Id> {
    let history = ports.history();
    let mut built = Vec::new();
    for (_, entry) in &history {
        let Entry::Verified(VerifiedEntry {
            what: What::Input { input },
            ..
        }) = entry
        else {
            continue;
        };
        let Observed::RouteChosen { handed_on_from, .. } = &input.observed else {
            continue;
        };
        let Some(order) = Id::new(input.subject.order.as_str()) else {
            continue;
        };
        if ports.intake().started(&order).is_some() || ports.intake().held(&order) {
            continue;
        }
        let started = match handed_on_from {
            None => started_plain(ports, &history, &order, input),
            Some(from) => Id::new(from.as_str())
                .and_then(|from| started_linked(ports, &history, &order, &from, input)),
        };
        if let Some(started) = started {
            ports.with_intake(&mut |intake| intake.hold_started(&order, started.clone()));
            if ports.intake().started(&order).is_some() {
                built.push(order);
            }
        }
    }
    // The segments follow each started order from its start, as the log took it (`T-020-08`): the only start of an
    // order they act on.
    for order in ports.intake().started_orders() {
        if ports.segments().order(&order).is_some() {
            continue;
        }
        let Some(start) = crate::intake::start_in(&history, &order) else {
            continue;
        };
        ports.with_segments(&mut |segments, parts| {
            let _ = segments.follow(parts.clock, &start);
        });
    }
    // The record of each route recalculated since an order started, read back as its first route's is.
    for order in ports.intake().started_orders() {
        let later: Vec<Id> = ports
            .segments()
            .routes_ran(&order)
            .map(|(_, later)| later.iter().map(|r| r.record.clone()).collect())
            .unwrap_or_default();
        for id in later {
            if ports.intake().decision_record(&id).is_some() {
                continue;
            }
            if let Some(record) = held_record(&history, &mut |c| ports.kept(c), &id) {
                ports.with_intake(&mut |intake| intake.hold_record(record.clone()));
            }
        }
    }
    ports.fed();
    built
}

/// What an order intake opened, or one filed on an automatic deposit, runs on, from the log.
fn started_plain(
    ports: &mut dyn Ports,
    history: &[(Receipt, Entry)],
    order: &Id,
    start: &RecordedInput,
) -> Option<Started> {
    let authorisation = accepted_authorisation(history, order)?;
    let authorised = ports.in_force(order, &authorisation)?;
    let begun = if let Some(request) = ports.intake().request_of(order) {
        Begun {
            origin: request.origin_reserve.clone(),
            session: Some(request.session.clone()),
            inputs: request.inputs.clone(),
            delivers: None,
        }
    } else {
        let filing = ports.arrivals().filing_of(order)?.clone();
        let mut inputs = filing.inputs.clone();
        inputs.insert(
            DESTINATION_INPUT.to_owned(),
            filing.destination.account.as_str().to_owned(),
        );
        Begun {
            origin: authorised.origin_reserve.clone(),
            session: Some(filing.session.clone()),
            inputs,
            // What the route delivers before the operator's fee: the chosen candidate's, as the filing's quote showed it.
            delivers: None,
        }
    };
    let built = started_on(
        history,
        &mut |c| ports.kept(c),
        order,
        start,
        &authorised,
        begun.clone(),
    );
    // A filed order's route delivers what its chosen candidate's parts give out, where no move names it yet.
    built.or_else(|| {
        let Observed::RouteChosen {
            decision_record, ..
        } = &start.observed
        else {
            return None;
        };
        let record = held_record(
            history,
            &mut |c| ports.kept(c),
            &Id::new(decision_record.as_str())?,
        )?;
        let delivers = record
            .chosen()?
            .parts
            .iter()
            .try_fold(rutent_contracts::router_api::Amount::zero(), |sum, part| {
                sum.checked_add(&part.operations.last().amount_out)
            })?;
        started_on(
            history,
            &mut |c| ports.kept(c),
            order,
            start,
            &authorised,
            Begun {
                delivers: Some(delivers),
                ..begun
            },
        )
    })
}

/// What a linked order a hand-on started runs on (`REQ-ORD-43`): its acceptance's authorisation, as policy holds it in
/// force since the customer gave it at the acceptance under its own floor ([`crate::intake::linked::accept_linked`]), the
/// first order's route end, the acceptance's session, and the customer's inputs as Core kept them, with the destination's
/// account as the deposit's input.
fn started_linked(
    ports: &mut dyn Ports,
    history: &[(Receipt, Entry)],
    order: &Id,
    from: &Id,
    start: &RecordedInput,
) -> Option<Started> {
    let (authorisation, session) = history.iter().find_map(|(_, e)| match e {
        Entry::Verified(VerifiedEntry {
            what:
                What::Acceptance {
                    order: o,
                    authorisation,
                    session,
                    ..
                },
            ..
        }) if o == order => Some((authorisation.clone(), session.clone())),
        _ => None,
    })?;
    let original = ports.intake().started(from)?.route.clone();
    let Observed::RouteChosen { parts, .. } = &start.observed else {
        return None;
    };
    let first = Id::new(parts.first()?.segments.first()?.provider.as_str())?;
    // The authorisation the customer gave at its acceptance, under its own floor, as policy holds it in force: Core
    // composes none in the customer's name (the review of #355, H2). With none in force, the linked order never starts.
    let authorised = ports.in_force(order, &authorisation)?;
    // The customer's inputs, as Core kept them at its acceptance, and the destination's account as the deposit's.
    let mut inputs = crate::intake::linked::linked_inputs(history, &mut |c| ports.kept(c), order);
    inputs.insert(
        DESTINATION_INPUT.to_owned(),
        authorised.destination.account.as_str().to_owned(),
    );
    started_on(
        history,
        &mut |c| ports.kept(c),
        order,
        start,
        &authorised,
        Begun {
            origin: first,
            session,
            inputs,
            delivers: Some(original.delivers.clone()),
        },
    )
    .map(|mut started| {
        // The route ends where the first order's does: a linked order delivers to the same destination.
        started.route = original;
        started
    })
}

impl Jobs<'_> {
    /// The run's body for one tick (see the module's docs).
    pub(super) fn run_orders(&mut self) -> Runs {
        let started = resume(self.ports_mut());
        let mut parts = Vec::new();
        for order in self.ports().intake().started_orders() {
            let Some(started) = self.ports().intake().started(&order).cloned() else {
                continue;
            };
            for placed in started.parts() {
                parts.push(self.run_part(&order, &started, &placed));
            }
        }
        // A hand-on recorded now starts its linked order, which the next tick builds and runs.
        let handed_on = self.hand_ons();
        Runs {
            handed_on,
            started,
            parts,
        }
    }

    /// The route `placed` of `order` runs on now: the last recalculated, else the first.
    fn route_now(&mut self, order: &Id, started: &Started, placed: &Placed) -> Option<Route> {
        let recalculated = self
            .ports()
            .segments()
            .routes_ran(order)
            .is_some_and(|(_, later)| later.iter().any(|r| r.part == placed.part));
        if !recalculated {
            return Some(Route {
                record: started.record.clone(),
                segments: placed.segments.clone(),
                at: placed.at,
                end: started.route.clone(),
            });
        }
        let (record, segments) = current_route(self.ports(), order, started, placed)?;
        let authorised = self.ports_mut().in_force(order, &started.authorisation)?;
        let end = route_end(
            record.chosen()?.parts.first(),
            &record.bounds.operator_fee,
            &authorised,
        )?;
        Some(Route {
            record,
            segments,
            at: 0,
            end,
        })
    }

    fn run_part(&mut self, order: &Id, started: &Started, placed: &Placed) -> PartRan {
        let mut ran = PartRan {
            order: order.clone(),
            part: placed.part.clone(),
            carried_on: Vec::new(),
            ran: None,
            recalculated: None,
            no_route: None,
            asked: Vec::new(),
            reversed: Vec::new(),
        };
        let Some(route) = self.route_now(order, started, placed) else {
            ran.ran = Some(Err("the route the part runs on now isn't held".to_owned()));
            return ran;
        };
        // A segment begun before Core stopped, carried on (item 8).
        let core = self.ports().actors().core;
        for segment in &route.segments {
            if !self.ports().segments().awaits_answer(order, segment) {
                continue;
            }
            let mut carried = CarriedOn::Failed;
            self.ports_mut().with_segments(&mut |segments, parts| {
                carried = segments.carry_on(
                    parts,
                    &CarryOn {
                        order,
                        segment,
                        core: &core,
                    },
                );
            });
            self.ports_mut().fed();
            ran.carried_on.push((segment.clone(), carried));
        }
        // Each segment that failed owing its reversal on an earlier tick is taken a step on: recorded owed and its provider
        // asked, or its reversal confirmed (item 10). One step a tick, as recalculation is.
        ran.reversed = self.reverse_owed(order, &placed.part, &route);
        // A part runs, or, where nothing of it is left to run and its route failed, is recalculated: one step a tick, so the
        // recalculated route runs on the next (`REQ-ORD-22`).
        if self
            .ports()
            .segments()
            .work_left(order, &placed.part, &route.segments)
        {
            ran.ran = Some(self.run_route(order, started, &placed.part, &route));
            ran.asked
                .extend(self.ask_unknown(order, &placed.part, &route.segments));
        } else if !ran.reversed.iter().any(ReversalRan::completed)
            && self.failed(order, &placed.part, &route)
        {
            // A reversal that completed this tick was the part's step: the part is recalculated from its value on the next.
            let recalculated = self.recalculate(order, started, &placed.part);
            match &recalculated {
                Ok(Recalculation::Recalculated(new)) => {
                    let record = new.record.clone();
                    self.ports_mut()
                        .with_intake(&mut |intake| intake.hold_record(record.clone()));
                }
                Ok(
                    outcome @ (Recalculation::NoRouteLeft { .. }
                    | Recalculation::NoRoute { .. }
                    | Recalculation::Rejected { .. }),
                ) => {
                    ran.no_route = Some(self.no_route(order, &placed.part, outcome, &route.record));
                }
                _ => {}
            }
            ran.recalculated = Some(recalculated);
        }
        ran
    }

    /// Runs `route` of `part` through the segments, the one way a part runs.
    fn run_route(
        &mut self,
        order: &Id,
        started: &Started,
        part: &Id,
        route: &Route,
    ) -> Result<Ran, String> {
        let actors = self.ports().actors();
        let history = self.ports().history();
        let entry = record_entry(&history, &route.record);
        let live = route.live(order, part, started, &actors, entry.as_ref());
        // The judgements the route's eligibility figures carry, each found in the log (`REQ-CPL-12`): what Core declares
        // the inputs of a segment it hands over rest on, before the segment is handed over.
        let judgements: Vec<_> = route
            .record
            .snapshot
            .figures
            .iter()
            .filter_map(|f| match &f.value {
                rutent_contracts::router_api::FigureValue::Eligibility { judgement, .. } => {
                    let ports = self.ports_mut();
                    crate::reliance::stated_judgement(&history, &mut |c| ports.kept(c), judgement)
                        .map(|found| ((**judgement).clone(), found))
                }
                _ => None,
            })
            .collect();
        let mut ran = Err("the segments weren't lent".to_owned());
        self.ports_mut().with_segments(&mut |segments, parts| {
            segments.declare_cited(&live, &mut |statement| {
                judgements
                    .iter()
                    .find(|(held, _)| held == statement)
                    .map(|(_, found)| found.clone())
            });
            ran = segments
                .run_part(parts, &live)
                .map_err(|why| format!("{why:?}"));
        });
        self.ports_mut().fed();
        ran
    }

    /// Takes a step on the reversal of each segment of `part` that failed owing it (`REQ-ORD-3`, `REQ-ORD-11`,
    /// `REQ-ORD-30`; `T-020-61`, item 10), one step a tick: recorded owed in the catalogue's list; then, while the failed
    /// provider carries it, the step [`Reversals::next_step`](crate::reversal::Reversals::next_step) names. The provider
    /// is asked once, under one transaction, and again only where every earlier ask failed, moving nothing, on the
    /// retry cadence and up to the cap; an ask whose outcome is unknown is carried on once under its own transaction
    /// (its status, a safe repeat, or the answer the platform recorded), never asked again (the review of #355's round
    /// 2, H1); and the origin reserve, or the node's holder, is asked to confirm on the cadence and up to the cap, past
    /// which the part is the stuck check's. Where the provider can't be asked (no reserve at the node, policy's
    /// refusal), the confirmation is asked on the same tick. A Core that reopens rebuilds all of it from the log. One
    /// the customer or the operator carries, on the provider's showing they accepted, is theirs: Core asks no one. A
    /// stuck part's reversal still runs: the provider carries it while a dispute over its fault stands, and the value it
    /// returns goes back where it came from, which the part's resolution may then cite. A deposit owes none.
    fn reverse_owed(&mut self, order: &Id, part: &Id, route: &Route) -> Vec<ReversalRan> {
        use crate::reversal::ReversalStep;
        let segments = self.ports().segments();
        let owed: Vec<Id> = segments
            .order(order)
            .into_iter()
            .flat_map(|o| o.parts.iter())
            .filter(|p| p.part_id.as_str() == part.as_str())
            .flat_map(|p| p.segments.iter())
            .filter(|s| {
                s.state == SegmentState::FailedReversalOwed
                    && s.kind != rutent_contracts::state_machine::OperationKind::Deposit
            })
            .filter_map(|s| Id::new(s.segment_id.as_str()))
            .filter(|s| self.ports().reversals().completed(order, s).is_none())
            .collect();
        let mut done = Vec::new();
        for segment in owed {
            let history = self.ports().history();
            let bounds = route.record.bounds.clone();
            let mut step = ReversalRan {
                segment: segment.clone(),
                owed: Err(ReversalError::Unknown),
                asked: None,
                carried_on: None,
                confirmed: None,
            };
            self.ports_mut()
                .with_reversal_desk(&mut |reversals, parts, desk| {
                    reversals.follow(order, &history);
                    let at = At {
                        order,
                        part,
                        segment: &segment,
                    };
                    // Recorded owed on the first tick it is seen, its own step; taken on from the tick after.
                    let first = reversals.owing(order, &segment).is_none();
                    step.owed = reversals.owe(parts, at);
                    if first || step.owed.is_err() {
                        return;
                    }
                    let carried_by_provider = reversals
                        .carrier(order, &segment)
                        .is_none_or(|c| matches!(c, Carrier::Provider(_)));
                    if !carried_by_provider {
                        return;
                    }
                    let now = parts.clock.now().timestamp().map_or(0, |t| {
                        u64::try_from(t.unix_nanos() / 1_000_000_000).unwrap_or(0)
                    });
                    let mut moving = crate::reversal::Moving {
                        decisions: &mut *desk.decisions,
                        modules: desk.platform.modules(),
                    };
                    match reversals.next_step(order, &segment, now) {
                        ReversalStep::Ask => {
                            struct Nobody;
                            impl crate::rounds::Providers for Nobody {
                                fn ask(&mut self, _: &Id, _: &[u8]) -> Option<Vec<u8>> {
                                    None
                                }
                            }
                            let mut asking = crate::reversal::Asking {
                                bounds: &bounds,
                                key: &mut *desk.key,
                                providers: &mut Nobody,
                                records: desk.records,
                            };
                            let before = reversals.asks_sent(order, &segment);
                            let asked = reversals.ask(parts, &mut asking, &mut moving, at);
                            // Asked, its answer the provider's to give: taken on from the next tick. Where the provider
                            // couldn't be asked and nothing went (no reserve at the node, policy's refusal), the
                            // confirmation is asked now.
                            let sent_nothing =
                                asked.is_err() && reversals.asks_sent(order, &segment) == before;
                            step.asked = Some(asked);
                            if !sent_nothing {
                                return;
                            }
                        }
                        ReversalStep::CarryOn => {
                            step.carried_on =
                                Some(reversals.carry_on_reversal(parts, &mut moving, at));
                            return;
                        }
                        ReversalStep::Confirm => {}
                        ReversalStep::Wait => return,
                    }
                    if !reversals.confirm_due(order, &segment, now) {
                        return;
                    }
                    let mut asking = crate::reversal::Asking {
                        bounds: &bounds,
                        key: &mut *desk.key,
                        providers: desk.platform.providers(),
                        records: desk.records,
                    };
                    step.confirmed = Some(reversals.confirm_reversal(parts, &mut asking, at));
                });
            self.ports_mut().fed();
            done.push(step);
        }
        done
    }

    /// Asks where each of `segments` stands that the run left unknown, once, at a provider that declares a status
    /// query (`REQ-ORD-1`, `T-020-47`); the schedule asks again at its provider's cadence.
    fn ask_unknown(&mut self, order: &Id, part: &Id, segments: &[Id]) -> Vec<StatusTried> {
        let mut asked = Vec::new();
        for segment in segments {
            let Some(moved) = self.ports().segments().unknown_move(order, segment) else {
                continue;
            };
            let unknown = Unknown {
                order: order.clone(),
                segment: segment.clone(),
                provider: moved.provider.as_str().to_owned(),
            };
            let history = self.ports().history();
            let mut tried: Result<Option<crate::reversal::StatusAsked>, String> =
                Err("the reversals weren't lent".to_owned());
            self.ports_mut()
                .with_reversals(&mut |reversals, parts, moving| {
                    reversals.follow(order, &history);
                    tried = reversals
                        .ask_unknown_status(
                            parts,
                            moving,
                            At {
                                order,
                                part,
                                segment,
                            },
                        )
                        .map_err(|why| format!("{why:?}"));
                });
            self.ports_mut().fed();
            match tried {
                Ok(None) => {}
                Ok(Some(status)) => asked.push(StatusTried {
                    unknown,
                    asked: Ok(status.effect.clone()),
                    kept: status.answer.clone(),
                    status: Some(status),
                }),
                Err(why) => asked.push(StatusTried {
                    unknown,
                    asked: Err(why),
                    kept: None,
                    status: None,
                }),
            }
        }
        asked
    }

    /// Whether something of `route` failed where its part may be recalculated from: a segment Core holds ended without
    /// settling, the part refused by its guard, or the state machine's next segment of the part failed or reversed.
    /// A part stuck, waiting for the customer or ended is the operator's or the customer's, never recalculated here.
    fn failed(&self, order: &Id, part: &Id, route: &Route) -> bool {
        let segments = self.ports().segments();
        if segments.stuck(order, part) || segments.waiting(order, part).is_some() {
            return false;
        }
        let Some(followed) = segments.order(order) else {
            return false;
        };
        let Some(stm_part) = followed
            .parts
            .iter()
            .find(|p| p.part_id.as_str() == part.as_str())
        else {
            return false;
        };
        if rutent_contracts::state_machine::rules::is_ended(stm_part.state)
            || matches!(
                stm_part.state,
                rutent_contracts::state_machine::OrderState::Stuck
                    | rutent_contracts::state_machine::OrderState::WaitingForCustomer
            )
        {
            return false;
        }
        segments.guard_refused(order, part)
            || route
                .segments
                .iter()
                .any(|s| segments.ended_unsettled(order, s))
            || stm_part
                .segments
                .iter()
                .find(|s| s.state != SegmentState::Settled)
                .is_some_and(|next| {
                    matches!(
                        next.state,
                        SegmentState::FailedReversalOwed | SegmentState::ReversalComplete
                    )
                })
    }

    /// Recalculates `part` of `order` from where its value is evidenced, at the user's cost, over Core's own rounds
    /// (`REQ-ORD-22`, `REQ-ORD-4`). A part whose failed segment owes its reversal isn't: its value is the reversal's
    /// until it completes (the review of `T-020-57`).
    fn recalculate(
        &mut self,
        order: &Id,
        started: &Started,
        part: &Id,
    ) -> Result<Recalculation, String> {
        // A segment that owes its reversal, not yet complete, holds the part's value in flight: the recalculation is
        // refused here, as the recalculations' own rule refuses it (`recalc::evidenced`), so neither alone carries it.
        let owed = self
            .ports()
            .segments()
            .order(order)
            .into_iter()
            .flat_map(|o| o.parts.iter())
            .filter(|p| p.part_id.as_str() == part.as_str())
            .flat_map(|p| p.segments.iter())
            .find(|s| {
                Id::new(s.segment_id.as_str()).is_some_and(|id| {
                    self.ports().reversals().owing(order, &id).is_some()
                        && self.ports().reversals().completed(order, &id).is_none()
                })
            })
            .map(|s| (s.segment_id.clone(), s.state));
        if let Some((segment, state)) = owed {
            return Ok(Recalculation::NotNow(
                crate::recalc::NotRecalculable::InFlight { segment, state },
            ));
        }
        let authorised = self
            .ports_mut()
            .in_force(order, &started.authorisation)
            .ok_or("policy holds the order's authorisation in force no more")?;
        let session = started.session.clone().ok_or("a user is present")?;
        let actors = self.ports().actors();
        let routed = rounds::Order {
            order: order.clone(),
            caller: actors.core.clone(),
            origin_reserve: started.origin.clone(),
            presence: Presence::User { session },
            inputs: started.inputs.clone(),
            needs: Vec::new(),
        };
        let history = self.ports().history();
        let inputs: Vec<RecordedInput> = history
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(VerifiedEntry {
                    what: What::Input { input },
                    ..
                }) => Some(input.clone()),
                _ => None,
            })
            .collect();
        let previous = history
            .iter()
            .rev()
            .find(|(_, e)| {
                matches!(e, Entry::Verified(v)
                    if matches!(&v.what, What::DecisionRecord { order: o, .. } if o == order))
            })
            .map(|(receipt, _)| receipt.clone());
        let bounds = self.ports().bounds();
        let objective = started.record.request.objective;
        let follows = started.record.request.request_id.clone();
        let mut done = Err("the recalculations weren't lent".to_owned());
        self.ports_mut()
            .with_recalculations(&mut |recalculations, parts, prices| {
                recalculations.follow_recorded(order, &follows, &inputs);
                done = recalculations
                    .recalculate_chained::<RouteFinder>(
                        parts,
                        Ask {
                            order: &routed,
                            part,
                            authorisation: &authorised,
                            objective: &objective,
                            bounds: &bounds,
                            prices,
                            core: &actors.core,
                            operator: &actors.operator,
                        },
                        previous.clone(),
                    )
                    .map_err(|why| format!("{why:?}"));
            });
        self.ports_mut().fed();
        done
    }

    /// Declares `part` of `order` stuck for want of a route (`REQ-ORD-28`), its recalculation having left none, the value
    /// resting where the last segment that settled put it; Core's record of the order's ends then follows the log.
    fn no_route(
        &mut self,
        order: &Id,
        part: &Id,
        outcome: &Recalculation,
        last: &DecisionRecord,
    ) -> Result<Receipt, String> {
        let core = self.ports().actors().core;
        let mut recorded = Err("the stuck orders weren't lent".to_owned());
        self.ports_mut().with_stuck(&mut |stuck, parts, _, _| {
            recorded = stuck
                .no_route(
                    parts,
                    NoRouteLeft {
                        order,
                        part,
                        outcome,
                        last_record: last.record_id(),
                        core: &core,
                    },
                )
                .map(|recorded| recorded.receipt)
                .map_err(|why| format!("{why:?}"));
        });
        self.ports_mut().fed();
        if recorded.is_ok() {
            let history: Vec<Entry> = self.ports().history().into_iter().map(|(_, e)| e).collect();
            self.ports_mut().with_stuck(&mut |stuck, parts, _, _| {
                let _ = stuck.follow(&*parts.segments, &history, order);
            });
        }
        recorded
    }

    /// Declares `part` of `order` stuck where Core can't name the route it runs on now (#316's round-3 L7): its
    /// recalculated route's record isn't held, or its segments aren't that route's. It is never timed on another route;
    /// it is the operator's, on the last route the log names for it.
    pub(super) fn unnamed_route_stuck(
        &mut self,
        order: &Id,
        part: &Id,
    ) -> Result<crate::stuck::Watch, String> {
        let core = self.ports().actors().core;
        let last = self
            .ports()
            .segments()
            .routes_ran(order)
            .and_then(|(first, later)| {
                later
                    .iter()
                    .rev()
                    .find(|r| &r.part == part)
                    .map(|r| r.record.clone())
                    .or_else(|| Some(first.clone()))
            })
            .ok_or("Core follows no route of the order")?;
        let outcome = Recalculation::NoRouteLeft { recalculated: 0 };
        let mut recorded = Err("the stuck orders weren't lent".to_owned());
        self.ports_mut().with_stuck(&mut |stuck, parts, _, _| {
            recorded = stuck
                .no_route(
                    parts,
                    NoRouteLeft {
                        order,
                        part,
                        outcome: &outcome,
                        last_record: &last,
                        core: &core,
                    },
                )
                .map_err(|why| format!("{why:?}"));
        });
        self.ports_mut().fed();
        match recorded {
            Ok(_) => Err(format!(
                "the route the part runs on now isn't held: declared stuck on {}",
                last.as_str()
            )),
            Err(why) if why.contains("AlreadyStuck") => Ok(crate::stuck::Watch::AlreadyStuck),
            Err(why) => Err(format!(
                "the route the part runs on now isn't held, and it couldn't be declared stuck: {why}"
            )),
        }
    }

    /// Each hand-on done that the log holds no route of its linked order for yet, handed on and its start proven.
    fn hand_ons(&mut self) -> Vec<HandedOnRan> {
        let history = self.ports().history();
        let mut handed = Vec::new();
        for (receipt, entry) in &history {
            let Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::ResolveStuck {
                                order,
                                resolution: Resolution::HandOn { linked_order, .. },
                                ..
                            },
                        decision,
                    },
                ..
            }) = entry
            else {
                continue;
            };
            let done = history.iter().any(|(_, e)| {
                matches!(e, Entry::Verified(v)
                    if matches!(&v.what, What::Result { effect } if effect == receipt)
                        && matches!(v.outcome, Outcome::Done {}))
            });
            let proven = history.iter().any(|(_, e)| {
                matches!(e, Entry::Verified(v)
                    if matches!(&v.what, What::HoldingWithdrawal { order: o, .. } if o == linked_order))
            });
            if !done || proven {
                continue;
            }
            // The mover who carried the resolution out, as policy's permit names it: the operator who states the
            // linked order's route.
            let Some(mover) = mover_of(&history, decision.receipt()) else {
                continue;
            };
            let done = self.hand_on(order, receipt, &mover, &history);
            handed.push(HandedOnRan {
                order: order.clone(),
                linked: linked_order.clone(),
                handed: done,
            });
        }
        handed
    }

    fn hand_on(
        &mut self,
        order: &Id,
        effect: &Receipt,
        by: &Actor,
        history: &[(Receipt, Entry)],
    ) -> Result<HandedOn, String> {
        let part = self
            .ports()
            .intake()
            .started(order)
            .and_then(Started::first)
            .ok_or("the order handed on started")?
            .part;
        let in_memory = self.ports().mover().withdrawn(effect).map(|withdrawn| {
            (
                withdrawn.linked.clone(),
                withdrawn.route.clone(),
                withdrawn.start.clone(),
                withdrawn.statement.cloned(),
            )
        });
        let withdrawn = match in_memory {
            Some((linked, route, start, statement)) => Some((
                linked,
                route,
                start,
                statement.ok_or("the withdrawal's answer isn't the log's")?,
            )),
            // A Core that reopened holds no mover's record of the withdrawal: the log's is read (`T-020-46`).
            None => crate::hand_on::withdrawn_in_the_log(
                history,
                &mut |c| self.ports_mut().kept(c),
                effect,
            ),
        };
        let (linked, route, start, statement) =
            withdrawn.ok_or("no withdrawal is done for the hand-on")?;
        let core = self.ports().actors().core;
        let publisher = self
            .ports()
            .catalogue()
            .publishers()
            .publisher_of_node(&start.node)
            .and_then(Id::new);
        let mut done = Err("the hand-ons weren't lent".to_owned());
        self.ports_mut().with_hand_on(&mut |hand_on, parts| {
            done = hand_on
                .hand_on(
                    parts,
                    Handing {
                        order,
                        part: &part,
                        linked: &linked,
                        decision_record: &route.decision_record,
                        plan: &route.plan,
                        acceptance: &route.acceptance,
                        basis: Basis::Resolution { effect },
                        recorded: history,
                        by,
                        core: &core,
                    },
                )
                .and_then(|handed| {
                    hand_on
                        .prove_start(
                            parts,
                            Start {
                                linked: &linked,
                                record: &start.record,
                                statement: &statement,
                                core: &core,
                                publisher: publisher.as_ref(),
                            },
                        )
                        .map(|_| handed)
                })
                .map_err(|why| format!("{why:?}"));
        });
        self.ports_mut().fed();
        done
    }
}

/// The actor whose request policy permitted at `decision`: a move's mover, as its decided request names it (a stuck
/// order's move step, or any decided action's caller).
fn mover_of(history: &[(Receipt, Entry)], decision: &Receipt) -> Option<Actor> {
    history.iter().find_map(|(receipt, entry)| match entry {
        Entry::Verified(VerifiedEntry {
            what: What::StuckMove { request, .. },
            ..
        }) if receipt == decision => Some(request.mover.clone()),
        Entry::Verified(VerifiedEntry {
            what: What::Decide { request, .. },
            ..
        }) if receipt == decision => Some(request.caller.clone()),
        _ => None,
    })
}
