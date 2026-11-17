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

/// The linked order's accepted route, from where the assembly keeps what the order was accepted on.
pub trait LinkedRoutes {
    /// The route `linked` was accepted on, or `None` where Core holds none.
    fn accepted(&self, linked: &Id) -> Option<AcceptedRoute>;
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
        let operator = Asking {
            inner: self.operator,
            asked: RefCell::new(asked),
        };
        self.holdings
            .start_handed_on(self.platform, &operator, route, order, amount)
    }
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
