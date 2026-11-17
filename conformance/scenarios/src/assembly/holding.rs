//! What the assembly gives a hand-on (`T-020-26`; `REQ-ORD-43`, `REQ-ORD-40`, `INV-5`): the operator's holding
//! accounts, the routes the linked orders were accepted on, and the log itself, which the production mover is handed
//! when it carries out a stuck order's hand-on ([`rutent_core_orch::stuck::Delivering::handing_on`]); and the
//! hand-on and the start of the linked order's route, written once the move is done.
//!
//! The mover asks the holding account's wallet through [`HeldWallets`], which runs it on the same platform the
//! mover's other calls go to, one call at a time: the platform sits in a `RefCell` shared by [`Shared`], which is
//! what the mover's `modules` is, and by the wallets. The log the mover reads the linked order's acceptance and
//! decision record from is [`LogHandle`], the log's own handle and never a copy of its entries, so what it reads is
//! what the log holds when the wallet is about to be asked (`T-020-17`'s review).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_contracts::core_internal::{Actor, Effect, Entry, Receipt};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_contracts::router_api::{DecisionRecord, Id, PolicyBounds};
use rutent_contracts::state_machine::SegmentPlan;
use rutent_core_orch::hand_on::{
    Basis, HandOn, HandOnError, HandedOn, Handing, Parts as HandParts, Start,
};
use rutent_core_orch::segments::{Answered, Modules};
use rutent_core_orch::snapshot::Clock;
use rutent_core_orch::stuck::{
    AcceptedRoute, LinkedRoutes, LogRead, OperatorSigns, Show, Wallets, Withdrawals,
};
use rutent_prov::catalogue::Instant;
use rutent_prov::holding::wallet::{HandedOnStart, Holdings, StartError};
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord};
use rutent_prov::module::types::Events;

use super::{Core, CoreLog, Running, StepError};
use crate::kit::{self, RealKey};

/// The log, as the mover reads it: the log's own handle, so every read is of the log as it stands then.
pub(super) struct LogHandle(pub(super) Rc<RefCell<CoreLog>>);

impl LogRead for LogHandle {
    fn entries(&self) -> Vec<Entry> {
        self.0
            .borrow()
            .entries()
            .iter()
            .map(|(_, entry)| entry.clone())
            .collect()
    }
}

/// The routes the linked orders were accepted on, as the assembly keeps them.
pub(super) struct Accepted<'a>(pub(super) &'a BTreeMap<String, AcceptedRoute>);

impl LinkedRoutes for Accepted<'_> {
    fn accepted(&self, linked: &Id) -> Option<AcceptedRoute> {
        self.0.get(linked.as_str()).cloned()
    }
}

/// The platform of a move, shared by the mover's calls and the holding accounts' wallets, one borrower at a time.
pub(super) struct Shared<'a, 'b>(pub(super) &'a RefCell<Running<'b>>);

impl Modules for Shared<'_, '_> {
    fn begin_segment(
        &mut self,
        provider: &Id,
        transaction: &Identifier,
    ) -> Result<String, PlatformRefusal> {
        self.0.borrow_mut().begin_segment(provider, transaction)
    }

    fn name_calls(
        &mut self,
        transaction: &Identifier,
        record: &DecisionRecord,
        at: (usize, usize),
        segment: &Identifier,
    ) -> Result<usize, rutent_prov::proxy::RouteNotNamed> {
        self.0
            .borrow_mut()
            .name_calls(transaction, record, at, segment)
    }

    fn carry_out(
        &mut self,
        effect: &Effect,
        provider: &Id,
        operation: &Statement<Operation>,
    ) -> Answered {
        self.0.borrow_mut().carry_out(effect, provider, operation)
    }

    fn take_events(&mut self) -> Events {
        self.0.borrow_mut().take_events()
    }

    fn end_segment(&mut self, transaction: &Identifier) -> Option<SegmentRecord> {
        self.0.borrow_mut().end_segment(transaction)
    }

    fn end_calls(&mut self, transaction: &Identifier) {
        self.0.borrow_mut().end_calls(transaction);
    }

    fn staged(&self, provider: &Id) -> Option<String> {
        self.0.borrow().staged(provider)
    }

    fn promote(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal> {
        self.0.borrow_mut().promote(effect, provider, by, at)
    }
}

/// The operator's holding accounts, asked through the platform's wallets: the operator's statement is signed with the
/// operator's key and shown to the mover before the wallet is handed it (`Wallets`).
pub(super) struct HeldWallets<'a, 'b> {
    pub(super) holdings: &'a mut Holdings,
    pub(super) running: &'a RefCell<Running<'b>>,
    pub(super) bounds: &'a PolicyBounds,
}

impl Withdrawals for HeldWallets<'_, '_> {
    fn holder(&self, node: &str) -> Option<String> {
        self.holdings
            .account(node)
            .map(|account| account.provider().to_owned())
    }

    fn withdraw(
        &mut self,
        route: &[SegmentPlan],
        order: &str,
        amount: &str,
        asked: &mut Show<'_>,
    ) -> Result<HandedOnStart, StartError> {
        let mut running = self.running.borrow_mut();
        let at = running.clock.instant();
        running.platform.world.set_clock(at);
        let made_at = running.clock.now().timestamp().ok_or(StartError::Wallet(
            rutent_prov::holding::wallet::WalletError::NotAnswered(PlatformRefusal::Unavailable),
        ))?;
        let mut key = RealKey::assembly();
        let operator = OperatorSigns::new(&mut key, self.bounds, kit::operator().acts, made_at);
        Wallets {
            holdings: &mut *self.holdings,
            platform: &mut *running.platform,
            operator: &operator,
        }
        .withdraw(route, order, amount, asked)
    }
}

/// Why a hand-on couldn't be recorded.
#[derive(Debug)]
pub enum HandingError {
    /// The move's withdrawal isn't done: the mover holds no start the holding account's own answer proves for it.
    NotWithdrawn,
    /// The hand-on, or the start of the linked order's route, was refused.
    HandOn(HandOnError),
    /// The assembly's own step stopped.
    Step(StepError),
}

impl From<HandOnError> for HandingError {
    fn from(e: HandOnError) -> Self {
        HandingError::HandOn(e)
    }
}

impl From<StepError> for HandingError {
    fn from(e: StepError) -> Self {
        HandingError::Step(e)
    }
}

impl Core {
    /// The operator's holding accounts, which a stuck order's hand-on withdraws from.
    pub fn holdings(&mut self) -> &mut Holdings {
        &mut self.holdings
    }

    /// Records the route a linked order was accepted on, which the mover holds a hand-on's route to before the
    /// holding account is asked anything (`T-020-17`). The acceptance and the decision record it names are the log's.
    pub fn accepted_linked(&mut self, route: AcceptedRoute) {
        self.linked.insert(route.order.as_str().to_owned(), route);
    }

    /// Hands `order`'s stuck part on to the linked order of the hand-on `effect` carried out, once the move is done,
    /// then records the holding account's withdrawal as the linked order's start, so its segments are handed over
    /// (`REQ-ORD-43`, `REQ-ORD-40`). The route is the one the mover withdrew for. The record is the holding
    /// account's own answer, which the mover kept as its stated entry; at a private node it is labelled its
    /// publisher's word, as the catalogue names the node's publisher.
    ///
    /// # Errors
    ///
    /// If the mover holds no done withdrawal for the effect, or the hand-on or the start is refused.
    pub fn hand_on(
        &mut self,
        order: &Id,
        effect: &Receipt,
        by: &Actor,
    ) -> Result<HandedOn, HandingError> {
        let part = self
            .first_part(order)
            .ok_or(StepError::NotNow("the order has a part"))?;
        let (linked, route, start, statement) = {
            let withdrawn = self
                .mover
                .withdrawn(effect)
                .ok_or(HandingError::NotWithdrawn)?;
            (
                withdrawn.linked.clone(),
                withdrawn.route.clone(),
                withdrawn.start.clone(),
                withdrawn
                    .statement
                    .cloned()
                    .ok_or(HandingError::NotWithdrawn)?,
            )
        };
        let recorded: Vec<(Receipt, Entry)> = self
            .log
            .borrow()
            .entries()
            .iter()
            .map(|(receipt, entry)| (receipt.clone(), entry.clone()))
            .collect();
        let core = kit::core();
        let publisher = self
            .catalogue
            .publishers()
            .publisher_of_node(&start.node)
            .and_then(Id::new);
        let handed = {
            let mut parts = HandParts {
                clock: &self.clock,
                log: &mut self.general,
                fresh: &mut self.fresh,
                segments: &mut self.segments,
                exposures: &mut self.exposures,
                stuck: &mut self.stuck,
            };
            let handed = self.hand_on.hand_on(
                &mut parts,
                Handing {
                    order,
                    part: &part,
                    linked: &linked,
                    decision_record: &route.decision_record,
                    plan: &route.plan,
                    acceptance: &route.acceptance,
                    basis: Basis::Resolution { effect },
                    recorded: &recorded,
                    by,
                    core: &core,
                },
            )?;
            self.hand_on.prove_start(
                &mut parts,
                Start {
                    linked: &linked,
                    record: &start.record,
                    statement: &statement,
                    core: &core,
                    publisher: publisher.as_ref(),
                },
            )?;
            handed
        };
        self.feed()?;
        Ok(handed)
    }

    /// The mover's read of the log: the log's own handle, never a copy of its entries.
    pub fn log_read(&self) -> impl LogRead + use<> {
        LogHandle(Rc::clone(&self.log))
    }

    /// Proves again, from the log, the starts of the routes handed on whose `holding_withdrawal` it holds: what a
    /// restarted Core's segments forget (`REQ-ORD-43`).
    pub(super) fn recover_starts(&mut self) -> usize {
        let entries: Vec<Entry> = self
            .log
            .borrow()
            .entries()
            .iter()
            .map(|(_, entry)| entry.clone())
            .collect();
        HandOn::recover_starts(&mut self.segments, &entries)
    }
}
