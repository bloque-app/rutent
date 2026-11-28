//! A failed segment's reversal, asked of its provider once and carried on where its outcome is unknown (`REQ-ORD-3`,
//! `INV-8`, `NFR-6`; `T-020-61`, the review of #355's round 2, H1).
//!
//! Each ask of a reversal goes under a transaction of its own, so a provider can't tell a second ask from a new reversal
//! of the same move: one that keys reversals by its own transaction would pay the value back twice. So:
//!
//! - **A reversal is sent once.** Core asks the failed provider again only where every earlier ask failed, which moved
//!   nothing (the provider refused it, or the platform refused it before the provider's module passed anything out), and
//!   then only on the retry cadence ([`Reversals::retry_seconds`]) and up to [`MAX_REVERSAL_TRIES`] asks. The log holds
//!   Core to it (`reversal-is-asked-again-only-after-a-refusal`).
//! - **An ask whose outcome is unknown** (its result `unanswered`, or no result, as a Core that stopped after its effect
//!   left it) is carried on as item (8) carries a segment on, once, under its own transaction, each step a decided
//!   effect: its status asked where the provider answers status queries; its own operation repeated only at a provider
//!   that declares a safe repeat and no status query; otherwise the answer the platform recorded read back. Never a
//!   fresh transaction. An answer of the reversal's own transaction is judged as the ask's would have been.
//! - **An ask the provider carried out** whose word doesn't count, and an unknown one carried on, wait for the origin
//!   reserve's (or the node's holder's) confirmation, asked on the cadence and up to [`MAX_REVERSAL_TRIES`] times; past
//!   that, Core asks no more and the stuck check times the part.
//!
//! Everything here is the log's: a Core that reopens rebuilds the asks, their outcomes and answers, whether the last was
//! carried on, the confirmations asked and who carries the reversal from its entries ([`Reversals::rebuild_asks`]).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::{
    About, Action, DecisionRequest, Effect, Entry, Outcome, Permit, Receipt, What,
};
use rutent_contracts::module_interface::{Digest, PlatformRefusal};
use rutent_contracts::provider_role::{
    OPERATION_SHAPE, Operation as ProviderOperation, RESULT_SHAPE, Statement, Tracking,
};
use rutent_contracts::router_api::{Id, Timestamp};

use super::journal::{self, Step};
use super::judge::{self, NotCounted};
use super::{At, Carrier, Completed, Moving, Parts, ReversalError, Reversals, key};

/// The most times Core asks one reversal's confirmation, and the most asks of one reversal its provider refused, before
/// it asks no more and leaves the part to the stuck check: at the default cadence, twelve minutes' worth.
pub const MAX_REVERSAL_TRIES: u32 = 24;

/// How often a reversal's confirmation, or an ask its provider refused, is tried again, in seconds, until the deployment
/// sets another ([`Reversals::set_retry_seconds`]): half a minute, so the stuck check's window of a provider that
/// advertises settling in a minute holds a few asks, and the origin reserve is asked again before the operator is.
pub const DEFAULT_REVERSAL_RETRY_SECONDS: u64 = 30;

/// Whole seconds of `at`.
pub(super) fn seconds(at: &Timestamp) -> u64 {
    u64::try_from(at.unix_nanos() / 1_000_000_000).unwrap_or(0)
}

/// Whether `retry` seconds have passed since `last` at `now`, a clock that went back counting as passed.
fn passed(now: u64, last: u64, retry: u64) -> bool {
    now < last || now - last >= retry
}

/// How many times something was asked, and when the last was, in whole seconds of Core's clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Tries {
    pub(super) count: u32,
    pub(super) last: u64,
}

impl Tries {
    pub(super) fn note(&mut self, at: u64) {
        self.count = self.count.saturating_add(1);
        self.last = self.last.max(at);
    }
}

/// One ask Core sent the failed provider: its transaction and signed operation, its effect's receipt and time, the
/// outcome its result recorded, and the provider's answer as it was stated, where it gave one.
#[derive(Debug, Clone)]
pub(super) struct Sent {
    pub(super) transaction: Id,
    pub(super) statement: Statement<ProviderOperation>,
    pub(super) effect: Receipt,
    pub(super) at: u64,
    pub(super) outcome: Option<Outcome>,
    pub(super) answer: Option<(Vec<u8>, Receipt)>,
}

impl Sent {
    /// Whether this ask moved nothing, by its result: it failed.
    fn failed(&self) -> bool {
        self.outcome == Some(Outcome::Failed {})
    }

    /// Whether its outcome is unknown: no result, or an `unanswered` one.
    fn unknown(&self) -> bool {
        matches!(self.outcome, None | Some(Outcome::Unanswered {}))
    }
}

/// How an unknown ask is carried on.
#[derive(Debug, Clone)]
pub(super) enum Way {
    /// Its status asked, under this fresh transaction of Core's.
    Status(Id),
    /// Its own operation carried out again, under its own transaction.
    Repeat,
    /// The answer the platform recorded for it, read back.
    Recorded,
}

/// The carrying-on of an unknown ask, from its decision on.
#[derive(Debug, Clone)]
pub(super) struct Resolving {
    permit: Permit,
    action: Action,
    way: Way,
    effect: Step,
    carried: bool,
    answer: Option<Vec<u8>>,
    refused: bool,
    result: Option<Step>,
    answer_step: Option<Step>,
}

/// The next step Core's run takes on a reversal its failed provider carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReversalStep {
    /// Ask the failed provider: it was never asked, every ask failed and the cadence has passed, or an ask stopped
    /// before its answer was judged.
    Ask,
    /// Carry the last ask's unknown outcome on, once.
    CarryOn,
    /// Ask the origin reserve, or the node's holder, to confirm.
    Confirm,
    /// Nothing now: not due, or past the cap, which leaves the part to the stuck check.
    Wait,
}

/// An unknown ask carried on: the decided effect, the answer of the reversal's own transaction it gave and its stated
/// entry, and the reversal where that answer completed it, or why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarriedReversal {
    /// The receipt of the `ask_status`, `repeat_operation` or `read_recorded_answer` effect.
    pub effect: Receipt,
    /// The receipt of the answer's stated entry, where an answer of the reversal's transaction came back.
    pub answer: Option<Receipt>,
    /// The reversal, where the answer completed it.
    pub completed: Option<Completed>,
    /// Why nothing completed, where nothing did.
    pub why: Option<NotCounted>,
}

impl Reversals {
    /// How often a reversal's confirmation, or an ask its provider refused, is tried again, in seconds.
    pub fn retry_seconds(&self) -> u64 {
        self.retry.unwrap_or(DEFAULT_REVERSAL_RETRY_SECONDS)
    }

    /// Sets how often a reversal's confirmation, or an ask its provider refused, is tried again.
    pub fn set_retry_seconds(&mut self, seconds: u64) {
        self.retry = Some(seconds);
    }

    /// How many asks of the reversal of `segment` of `order` Core sent, as the log holds them.
    pub fn asks_sent(&self, order: &Id, segment: &Id) -> usize {
        self.reversals
            .get(&key(order, segment))
            .map_or(0, |r| r.sent.len())
    }

    /// How many confirmations of the reversal of `segment` of `order` Core asked, as the log holds them.
    pub fn confirmations_asked(&self, order: &Id, segment: &Id) -> u32 {
        self.reversals
            .get(&key(order, segment))
            .map_or(0, |r| r.confirms.count)
    }

    /// Whether a fresh ask of the reversal of `segment` of `order` may go out at `now`: none went yet, or every earlier
    /// one failed, the cadence has passed since the last, and fewer than [`MAX_REVERSAL_TRIES`] went.
    pub(super) fn ask_allowed(
        &self,
        order: &Id,
        segment: &Id,
        now: u64,
    ) -> Result<(), ReversalError> {
        let Some(reversal) = self.reversals.get(&key(order, segment)) else {
            return Ok(());
        };
        if let Some(sent) = reversal.sent.iter().find(|s| !s.failed()) {
            return Err(ReversalError::AskedAlready(sent.transaction.clone()));
        }
        match reversal.sent.last() {
            None => Ok(()),
            Some(_) if reversal.sent.len() >= MAX_REVERSAL_TRIES as usize => {
                Err(ReversalError::TriedEnough)
            }
            Some(last) if !passed(now, last.at, self.retry_seconds()) => Err(ReversalError::NotDue),
            Some(_) => Ok(()),
        }
    }

    /// The next step Core's run takes at `now` on the reversal of `segment` of `order`, which its failed provider
    /// carries (see the module's docs).
    pub fn next_step(&self, order: &Id, segment: &Id, now: u64) -> ReversalStep {
        let Some(reversal) = self.reversals.get(&key(order, segment)) else {
            return ReversalStep::Ask;
        };
        if reversal.asking.is_some() || self.ask_allowed(order, segment, now).is_ok() {
            return ReversalStep::Ask;
        }
        if reversal.resolving.is_some()
            || (!reversal.carried_on && reversal.sent.last().is_some_and(Sent::unknown))
        {
            return ReversalStep::CarryOn;
        }
        if self.confirm_due(order, segment, now) {
            return ReversalStep::Confirm;
        }
        ReversalStep::Wait
    }

    /// Whether the confirmation of the reversal of `segment` of `order` may be asked at `now`: one is under way, or
    /// fewer than [`MAX_REVERSAL_TRIES`] were asked and the cadence has passed since the last.
    pub fn confirm_due(&self, order: &Id, segment: &Id, now: u64) -> bool {
        self.reversals.get(&key(order, segment)).is_none_or(|r| {
            r.question.is_some()
                || r.confirms.count == 0
                || (r.confirms.count < MAX_REVERSAL_TRIES
                    && passed(now, r.confirms.last, self.retry_seconds()))
        })
    }

    /// Carries the unknown outcome of the last ask of `at`'s reversal on, once (see the module's docs): its status
    /// asked where its provider answers status queries, its own operation repeated where the provider declares a safe
    /// repeat and no status query, and otherwise the answer the platform recorded read back; each a decided effect,
    /// recorded before the platform is asked, its result after. An answer of the reversal's own transaction is the
    /// provider's word about it, kept and judged as the ask's answer is. Nothing is ever sent under a fresh transaction
    /// for the same reversal. A call that stopped carries on from where it stopped.
    ///
    /// # Errors
    ///
    /// [`ReversalError::StatusNotNeeded`] where no ask's outcome waits to be carried on; policy's refusal; and as
    /// [`Reversals::ask`].
    pub fn carry_on_reversal(
        &mut self,
        parts: &mut Parts<'_>,
        moving: &mut Moving<'_>,
        at: At<'_>,
    ) -> Result<CarriedReversal, ReversalError> {
        let at_key = key(at.order, at.segment);
        let (provider, sent) = {
            let reversal = self.reversals.get(&at_key).ok_or(ReversalError::Unknown)?;
            let sent = reversal
                .sent
                .last()
                .cloned()
                .ok_or(ReversalError::StatusNotNeeded)?;
            if reversal.resolving.is_none() && (reversal.carried_on || !sent.unknown()) {
                return Err(ReversalError::StatusNotNeeded);
            }
            (reversal.provider.clone(), sent)
        };
        let target = self.target(parts, at)?;
        if self
            .reversals
            .get(&at_key)
            .is_some_and(|r| r.resolving.is_none())
        {
            let declared = parts.catalogue.declared_by(provider.as_str());
            let asks = declared.is_some_and(|d| d.tracking.contains(&Tracking::StatusQuery));
            let repeats = !asks && declared.is_some_and(|d| d.safe_retry == Some(true));
            let (action, way) = if asks {
                let asking = parts
                    .segments
                    .claim_transaction(parts.fresh, parts.issued)
                    .map_err(|_| ReversalError::Fresh)?;
                (
                    Action::AskStatus {
                        order: at.order.clone(),
                        segment: at.segment.clone(),
                        provider: provider.clone(),
                        transaction: asking.clone(),
                    },
                    Way::Status(asking),
                )
            } else if repeats {
                (
                    Action::RepeatOperation {
                        order: at.order.clone(),
                        segment: at.segment.clone(),
                        provider: provider.clone(),
                        transaction: sent.transaction.clone(),
                    },
                    Way::Repeat,
                )
            } else {
                (
                    Action::ReadRecordedAnswer {
                        order: at.order.clone(),
                        segment: at.segment.clone(),
                        provider: provider.clone(),
                        transaction: sent.transaction.clone(),
                    },
                    Way::Recorded,
                )
            };
            let request = DecisionRequest {
                request: parts.fresh.id(),
                caller: parts.core.clone(),
                action: action.clone(),
                at: journal::now(parts.clock)?,
            };
            let decision = moving.decisions.decide(&request)?;
            let Some(permit) = decision.permit() else {
                // Refused: nothing is asked, now or again; the reversal waits on its confirmation and the stuck check.
                if let Some(reversal) = self.reversals.get_mut(&at_key) {
                    reversal.carried_on = true;
                }
                return Err(ReversalError::NotPermitted(decision.outcome));
            };
            let effect = Step::new(journal::recorded(
                parts.clock,
                parts.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                Outcome::ToTakeEffect {},
            )?);
            if let Some(reversal) = self.reversals.get_mut(&at_key) {
                reversal.resolving = Some(Resolving {
                    permit,
                    action,
                    way,
                    effect,
                    carried: false,
                    answer: None,
                    refused: false,
                    result: None,
                    answer_step: None,
                });
            }
        }
        let bound = self
            .bound
            .get(at.order.as_str())
            .and_then(|b| target.node.as_ref().and_then(|n| b.get(n)))
            .map(|p| p.as_str().to_owned());
        let reversal = self
            .reversals
            .get_mut(&at_key)
            .ok_or(ReversalError::Unknown)?;
        let state = reversal.resolving.as_mut().ok_or(ReversalError::Unknown)?;
        // The effect before anything is asked (`INV-7`).
        let effect_receipt = state.effect.append(parts.log)?;
        let effect = Effect {
            action: state.action.clone(),
            decision: state.permit.clone(),
            receipt: effect_receipt.clone(),
        };
        if !state.carried {
            if !parts.segments.act_on(&effect_receipt) {
                return Err(ReversalError::AlreadyActed(effect_receipt));
            }
            let answered = put(moving, &effect, &state.way, &provider, &sent);
            match answered {
                crate::segments::Answered::Bytes(bytes) => state.answer = Some(bytes),
                crate::segments::Answered::Refused(refusal) => {
                    state.refused = refusal != PlatformRefusal::OutcomeUnknown;
                }
            }
            state.carried = true;
        }
        // Only the provider's own result of the reversal's transaction is its answer: a status that knows none, or
        // anything of another transaction, gives none.
        let answer = state
            .answer
            .as_ref()
            .and_then(|bytes| match &state.way {
                Way::Status(asking) => {
                    super::status_result(bytes, &sent.statement.statement, asking, &provider)
                }
                Way::Repeat | Way::Recorded => Some(bytes.clone()),
            })
            .filter(|bytes| names_transaction(bytes, sent.transaction.as_str()));
        // Kept as the provider's word about the reversal, once: the same answer the ask kept is that entry.
        let answer_receipt = match &answer {
            None => None,
            Some(bytes) if sent.answer.as_ref().is_some_and(|(kept, _)| kept == bytes) => {
                sent.answer.as_ref().map(|(_, receipt)| receipt.clone())
            }
            Some(bytes) => {
                if state.answer_step.is_none() {
                    let about = About {
                        order: Some(at.order.clone()),
                        segment: Some(at.segment.clone()),
                        transaction: Some(sent.transaction.clone()),
                        node: None,
                    };
                    let presenter =
                        journal::provider_maker(parts.catalogue, parts.operator, &provider);
                    let maker =
                        journal::claimed_by(parts.catalogue, parts.operator, bytes, &presenter);
                    match journal::stated(
                        parts.evidence,
                        parts.clock,
                        bytes,
                        maker,
                        &presenter,
                        about,
                    ) {
                        Ok(entry) => state.answer_step = Some(Step::new(entry)),
                        Err(ReversalError::Unrecordable) => {}
                        Err(other) => return Err(other),
                    }
                }
                match state.answer_step.as_mut() {
                    Some(step) => Some(step.append(parts.log)?),
                    None => None,
                }
            }
        };
        if state.result.is_none() {
            let outcome = if state.answer.is_some() {
                Outcome::Done {}
            } else if state.refused {
                Outcome::Failed {}
            } else {
                Outcome::Unanswered {}
            };
            state.result = Some(Step::new(journal::recorded(
                parts.clock,
                parts.core,
                What::Result {
                    effect: effect_receipt.clone(),
                },
                outcome,
            )?));
        }
        state
            .result
            .as_mut()
            .ok_or(ReversalError::Unknown)?
            .append(parts.log)?;
        reversal.resolving = None;
        reversal.carried_on = true;
        if let (Some(bytes), Some(receipt)) = (&answer, &answer_receipt)
            && let Some(last) = reversal.sent.last_mut()
            && last.effect == sent.effect
        {
            last.answer = Some((bytes.clone(), receipt.clone()));
        }
        let judged = match (&answer, &answer_receipt) {
            (Some(bytes), Some(_)) => judge::judge_reversal(
                parts.catalogue,
                parts.clock.now().floor(),
                parts
                    .segments
                    .began_of(target.reversed().transaction)
                    .as_ref(),
                &target.reversed(),
                &sent.transaction,
                bound.as_deref(),
                bytes,
            ),
            _ => Err(NotCounted::Unanswered),
        };
        let (completed, why) = match judged {
            Ok(whose) => {
                let statement = answer_receipt.clone().ok_or(ReversalError::Unknown)?;
                let completion = Self::completion(parts, at, whose, statement)?;
                self.reversals
                    .get_mut(&at_key)
                    .ok_or(ReversalError::Unknown)?
                    .completion = Some(completion);
                (Some(self.complete(parts, at)?), None)
            }
            Err(why) => (None, Some(why)),
        };
        Ok(CarriedReversal {
            effect: effect_receipt,
            answer: answer_receipt,
            completed,
            why,
        })
    }

    /// Rebuilds, from `history`, what this module keeps of each reversal (see the module's docs): every ask Core sent,
    /// its transaction and operation (read back through `kept`), its effect's time, its result and the answer stated of
    /// its transaction; whether the last ask was carried on (a decided carrying-on of the segment after it); the
    /// confirmations Core asked; and who carries the reversal, from its last `reversal_carrier`.
    pub(super) fn rebuild_asks(
        &mut self,
        history: &[(Receipt, Entry)],
        kept: &mut dyn FnMut(&Digest) -> Option<Vec<u8>>,
    ) {
        let operation_of = |bytes: &[u8]| -> Option<Statement<ProviderOperation>> {
            serde_json::from_slice(bytes).ok()
        };
        for (position, (receipt, entry)) in history.iter().enumerate() {
            match entry {
                Entry::Verified(v) => match &v.what {
                    What::Effect {
                        action:
                            Action::AskReversal {
                                order,
                                segment,
                                provider,
                            },
                        ..
                    } => {
                        // Its operation: the last stated reversal operation about the segment before the effect.
                        let Some((statement, transaction)) =
                            history[..position].iter().rev().find_map(|(_, e)| {
                                let Entry::Stated(s) = e else { return None };
                                let about = s.about.as_ref()?;
                                if s.what.statement.as_str() != OPERATION_SHAPE
                                    || about.order.as_ref() != Some(order)
                                    || about.segment.as_ref() != Some(segment)
                                {
                                    return None;
                                }
                                let statement = operation_of(&kept(&s.commitment)?)?;
                                matches!(statement.statement, ProviderOperation::Reversal(_))
                                    .then(|| (statement, about.transaction.clone()))
                                    .and_then(|(st, tx)| Some((st, tx?)))
                            })
                        else {
                            continue;
                        };
                        let reversal = self.reversal_of(order, segment, provider);
                        if reversal.sent.iter().any(|s| &s.effect == receipt) {
                            continue;
                        }
                        reversal.sent.push(Sent {
                            transaction,
                            statement,
                            effect: receipt.clone(),
                            at: seconds(&v.when.at),
                            outcome: None,
                            answer: None,
                        });
                        reversal.carried_on = false;
                    }
                    What::Effect {
                        action:
                            Action::AskStatus { order, segment, .. }
                            | Action::RepeatOperation { order, segment, .. }
                            | Action::ReadRecordedAnswer { order, segment, .. },
                        ..
                    } => {
                        // A carrying-on after the last ask is that ask's: never carried on twice.
                        if let Some(reversal) = self.reversals.get_mut(&key(order, segment))
                            && !reversal.sent.is_empty()
                        {
                            reversal.carried_on = true;
                        }
                    }
                    What::Result { effect } => {
                        for reversal in self.reversals.values_mut() {
                            if let Some(sent) =
                                reversal.sent.iter_mut().find(|s| &s.effect == effect)
                            {
                                sent.outcome = Some(v.outcome);
                            }
                        }
                    }
                    What::ReversalCarrier {
                        order,
                        segment,
                        carrier,
                        ..
                    } => match self.reversals.get_mut(&key(order, segment)) {
                        Some(reversal) => {
                            reversal.carrier = carried_by(carrier, &reversal.provider);
                        }
                        None => {
                            self.pending_carriers.insert(key(order, segment), *carrier);
                        }
                    },
                    _ => {}
                },
                Entry::Stated(s) => {
                    let Some(about) = &s.about else { continue };
                    let (Some(order), Some(segment), Some(transaction)) =
                        (&about.order, &about.segment, &about.transaction)
                    else {
                        continue;
                    };
                    let at_key = key(order, segment);
                    let shape = s.what.statement.as_str();
                    if shape == RESULT_SHAPE {
                        // The provider's answer to an ask, stated of its transaction.
                        let Some(reversal) = self.reversals.get_mut(&at_key) else {
                            continue;
                        };
                        let Some(sent) = reversal
                            .sent
                            .iter_mut()
                            .find(|sent| &sent.transaction == transaction && sent.answer.is_none())
                        else {
                            continue;
                        };
                        if let Some(bytes) = kept(&s.commitment) {
                            sent.answer = Some((bytes, receipt.clone()));
                        }
                    } else if shape == OPERATION_SHAPE
                        && let Some(bytes) = kept(&s.commitment)
                        && operation_of(&bytes).is_some_and(|st| {
                            matches!(st.statement, ProviderOperation::ReversalConfirmation(_))
                        })
                    {
                        let at = seconds(&s.when.at);
                        // A reversal no ask was sent for yet (one the customer or the operator carries, a confirmation
                        // of an undone start) takes its confirmations as it is first followed.
                        match self.reversals.get_mut(&at_key) {
                            Some(reversal) => reversal.confirms.note(at),
                            None => self.pending_confirms.entry(at_key).or_default().note(at),
                        }
                    }
                }
            }
        }
    }
}

impl Reversals {
    /// Rebuilds, from `history`, each failed segment's showing of fault its provider gave Core before a restart (the
    /// coordinator's ruling on round 3 of #355; `REQ-ORD-11`, `NFR-6`), as item (6) rebuilds the late statements: the
    /// last stated result of the segment's own transaction, by the provider its move went to, that the log took after
    /// the move's first result and whose kept bytes, read back through `kept`, name a fault, with the receipt the log
    /// gave it; disputed where the log holds a `dispute` stuck declaration of the order after it. So the party it names
    /// accepts it after a restart as before one, with no new showing; one accepted already is settled by its carrier.
    pub(super) fn rebuild_showings(
        &mut self,
        history: &[(Receipt, Entry)],
        kept: &mut dyn FnMut(&Digest) -> Option<Vec<u8>>,
    ) {
        use rutent_contracts::state_machine::{Observed, StuckReason};
        // Each move: its provider, and where its first result is.
        let mut moves: BTreeMap<(String, String), (Receipt, Id, Option<usize>)> = BTreeMap::new();
        let mut transactions: BTreeMap<(String, String), Id> = BTreeMap::new();
        for (place, (receipt, entry)) in history.iter().enumerate() {
            match entry {
                Entry::Verified(v) => match &v.what {
                    What::Effect {
                        action:
                            Action::MoveValue {
                                order,
                                segment,
                                provider,
                                ..
                            },
                        ..
                    } => {
                        moves
                            .entry(key(order, segment))
                            .or_insert_with(|| (receipt.clone(), provider.clone(), None));
                    }
                    What::Result { effect } => {
                        for (effect_at, _, first) in moves.values_mut() {
                            if effect_at == effect && first.is_none() {
                                *first = Some(place);
                            }
                        }
                    }
                    _ => {}
                },
                Entry::Stated(s) if s.what.statement.as_str() == OPERATION_SHAPE => {
                    if let Some(about) = &s.about
                        && let (Some(order), Some(segment), Some(transaction)) =
                            (&about.order, &about.segment, &about.transaction)
                    {
                        transactions
                            .entry(key(order, segment))
                            .or_insert_with(|| transaction.clone());
                    }
                }
                Entry::Stated(_) => {}
            }
        }
        for (place, (receipt, entry)) in history.iter().enumerate() {
            let Entry::Stated(stated) = entry else {
                continue;
            };
            if stated.what.statement.as_str() != RESULT_SHAPE {
                continue;
            }
            let Some(about) = &stated.about else { continue };
            let (Some(order), Some(segment), Some(transaction)) =
                (&about.order, &about.segment, &about.transaction)
            else {
                continue;
            };
            let at_key = key(order, segment);
            let Some((_, provider, Some(first))) = moves.get(&at_key) else {
                continue;
            };
            if place < *first
                || transactions.get(&at_key) != Some(transaction)
                || &stated.attribution.maker().acts != provider
            {
                continue;
            }
            let Some(bytes) = kept(&stated.commitment) else {
                continue;
            };
            let Some(fault) = super::fault_shown(&bytes, transaction) else {
                continue;
            };
            let mut appended = stated.clone();
            appended.attribution = rutent_contracts::core_internal::Attribution::Unverified {
                claimed_by: appended.attribution.maker().clone(),
            };
            let Ok(general) = rutent_contracts::core_internal::calls::GeneralEntry::try_from(
                Entry::Stated(appended),
            ) else {
                continue;
            };
            let disputed = history[place..].iter().any(|(_, e)| match e {
                Entry::Verified(v) => matches!(
                    &v.what,
                    What::Input { input } if input.subject.order.as_str() == order.as_str()
                        && matches!(input.observed, Observed::StuckDeclared { reason: StuckReason::Dispute, .. })
                ),
                Entry::Stated(_) => false,
            });
            let provider = provider.clone();
            let reversal = self.reversal_of(order, segment, &provider);
            if !reversal.showings.iter().any(|(b, _)| *b == bytes) {
                reversal.showings.push((
                    bytes,
                    Step {
                        entry: general,
                        receipt: Some(receipt.clone()),
                    },
                ));
            }
            reversal.showing = Some(super::Showing {
                fault,
                receipt: receipt.clone(),
                commitment: stated.commitment.clone(),
                transaction: transaction.clone(),
                disputed,
                pending: false,
            });
        }
    }
}

/// The carrier a `reversal_carrier` entry names, for a reversal `provider` failed.
pub(super) fn carried_by(
    carrier: &rutent_contracts::core_internal::Carrier,
    provider: &Id,
) -> Carrier {
    match carrier {
        rutent_contracts::core_internal::Carrier::Provider => Carrier::Provider(provider.clone()),
        rutent_contracts::core_internal::Carrier::Customer => Carrier::Customer,
        rutent_contracts::core_internal::Carrier::Operator => Carrier::Operator,
    }
}

/// Puts `way` to the platform as the recorded `effect` lets it, about the ask `sent` of `provider`: the reversal's status,
/// its own operation carried out again under its own transaction, or the answer the platform recorded for it read back.
fn put(
    moving: &mut Moving<'_>,
    effect: &Effect,
    way: &Way,
    provider: &Id,
    sent: &Sent,
) -> crate::segments::Answered {
    match way {
        Way::Status(asking) => moving
            .modules
            .status(effect, provider, asking, &sent.statement),
        Way::Repeat => moving
            .modules
            .repeat_operation(effect, provider, &sent.transaction),
        Way::Recorded => moving
            .modules
            .recorded_answer(effect, provider, &sent.transaction),
    }
}

/// Whether `result`, a provider's signed result, names `transaction` as the one it answers.
fn names_transaction(result: &[u8], transaction: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(result)
        .ok()
        .is_some_and(|value| value["statement"]["transaction"] == transaction)
}
