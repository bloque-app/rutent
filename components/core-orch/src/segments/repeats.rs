//! Carrying a segment's own operation out again (`REQ-ORD-1`, `REQ-CAT-10`, `REQ-AUZ-1`, `AC-5`, `T-020-63`).
//!
//! A segment left with its outcome unknown at a provider that declares a repeat safe and no status query is asked by its
//! operation again, on its provider's cadence ([`Schedule`](crate::schedule::Schedule)). A first run the platform ended
//! before it took effect makes the repeat that move's first carrying-out, so it is read like any action policy decides
//! ([`Segments::carry_again`]): policy decides `repeat_operation` as Core, which carries the operator's standing
//! permission onward, the effect is recorded before anything is handed over, acted on once, the platform carries the
//! operation out again as that effect lets it ([`Modules::repeat_operation`]) and how it came out is recorded. A refusal
//! by policy hands nothing over. The provider's answer, where it gave one, stays with the platform as a late answer until
//! Core has kept it ([`Platform::late_answers`](rutent_prov::module::platform::Platform::late_answers)).

use rutent_contracts::core_internal::{Action, Actor, DecisionRequest, Effect, Outcome, What};
use rutent_contracts::router_api::Id;

use super::ports::Answered;
use super::{Parts, Segments, journal, now};

/// One repeat: the segment whose own operation is carried out again, its provider and its own transaction, and Core, who
/// asks policy and records the effect and its result.
#[derive(Debug, Clone, Copy)]
pub struct RepeatOf<'a> {
    /// The order.
    pub order: &'a Id,
    /// The segment.
    pub segment: &'a Id,
    /// The segment's provider.
    pub provider: &'a Id,
    /// The segment's own transaction.
    pub transaction: &'a Id,
    /// Core, who asks policy and records the effect and its result.
    pub core: &'a Actor,
}

/// How a repeat came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeated {
    /// Policy refused it: nothing was handed over and nothing recorded but policy's own refusal.
    Denied,
    /// It couldn't be done now: an entry the log couldn't take, no time to stamp it with. Nothing was handed over, or
    /// what came of it isn't recorded.
    Failed,
    /// The operation was carried out again and its result recorded: whether the provider answered.
    Carried(bool),
}

impl Segments {
    /// One repeat: decided, recorded as an effect, handed over, and its result recorded. Nothing is handed over before its
    /// effect is in the log (`INV-7`), and a result is recorded for each effect, `failed` where the clock can't be read
    /// after it (`REQ-AUD-1`).
    pub fn carry_again(&mut self, parts: &mut Parts<'_>, repeat: &RepeatOf<'_>) -> Repeated {
        let action = Action::RepeatOperation {
            order: repeat.order.clone(),
            segment: repeat.segment.clone(),
            provider: repeat.provider.clone(),
            transaction: repeat.transaction.clone(),
        };
        let Ok(at) = now(parts.clock) else {
            return Repeated::Failed;
        };
        let request = DecisionRequest {
            request: parts.fresh.id(),
            caller: repeat.core.clone(),
            action: action.clone(),
            at,
        };
        let Ok(decision) = parts.decisions.decide(&request) else {
            return Repeated::Failed;
        };
        let Some(permit) = decision.permit() else {
            return Repeated::Denied;
        };
        let Ok(effect_at) = now(parts.clock) else {
            return Repeated::Failed;
        };
        let Ok(receipt) = journal::append(
            parts.log,
            journal::verified(
                repeat.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &effect_at,
                Outcome::ToTakeEffect {},
            ),
        ) else {
            return Repeated::Failed;
        };
        let effect = Effect {
            action,
            decision: permit,
            receipt: receipt.clone(),
        };
        let answered = if self.act_on(&receipt) {
            hand_over_again(parts, &effect, repeat)
        } else {
            Answered::Refused(rutent_contracts::module_interface::PlatformRefusal::Unavailable)
        };
        let outcome = match &answered {
            Answered::Bytes(_) => Outcome::Done {},
            Answered::Refused(_) => Outcome::Unanswered {},
        };
        // The result is recorded whatever the clock does: a time it can't be read at, the log's own, is a `failed` result
        // stamped at the effect's (`T-020-63`).
        let (at, outcome) = match now(parts.clock) {
            Ok(at) => (at, outcome),
            Err(_) => (effect_at, Outcome::Failed {}),
        };
        let wrote = journal::append(
            parts.log,
            journal::verified(repeat.core, What::Result { effect: receipt }, &at, outcome),
        );
        match (answered, wrote) {
            (Answered::Bytes(_), Ok(_)) => Repeated::Carried(true),
            (Answered::Refused(_), Ok(_)) => Repeated::Carried(false),
            _ => Repeated::Failed,
        }
    }
}

/// Hands the operation over again, as `effect` lets: the one call that carries it out.
fn hand_over_again(parts: &mut Parts<'_>, effect: &Effect, repeat: &RepeatOf<'_>) -> Answered {
    parts
        .modules
        .repeat_operation(effect, repeat.provider, repeat.transaction)
}
