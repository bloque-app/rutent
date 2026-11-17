//! What happens to an order once it goes wrong or ends, over the assembled Core (`T-024-06`): its part recalculated
//! from where its value sits, timed and declared stuck, resolved in the stuck order's two steps, a segment's reversal
//! owed, asked and confirmed, an order ended or returned, and every change of an order notified to the user and the
//! operator.
//!
//! Each method hands Core's own record (`Recalculations`, `Stuck`, `Reversals`, `Notices`) the parts it reaches, as
//! [`Core::run`] hands the segments theirs: Core's clock, log, catalogue, policy, evidence store, exposures and the
//! platform with the simulated providers. This module decides nothing and checks nothing: every rule is Core's own,
//! in `rutent_core_orch`. What a scenario can't reach honestly is named in `F-024`'s notes.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::Operator as _;
use rutent_contracts::core_internal::{Actor, Entry, What};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_core_orch::notice::{About, Channel, Notice, NoticeError, Receiver};
use rutent_core_orch::recalc::{self, Ask, Recalculated, Recalculation, Recalculations};
use rutent_core_orch::reversal::{
    self, Afterward, Asked, Asking as ReversalAsking, At, Confirmation, Moving as ReversalMoving,
    Owing, Reversals,
};
use rutent_core_orch::rounds::{self, History, Presence};
use rutent_core_orch::segments::{Live, Ran, StatusError};
use rutent_core_orch::stuck::{
    Deciding, Moved, Moving, NoRouteLeft, NoRouteRecorded, Resolutions, ResolveError, Stuck, Watch,
    Watching,
};
use rutent_router::RouteFinder;
use serde_json::Value;

use super::{Asking, Core, Running, StepError, authorised};
use crate::kit;

/// Core's own records of what goes wrong with an order, kept beside the segments': the assembly holds them as the
/// production assembly will (`F-030`).
#[derive(Debug, Default)]
pub struct Lifecycle {
    recalculations: Recalculations,
    histories: BTreeMap<String, History>,
    /// The route each order's part runs now, once recalculated: its record and its segments.
    routes: BTreeMap<String, (DecisionRecord, Vec<Id>)>,
    stuck: Stuck,
    reversals: Reversals,
}

impl Lifecycle {
    /// Nothing recorded yet.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Core {
    /// What `order` runs under: the record of its route and the segments of its one part, as recalculated last.
    fn current_route(&self, order: &Id) -> Result<(DecisionRecord, Vec<Id>), StepError> {
        if let Some(route) = self.life.routes.get(order.as_str()) {
            return Ok(route.clone());
        }
        let started = self
            .started
            .get(order.as_str())
            .ok_or(StepError::NotNow("the order started"))?;
        let part = started
            .plan
            .first()
            .ok_or(StepError::NotNow("an order has a part"))?;
        Ok((
            started.record.clone(),
            part.segments
                .iter()
                .map(|s| kit::id(s.segment_id.as_str()))
                .collect(),
        ))
    }

    /// The destination `order` was requested to be delivered at.
    pub fn requested_destination(
        &self,
        order: &Id,
    ) -> Option<rutent_contracts::core_internal::Destination> {
        self.requests
            .get(order.as_str())
            .map(|request| request.destination.clone())
    }

    /// The id of `order`'s one part.
    pub fn part_of(&self, order: &Id) -> Result<Id, StepError> {
        let started = self
            .started
            .get(order.as_str())
            .ok_or(StepError::NotNow("the order started"))?;
        started
            .plan
            .first()
            .map(|p| kit::id(p.part_id.as_str()))
            .ok_or(StepError::NotNow("an order has a part"))
    }

    /// Restates what the provider `provider` charges on each movement that ends at its outer node (a deposit), as
    /// Core's own table of prices has it: a price that moved after the quote was shown.
    pub fn reprice_deposits(
        &mut self,
        provider: &str,
        fee: rutent_contracts::router_api::FeeFormula,
    ) {
        for price in &mut self.prices {
            if price.tuple.provider.as_str() == provider
                && matches!(
                    price.tuple.to.node,
                    rutent_contracts::provider_role::NodeRef::Outer(_)
                )
            {
                price.fee = fee.clone();
            }
        }
    }

    /// Sets how many balance checks of one account the deployment counts in a day (`REQ-CAT-4`): a deployment
    /// setting, which a scenario of many orders for one customer raises.
    pub fn set_check_bound(&mut self, max_counted: u32) {
        self.catalogue
            .set_check_bound(rutent_prov::catalogue::CheckBound {
                max_counted,
                window_secs: 86_400,
            });
    }

    // Recalculation ----------------------------------------------------------------------------------------------

    /// Recalculates `order`'s part from where its value is evidenced, at the user's cost, over Core's own rounds.
    ///
    /// # Errors
    ///
    /// If the order didn't start, or the recalculation couldn't be tried.
    pub fn recalculate(&mut self, order: &Id) -> Result<Recalculation, StepError> {
        let started = self
            .started
            .get(order.as_str())
            .cloned()
            .ok_or(StepError::NotNow("the order started"))?;
        let part = self.part_of(order)?;
        let request = self
            .requests
            .get(order.as_str())
            .cloned()
            .ok_or(StepError::NotNow("an order has its request"))?;
        let entries: Vec<(rutent_contracts::core_internal::Receipt, Entry)> =
            self.log.borrow().entries().to_vec();
        let logged: Vec<Entry> = entries.iter().map(|(_, e)| e.clone()).collect();
        let authorisation = authorised(&logged, order, &started.authorisation).ok_or(
            StepError::NotNow("an acceptance rests on its authorisation"),
        )?;
        self.life
            .recalculations
            .follow(order, &started.record.request.request_id);
        if !self.life.histories.contains_key(order.as_str()) {
            let previous = entries
                .iter()
                .rev()
                .find(|(_, e)| {
                    matches!(e, Entry::Verified(v)
                        if matches!(&v.what, What::DecisionRecord { order: o, .. } if o == order))
                })
                .map(|(r, _)| r.clone())
                .ok_or(StepError::NotNow(
                    "the log holds the order's decision record",
                ))?;
            self.life.histories.insert(
                order.as_str().to_owned(),
                History::recorded(previous, Vec::new()),
            );
        }
        let session = started
            .session
            .clone()
            .ok_or(StepError::NotNow("a user is present"))?;
        let routed = rounds::Order {
            order: order.clone(),
            caller: kit::core(),
            origin_reserve: started.origin.clone(),
            presence: Presence::User { session },
            inputs: started.inputs.clone(),
            needs: Vec::new(),
        };
        let (core, operator) = (kit::core(), kit::operator());
        let mut charges = self.policy.clone();
        let mut persons = self.policy.clone();
        let mut asking = Asking {
            platform: &mut self.fleet.platform,
            clock: &self.clock,
        };
        let mut parts = recalc::Parts {
            catalogue: &mut self.catalogue,
            clock: &self.clock,
            exposures: &mut self.exposures,
            releases: &self.releases,
            log: &mut self.general,
            evidence: &mut self.keeper,
            charges: &mut charges,
            persons: &mut persons,
            providers: &mut asking,
            operator_key: &mut self.signer,
            fresh: &mut self.fresh,
            issued: &mut self.issued,
            segments: &mut self.segments,
        };
        let history = self
            .life
            .histories
            .get_mut(order.as_str())
            .ok_or(StepError::NotNow("an order has its history"))?;
        let done = self
            .life
            .recalculations
            .recalculate::<RouteFinder>(
                &mut parts,
                Ask {
                    order: &routed,
                    part: &part,
                    authorisation: &authorisation,
                    objective: &request.route.objective,
                    bounds: &self.bounds,
                    prices: &self.prices,
                    core: &core,
                    operator: &operator,
                },
                history,
            )
            .map_err(StepError::Recalc);
        self.feed()?;
        let done = done?;
        if let Recalculation::Recalculated(recalculated) = &done {
            self.life.routes.insert(
                order.as_str().to_owned(),
                (recalculated.record.clone(), recalculated.segments.clone()),
            );
        }
        Ok(done)
    }

    /// Runs the part of `order` on the route a recalculation recorded, as the segments run any part.
    ///
    /// # Errors
    ///
    /// If the order didn't start or its segments can't run.
    pub fn run_recalculated(
        &mut self,
        order: &Id,
        recalculated: &Recalculated,
    ) -> Result<Ran, StepError> {
        let started = self
            .started
            .get(order.as_str())
            .cloned()
            .ok_or(StepError::NotNow("the order started"))?;
        let part = self.part_of(order)?;
        let (core, operator) = (kit::core(), kit::operator());
        let live = Live {
            order,
            part: &part,
            segments: &recalculated.segments,
            record: &recalculated.record,
            at: 0,
            authorisation: &started.authorisation,
            origin_reserve: &started.origin,
            route: &recalculated.route,
            caller: &core,
            core: &core,
            operator: &operator,
            session: started.session.as_ref(),
            inputs: &started.inputs,
            floor: started.floor.as_ref(),
        };
        self.run_live(&live)
    }

    // Stuck orders -----------------------------------------------------------------------------------------------

    /// Times `order`'s part: raises a material change of a p99 to the operator, and declares the part stuck once
    /// funds it handed over are held past twice their p99 or advertised time.
    ///
    /// # Errors
    ///
    /// As `Stuck::watch`.
    pub fn watch(&mut self, order: &Id) -> Result<Watch, StepError> {
        let part = self.part_of(order)?;
        let (record, segments) = self.current_route(order)?;
        let core = kit::core();
        let mut channel = Channel {
            notices: &mut self.notices,
            key: &mut self.notice_signer,
            transport: &mut self.wire,
        };
        let mut parts = rutent_core_orch::stuck::Parts {
            clock: &self.clock,
            log: &mut self.general,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
        };
        let watched = self
            .life
            .stuck
            .watch(
                &mut parts,
                &mut channel,
                Watching {
                    order,
                    part: &part,
                    segments: &segments,
                    record: &record,
                    at: 0,
                    core: &core,
                },
            )
            .map_err(StepError::Stuck);
        self.feed()?;
        watched
    }

    /// Records the outcome of a recalculation that left `order`'s part no route: the order is stuck, or waits for the
    /// customer where its value sits at the node a failed final deposit left it.
    ///
    /// # Errors
    ///
    /// As `Stuck::no_route`.
    pub fn record_no_route(
        &mut self,
        order: &Id,
        outcome: &Recalculation,
    ) -> Result<NoRouteRecorded, StepError> {
        let part = self.part_of(order)?;
        let (record, _) = self.current_route(order)?;
        let core = kit::core();
        let mut parts = rutent_core_orch::stuck::Parts {
            clock: &self.clock,
            log: &mut self.general,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
        };
        let recorded = self
            .life
            .stuck
            .no_route(
                &mut parts,
                NoRouteLeft {
                    order,
                    part: &part,
                    outcome,
                    last_record: record.record_id(),
                    core: &core,
                },
            )
            .map_err(StepError::Stuck);
        self.feed()?;
        let recorded = recorded?;
        self.follow_stuck(order)?;
        Ok(recorded)
    }

    /// Has Core's stuck record follow `order`, from the log's own record of its authorisation.
    ///
    /// # Errors
    ///
    /// If Core doesn't follow the order.
    pub fn follow_stuck(&mut self, order: &Id) -> Result<(), StepError> {
        let logged: Vec<Entry> = self
            .log
            .borrow()
            .entries()
            .iter()
            .map(|(_, e)| e.clone())
            .collect();
        self.life
            .stuck
            .follow(&self.segments, &logged, order)
            .map_err(StepError::Stuck)
    }

    /// `decider`'s decision of how `order`'s stuck part is resolved, under policy, naming `evidence`.
    ///
    /// # Errors
    ///
    /// As `Stuck::decide`.
    pub fn decide_stuck(
        &mut self,
        order: &Id,
        decider: &Actor,
        resolution: &rutent_contracts::core_internal::Resolution,
        evidence: &[rutent_contracts::module_interface::Digest],
    ) -> Result<rutent_contracts::core_internal::Decision, ResolveError> {
        let part = self.part_of(order).map_err(|_| ResolveError::NotStuck)?;
        let mut parts = rutent_core_orch::stuck::Parts {
            clock: &self.clock,
            log: &mut self.general,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
        };
        self.life.stuck.decide(
            &mut parts,
            &mut self.policy,
            Deciding {
                order,
                part: &part,
                decider,
                resolution,
                evidence,
            },
        )
    }

    /// `mover`'s carrying out of a decision, through `moves`: the one step that moves value.
    ///
    /// # Errors
    ///
    /// As `Stuck::carry_out`.
    pub fn carry_out_stuck(
        &mut self,
        order: &Id,
        carries_out: &rutent_contracts::core_internal::Decided,
        mover: &Actor,
        moves: &mut dyn Resolutions,
    ) -> Result<Moved, ResolveError> {
        let core = kit::core();
        let mut parts = rutent_core_orch::stuck::Parts {
            clock: &self.clock,
            log: &mut self.general,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
        };
        self.life.stuck.carry_out(
            &mut parts,
            &mut self.policy,
            moves,
            Moving {
                order,
                carries_out,
                mover,
                core: &core,
            },
        )
    }

    /// What the operator sees of the value resting in stuck orders.
    pub fn stuck_values(&self) -> Vec<rutent_core_orch::stuck::StuckValue> {
        self.life.stuck.stuck_values(&self.exposures, &self.clock)
    }

    /// What `reader`, a provider, reads of `segment` of `order`.
    ///
    /// # Errors
    ///
    /// As `seen_by_provider`.
    pub fn seen_by_provider(
        &mut self,
        reader: &Actor,
        order: &Id,
        segment: &Id,
    ) -> Result<rutent_core_orch::stuck::ProviderView, StatusError> {
        rutent_core_orch::stuck::seen_by_provider(
            &self.segments,
            &mut self.policy.clone(),
            &self.clock,
            &mut self.fresh,
            reader,
            order,
            segment,
        )
    }

    /// The state of `segment`'s part as Core follows it, with the segment's.
    pub fn followed(&self, order: &Id) -> Option<rutent_contracts::state_machine::Order> {
        self.segments.order(order).cloned()
    }

    /// Whether `order`'s part is held stuck by Core's segments.
    pub fn is_stuck(&self, order: &Id) -> bool {
        self.part_of(order)
            .is_ok_and(|part| self.segments.stuck(order, &part))
    }

    /// The provider's answer to `segment`'s operation of `order`, asked again under its own transaction: what a
    /// provider that answered late would deliver afterwards.
    pub fn provider_again(&mut self, order: &Id, segment: &Id) -> Option<Vec<u8>> {
        let (transaction, _) = self.segments.sent(order, segment)?;
        let transaction = transaction.as_str().to_owned();
        let provider = self
            .segments
            .order(order)?
            .parts
            .iter()
            .flat_map(|p| p.segments.iter())
            .find(|s| s.segment_id.as_str() == segment.as_str())?
            .provider
            .as_str()
            .to_owned();
        let operation: Statement<Operation> = self
            .fleet
            .platform
            .world
            .journal(&provider)
            .iter()
            .find(|e| {
                rutent_conformance::sim::transaction_of(&e.operation.statement).as_deref()
                    == Some(transaction.as_str())
            })?
            .operation
            .clone();
        self.fleet.platform.world.set_clock(self.clock.instant());
        match self.fleet.platform.handle(&provider, &operation) {
            rutent_prov::module::types::Handled::Answered(result) => {
                serde_json::to_vec(&*result).ok()
            }
            rutent_prov::module::types::Handled::Refused(_) => None,
        }
    }

    // Reversals and endings ----------------------------------------------------------------------------------------

    /// Runs `with` over Core's reversals, handed the parts they reach and, for a reversal asked of a failed provider,
    /// the platform that carries it out: the providers' modules are the platform's, so no question goes to a provider
    /// by `asking` here.
    fn reversing<T>(
        &mut self,
        order: &Id,
        with: impl FnOnce(
            &mut Reversals,
            &mut reversal::Parts<'_>,
            &mut ReversalAsking<'_>,
            &mut ReversalMoving<'_>,
        ) -> T,
    ) -> Result<T, StepError> {
        struct Nobody;
        impl rutent_core_orch::rounds::Providers for Nobody {
            fn ask(&mut self, _: &Id, _: &[u8]) -> Option<Vec<u8>> {
                None
            }
        }
        let (record, _) = self.current_route(order)?;
        let entries = self.log.borrow().entries().to_vec();
        self.life.reversals.follow(order, &entries);
        let (core, operator) = (kit::core(), kit::operator());
        let readings = self.readings.clone();
        readings.refresh(&self.fleet.platform.world);
        let bounds = record.bounds.clone();
        let mut asked = ReversalAsking {
            bounds: &bounds,
            key: &mut self.signer,
            providers: &mut Nobody,
        };
        let mut running = Running {
            platform: &mut self.fleet.platform,
            clock: &self.clock,
            faults: &mut self.faults,
            feed: &mut self.feed,
            log: &self.log,
            swap_to: self.swap_to.as_deref_mut(),
            readings: &readings,
        };
        let mut decisions = self.policy.clone();
        let mut moving = ReversalMoving {
            decisions: &mut decisions,
            modules: &mut running,
        };
        let mut parts = reversal::Parts {
            clock: &self.clock,
            catalogue: &mut self.catalogue,
            log: &mut self.general,
            evidence: &mut self.keeper,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
            core: &core,
            operator: &operator,
        };
        Ok(with(
            &mut self.life.reversals,
            &mut parts,
            &mut asked,
            &mut moving,
        ))
    }

    /// Runs `with` over Core's reversals, handed the parts they reach and the providers' modules, which Core asks
    /// whether a credit arrived.
    fn asking<T>(
        &mut self,
        order: &Id,
        with: impl FnOnce(&mut Reversals, &mut reversal::Parts<'_>, &mut ReversalAsking<'_>) -> T,
    ) -> Result<T, StepError> {
        let (record, _) = self.current_route(order)?;
        let entries = self.log.borrow().entries().to_vec();
        self.life.reversals.follow(order, &entries);
        let (core, operator) = (kit::core(), kit::operator());
        let mut asking = Asking {
            platform: &mut self.fleet.platform,
            clock: &self.clock,
        };
        let bounds = record.bounds.clone();
        let mut asked = ReversalAsking {
            bounds: &bounds,
            key: &mut self.signer,
            providers: &mut asking,
        };
        let mut parts = reversal::Parts {
            clock: &self.clock,
            catalogue: &mut self.catalogue,
            log: &mut self.general,
            evidence: &mut self.keeper,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
            core: &core,
            operator: &operator,
        };
        Ok(with(&mut self.life.reversals, &mut parts, &mut asked))
    }

    /// Records the reversal `segment` of `order` owes, in the catalogue's list.
    ///
    /// # Errors
    ///
    /// As `Reversals::owe`.
    pub fn owe_reversal(&mut self, order: &Id, segment: &Id) -> Result<Owing, StepError> {
        let part = self.part_of(order)?;
        let done = self.asking(order, |reversals, parts, _| {
            reversals.owe(
                parts,
                At {
                    order,
                    part: &part,
                    segment,
                },
            )
        })?;
        self.feed()?;
        done.map_err(StepError::Reversal)
    }

    /// Asks the failed provider of `segment` to reverse it: its answer is its own word.
    ///
    /// # Errors
    ///
    /// As `Reversals::ask`.
    pub fn ask_reversal(&mut self, order: &Id, segment: &Id) -> Result<Asked, StepError> {
        let part = self.part_of(order)?;
        let done = self.reversing(order, |reversals, parts, asking, moving| {
            reversals.ask(
                parts,
                asking,
                moving,
                At {
                    order,
                    part: &part,
                    segment,
                },
            )
        })?;
        self.feed()?;
        done.map_err(StepError::Reversal)
    }

    /// Asks the origin reserve whether the credit of `segment`'s reversal arrived, and completes the reversal where
    /// its answer counts.
    ///
    /// # Errors
    ///
    /// As `Reversals::confirm`.
    pub fn confirm_reversal(
        &mut self,
        order: &Id,
        segment: &Id,
    ) -> Result<Confirmation, StepError> {
        let part = self.part_of(order)?;
        let done = self.asking(order, |reversals, parts, asking| {
            reversals.confirm(
                parts,
                asking,
                At {
                    order,
                    part: &part,
                    segment,
                },
            )
        })?;
        self.feed()?;
        done.map_err(StepError::Reversal)
    }

    /// Ends `order` without completing, on its customer's request.
    ///
    /// # Errors
    ///
    /// As `Reversals::end`.
    pub fn end(
        &mut self,
        order: &Id,
    ) -> Result<rutent_contracts::core_internal::Receipt, StepError> {
        let done = self.asking(order, |reversals, parts, _| reversals.end(parts, order))?;
        self.feed()?;
        done.map_err(StepError::Reversal)
    }

    /// Records that `order`, complete, was returned: `bytes`, its destination reserve's report.
    ///
    /// # Errors
    ///
    /// As `Reversals::returned`.
    pub fn returned(&mut self, order: &Id, bytes: &[u8]) -> Result<Afterward, StepError> {
        let done = self.asking(order, |reversals, parts, _| {
            reversals.returned(parts, order, bytes)
        })?;
        self.feed()?;
        done.map_err(StepError::Reversal)
    }

    /// Keeps `bytes`, a provider's result about `segment` arriving after its answer was taken, moving nothing.
    ///
    /// # Errors
    ///
    /// As `Reversals::later`.
    pub fn late_delivery(
        &mut self,
        order: &Id,
        segment: &Id,
        bytes: &[u8],
    ) -> Result<rutent_contracts::core_internal::Receipt, StepError> {
        let part = self.part_of(order)?;
        let done = self.asking(order, |reversals, parts, _| {
            reversals.later(
                parts,
                At {
                    order,
                    part: &part,
                    segment,
                },
                bytes,
            )
        })?;
        self.feed()?;
        done.map_err(StepError::Reversal)
    }

    /// What came after `order` ended, as `reader` may read it.
    ///
    /// # Errors
    ///
    /// As `Reversals::seen`.
    pub fn after_the_end(
        &mut self,
        reader: &Actor,
        order: &Id,
    ) -> Result<Vec<Afterward>, StatusError> {
        self.life.reversals.seen(
            &mut self.policy.clone(),
            &self.clock,
            &mut self.fresh,
            reader,
            None,
            order,
        )
    }

    /// The reversal `segment` of `order` completed, as Core holds it.
    pub fn completed_reversal(
        &self,
        order: &Id,
        segment: &Id,
    ) -> Option<rutent_core_orch::reversal::Completed> {
        self.life.reversals.completed(order, segment).cloned()
    }

    // Notices ----------------------------------------------------------------------------------------------------

    /// Sends the notices the log's changes call for, to each order's user and to the operator, from where Core last
    /// stopped. How many were sent.
    ///
    /// # Errors
    ///
    /// If the log isn't the one the notices read before.
    pub fn notify(&mut self) -> Result<usize, NoticeError> {
        let entries = self.log.borrow().entries().to_vec();
        self.notices
            .notify(&entries, &mut self.notice_signer, &mut self.wire)
    }

    /// The notices of `order` sent to `to`, as its inbox would read them, in order.
    pub fn notices_to(&self, to: &Receiver, order: &Id) -> Vec<Notice> {
        self.wire
            .sent
            .iter()
            .filter(|(receiver, _)| receiver == to)
            .filter_map(|(_, sealed)| {
                let value: Value = serde_json::from_slice(sealed).ok()?;
                serde_json::from_value::<Notice>(value.get("notice")?.clone()).ok()
            })
            .filter(|notice| {
                matches!(&notice.about, About::Order { order: o, .. } if o.as_str() == order.as_str())
            })
            .collect()
    }

    /// The user's receiver, the one an order's customer is notified as.
    pub fn user_receiver() -> Receiver {
        Receiver {
            party: kit::customer().party,
            role: rutent_contracts::core_internal::Role::Customer,
        }
    }

    /// The operator's receiver.
    pub fn operator_receiver(&self) -> Receiver {
        self.notices.operator().clone()
    }

    /// The raises Core made to the operator, delivered or not, as the log holds them.
    pub fn raise_entries(&self) -> Vec<rutent_contracts::core_internal::Raise> {
        super::charges::raises_in(self.log.borrow().entries())
    }

    /// Delivers a raise to the operator's channel, as Core would.
    pub fn raise_to_operator(&mut self, raise: &rutent_contracts::core_internal::Raise) -> bool {
        let mut channel = Channel {
            notices: &mut self.notices,
            key: &mut self.notice_signer,
            transport: &mut self.wire,
        };
        channel.raise(raise) == rutent_contracts::core_internal::calls::Delivery::Delivered
    }
}
