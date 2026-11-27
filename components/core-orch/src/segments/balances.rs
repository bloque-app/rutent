//! Reading a holding account's balance (`REQ-AUD-15`, `REQ-AUZ-1`, `T-020-63`).
//!
//! Core's schedule reads each holding account's balance from the module that holds it: a public node's wallet, and a
//! private node's publisher's module. Until `T-020-63` that question went to the platform as the operator's own, with no
//! decided action and no effect. Now it is read like any action policy decides ([`Segments::read_holding_balance`]): a
//! fresh transaction is drawn, policy decides `read_holding_balance` as Core, which carries the operator's standing
//! permission onward (`core-authz`'s roles, as `ask_status` and `read_node_record` are decided), the effect is recorded
//! before anything is sent, acted on once, the question is put to the platform as that effect lets
//! ([`Modules::ask_for_balance`]) and how it came out is recorded. A refusal by policy asks nothing: the account is raised
//! as not read (it fails closed). A read is always recorded to its result, and the answer is handed back only once the
//! result is in the log.

use rutent_contracts::core_internal::{Action, Actor, DecisionRequest, Effect, Outcome, What};
use rutent_contracts::provider_role::{HoldingBalance, Operation, Statement};
use rutent_contracts::router_api::{Id, PolicyBounds};

use super::ports::Answered;
use super::{Parts, Segments, journal, now, sign};

/// One read of a holding account's balance: the node, the module asked and its asset, the bounds the question is made
/// in, the operator in whose name it is signed and Core, who asks policy and records it.
#[derive(Debug, Clone, Copy)]
pub struct BalanceRead<'a> {
    /// The inner node whose holding account is read.
    pub node: &'a str,
    /// The provider whose module holds the account, asked.
    pub provider: &'a str,
    /// The asset whose balance is read.
    pub asset: &'a str,
    /// The bounds the question is made in: the deployment's and protocol version.
    pub bounds: &'a PolicyBounds,
    /// The operator, in whose name the question is signed.
    pub operator: &'a Actor,
    /// Core, who asks policy and records the effect and its result.
    pub core: &'a Actor,
}

/// How a read came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Balance {
    /// Policy refused it: nothing was asked and nothing recorded but policy's own refusal.
    Denied,
    /// It couldn't be done now (an entry the log couldn't take, no time to stamp it with, no answer from the platform):
    /// worth trying again.
    Failed,
    /// The transaction it was asked under, and the answer as the platform passed it on.
    Answered(Id, Vec<u8>),
}

impl Segments {
    /// One read: decided, recorded as an effect, asked, and its result recorded. Nothing is asked before its effect is in
    /// the log (`INV-7`), and an answer is handed back only once its result is.
    pub fn read_holding_balance(
        &mut self,
        parts: &mut Parts<'_>,
        read: &BalanceRead<'_>,
    ) -> Balance {
        let (Some(node), Some(provider), Some(asset)) = (
            Id::new(read.node),
            Id::new(read.provider),
            Id::new(read.asset),
        ) else {
            return Balance::Failed;
        };
        let Ok(transaction) = self.claim_transaction(parts.fresh, parts.issued) else {
            return Balance::Failed;
        };
        let action = Action::ReadHoldingBalance {
            node,
            provider: provider.clone(),
            asset,
            transaction: transaction.clone(),
        };
        let Ok(at) = now(parts.clock) else {
            return Balance::Failed;
        };
        let request = DecisionRequest {
            request: parts.fresh.id(),
            caller: read.core.clone(),
            action: action.clone(),
            at,
        };
        let Ok(decision) = parts.decisions.decide(&request) else {
            return Balance::Failed;
        };
        let Some(permit) = decision.permit() else {
            return Balance::Denied;
        };
        let Ok(effect_at) = now(parts.clock) else {
            return Balance::Failed;
        };
        let Ok(receipt) = journal::append(
            parts.log,
            journal::verified(
                read.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &effect_at,
                Outcome::ToTakeEffect {},
            ),
        ) else {
            return Balance::Failed;
        };
        let effect = Effect {
            action,
            decision: permit,
            receipt: receipt.clone(),
        };
        // The result's time is read now, before anything is asked, so no early return can follow the question.
        let result_at = now(parts.clock);
        let question = self
            .act_on(&receipt)
            .then(|| {
                let signed = sign::sign(
                    parts.operator_key,
                    read.bounds,
                    &read.operator.acts,
                    result_at.as_ref().ok()?,
                    Operation::HoldingBalance(HoldingBalance {
                        transaction: transaction.as_str().to_owned(),
                        provider: read.provider.to_owned(),
                        node: read.node.to_owned(),
                        asset: read.asset.to_owned(),
                    }),
                )
                .ok()?;
                serde_json::from_slice::<Statement<Operation>>(&signed).ok()
            })
            .flatten();
        let answered = match &question {
            Some(question) => parts.modules.ask_for_balance(&effect, &provider, question),
            None => {
                Answered::Refused(rutent_contracts::module_interface::PlatformRefusal::Unavailable)
            }
        };
        let outcome = match (&question, &answered) {
            (_, Answered::Bytes(_)) => Outcome::Done {},
            (Some(_), Answered::Refused(_)) => Outcome::Unanswered {},
            (None, _) => Outcome::Failed {},
        };
        let wrote = result_at.ok().and_then(|at| {
            journal::append(
                parts.log,
                journal::verified(read.core, What::Result { effect: receipt }, &at, outcome),
            )
            .ok()
        });
        // An answer is used only once the entry that records how the read came out is in the log.
        match (answered, wrote) {
            (Answered::Bytes(bytes), Some(_)) => Balance::Answered(transaction, bytes),
            _ => Balance::Failed,
        }
    }
}
