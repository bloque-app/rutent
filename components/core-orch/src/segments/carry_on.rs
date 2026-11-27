//! A segment begun before Core stopped, carried on by the Core that reopened (`T-020-61`, item 8; `NFR-6`, `INV-3`,
//! `INV-8`).
//!
//! A reopened Core rebuilds each segment it handed over from the log ([`Segments::reopened`]). One whose effect the log
//! holds with no result and no answer is in motion, its value held, and is never handed over as new: its first carrying
//! out may have moved value (`Segments::AlreadyActed`). Before this row it stayed so until the stuck check timed it out.
//! Now Core's run carries it on ([`Segments::carry_on`]), each step a decided action recorded as an effect before
//! anything is asked, as every action policy decides is:
//!
//! 1. **Its status asked**, where its provider declares a status query (`ask_status`, `REQ-ORD-1`): under a fresh
//!    transaction, about the segment's own signed operation. A provider that answers with its own result of the
//!    segment's transaction gives the segment its answer, which Core then takes as it takes any answer: judged, graded
//!    and settled, each step once (`REQ-ORD-1`'s "a result counts only if attributable to that segment's provider and
//!    matching that transaction").
//! 2. **Its own operation handed again, under its own transaction** (`repeat_operation`, `T-020-63`), where the
//!    provider declares no status query or knows no such transaction: the platform carries an operation out only under a
//!    transaction it never carried ([`Modules::carry_out`](super::Modules::carry_out)), and refuses one it did (`DuplicateTransaction`, `T-013-09`,
//!    held in its record across its own restart), so a move whose first carrying out never reached the platform is
//!    carried out now, and one that did is never carried out twice, whatever its provider declares (`INV-8`). Where the
//!    platform carried it and got no answer, a provider that declares a safe repeat has the platform run it again
//!    ([`Modules::repeat_operation`](super::Modules::repeat_operation), as `T-020-63`'s repeat does).
//! 3. **Otherwise it waits**: nothing more is asked, and the stuck check times it on its provider's figures
//!    (`REQ-ORD-29`). Core never writes a result for it it has no answer for: it is never timed out.

use rutent_contracts::core_internal::{Action, Actor, DecisionRequest, Effect, Outcome, What};
use rutent_contracts::module_interface::PlatformRefusal;
use rutent_contracts::provider_role::Tracking;
use rutent_contracts::router_api::Id;

use super::ports::Answered;
use super::{Parts, Segments, journal, now};

/// One segment to carry on: its order and itself, and Core, who asks policy and records each effect and its result.
#[derive(Debug, Clone, Copy)]
pub struct CarryOn<'a> {
    /// The order.
    pub order: &'a Id,
    /// The segment.
    pub segment: &'a Id,
    /// Core, who asks policy and records.
    pub core: &'a Actor,
}

/// How carrying a segment on came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarriedOn {
    /// The segment awaits no answer: it isn't one a reopened Core rebuilt with its effect and no answer.
    NotAwaiting,
    /// Its provider gave its answer, by its status or by the operation carried out: the segment takes it on its next
    /// run.
    Answered,
    /// Nothing gave an answer now; the segment waits for the stuck check, with why.
    Waiting(&'static str),
    /// A step couldn't be decided or recorded now: it is tried again on the next tick.
    Failed,
}

impl Segments {
    /// Whether `segment` of `order` is one a Core that reopened rebuilt from the log with its effect and no answer: in
    /// motion, never handed over as new, and what [`Segments::carry_on`] carries on.
    pub fn awaits_answer(&self, order: &Id, segment: &Id) -> bool {
        self.flights
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))
            .is_some_and(|f| {
                f.reopened
                    && !f.answer_logged
                    && f.carried
                    && f.effect.is_some()
                    && f.answered.is_none()
                    && f.outcome.is_none()
            })
    }

    /// Carries `carry.segment` on (see the module's docs): its status asked where its provider declares a status query,
    /// else its operation handed again under its own transaction. An answer taken is the segment's own, which its next
    /// run judges as any answer is.
    pub fn carry_on(&mut self, parts: &mut Parts<'_>, carry: &CarryOn<'_>) -> CarriedOn {
        let at = (
            carry.order.as_str().to_owned(),
            carry.segment.as_str().to_owned(),
        );
        if !self.awaits_answer(carry.order, carry.segment) {
            return CarriedOn::NotAwaiting;
        }
        let Some(flight) = self.flights.get(&at) else {
            return CarriedOn::NotAwaiting;
        };
        let (provider, transaction, statement) = (
            flight.provider.clone(),
            flight.transaction.clone(),
            flight.statement.clone(),
        );
        let declared = parts.catalogue.declared_by(provider.as_str());
        let asks = declared.is_some_and(|d| d.tracking.contains(&Tracking::StatusQuery));
        let repeats = declared.is_some_and(|d| d.safe_retry == Some(true));
        if asks {
            let Ok(asking) = self.claim_transaction(parts.fresh, parts.issued) else {
                return CarriedOn::Failed;
            };
            let action = Action::AskStatus {
                order: carry.order.clone(),
                segment: carry.segment.clone(),
                provider: provider.clone(),
                transaction: asking.clone(),
            };
            let answered = match self.decided(parts, carry.core, action, |modules, effect| {
                modules.status(effect, &provider, &asking, &statement)
            }) {
                Ok(answered) => answered,
                Err(stopped) => return stopped,
            };
            if let Answered::Bytes(bytes) = &answered {
                // Only the provider's own result of the segment's transaction is its answer: a status that knows
                // none, or a refusal, leaves it for the operation handed again.
                if let Some(result) =
                    crate::reversal::status_result(bytes, &statement.statement, &asking, &provider)
                        .filter(|result| names_transaction(result, transaction.as_str()))
                {
                    return self.take_answer(&at, result);
                }
            }
        }
        // The operation handed again, under its own transaction: carried out only where the platform never carried it.
        let action = Action::RepeatOperation {
            order: carry.order.clone(),
            segment: carry.segment.clone(),
            provider: provider.clone(),
            transaction: transaction.clone(),
        };
        let answered =
            match self.decided(parts, carry.core, action, |modules, effect| {
                match modules.carry_out(effect, &provider, &statement) {
                    Answered::Refused(PlatformRefusal::DuplicateTransaction) if repeats => {
                        modules.repeat_operation(effect, &provider, &transaction)
                    }
                    answered => answered,
                }
            }) {
                Ok(answered) => answered,
                Err(stopped) => return stopped,
            };
        match answered {
            Answered::Bytes(bytes) => self.take_answer(&at, bytes),
            Answered::Refused(PlatformRefusal::DuplicateTransaction) => {
                CarriedOn::Waiting("the platform carried the operation out already")
            }
            Answered::Refused(_) => CarriedOn::Waiting("the platform gave no answer"),
        }
    }

    /// The segment at `at` takes `bytes` as its provider's answer: its next run judges it, as any answer.
    fn take_answer(&mut self, at: &(String, String), bytes: Vec<u8>) -> CarriedOn {
        match self.flights.get_mut(at) {
            Some(flight) if flight.answered.is_none() => {
                flight.answered = Some(Answered::Bytes(bytes));
                CarriedOn::Answered
            }
            _ => CarriedOn::NotAwaiting,
        }
    }

    /// One decided action of Core's: policy asked, the effect recorded before anything is asked, acted on once, `ask`
    /// put to the platform as the effect lets it, and its result recorded whatever came back (`INV-7`, `REQ-AUD-1`).
    fn decided(
        &mut self,
        parts: &mut Parts<'_>,
        core: &Actor,
        action: Action,
        ask: impl FnOnce(&mut dyn super::Modules, &Effect) -> Answered,
    ) -> Result<Answered, CarriedOn> {
        let at = now(parts.clock).map_err(|_| CarriedOn::Failed)?;
        let request = DecisionRequest {
            request: parts.fresh.id(),
            caller: core.clone(),
            action: action.clone(),
            at,
        };
        let decision = parts
            .decisions
            .decide(&request)
            .map_err(|_| CarriedOn::Failed)?;
        let permit = decision
            .permit()
            .ok_or(CarriedOn::Waiting("policy didn't permit carrying it on"))?;
        let effect_at = now(parts.clock).map_err(|_| CarriedOn::Failed)?;
        let receipt = journal::append(
            parts.log,
            journal::verified(
                core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &effect_at,
                Outcome::ToTakeEffect {},
            ),
        )
        .map_err(|_| CarriedOn::Failed)?;
        let effect = Effect {
            action,
            decision: permit,
            receipt: receipt.clone(),
        };
        let answered = if self.act_on(&receipt) {
            ask(&mut *parts.modules, &effect)
        } else {
            Answered::Refused(PlatformRefusal::Unavailable)
        };
        let outcome = match &answered {
            Answered::Bytes(_) => Outcome::Done {},
            Answered::Refused(PlatformRefusal::OutcomeUnknown) => Outcome::Unanswered {},
            Answered::Refused(_) => Outcome::Failed {},
        };
        let (at, outcome) = match now(parts.clock) {
            Ok(at) => (at, outcome),
            Err(_) => (effect_at, Outcome::Failed {}),
        };
        journal::append(
            parts.log,
            journal::verified(core, What::Result { effect: receipt }, &at, outcome),
        )
        .map_err(|_| CarriedOn::Failed)?;
        Ok(answered)
    }
}

impl Segments {
    /// Whether `part` of `order`, on the route of `segments`, has a segment Core's run hands over or takes on now
    /// (`T-020-61`): the part isn't stuck, ended, waiting for the customer, nor refused by its guard, and the first of its
    /// segments without a final outcome is one not handed over yet, one that stopped before its outcome (the log down,
    /// say), or one whose grading waits for evidence. A segment whose outcome is unknown, or that awaits its answer after
    /// a restart, is the schedule's to ask and Core's run's to carry on ([`Segments::carry_on`]), never handed over again;
    /// one policy refused, one waiting on the person, and one whose answer didn't count stay as they are: running them
    /// again would ask again on every tick for nothing new. A route all of whose segments settled has nothing left.
    pub fn work_left(&self, order: &Id, part: &Id, segments: &[Id]) -> bool {
        use super::SegmentOutcome;
        if self.stuck(order, part)
            || self.ended_by_resolution(order, part)
            || self.ended(order, part)
            || self.waiting(order, part).is_some()
            || self
                .refused
                .contains_key(&(order.as_str().to_owned(), part.as_str().to_owned()))
        {
            return false;
        }
        for segment in segments {
            let at = (order.as_str().to_owned(), segment.as_str().to_owned());
            let Some(flight) = self.flights.get(&at) else {
                // Next to hand over, unless a reopened Core holds it as acted on, its operation not read back.
                return self.unread_effect(&at).is_none() && !self.retired(order, segment);
            };
            match &flight.outcome {
                Some(SegmentOutcome::Settled { amount_out, .. }) if !amount_out.is_zero() => {}
                Some(SegmentOutcome::AwaitsEvidence(_)) => return true,
                Some(_) => return false,
                None => return flight.answered.is_some() || !flight.carried,
            }
        }
        false
    }

    /// Whether `segment` of `order` was handed over, or refused before it was, and ended without settling: not handed
    /// over by the platform, its move not permitted by policy, refused, failed, or its withdrawal cancelled (`T-020-61`): what a recalculation may start
    /// from.
    pub fn ended_unsettled(&self, order: &Id, segment: &Id) -> bool {
        use super::SegmentOutcome;
        self.flights
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))
            .and_then(|f| f.outcome.as_ref())
            .is_some_and(|outcome| {
                matches!(
                    outcome,
                    SegmentOutcome::NotHandedOver(_)
                        | SegmentOutcome::NotPermitted(_)
                        | SegmentOutcome::Failed
                        | SegmentOutcome::Refused(_)
                        | SegmentOutcome::WithdrawalCancelled
                )
            })
    }

    /// Whether `part` of `order` was refused by its guard before a segment's first hand-over, the refusal recorded.
    pub fn guard_refused(&self, order: &Id, part: &Id) -> bool {
        self.refused
            .contains_key(&(order.as_str().to_owned(), part.as_str().to_owned()))
    }
}

/// Whether `result`, a provider's signed result, names `transaction` as the one it answers.
fn names_transaction(result: &[u8], transaction: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(result)
        .ok()
        .is_some_and(|value| value["statement"]["transaction"] == transaction)
}
