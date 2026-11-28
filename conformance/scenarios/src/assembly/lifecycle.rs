//! What happens to an order once it goes wrong or ends, over the assembled Core (`T-024-06`): its part recalculated
//! from where its value sits, timed and declared stuck, resolved in the stuck order's two steps, a segment's reversal
//! owed, asked and confirmed, an order ended or returned, and every change of an order notified to the user and the
//! operator.
//!
//! Each method hands Core's own record (`Recalculations`, `Stuck`, `Reversals`, `Notices`) the parts it reaches, as
//! Core's run hands the segments theirs (`rutent_core_orch::schedule::run`, `T-020-61`): Core's clock, log, catalogue, policy, evidence store, exposures and the
//! platform with the simulated providers. This module decides nothing and checks nothing: every rule is Core's own,
//! in `rutent_core_orch`. What a scenario can't reach honestly is named in `F-024`'s notes.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::Operator as _;
use rutent_contracts::core_internal::{Actor, Entry};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_contracts::router_api::Id;
use rutent_core_orch::notice::{About, Channel, Notice, NoticeError, Receiver};
use rutent_core_orch::recalc::Recalculations;
use rutent_core_orch::reversal::{self, Afterward, Asked, Confirmation, Owing, Reversals};
use rutent_core_orch::segments::StatusError;
use serde_json::Value;

use super::{Core, StepError};
use crate::kit;

/// Core's own records of what goes wrong with an order, kept beside the segments': the assembly holds them as the
/// production assembly will (`F-030`).
#[derive(Debug, Default)]
pub struct Lifecycle {
    /// Core's recalculations, which its run lends ([`rutent_core_orch::schedule::Ports::with_recalculations`]).
    pub(super) recalculations: Recalculations,
    pub(super) reversals: Reversals,
    /// What Core's status question of each segment it left unknown came to, by order and segment (`T-020-47`).
    pub(super) status_asks:
        BTreeMap<(String, String), Result<rutent_core_orch::reversal::StatusAsked, String>>,
    /// What Core's run's ask of each failed provider to reverse its segment came to, by order and segment, as a tick told
    /// it and until it is read.
    pub(super) asks_told: BTreeMap<(String, String), Result<Asked, reversal::ReversalError>>,
    /// What Core's run's confirmation of each reversal came to, as a tick told it and until it is read.
    pub(super) confirms_told:
        BTreeMap<(String, String), Result<Confirmation, reversal::ReversalError>>,
}

impl Lifecycle {
    /// Nothing recorded yet.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Core {
    /// A fault the scenarios plant (`T-020-60` item (8)): the statement kept under `commitment` can't be read back by a
    /// Core reopened on this disk from now on; the evidence store keeps it, so the log opens.
    pub fn unreadable_on_reopen(&self, commitment: &rutent_contracts::module_interface::Digest) {
        self.disk.unreadable_on_reopen(commitment);
    }

    /// From now on Core holds no decision record `id` of a recalculated route: a fault a scenario plants (#316's round-3 L7),
    /// as a Core whose evidence store no longer gives the record back would hold it.
    pub fn forget_decision_record(&mut self, id: &Id) {
        self.intake.forget_record(id);
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
            .intake
            .started(order)
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

    // Stuck orders -----------------------------------------------------------------------------------------------

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
        self.stuck
            .follow(&self.segments, &logged, order)
            .map_err(StepError::Stuck)
    }

    /// What the operator sees of the value resting in stuck orders.
    pub fn stuck_values(&self) -> Vec<rutent_core_orch::stuck::StuckValue> {
        self.stuck.stuck_values(&self.exposures, &self.clock)
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
    //
    // A failed segment's reversal is owed, asked and confirmed by Core's run on its ticks, and the inbound commands and
    // statements are handed to Core's entry points (`rutent_core_orch::reversal::command`, `T-020-61` item 10): what is
    // here reads what Core holds, or what a tick told, and hands each command over the deployment's ports.

    /// The reversal `segment` of `order` owes, as Core's run recorded it in the catalogue's list: on a tick since it
    /// failed, else the next tick.
    ///
    /// # Errors
    ///
    /// Where Core's run recorded none.
    pub fn owed_reversal(&mut self, order: &Id, segment: &Id) -> Result<Owing, StepError> {
        if self.life.reversals.owing(order, segment).is_none() {
            self.tick();
        }
        self.life
            .reversals
            .owing(order, segment)
            .cloned()
            .ok_or(StepError::NotNow("Core's run recorded the reversal owed"))
    }

    /// The failed provider of `segment` asked to reverse it by Core's run: the ask a tick since the last read told, else
    /// the next tick's.
    ///
    /// # Errors
    ///
    /// As `Reversals::ask`, or where Core's run asked nothing.
    pub fn asked_on_tick(&mut self, order: &Id, segment: &Id) -> Result<Asked, StepError> {
        let at = (order.as_str().to_owned(), segment.as_str().to_owned());
        // Recorded owed on one tick, asked on the next, confirmed from the one after: as many ticks as it takes.
        for _ in 0..3 {
            if self.life.asks_told.contains_key(&at) {
                break;
            }
            self.tick();
        }
        self.life
            .asks_told
            .remove(&at)
            .ok_or(StepError::NotNow("Core's run asked the failed provider"))?
            .map_err(StepError::Reversal)
    }

    /// The confirmation of `segment`'s reversal Core's run asked of the origin reserve, or the node's holder: the one a
    /// tick since the last read told, else the next tick's.
    ///
    /// # Errors
    ///
    /// As `Reversals::confirm_reversal`, or where Core's run asked nothing.
    pub fn confirmed_on_tick(
        &mut self,
        order: &Id,
        segment: &Id,
    ) -> Result<Confirmation, StepError> {
        let at = (order.as_str().to_owned(), segment.as_str().to_owned());
        // Recorded owed on one tick, asked on the next, confirmed from the one after: as many ticks as it takes. Core
        // asks a confirmation again only on its retry cadence (the review of #355's round 2, H1): the deployment's timer
        // ticks again once it has passed.
        for _ in 0..3 {
            if self.life.confirms_told.contains_key(&at) {
                break;
            }
            // Core's clock, as its entries stamp it: rounded up to the second.
            let now = u64::try_from(self.clock.peek().div_ceil(1_000_000_000)).unwrap_or(u64::MAX);
            if self.life.reversals.confirmations_asked(order, segment) > 0
                && !self.life.reversals.confirm_due(order, segment, now)
            {
                self.clock.advance(self.life.reversals.retry_seconds());
            }
            self.tick();
        }
        self.life
            .confirms_told
            .remove(&at)
            .ok_or(StepError::NotNow("Core's run asked for a confirmation"))?
            .map_err(StepError::Reversal)
    }

    /// Keeps what a tick told of each failed segment's reversal, for [`Core::asked_on_tick`] and
    /// [`Core::confirmed_on_tick`].
    pub(super) fn told_reversals(&mut self, round: &rutent_core_orch::schedule::Round) {
        for part in &round.runs.parts {
            for reversed in &part.reversed {
                let at = (
                    part.order.as_str().to_owned(),
                    reversed.segment.as_str().to_owned(),
                );
                if let Some(asked) = &reversed.asked {
                    self.life.asks_told.insert(at.clone(), asked.clone());
                }
                if let Some(confirmed) = &reversed.confirmed {
                    self.life.confirms_told.insert(at, confirmed.clone());
                }
            }
        }
    }

    /// Hands Core's entry point the failed provider's showing that the failure of `segment` was not its own
    /// (`rutent_core_orch::reversal::command::show_fault`, `REQ-ORD-11`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    pub fn show_fault(
        &mut self,
        order: &Id,
        segment: &Id,
        bytes: &[u8],
    ) -> Result<rutent_contracts::core_internal::Receipt, StepError> {
        reversal::command::show_fault(&mut self.ports(), order, segment, bytes)
            .map_err(StepError::Reversal)
    }

    /// Hands Core's entry point the acceptance of `segment`'s showing by `by`, the customer in `session` or the
    /// operator (`rutent_core_orch::reversal::command::accept_fault`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    pub fn accept_fault(
        &mut self,
        order: &Id,
        segment: &Id,
        by: &Actor,
        session: &Id,
    ) -> Result<reversal::Carrier, StepError> {
        reversal::command::accept_fault(&mut self.ports(), order, segment, by, session)
            .map_err(StepError::Reversal)
    }

    /// Hands Core's entry point the dispute of `segment`'s showing by `by`
    /// (`rutent_core_orch::reversal::command::dispute`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    pub fn dispute_fault(
        &mut self,
        order: &Id,
        segment: &Id,
        by: &Actor,
    ) -> Result<rutent_contracts::core_internal::Receipt, StepError> {
        reversal::command::dispute(&mut self.ports(), order, segment, by)
            .map_err(StepError::Reversal)
    }

    /// The provider's result Core kept for `segment` of `order`, as its bytes: the signed statement the evidence store
    /// holds behind the log's commitment, which is what a provider's showing of fault is made of (`REQ-ORD-11`).
    /// `None` where Core kept none. The platform answers a repeat of an operation whose segment ended with a refusal,
    /// so the answer is read from what Core kept, as it arrived, and not asked of the provider again.
    pub fn kept_result(&mut self, order: &Id, segment: &Id) -> Option<Vec<u8>> {
        let commitment = self
            .log
            .borrow()
            .entries()
            .iter()
            .rev()
            .find_map(|(_, e)| match e {
                Entry::Stated(s)
                    if s.what.statement.as_str()
                        == rutent_contracts::provider_role::RESULT_SHAPE
                        && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order)
                        && s.about.as_ref().and_then(|a| a.segment.as_ref()) == Some(segment) =>
                {
                    Some(s.commitment.clone())
                }
                _ => None,
            })?;
        let kept = rutent_core_audit::receipts::store::Shelf::load(&mut self.disk.shelf())
            .ok()?
            .kept;
        let held = kept.iter().find(|k| k.kept.commitment == commitment)?;
        rutent_core_audit::log::kept::from_base64url(held.kept.bytes.as_str())
    }

    /// Who carries the reversal of `segment` of `order`, as Core holds it.
    pub fn reversal_carrier(&self, order: &Id, segment: &Id) -> Option<reversal::Carrier> {
        self.life.reversals.carrier(order, segment).cloned()
    }

    /// Whether `segment` of `order` is a move whose outcome Core holds unknown.
    pub fn is_unknown(&self, order: &Id, segment: &Id) -> bool {
        self.segments.unknown_move(order, segment).is_some()
    }

    /// How many confirmations of the reversal of `segment` of `order` Core asked, as the log holds them (the review of
    /// #355's round 2, H1).
    pub fn confirmations_asked(&self, order: &Id, segment: &Id) -> u32 {
        self.life.reversals.confirmations_asked(order, segment)
    }

    /// The transactions of the reversals Core carried out for `order`, in the order it did (`REQ-ORD-41`).
    pub fn reversal_transactions(&self, order: &Id) -> Vec<Id> {
        self.segments.reversal_transactions(order).to_vec()
    }

    /// What Core's status question of the unknown `segment` of `order` came to: the question it asked, or why none was
    /// asked (the step Core's run takes after each part).
    pub fn status_asked(
        &self,
        order: &Id,
        segment: &Id,
    ) -> Option<&Result<rutent_core_orch::reversal::StatusAsked, String>> {
        self.life
            .status_asks
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))
    }

    /// Hands Core's entry point the question whether the start of the part `withdrawal` begins was undone, asked of its
    /// origin reserve (`rutent_core_orch::reversal::command::confirm_undone`, `AC-8`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    pub fn confirm_undone(
        &mut self,
        order: &Id,
        withdrawal: &Id,
    ) -> Result<Confirmation, StepError> {
        reversal::command::confirm_undone(&mut self.ports(), order, withdrawal)
            .map_err(StepError::Reversal)
    }

    /// Hands Core's entry point `by`'s request to end `order` without completing
    /// (`rutent_core_orch::reversal::command::end`, `REQ-ORD-12`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    pub fn end(
        &mut self,
        order: &Id,
        by: &Actor,
    ) -> Result<rutent_contracts::core_internal::Receipt, StepError> {
        reversal::command::end(&mut self.ports(), order, by).map_err(StepError::Reversal)
    }

    /// Hands Core's entry point `bytes`, the destination reserve's report that `order`, complete, was returned
    /// (`rutent_core_orch::reversal::command::returned`, `REQ-ORD-31`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    pub fn returned(&mut self, order: &Id, bytes: &[u8]) -> Result<Afterward, StepError> {
        reversal::command::returned(&mut self.ports(), order, bytes).map_err(StepError::Reversal)
    }

    /// Hands Core's entry point `bytes`, a provider's result about `segment` arriving after its answer was taken, with who
    /// the deployment's transport says handed it over: the provider of the segment, whose module the platform carried
    /// it from, never the maker the bytes claim (`rutent_core_orch::reversal::command::report_late`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    pub fn late_delivery(
        &mut self,
        order: &Id,
        segment: &Id,
        bytes: &[u8],
    ) -> Result<rutent_contracts::core_internal::Receipt, StepError> {
        let provider = self
            .segments
            .order(order)
            .and_then(|o| {
                o.parts
                    .iter()
                    .flat_map(|p| p.segments.iter())
                    .find(|s| s.segment_id.as_str() == segment.as_str())
                    .and_then(|s| Id::new(s.provider.as_str()))
            })
            .ok_or(StepError::NotNow("Core follows the segment"))?;
        let presented_by = super::handed_over_by(&self.catalogue, provider);
        reversal::command::report_late(&mut self.ports(), order, segment, bytes, &presented_by)
            .map_err(StepError::Reversal)
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
