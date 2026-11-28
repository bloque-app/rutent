//! What the hand-on's arm of the production mover reaches (`T-020-17`): the holding account's withdrawal, the linked
//! order's accepted route, and what signs the operator's question to the holding account's wallet.
//!
//! A part handed on is moved by the holding account's own withdrawal for the linked order
//! ([`Holdings::start_handed_on`], `T-016-03`), which takes a route the resolution's effect doesn't name. The route
//! comes from [`LinkedRoutes`], the linked order's own accepted route, and the mover holds it to where Core's own
//! records say the value rests before the wallet is asked anything.
//!
//! The wallet is asked through [`Withdrawals`]. Its one rule is the one every question under `REQ-AUZ-4` keeps: the
//! operator's signed statement is shown to the caller before the wallet is handed it, and **a caller that answers no
//! gets the wallet handed nothing**. [`Wallets`] keeps it over `Holdings` and the platform, where the wallet's
//! statement is built inside `start_handed_on`: a statement the caller refused is replaced by one of a kind no plain
//! wallet handles, which the platform refuses before it runs anything.

use std::cell::RefCell;

use rutent_contracts::core_internal::Entry;
use rutent_contracts::provider_role::{
    OPERATION_SHAPE, Operation as ProviderOperation, Reversal, Signature, SignatureScheme,
    Statement,
};
use rutent_contracts::router_api::{Id, PolicyBounds, Timestamp};
use rutent_contracts::state_machine::{PartPlan, SegmentPlan};
use rutent_prov::holding::wallet::{HandedOnStart, Holdings, Operator, StartError};
use rutent_prov::module::platform::{Platform, World};

use crate::rounds::OperatorKey;
use crate::segments::sign_operation;

/// A linked order's route as it was accepted: the decision record the order's `route_chosen` cites and the parts'
/// plans, each starting where the value sits, with no withdrawal (`REQ-ORD-43`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedRoute {
    /// The order the route is of.
    pub order: Id,
    /// The decision record the route was chosen by.
    pub decision_record: Id,
    /// The customer's acceptance of the linked order, which the log holds.
    pub acceptance: Id,
    /// The parts' plans.
    pub plan: Vec<PartPlan>,
}

/// The linked order's accepted route, as Core's own register keeps what the order was accepted on ([`Linked`]).
pub trait LinkedRoutes {
    /// The route `linked` was accepted on, or `None` where Core holds none.
    fn accepted(&self, linked: &Id) -> Option<AcceptedRoute>;
}

/// The routes the linked orders were accepted on: Core's own register (`T-020-60` item (8), from the check of
/// `T-020-61` at `7557cd9e`), which the mover holds a hand-on's route to before the holding account is asked anything.
/// A linked order Core accepts is taken in ([`Linked::accept`]); a reopened Core rebuilds every one from its log
/// ([`Linked::reopened`], which `schedule::reopen` runs): each acceptance of an order intake never opened, read with the
/// decision record its route was chosen by, held to the log's digest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Linked {
    routes: std::collections::BTreeMap<String, AcceptedRoute>,
}

impl Linked {
    /// No route accepted yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes in the route a linked order was accepted on, in place of any earlier one of that order.
    pub fn accept(&mut self, route: AcceptedRoute) {
        self.routes.insert(route.order.as_str().to_owned(), route);
    }

    /// The register as `history`, the log's entries in order, rebuilds it, the decision records read back through
    /// `kept`: every acceptance of an order no `order_opened` names (a hand-on's linked order), with its accepted route
    /// as the log holds it ([`crate::hand_on::accepted_in_the_log`]). An acceptance whose route can't be read back is
    /// left out, and the mover holds nothing to it (fails closed).
    pub fn reopened(
        history: &[(rutent_contracts::core_internal::Receipt, Entry)],
        kept: &mut dyn FnMut(&rutent_contracts::module_interface::Digest) -> Option<Vec<u8>>,
    ) -> Self {
        use rutent_contracts::core_internal::{VerifiedEntry, What};
        let opened: Vec<&Id> = history
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(VerifiedEntry {
                    what: What::OrderOpened { order, .. },
                    ..
                }) => Some(order),
                _ => None,
            })
            .collect();
        let mut linked = Linked::new();
        for (_, entry) in history {
            let Entry::Verified(VerifiedEntry {
                what: What::Acceptance { order, .. },
                ..
            }) = entry
            else {
                continue;
            };
            if opened.contains(&order) {
                continue;
            }
            if let Some(route) = crate::hand_on::accepted_in_the_log(history, kept, order) {
                linked.accept(route);
            }
        }
        linked
    }

    /// How many routes it holds.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

impl LinkedRoutes for Linked {
    fn accepted(&self, linked: &Id) -> Option<AcceptedRoute> {
        self.routes.get(linked.as_str()).cloned()
    }
}

/// The log's entries, as it holds them: where the linked order's acceptance and decision record, and the original
/// order's requester, are read from before the holding account is asked anything.
pub trait LogRead {
    /// Every entry of the log.
    fn entries(&self) -> Vec<Entry>;
}

/// The holding accounts' withdrawals for a route handed on.
pub trait Withdrawals {
    /// The provider whose wallet is the holding account of `node`, where there is one.
    fn holder(&self, node: &str) -> Option<String>;

    /// Withdraws `amount` of `order`'s value from the holding account at `route`'s first node for the route, and
    /// answers the account's own record of it.
    ///
    /// `asked` is called once with the operator's signed statement of the withdrawal, before the wallet is handed
    /// anything. Where it answers `false` the wallet must not be handed the statement, and the call answers an
    /// error. A call that never reaches `asked` refused the route before it asked the wallet anything.
    fn withdraw(
        &mut self,
        route: &[SegmentPlan],
        order: &str,
        amount: &str,
        asked: &mut Show<'_>,
    ) -> Result<HandedOnStart, StartError>;
}

/// What the hand-on's arm reaches, on top of the mover's own ports.
pub struct HandingOn<'a> {
    /// The holding accounts.
    pub withdrawals: &'a mut dyn Withdrawals,
    /// The linked orders' accepted routes.
    pub routes: &'a dyn LinkedRoutes,
    /// The log's entries.
    pub log: &'a dyn LogRead,
}

/// [`Withdrawals`] over the operator's [`Holdings`] and the platform that runs their wallets.
pub struct Wallets<'a, W: World, O: Operator> {
    /// The operator's holding accounts.
    pub holdings: &'a mut Holdings,
    /// The platform that runs the wallets.
    pub platform: &'a mut Platform<W>,
    /// The operator, whose key signs the withdrawal.
    pub operator: &'a O,
    /// What the log holds for each order at each node, by node and order ([`crate::segments::Segments::resting_totals`]):
    /// what a restore restores, the order's whole as the wallet and the host count it, never the part withdrawn alone.
    pub resting:
        &'a std::collections::BTreeMap<(String, String), rutent_contracts::router_api::Amount>,
}

/// What the caller is shown each statement through.
pub type Show<'a> = dyn FnMut(&Statement<ProviderOperation>) -> bool + 'a;

/// The operator, showing each statement it signs to the caller before it can be sent.
struct Asking<'a, O> {
    inner: &'a O,
    asked: RefCell<&'a mut Show<'a>>,
}

impl<O: Operator> Operator for Asking<'_, O> {
    fn statement(&self, operation: ProviderOperation) -> Statement<ProviderOperation> {
        let statement = self.inner.statement(operation.clone());
        if (self.asked.borrow_mut())(&statement) {
            return statement;
        }
        // Not allowed to go: a reversal, which no plain wallet handles, so the platform refuses it as not handled
        // before it runs anything.
        let (transaction, provider, asset, amount) = match &operation {
            ProviderOperation::Withdrawal(w) => (
                w.transaction.clone(),
                w.provider.clone(),
                w.asset.clone(),
                w.amount.clone(),
            ),
            _ => (
                "refused".to_owned(),
                "refused".to_owned(),
                "refused".to_owned(),
                "0".to_owned(),
            ),
        };
        self.inner.statement(ProviderOperation::Reversal(Reversal {
            transaction: format!("{transaction}-refused"),
            provider,
            reverses: transaction,
            node: "refused".to_owned(),
            asset,
            amount,
        }))
    }
}

impl<W: World, O: Operator> Withdrawals for Wallets<'_, W, O> {
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
        // The account's module restarted, and the wallet's record of the order with it: what the log holds for the
        // order, the value handed on, is restored in the account first, under Core's own transaction, so the
        // withdrawal finds it (`REQ-ORD-43`, `INV-1`, `T-020-46`). The restore isn't the withdrawal's question, so it
        // is not shown to the caller; a restore that fails starts nothing.
        if let Some(SegmentPlan {
            from:
                rutent_contracts::state_machine::Endpoint {
                    node: rutent_contracts::provider_role::NodeRef::Inner(node),
                    ..
                },
            ..
        }) = route.first()
            && self.holdings.restarted(self.platform, node)
        {
            // Under a transaction derived from the node, the order and the module's process now
            // ([`Holdings::restore_transaction`]): never one a restarted Core could draw again.
            let transaction = self
                .holdings
                .restore_transaction(self.platform, node, order)
                .ok_or(StartError::Wallet(
                    rutent_prov::holding::wallet::WalletError::NotKnown,
                ))?;
            let whole = restored_whole(self.resting, node, order, amount);
            self.holdings.restore(
                self.platform,
                self.operator,
                node,
                order,
                &whole,
                &transaction,
            )?;
        }
        let operator = Asking {
            inner: self.operator,
            asked: RefCell::new(asked),
        };
        self.holdings
            .start_handed_on(self.platform, &operator, route, order, amount)
    }
}

/// What a hand-on's restore restores at `node` for `order`: the order's whole value resting there as the log holds
/// it (`resting`, by node and order), never the part withdrawn alone, so the wallet holds every value of the order
/// the log holds at the node; `amount`, the part handed on, where the log holds nothing resting there (`T-020-46`).
fn restored_whole(
    resting: &std::collections::BTreeMap<(String, String), rutent_contracts::router_api::Amount>,
    node: &str,
    order: &str,
    amount: &str,
) -> String {
    resting
        .get(&(node.to_owned(), order.to_owned()))
        .map_or_else(|| amount.to_owned(), |total| total.as_str().to_owned())
}

/// The operator key as the wallet's [`Operator`]: each statement signed with it, in the bounds' deployment and
/// protocol version, made by `maker` at `made_at`. Where the key can't sign, the statement carries no signature
/// that verifies, which the log doesn't attribute, so the question is never recorded as the operator's and the
/// mover hands nothing over.
pub struct OperatorSigns<'a> {
    key: RefCell<&'a mut dyn OperatorKey>,
    bounds: &'a PolicyBounds,
    maker: Id,
    made_at: Timestamp,
}

impl<'a> OperatorSigns<'a> {
    /// The operator key `key`, signing as `maker` at `made_at`.
    pub fn new(
        key: &'a mut dyn OperatorKey,
        bounds: &'a PolicyBounds,
        maker: Id,
        made_at: Timestamp,
    ) -> Self {
        Self {
            key: RefCell::new(key),
            bounds,
            maker,
            made_at,
        }
    }
}

impl Operator for OperatorSigns<'_> {
    fn statement(&self, operation: ProviderOperation) -> Statement<ProviderOperation> {
        let signed = sign_operation(
            &mut **self.key.borrow_mut(),
            self.bounds,
            &self.maker,
            &self.made_at,
            operation.clone(),
        )
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        signed.unwrap_or_else(|| Statement {
            deployment: self.bounds.deployment.clone(),
            protocol_version: self.bounds.protocol_version.as_str().to_owned(),
            shape: OPERATION_SHAPE.to_owned(),
            maker: self.maker.as_str().to_owned(),
            made_at: self.made_at.as_str().to_owned(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: self.key.borrow().key(),
                value: "A".repeat(86),
            },
            statement: operation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::restored_whole;
    use rutent_contracts::router_api::Amount;
    use std::collections::BTreeMap;

    /// Two values of one order rest at one node, the part handed on is one of them: the restore before its withdrawal
    /// is of both, the order's whole as the log holds it there, so the other part still finds its value (`T-020-46`).
    // verifies: INV-1
    #[test]
    fn a_hand_ons_restore_is_the_orders_whole_at_its_node() {
        let mut resting = BTreeMap::new();
        resting.insert(
            ("ledger".to_owned(), "ord-1".to_owned()),
            Amount::new("500").expect("an amount"),
        );
        assert_eq!(restored_whole(&resting, "ledger", "ord-1", "200"), "500");
        assert_eq!(
            restored_whole(&resting, "ledger", "ord-2", "200"),
            "200",
            "nothing resting for the order: the part handed on"
        );
    }
}
