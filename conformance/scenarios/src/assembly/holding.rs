//! What the assembly gives a hand-on (`T-020-26`; `REQ-ORD-43`, `REQ-ORD-40`, `INV-5`): the operator's holding
//! accounts, the routes the linked orders were accepted on, and the log itself, which the production mover is handed
//! when it carries out a stuck order's hand-on ([`rutent_core_orch::stuck::Delivering::handing_on`]); and the
//! hand-on and the start of the linked order's route, written once the move is done.
//!
//! The mover asks the holding account's wallet through `HeldWallets`, which runs it on the same platform the
//! mover's other calls go to, one call at a time: the platform sits in a `RefCell` shared by `Shared`, which is
//! what the mover's `modules` is, and by the wallets. The log the mover reads the linked order's acceptance and
//! decision record from is `LogHandle`, the log's own handle and never a copy of its entries, so what it reads is
//! what the log holds when the wallet is about to be asked (`T-020-17`'s review).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    AppendFailure, Decisions, Evidence, GeneralEntry, Log,
};
use rutent_contracts::core_internal::{
    About, Action, Actor, CatalogueChange, DecisionRequest, Effect, Entry, Outcome, Receipt,
    Violation, What,
};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_contracts::router_api::{DecisionRecord, Id, PolicyBounds};
use rutent_contracts::state_machine::SegmentPlan;
use rutent_core_authz::roles::Permission;
use rutent_core_orch::hand_on::{
    Basis, HandOn, HandOnError, HandedOn, Handing, Parts as HandParts, Start,
};
use rutent_core_orch::rounds::Fresh;
use rutent_core_orch::segments::{Answered, Landed, Landings, Modules};
use rutent_core_orch::snapshot::Clock;
use rutent_core_orch::stuck::{
    AcceptedRoute, LinkedRoutes, LogRead, OperatorSigns, Show, Wallets, Withdrawals,
};
use rutent_prov::catalogue::Instant;
use rutent_prov::holding::wallet::{HandedOnStart, Holdings, Operator, StartError, WalletError};
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord};
use rutent_prov::module::platform::{Platform, World};
use rutent_prov::module::types::Events;
use rutent_prov::module::types::Handled;

use super::{AssemblyError, Core, CoreLog, Running, StepError};
use crate::kit::{self, RealKey};
use crate::sim::InstalledWallet;

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
    pub(super) holdings: &'a RefCell<Holdings>,
    pub(super) running: &'a RefCell<Running<'b>>,
    pub(super) bounds: &'a PolicyBounds,
    /// What the log holds for each order at each node (`Segments::resting_totals`).
    pub(super) resting: BTreeMap<(String, String), rutent_contracts::router_api::Amount>,
}

impl Withdrawals for HeldWallets<'_, '_> {
    fn holder(&self, node: &str) -> Option<String> {
        self.holdings
            .borrow()
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
            holdings: &mut self.holdings.borrow_mut(),
            platform: &mut *running.platform,
            operator: &operator,
            resting: &self.resting,
        }
        .withdraw(route, order, amount, asked)
    }
}

/// The operator's holding accounts, as the segments land the value that settles at their nodes (`T-020-33`): the wallet
/// is asked through the platform, in a statement the operator signs.
pub(super) struct Landing<'a, W: World, O: Operator> {
    pub(super) holdings: &'a mut Holdings,
    pub(super) platform: &'a mut Platform<W>,
    pub(super) operator: &'a O,
}

impl<W: World, O: Operator> Landings for Landing<'_, W, O> {
    fn holds(&self, node: &str) -> bool {
        self.holdings.account(node).is_some()
    }

    fn land(
        &mut self,
        node: &str,
        asset: &str,
        order: &str,
        amount: &rutent_contracts::router_api::Amount,
    ) -> Landed {
        answered(self.holdings.land(
            self.platform,
            self.operator,
            node,
            asset,
            order,
            amount.as_str(),
        ))
    }

    fn release(
        &mut self,
        node: &str,
        asset: &str,
        order: &str,
        amount: &rutent_contracts::router_api::Amount,
    ) -> Landed {
        answered(self.holdings.release(
            self.platform,
            self.operator,
            node,
            asset,
            order,
            amount.as_str(),
        ))
    }

    fn restarted(&self, node: &str) -> bool {
        self.holdings.restarted(self.platform, node)
    }

    fn restore(
        &mut self,
        node: &str,
        asset: &str,
        order: &str,
        amount: &rutent_contracts::router_api::Amount,
    ) -> Landed {
        if self
            .holdings
            .account(node)
            .is_none_or(|account| account.asset() != asset)
        {
            return Landed::Unconfirmed;
        }
        // Under a transaction derived from the node, the order and the module's start now (`T-020-46`, `T-024-21`). A
        // module that runs no process now, one whose releases a restarted platform holds until the operator confirms
        // them among others, was asked nothing: nothing took effect, and it is asked again once it runs.
        let Some(transaction) = self
            .holdings
            .restore_transaction(self.platform, node, order)
        else {
            return Landed::NotNow;
        };
        answered(
            self.holdings
                .restore(
                    self.platform,
                    self.operator,
                    node,
                    order,
                    amount.as_str(),
                    transaction.as_str(),
                )
                .map(|_| ()),
        )
    }
}

/// What the wallet's answer to a landing or a release says to Core (`T-020-33`).
fn answered(result: Result<(), WalletError>) -> Landed {
    match result {
        Ok(()) => Landed::Placed,
        // Nothing of the operation took effect: it can be asked again.
        Err(WalletError::NotAnswered(
            PlatformRefusal::Unavailable | PlatformRefusal::StatementKeyUnavailable,
        )) => Landed::NotNow,
        // Anything else may have moved value, or breaks the account's own rules: never asked again.
        Err(_) => Landed::Unconfirmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rutent_prov::holding::HoldingError;

    /// A wallet answer reaches Core as asked again only where the platform says nothing took effect; every other
    /// answer, an unknown outcome and the account's own refusals included, is never asked again.
    // verifies: REQ-AUD-15, INV-5
    #[test]
    fn only_a_refusal_that_moved_nothing_is_asked_again() {
        assert_eq!(answered(Ok(())), Landed::Placed);
        for nothing_passed_out in [
            PlatformRefusal::Unavailable,
            PlatformRefusal::StatementKeyUnavailable,
        ] {
            assert_eq!(
                answered(Err(WalletError::NotAnswered(nothing_passed_out))),
                Landed::NotNow
            );
        }
        for refused in [
            PlatformRefusal::OutcomeUnknown,
            PlatformRefusal::NotHandled,
            PlatformRefusal::MalformedOutput,
        ] {
            assert_eq!(
                answered(Err(WalletError::NotAnswered(refused))),
                Landed::Unconfirmed
            );
        }
        for other in [
            WalletError::NoHoldingAccount("n".into()),
            WalletError::NotSucceeded,
            WalletError::WrongResult,
            WalletError::Disagrees {
                asked: 1,
                stated: "2".into(),
            },
            WalletError::Holding(HoldingError::BadAmount("x".into())),
        ] {
            assert_eq!(answered(Err(other)), Landed::Unconfirmed);
        }
    }
}

/// The operator, keeping the bytes of the one statement it signed.
struct Recording<'a, O> {
    inner: &'a O,
    asked: RefCell<Option<Vec<u8>>>,
}

impl<O: Operator> Operator for Recording<'_, O> {
    fn statement(&self, operation: Operation) -> Statement<Operation> {
        let statement = self.inner.statement(operation);
        *self.asked.borrow_mut() = serde_json::to_vec(&statement).ok();
        statement
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
    pub fn holdings(&mut self) -> std::cell::RefMut<'_, Holdings> {
        self.holdings.borrow_mut()
    }

    /// The host's holding accounts opened again after a restart of Core and its platform (`T-024-21`): nothing of the
    /// host's memory survives, so each account the deployment's register names is opened from the register (its node,
    /// wallet and asset) and the platform's record (the transactions it asked under), its attribution empty until Core
    /// restores what the log holds for each order there (`Segments::reconcile_landings`), and its wallet taken as
    /// restarted until it confirms a movement.
    pub(super) fn reopen_holdings(&mut self) {
        let accounts: Vec<(String, String, String)> = {
            let registers = self.deployed.registers.borrow();
            registers
                .holding_accounts
                .iter()
                .filter_map(|(node, provider)| {
                    let asset = registers.holding_assets.get(node)?;
                    Some((node.clone(), provider.clone(), asset.clone()))
                })
                .collect()
        };
        let mut holdings = self.holdings.borrow_mut();
        for (node, provider, asset) in accounts {
            holdings.reopen_account(&self.fleet.platform, &node, &asset, &provider);
        }
    }

    /// Whether `node`'s holding account's module restarted since it last confirmed a movement, as the host reads it
    /// from the platform's report of the module's start (`Holdings::restarted`, `T-024-21`).
    pub fn holding_account_restarted(&self, node: &str) -> bool {
        self.holdings.borrow().restarted(&self.fleet.platform, node)
    }

    /// The transaction a restore at `node`'s holding account for `order` would go under now
    /// (`Holdings::restore_transaction`, `T-020-46`, `T-024-21`).
    pub fn restore_transaction(&self, node: &str, order: &str) -> Option<String> {
        self.holdings
            .borrow()
            .restore_transaction(&self.fleet.platform, node, order)
    }

    /// A hand-on's withdrawal of `amount` of `order`'s value for `route`, through Core's own [`Wallets`], as the mover
    /// asks it, where the log holds `resting` of the order at the route's first node and the account's module may have
    /// restarted: a test hook (`T-020-46`) for an order of two values at one node, which the kit's routes don't make.
    ///
    /// # Errors
    ///
    /// The withdrawal's refusal.
    pub fn withdraw_for_a_hand_on(
        &mut self,
        route: &[SegmentPlan],
        order: &str,
        amount: &str,
        resting: &str,
    ) -> Result<HandedOnStart, StartError> {
        let node = match route.first().map(|s| &s.from.node) {
            Some(rutent_contracts::provider_role::NodeRef::Inner(node)) => node.as_str().to_owned(),
            _ => return Err(StartError::NoRecord),
        };
        let mut held = BTreeMap::new();
        if let Some(total) = rutent_contracts::router_api::Amount::new(resting) {
            held.insert((node, order.to_owned()), total);
        }
        let readings = self.readings.clone();
        readings.refresh(&self.fleet.platform.world);
        let running = RefCell::new(super::Running {
            platform: &mut self.fleet.platform,
            clock: &self.clock,
            faults: &mut self.faults,
            feed: &mut self.feed,
            log: &self.log,
            swap_to: self.swap_to.as_deref_mut(),
            readings: &readings,
        });
        let mut wallets = HeldWallets {
            holdings: &mut self.holdings,
            running: &running,
            bounds: &self.bounds,
            resting: held,
        };
        wallets.withdraw(route, order, amount, &mut |_| true)
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
        let withdrawn = match self.mover.withdrawn(effect) {
            Some(withdrawn) => Some((
                withdrawn.linked.clone(),
                withdrawn.route.clone(),
                withdrawn.start.clone(),
                withdrawn
                    .statement
                    .cloned()
                    .ok_or(HandingError::NotWithdrawn)?,
            )),
            // A Core that reopened holds no mover's record of the withdrawal: the log's is read (`T-020-46`).
            None => self.withdrawn_in_the_log(effect),
        };
        let (linked, route, start, statement) = withdrawn.ok_or(HandingError::NotWithdrawn)?;
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

    /// The route the linked order `linked` was accepted on, from the log (`T-020-46`): its acceptance (its id and its
    /// part) and the decision record its route was chosen by, read from the records Core keeps, each segment under the
    /// identifier [`Core::accept_linked`] derives. `None` where any of it isn't there.
    pub(super) fn accepted_in_the_log(
        &self,
        history: &[(Receipt, Entry)],
        linked: &Id,
    ) -> Option<AcceptedRoute> {
        let (acceptance, part) = history.iter().find_map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Acceptance {
                    acceptance,
                    order: o,
                    yeses,
                    ..
                } if o == linked => Some((acceptance.clone(), yeses.first()?.part.clone())),
                _ => None,
            },
            Entry::Stated(_) => None,
        })?;
        let decision_record = history.iter().rev().find_map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::DecisionRecord {
                    decision_record,
                    order: o,
                    ..
                } if o == linked => Some(decision_record.clone()),
                _ => None,
            },
            Entry::Stated(_) => None,
        })?;
        let record = super::reopen::held_record(&self.signer.records, history, &decision_record)?;
        let [only] = record.chosen()?.parts.as_slice() else {
            return None;
        };
        let part_id = rutent_contracts::state_machine::Id::new(part.as_str())?;
        let mut segments = Vec::new();
        for (place, operation) in only.operations.iter().enumerate() {
            segments.push(SegmentPlan {
                segment_id: super::linked::segment_id(&part_id, place).ok()?,
                provider: rutent_contracts::state_machine::Id::new(operation.provider.as_str())?,
                kind: operation.kind,
                from: operation.from.clone(),
                to: operation.to.clone(),
            });
        }
        Some(AcceptedRoute {
            order: linked.clone(),
            decision_record,
            acceptance,
            plan: vec![rutent_contracts::state_machine::PartPlan { part_id, segments }],
        })
    }

    /// The holding account's withdrawal for the hand-on `effect` carried out, as the log holds it, for a Core that
    /// reopened and holds no mover's record of it (`T-020-46`): the effect's `resolve_stuck` with its `done` result,
    /// handing the part's value on to its linked order; the linked order's accepted route, from its acceptance (its
    /// part) and the decision record its route was chosen by, each segment under the identifier
    /// [`Core::accept_linked`] derives; and the holding account's own answer, verified, about the linked order, its
    /// record read from the bytes the evidence store keeps (`proof_of_withdrawal`). `None` where any of it isn't there.
    fn withdrawn_in_the_log(
        &mut self,
        effect: &Receipt,
    ) -> Option<(Id, AcceptedRoute, HandedOnStart, Receipt)> {
        use rutent_contracts::core_internal::{Attribution, Resolution};
        use rutent_contracts::provider_role::{RESULT_SHAPE, ReserveMovementOutcome, ResultBody};
        let history: Vec<(Receipt, Entry)> = self.log.borrow().entries().to_vec();
        let (order, holder, linked) = history.iter().find_map(|(r, e)| match e {
            Entry::Verified(v) if r == effect => match &v.what {
                What::Effect {
                    action:
                        rutent_contracts::core_internal::Action::ResolveStuck {
                            order,
                            resolution:
                                Resolution::HandOn {
                                    by, linked_order, ..
                                },
                            ..
                        },
                    ..
                } => Some((order.clone(), by.clone(), linked_order.clone())),
                _ => None,
            },
            _ => None,
        })?;
        let done = history.iter().any(|(_, e)| {
            matches!(e, Entry::Verified(v) if matches!(&v.what, What::Result { effect: of } if of == effect)
                && matches!(v.outcome, rutent_contracts::core_internal::Outcome::Done {}))
        });
        if !done {
            return None;
        }
        let route = self.accepted_in_the_log(&history, &linked)?;
        let segments = route.plan.first()?.segments.clone();
        let first = segments.first()?.clone();
        let rutent_contracts::provider_role::NodeRef::Inner(node) = &first.from.node else {
            return None;
        };
        let (statement, commitment) = history.iter().find_map(|(r, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str() == RESULT_SHAPE
                    && matches!(&s.attribution, Attribution::Verified { who } if who.acts == holder)
                    && s.about
                        .as_ref()
                        .is_some_and(|a| a.order.as_ref() == Some(&linked)) =>
            {
                Some((r.clone(), s.commitment.clone()))
            }
            _ => None,
        })?;
        let kept = rutent_contracts::core_internal::calls::KeptStatements::kept(
            &mut self.kept,
            &commitment,
        )
        .ok()??;
        let answer = rutent_core_audit::receipts::store::bearing_out(&kept)?;
        let result: Statement<ResultBody> = serde_json::from_slice(&answer).ok()?;
        let ResultBody::Withdrawal(withdrawal) = result.statement else {
            return None;
        };
        let ReserveMovementOutcome::Succeeded(settled) = withdrawal.outcome else {
            return None;
        };
        let reference = rutent_prov::holding::wallet::proof_of_withdrawal(
            holder.as_str(),
            &withdrawal.transaction,
            &settled.evidence,
        )
        .ok()?;
        let start = HandedOnStart {
            reserve: holder.as_str().to_owned(),
            node: node.clone(),
            first_segment: first.segment_id.clone(),
            record: rutent_prov::holding::wallet::WithdrawalRecord {
                by: holder.as_str().to_owned(),
                node: node.clone(),
                asset: first.from.asset.id.clone(),
                order: order.as_str().to_owned(),
                amount: settled.amount_out.parse().ok()?,
                reference,
            },
            transaction: withdrawal.transaction,
            answer,
        };
        Some((linked, route, start, statement))
    }

    /// Installs the operator's plain wallet as the holding account of the public node `node`, holding `asset`
    /// (`REQ-CAT-23`, `T-024-17`): the wallet's two components admitted to the platform on the operator's own signed
    /// release, the node on the operator's public list, and the wallet registered as the node's account. Its release
    /// isn't accepted in the log yet ([`Core::accept_release`]), so the log verifies none of its answers.
    ///
    /// # Errors
    ///
    /// If the node refuses the wallet.
    pub fn install_wallet(
        &mut self,
        node: &str,
        asset: &str,
    ) -> Result<InstalledWallet, AssemblyError> {
        let wallet = self.fleet.install_wallet(kit::OPERATOR_WALLET);
        let _ = self.catalogue.publish_operator_module(&wallet.provider);
        let _ = self.catalogue.add_public_node(node);
        self.holdings
            .borrow_mut()
            .register(
                &mut self.catalogue,
                &self.fleet.platform,
                node,
                &wallet.provider,
                asset,
            )
            .map_err(|e| AssemblyError::Listing(format!("the wallet's registration: {e:?}")))?;
        // The deployment's register of holding accounts names the wallet as the node's holder (`T-020-55`): policy reads
        // it there, never from the host's holdings.
        self.deployed
            .registers
            .borrow_mut()
            .holding_accounts
            .insert(node.to_owned(), wallet.provider.clone());
        self.deployed
            .registers
            .borrow_mut()
            .holding_assets
            .insert(node.to_owned(), asset.to_owned());
        self.deployed
            .registers
            .borrow_mut()
            .holding_decimals
            .insert(node.to_owned(), super::schedule::DECIMALS);
        // The node listed for its wallet gives way to the operator's own setting of it (L2 of #337's review).
        self.reapply_catalogue_settings();
        Ok(wallet)
    }

    /// Accepts `wallet`'s release in the log, as a decided catalogue change (`REQ-CAT-23`, `REQ-AUD-2`): the operator's
    /// login decides it, Core records its effect naming the release's statements key, and its result is `done`. From
    /// then the log verifies the wallet's statements by that key, as it does any module the operator publishes.
    ///
    /// # Errors
    ///
    /// If policy doesn't permit the change, or the log takes no step of it.
    pub fn accept_release(&mut self, wallet: &InstalledWallet) -> Result<Receipt, AssemblyError> {
        self.log_wallet_release_change(
            wallet,
            CatalogueChange::AcceptModule,
            Some(wallet.statement_key()),
        )
    }

    /// Revokes `wallet`'s release in the log, as a decided catalogue change (`REQ-CAT-23`): from then the log attributes
    /// none of its statements but for a transaction a segment of it ran on, and Core asks it nothing more
    /// (`T-020-57`'s review, L1).
    ///
    /// # Errors
    ///
    /// If policy doesn't permit the change, or the log takes no step of it.
    pub fn revoke_release(&mut self, wallet: &InstalledWallet) -> Result<Receipt, AssemblyError> {
        self.log_wallet_release_change(wallet, CatalogueChange::RevokeRelease, None)
    }

    /// `change` of `wallet`'s release, decided by the operator's login, its effect recorded and its result `done`
    /// (`T-020-57`; named apart from the lifecycle command, `Core::change_release`, of `T-020-49`).
    fn log_wallet_release_change(
        &mut self,
        wallet: &InstalledWallet,
        change: CatalogueChange,
        statement_key: Option<rutent_contracts::provider_role::PublicKey>,
    ) -> Result<Receipt, AssemblyError> {
        let listing =
            |step: &str, e: AppendFailure| AssemblyError::Listing(format!("{step}: {e:?}"));
        self.deployed
            .registers
            .borrow_mut()
            .permit(&kit::operator_login(), Permission::Change(change));
        let action = Action::ChangeCatalogue {
            change,
            provider: kit::id(&wallet.provider),
            version: Some(kit::id(&wallet.version)),
            tuple: None,
            class: None,
            advertising_keys: Vec::new(),
            statement_key,
            setting: None,
            versions: Vec::new(),
            public_network: None,
        };
        let request = DecisionRequest {
            request: self.fresh.id(),
            caller: kit::operator_login(),
            action: action.clone(),
            at: self.clock.timestamp(),
        };
        let decision = Decisions::decide(&mut self.policy, &request)
            .map_err(|e| listing("the release's decision", e))?;
        let permit = decision.permit().ok_or_else(|| {
            AssemblyError::Listing(format!(
                "the release wasn't permitted: {:?}",
                decision.outcome
            ))
        })?;
        let effect = self
            .verified(
                What::Effect {
                    action,
                    decision: permit,
                },
                Outcome::ToTakeEffect {},
            )
            .map_err(|e| listing("the release's effect", e))?;
        self.verified(
            What::Result {
                effect: effect.clone(),
            },
            Outcome::Done {},
        )
        .map_err(|e| listing("the release's result", e))?;
        Ok(effect)
    }

    /// What a reopened Core finds of the landings its segments owed (`Segments::reconcile_landings`, `T-020-46`): each
    /// value the log settled at a node a holding account holds, still resting there, restored in the account where
    /// the wallet lacks it, never landed blind. Answers how many it found placed.
    pub(super) fn reconcile_landings(&mut self) -> usize {
        let at = self.clock.instant();
        self.fleet.platform.world.set_clock(at);
        let Some(made_at) = self.clock.now().timestamp() else {
            return 0;
        };
        let mut key = RealKey::assembly();
        let operator = OperatorSigns::new(&mut key, &self.bounds, kit::operator().acts, made_at);
        let mut holdings = self.holdings.borrow_mut();
        let mut landing = Landing {
            holdings: &mut holdings,
            platform: &mut self.fleet.platform,
            operator: &operator,
        };
        self.segments.reconcile_landings(&mut landing)
    }

    /// Value arrives at the holding account of `node` for `order`, as the operator lands it by hand
    /// (`REQ-CAT-23`): the wallet is asked, in a statement signed with the operator's key, to place it in the account.
    /// For a scenario that lands value where no segment settles (`AC-39`'s account checks, `private_nodes.rs`); where a segment settles
    /// at the node, Core lands it itself (its schedule's landing, [`Core::tick`]).
    ///
    /// # Errors
    ///
    /// If the node has no account or the wallet doesn't confirm the movement.
    pub fn land(
        &mut self,
        node: &str,
        asset: &str,
        order: &str,
        amount: &str,
    ) -> Result<(), WalletError> {
        let at = self.clock.instant();
        self.fleet.platform.world.set_clock(at);
        let made_at = self
            .clock
            .now()
            .timestamp()
            .ok_or(WalletError::NotAnswered(PlatformRefusal::Unavailable))?;
        let mut key = RealKey::assembly();
        let operator = OperatorSigns::new(&mut key, &self.bounds, kit::operator().acts, made_at);
        self.holdings.borrow_mut().land(
            &mut self.fleet.platform,
            &operator,
            node,
            asset,
            order,
            amount,
        )
    }

    /// The operator registers `provider`, a module it publishes, as the holding account of the public node `node` in the
    /// catalogue, in place of the one registered there (`REQ-CAT-23`): what a deployment does when it replaces a node's
    /// wallet. Core's own record of who started an order handed on there, the log's `holding_withdrawal`, is unchanged
    /// (`T-020-57`'s review, L1).
    ///
    /// # Errors
    ///
    /// If the catalogue refuses the module or the node.
    pub fn register_node_holding(
        &mut self,
        node: &str,
        provider: &str,
    ) -> Result<(), AssemblyError> {
        let listing = |e| AssemblyError::Listing(format!("the node's holding account: {e:?}"));
        let _ = self.catalogue.publish_operator_module(provider);
        self.catalogue
            .register_public_holding(node, provider)
            .map_err(listing)
    }

    /// The world's account at the wallet `provider`'s node takes a credit back for the reversal of the segment that ran
    /// under `segment` ([`crate::sim::fleet::Fleet::credit_wallet`], `T-020-57`): the world's, never Core's. It stands
    /// in for the network reader's confirmation, owned by `T-020-60`.
    pub fn credit_wallet(&mut self, provider: &str, segment: &str, asset: &str, amount: &str) {
        self.fleet.credit_wallet(provider, segment, asset, amount);
    }

    /// Asks the wallet of `provider` `operation`, in a statement the operator signs, and gives the statement and the
    /// wallet's answer as the platform passed it on: a signed result, re-serialised (`REQ-AUD-2`).
    pub fn ask_wallet(
        &mut self,
        provider: &str,
        operation: Operation,
    ) -> Option<(Vec<u8>, Vec<u8>)> {
        let at = self.clock.instant();
        self.fleet.platform.world.set_clock(at);
        let made_at = self.clock.now().timestamp()?;
        let mut key = RealKey::assembly();
        let operator = OperatorSigns::new(&mut key, &self.bounds, kit::operator().acts, made_at);
        let question = operator.statement(operation);
        match self.fleet.platform.handle(provider, &question) {
            Handled::Answered(result) => Some((
                serde_json::to_vec(&question).ok()?,
                serde_json::to_vec(&*result).ok()?,
            )),
            Handled::Refused(_) => None,
        }
    }

    /// Asks the wallet `operation` as a move does: the operator's question is recorded as its stated entry about
    /// `about` before the answer, which is the wallet's stated entry under the same transaction (the log holds an
    /// answer to a question Core asked). Gives the answer's bytes and the two receipts.
    ///
    /// # Errors
    ///
    /// If the wallet doesn't answer, or the log refuses an entry.
    pub fn ask_wallet_stating(
        &mut self,
        provider: &str,
        operation: Operation,
        about: &About,
    ) -> Result<(Receipt, Receipt, Vec<u8>), AppendFailure> {
        let (question, answer) = self
            .ask_wallet(provider, operation)
            .ok_or(AppendFailure::Refused(Violation("the wallet answers")))?;
        let asked = self.state_by(&question, &kit::operator(), about)?;
        let answered = self.state_answer(&answer, about)?;
        Ok((asked, answered, answer))
    }

    /// The holding account's withdrawal for a route handed on, outside any move (`T-016-03`): the wallet's own
    /// signed answer and record, as [`Holdings::start_handed_on`] gives them, and the operator's statement of the
    /// question it was asked, as the operator signed it.
    ///
    /// # Errors
    ///
    /// If the route isn't handed on from a node with an account, or the wallet doesn't confirm the withdrawal.
    pub fn withdraw_handed_on(
        &mut self,
        route: &[SegmentPlan],
        order: &str,
        amount: &str,
    ) -> Result<(HandedOnStart, Vec<u8>), StartError> {
        let at = self.clock.instant();
        self.fleet.platform.world.set_clock(at);
        let made_at = self.clock.now().timestamp().ok_or(StartError::Wallet(
            rutent_prov::holding::wallet::WalletError::NotAnswered(PlatformRefusal::Unavailable),
        ))?;
        let mut key = RealKey::assembly();
        let signs = OperatorSigns::new(&mut key, &self.bounds, kit::operator().acts, made_at);
        let operator = Recording {
            inner: &signs,
            asked: RefCell::new(None),
        };
        let start = self.holdings.borrow_mut().start_handed_on(
            &mut self.fleet.platform,
            &operator,
            route,
            order,
            amount,
        )?;
        let question = operator.asked.into_inner().ok_or(StartError::NoRecord)?;
        Ok((start, question))
    }

    /// Keeps `answer`, a statement a module made, and appends its stated entry, claimed by the module for the
    /// operator it is published by, received now and about `about`. The log attributes it from the bytes: verified to
    /// the module only where the log holds an accepted release of it that names the key that signed it
    /// (`REQ-AUD-2`).
    ///
    /// # Errors
    ///
    /// If the answer isn't a statement, or the evidence store or the log refuses.
    pub fn state_answer(&mut self, answer: &[u8], about: &About) -> Result<Receipt, AppendFailure> {
        let maker = serde_json::from_slice::<serde_json::Value>(answer)
            .ok()
            .and_then(|v| v["maker"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let by = Actor {
            party: kit::operator().party,
            acts: Id::new(maker.as_str()).ok_or(AppendFailure::Refused(Violation(
                "a module's answer names its maker",
            )))?,
            role: rutent_contracts::core_internal::Role::Provider,
            kind: rutent_contracts::core_internal::ActorKind::Component,
        };
        self.state_by(answer, &by, about)
    }

    /// Keeps `statement` and appends its stated entry, claimed by `by`.
    ///
    /// # Errors
    ///
    /// If the statement isn't JSON, or the evidence store or the log refuses.
    pub fn state_by(
        &mut self,
        statement: &[u8],
        by: &Actor,
        about: &About,
    ) -> Result<Receipt, AppendFailure> {
        let refused = |why: &'static str| AppendFailure::Refused(Violation(why));
        let envelope: serde_json::Value =
            serde_json::from_slice(statement).map_err(|_| refused("a statement is JSON"))?;
        let kept = Evidence::keep(&mut self.keeper, statement)?;
        let entry: GeneralEntry = serde_json::from_value(serde_json::json!({
            "kind": "stated",
            "attribution": {"signature": "unverified", "claimed_by": by},
            "what": {"statement": envelope["shape"]},
            "when": {"at": envelope["made_at"], "source": "party_stated"},
            "received_at": self.clock.timestamp(),
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": "ed25519", "key": envelope["signature"]["key"]},
            "commitment": kept.commitment,
            "about": about,
        }))
        .map_err(|_| refused("a statement is a stated entry"))?;
        let receipt = self.general.append(&entry)?;
        let _ = self.feed();
        Ok(receipt)
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
