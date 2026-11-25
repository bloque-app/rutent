//! The segments rebuilt from the log when Core reopens (`T-020-46`, finding 3 of `M3`'s milestone review; `NFR-6`,
//! `INV-8`, `REQ-ORD-8`).
//!
//! The log is Core's durable record, so a Core that reopens reads its segments back from it, as `Closing::recorded`,
//! `Holds::recorded` and the proven starts (`HandOn::recover_starts`) already do:
//!
//! - **The orders followed**: every order whose `route_chosen` the log holds, moved by every input recorded about it
//!   since, in order ([`Segments::follow_recorded`]).
//! - **The effects acted on**: every effect the log holds, whatever its action. An effect is acted on only after its
//!   entry is in the log (`INV-7`), so the log names every one a Core acted on, and a reopened Core acts on none of them
//!   again, by a segment or a stuck resolution alike (`INV-8`).
//! - **The transactions**: every transaction the log names ([`Issued::recorded`]), so a fresh one is none of them.
//! - **Each segment handed over**: its `move_value` effect, the transaction and the signed operation of the operator's
//!   question about it (the bytes the evidence store keeps behind the question's commitment), and its outcome as the
//!   inputs recorded about it say. A segment the log settled is settled again on what its provider's result, kept
//!   behind its commitment, gave out, as the judge counted it when the result was received; a failed one is failed.
//!   A segment whose effect the log holds with no final input is one whose operation may have been carried out, its
//!   answer lost with the Core that stopped: it is never carried out again ([`SegmentsError::AlreadyActed`]), it stays
//!   in flight, held, and its value is in motion (`part-waits-for-value-in-motion`) until its outcome is settled the
//!   way an unknown one is. Asking the platform again under its transaction is a later choice (the notes' gaps).
//! - **The value in flight** ([`Exposures`]): every segment in flight held at its provider at the amount its
//!   operation moved, every stuck order marked, so a provider with a segment left unknown still shows its value in
//!   flight and a new order respects the ceilings (`REQ-ORD-8`, `THR-74`).
//! - **The parts resolved**: every part whose `stuck_resolved` the log holds.
//! - **The platform's records of operations that ended with no answer**, by transaction.
//! - **The charges checked** (`Charges::recorded`), each settled segment's charge as its kept result states it, and
//!   the terms the providers stated, so a raise recorded before the reopening is not raised again.
//!
//! What a reopened Core doesn't hold, it fails closed on: a segment whose decision record, question or kept bytes it
//! can't read back is never handed over again, and is held in flight where its operation can be read.

use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{OPERATION_SHAPE, RESULT_SHAPE};
use rutent_contracts::router_api::Operation;
use rutent_contracts::state_machine::SegmentPlan;

use super::*;

/// What a Core that reopens reads beside its log (`T-020-46`): the decision records it signed, and the statements the
/// evidence store keeps.
pub trait Reopening {
    /// The decision record `id`, as Core signed it, held to the digest the log names it by: `None` where Core holds
    /// no such record, or the one it holds isn't the one the log names.
    fn record(&self, id: &Id) -> Option<DecisionRecord>;

    /// The bytes the evidence store keeps behind `commitment`, as the log reads them: `None` where it keeps none.
    fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>>;
}

/// What a segment rebuilt in flight holds: the route's operation at the amount its signed operation moved, or, where
/// its decision record can't be read back, the value its signed operation states (or the order's own amount) at its
/// provider, so the ceilings still bind (`REQ-ORD-8`, `THR-74`): an unread segment is never held at nothing.
enum Hold {
    Route(Box<Operation>),
    Stated {
        provider: Id,
        asset: rutent_contracts::provider_role::Denomination,
        amount: Amount,
    },
}

/// A segment handed over, as the log names it.
struct Handed<'a> {
    order: &'a Id,
    segment: &'a Id,
    provider: &'a Id,
    effect: Effect,
    at: usize,
}

impl Segments {
    /// The segments rebuilt from `history`, the log's own entries, as a Core that reopens reads them (see the module's
    /// docs), with what is in flight held in `exposures`. `catalogue` judges a settled segment's result again, at the
    /// time it was received.
    pub fn reopened(
        clock: &dyn Clock,
        catalogue: &Catalogue,
        history: &[(Receipt, Entry)],
        reading: &mut dyn Reopening,
        exposures: &mut Exposures,
    ) -> Self {
        let mut segments = Segments::new();
        let inputs: Vec<RecordedInput> = history
            .iter()
            .filter_map(|(_, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what: What::Input { input },
                    ..
                }) => Some(input.clone()),
                _ => None,
            })
            .collect();
        for (at, input) in inputs.iter().enumerate() {
            if matches!(input.observed, Observed::RouteChosen { .. }) {
                segments.follow_recorded(clock, &inputs[at..]);
            }
        }
        segments.transactions = Issued::recorded(history)
            .transactions()
            .map(str::to_owned)
            .collect();
        for input in &inputs {
            if let (Observed::StuckResolved { .. }, Some(part)) =
                (&input.observed, &input.subject.part)
                && let (Some(order), Some(part)) = (
                    Id::new(input.subject.order.as_str()),
                    Id::new(part.as_str()),
                )
            {
                segments.mark_resolved(&order, &part);
            }
        }
        // A stuck move done moved its part's whole value (a delivery or a hand-on's withdrawal is of the whole, by
        // construction): its part holds nothing more, as the Core that ran on marked it once the move was done, before
        // its `stuck_resolved` or its `handed_on` (`T-020-46`, `INV-1`).
        for (receipt, entry) in history {
            let Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::ResolveStuck {
                                order, carries_out, ..
                            },
                        ..
                    },
                ..
            }) = entry
            else {
                continue;
            };
            let done = history.iter().any(|(_, e)| {
                matches!(e, Entry::Verified(v)
                    if matches!(&v.what, What::Result { effect } if effect == receipt)
                        && v.outcome == Outcome::Done {})
            });
            let part = history.iter().find_map(|(_, e)| match e {
                Entry::Verified(v) => match &v.what {
                    What::StuckDecide { request, .. } if request.request == carries_out.request => {
                        Some(request.part.clone())
                    }
                    _ => None,
                },
                Entry::Stated(_) => None,
            });
            if let (true, Some(part)) = (done, part) {
                segments.mark_resolved(order, &part);
            }
        }
        let mut holds = BTreeMap::new();
        for (receipt, entry) in history {
            let Entry::Verified(verified) = entry else {
                continue;
            };
            match &verified.what {
                What::Effect { action, decision } => {
                    segments.acted.insert(key(receipt));
                    if let Action::MoveValue {
                        order,
                        segment,
                        provider,
                        ..
                    } = action
                    {
                        let handed = Handed {
                            order,
                            segment,
                            provider,
                            effect: effect_of(action.clone(), decision.clone(), receipt.clone()),
                            at: history
                                .iter()
                                .position(|(r, _)| r == receipt)
                                .unwrap_or_default(),
                        };
                        if let Some(held) = segments
                            .rebuild_flight(clock, catalogue, history, &inputs, reading, &handed)
                        {
                            holds.insert(
                                (order.as_str().to_owned(), segment.as_str().to_owned()),
                                held,
                            );
                        }
                    }
                }
                What::HandOver {
                    item: HandedOver::OperationUnanswered { transaction, .. },
                    ..
                } => {
                    segments
                        .unanswered
                        .insert(transaction.as_str().to_owned(), receipt.clone());
                }
                _ => {}
            }
        }
        segments.recorded_charges(history);
        // The terms the providers stated, read back from the evidence store, so a charge is held to the formula that
        // bound it as a Core that ran on holds it (`REQ-QTE-6`).
        for (receipt, entry) in history {
            if let Entry::Stated(stated) = entry
                && stated.what.statement.as_str() == rutent_contracts::provider_role::TERMS_SHAPE
                && let Some(bytes) = reading.kept(&stated.commitment)
                && let Ok(terms) = serde_json::from_slice::<
                    Statement<rutent_contracts::provider_role::TupleTerms>,
                >(&bytes)
            {
                let _ = segments.terms.state(stated, receipt, &terms);
            }
        }
        // The guard's refusals the log took, by order and the route's decision record, and each part's waits at its
        // floor: answered again as the log took them, never recorded twice (`INV-8`).
        for (receipt, entry) in history {
            let Entry::Verified(v) = entry else {
                continue;
            };
            match &v.what {
                What::OrderRefusal {
                    request,
                    order: Some(order),
                    class: OrderRefusalClass::NoRouteRightNow,
                } if v.who.role == rutent_contracts::core_internal::Role::Core => {
                    segments
                        .refused_in_log
                        .entry((order.as_str().to_owned(), request.as_str().to_owned()))
                        .or_insert_with(|| receipt.clone());
                }
                What::Input { input } if input.observed == (Observed::WaitingAtFloor {}) => {
                    let (Some(part), Ok(general)) =
                        (&input.subject.part, GeneralEntry::try_from(entry.clone()))
                    else {
                        continue;
                    };
                    segments
                        .floor_waits
                        .entry((
                            input.subject.order.as_str().to_owned(),
                            part.as_str().to_owned(),
                        ))
                        .or_default()
                        .push(FloorWait {
                            input: input.clone(),
                            entry: general,
                            receipt: Some(receipt.clone()),
                        });
                }
                _ => {}
            }
        }
        // Which input rests on which judgement, and the reliances written (`REQ-CPL-12`).
        segments.reliances = Reliances::recorded(history);
        segments.measure_what_the_log_lacks(history);
        segments.park_unpaired_on_reopening(history);
        segments.hold_in_flight(exposures, &inputs, &holds);
        segments
    }

    /// Rebuilds the flight of one segment handed over, from what the log and the evidence store hold of it, and
    /// answers the route's operation at the amount it moved, what it holds in flight. Where its operation can't be read
    /// back, the segment is kept as acted on and never handed over again.
    fn rebuild_flight(
        &mut self,
        clock: &dyn Clock,
        catalogue: &Catalogue,
        history: &[(Receipt, Entry)],
        inputs: &[RecordedInput],
        reading: &mut dyn Reopening,
        handed: &Handed<'_>,
    ) -> Option<Hold> {
        let key_of = (
            handed.order.as_str().to_owned(),
            handed.segment.as_str().to_owned(),
        );
        if self.flights.contains_key(&key_of) {
            return None;
        }
        let Some((rebuilt, held)) =
            self.read_flight(clock, catalogue, history, inputs, reading, handed)
        else {
            self.unread.insert(key_of, handed.effect.receipt.clone());
            return stated_value(history, inputs, reading, handed);
        };
        self.flights.insert(key_of, rebuilt);
        held.map(|operation| Hold::Route(Box::new(operation)))
    }

    fn read_flight(
        &self,
        clock: &dyn Clock,
        catalogue: &Catalogue,
        history: &[(Receipt, Entry)],
        inputs: &[RecordedInput],
        reading: &mut dyn Reopening,
        handed: &Handed<'_>,
    ) -> Option<(InFlight, Option<Operation>)> {
        let (record_id, place, plan) = plan_of(inputs, handed.order, handed.segment)?;
        let record = reading.record(&record_id)?;
        let operation = operation_of(&record, &plan)?.clone();
        // The operator's question about the segment, the last before its effect: its transaction and signed bytes.
        let (question_receipt, question, transaction) = history[..handed.at]
            .iter()
            .rev()
            .find_map(|(receipt, entry)| match entry {
                Entry::Stated(stated)
                    if stated.what.statement.as_str() == OPERATION_SHAPE
                        && stated.about.as_ref().is_some_and(|a| {
                            a.order.as_ref() == Some(handed.order)
                                && a.segment.as_ref() == Some(handed.segment)
                        }) =>
                {
                    let transaction = stated.about.as_ref()?.transaction.clone()?;
                    Some((receipt.clone(), stated.clone(), transaction))
                }
                _ => None,
            })?;
        let bytes = reading.kept(&question.commitment)?;
        let statement: Statement<ProviderOperation> = serde_json::from_slice(&bytes).ok()?;
        let caller = history
            .iter()
            .find_map(|(receipt, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what: What::Decide { request, .. },
                    ..
                }) if receipt == handed.effect.decision.receipt() => Some(request.caller.clone()),
                _ => None,
            })
            .unwrap_or_else(|| match &history[handed.at].1 {
                Entry::Verified(v) => v.who.clone(),
                Entry::Stated(_) => unreachable!("an effect is a verified entry"),
            });
        // The question as Core appended it, its maker claimed: the log attributed it on append. Its receipt is the
        // log's, so it is never appended again.
        let mut appended = question;
        appended.attribution = rutent_contracts::core_internal::Attribution::Unverified {
            claimed_by: appended.attribution.maker().clone(),
        };
        let mut steps = BTreeMap::new();
        steps.insert(
            "question",
            Step {
                entry: GeneralEntry::try_from(Entry::Stated(appended)).ok()?,
                receipt: Some(question_receipt),
            },
        );
        steps.insert(
            "effect",
            Step {
                entry: GeneralEntry::try_from(history[handed.at].1.clone()).ok()?,
                receipt: Some(handed.effect.receipt.clone()),
            },
        );
        for (receipt, entry) in &history[handed.at..] {
            if let Entry::Verified(v) = entry
                && let What::Result { effect } = &v.what
                && effect == &handed.effect.receipt
            {
                let name = match v.outcome {
                    Outcome::Done {} => "result: done",
                    Outcome::Failed {} => "result: failed",
                    Outcome::Unanswered {} => "result: unanswered",
                    _ => continue,
                };
                if let Ok(entry) = GeneralEntry::try_from(entry.clone()) {
                    steps.insert(
                        name,
                        Step {
                            entry,
                            receipt: Some(receipt.clone()),
                        },
                    );
                }
            }
        }
        let outcome = outcome_of(
            catalogue,
            history,
            inputs,
            reading,
            (handed, &transaction),
            (&record, &operation, &statement),
        );
        // The provider's answer, where the log holds it and nothing of the segment's grading yet (no evidence and no
        // statement kept for it): the reopened Core goes on from it as a Core that ran on, judging, grading and settling,
        // each step once; the operation is never carried out again. Its `bound` input and the answer's stated entry are
        // the log's, so neither is appended again.
        let answered = if outcome.is_none() {
            resumed_answer(history, reading, handed, &transaction, &mut steps)
        } else {
            None
        };
        let judged = match &outcome {
            Some(SegmentOutcome::Settled { .. }) => settled_result(
                catalogue,
                history,
                reading,
                (handed, &transaction),
                (&record, &operation, &statement),
            ),
            _ => None,
        };
        let stated_out = judged.as_ref().and_then(|j| Amount::new(j.4.clone()));
        let charged = match (&outcome, judged) {
            (Some(SegmentOutcome::Settled { .. }), Some(judged)) => {
                let started_at = match &history[handed.at].1 {
                    Entry::Verified(v) => v.when.at.clone(),
                    Entry::Stated(_) => return None,
                };
                charged_of(
                    handed,
                    &started_at,
                    (&record, &operation),
                    (judged.1, judged.2, judged.3),
                )
            }
            _ => None,
        };
        let held = held_at(&operation, &statement.statement);
        let flight = InFlight {
            place,
            transaction,
            provider: handed.provider.clone(),
            caller,
            statement,
            effect: Some(handed.effect.clone()),
            begun: true,
            carried: true,
            answered,
            started: Some(clock.now()),
            unknown: Vec::new(),
            steps,
            evidence: None,
            graded: None,
            kept: None,
            fees: None,
            ended: outcome.as_ref().is_some_and(is_final),
            outcome,
            charged,
            stated_out,
            reported: None,
            begin_refused: None,
            verdict: None,
            drains: Vec::new(),
            record: None,
            late: BTreeMap::new(),
            carries: Vec::new(),
            relies: Vec::new(),
            priced: measured::Priced::of(&operation),
            heard: measured::hear(&record, &operation),
            // The log's `decision_record` entry of the record, which the answers' measure records cite.
            record_entry: history.iter().find_map(|(r, e)| match e {
                Entry::Verified(v) => match &v.what {
                    What::DecisionRecord {
                        decision_record,
                        order,
                        ..
                    } if order == handed.order && decision_record == record.record_id() => {
                        Some(r.clone())
                    }
                    _ => None,
                },
                Entry::Stated(_) => None,
            }),
            reopened: true,
        };
        Some((flight, held))
    }

    /// The measure records a restart dropped, kept again for the writer (`REQ-AUD-7`, `T-020-46`): each segment
    /// rebuilt with its ending (settled, failed, refused, its withdrawal cancelled) gives its records again, on the
    /// entries the log holds of it (its effect, the provider's answer, a failed result) and at the time the log
    /// recorded its ending, as does each completed reversal, on its input and the provider's confirmation before it;
    /// then every record the log already holds (the same provider, role, order, segment and measure) is dropped, so
    /// nothing is measured twice, and what a writer cut short is kept.
    fn measure_what_the_log_lacks(&mut self, history: &[(Receipt, Entry)]) {
        let logged: Vec<&rutent_contracts::core_internal::MeasureRecord> = history
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(v) => match &v.what {
                    What::Measure { record } => Some(record),
                    _ => None,
                },
                Entry::Stated(_) => None,
            })
            .collect();
        let about = |input: &RecordedInput, key: &(String, String)| {
            input.subject.order.as_str() == key.0
                && input
                    .subject
                    .segment
                    .as_ref()
                    .is_some_and(|s| s.as_str() == key.1)
        };
        let keys: Vec<(String, String)> = self.flights.keys().cloned().collect();
        for key in &keys {
            let (Some(order), Some(segment), Some(flight)) =
                (Id::new(&key.0), Id::new(&key.1), self.flights.get(key))
            else {
                continue;
            };
            let (Some(outcome), Some(effect)) = (
                flight.outcome.as_ref(),
                flight.effect.as_ref().map(|e| e.receipt.clone()),
            ) else {
                continue;
            };
            let at = history.iter().rev().find_map(|(_, e)| match e {
                Entry::Verified(v) => match &v.what {
                    What::Input { input }
                        if about(input, key)
                            && matches!(
                                input.observed,
                                Observed::SegmentSettled { .. }
                                    | Observed::SegmentFailed {}
                                    | Observed::WithdrawalCancelled {}
                            ) =>
                    {
                        Some(v.when.at.clone())
                    }
                    _ => None,
                },
                Entry::Stated(_) => None,
            });
            let Some(at) = at else {
                continue;
            };
            let mut cites = vec![effect];
            // As the Core that ran on cites them: the effect, the provider's answer under the segment's transaction
            // as the log took it, and a failed result where the segment failed on one.
            let answer = flight
                .steps
                .get("stated")
                .and_then(|s| s.receipt.clone())
                .or_else(|| {
                    history.iter().rev().find_map(|(r, e)| match e {
                        Entry::Stated(stated)
                            if stated.what.statement.as_str() == RESULT_SHAPE
                                && stated.attribution.maker().acts == flight.provider
                                && stated.about.as_ref().is_some_and(|a| {
                                    a.order.as_ref() == Some(&order)
                                        && a.segment.as_ref() == Some(&segment)
                                        && a.transaction.as_ref() == Some(&flight.transaction)
                                }) =>
                        {
                            Some(r.clone())
                        }
                        _ => None,
                    })
                });
            cites.extend(answer);
            if matches!(outcome, SegmentOutcome::Failed)
                && let Some(receipt) = flight
                    .steps
                    .get("result: failed")
                    .and_then(|s| s.receipt.clone())
            {
                cites.push(receipt);
            }
            let ending = measured::Ending {
                order: &order,
                segment: &segment,
                provider: &flight.provider,
                priced: &flight.priced,
                heard: &flight.heard,
                outcome,
                answers_rest_on: flight.record_entry.clone(),
                cites,
                at,
            };
            self.measuring.ended(&ending);
        }
        for (place, (receipt, entry)) in history.iter().enumerate() {
            let Entry::Verified(v) = entry else {
                continue;
            };
            let What::Input { input } = &v.what else {
                continue;
            };
            let Some(segment) = input.subject.segment.as_ref() else {
                continue;
            };
            if !matches!(input.observed, Observed::ReversalCompleted { .. }) {
                continue;
            }
            let key = (
                input.subject.order.as_str().to_owned(),
                segment.as_str().to_owned(),
            );
            let (Some(order), Some(segment), Some(flight)) =
                (Id::new(&key.0), Id::new(&key.1), self.flights.get(&key))
            else {
                continue;
            };
            let confirmation = history[..place].iter().rev().find_map(|(r, e)| match e {
                Entry::Stated(s)
                    if s.about.as_ref().is_some_and(|a| {
                        a.order.as_ref() == Some(&order) && a.segment.as_ref() == Some(&segment)
                    }) =>
                {
                    Some(r.clone())
                }
                _ => None,
            });
            let cites = confirmation.into_iter().chain([receipt.clone()]).collect();
            let provider = flight.provider.clone();
            self.measuring
                .reversed(&order, &segment, &provider, v.when.at.clone(), cites);
        }
        self.measuring.drop_written(|record| {
            logged.iter().any(|l| {
                l.provider == record.provider
                    && l.role == record.role
                    && l.order == record.order
                    && l.segment == record.segment
                    && l.measure == record.measure
            })
        });
    }

    /// Whether `order`'s `segment` was retired, as the state machine rebuilt from the log has it: a recalculation, a
    /// resume or a destination change replaced it (the part's retired segments, [`Segments::retired`]). A Core that
    /// ran on released its hold then (`retiring`), so a reopened one doesn't hold it again (`REQ-ORD-8`, `T-020-46`).
    fn retired_in_the_log(&self, order: &str, segment: &str) -> bool {
        let (Some(order_id), Some(segment_id)) = (Id::new(order), Id::new(segment)) else {
            return false;
        };
        self.retired(&order_id, &segment_id)
    }

    /// Holds in `exposures` every segment rebuilt in flight, at the amount its operation moved, and marks every stuck
    /// order, as a Core that ran on would hold them (`REQ-ORD-8`): a segment with no final outcome, or one that
    /// failed or was refused, whose value stays held until its reversal or the part's resolution, unless the part was
    /// resolved or the segment reversed.
    fn hold_in_flight(
        &self,
        exposures: &mut Exposures,
        inputs: &[RecordedInput],
        holds: &BTreeMap<(String, String), Hold>,
    ) {
        let reversed = |order: &str, segment: &str| {
            inputs.iter().any(|i| {
                matches!(i.observed, Observed::ReversalCompleted { .. })
                    && i.subject.order.as_str() == order
                    && i.subject
                        .segment
                        .as_ref()
                        .is_some_and(|s| s.as_str() == segment)
            })
        };
        for ((order, segment), flight) in &self.flights {
            let held = match &flight.outcome {
                None
                | Some(
                    SegmentOutcome::Unknown(_)
                    | SegmentOutcome::Failed
                    | SegmentOutcome::Refused(_)
                    | SegmentOutcome::WaitingOnThePerson(_)
                    | SegmentOutcome::NoResult(_)
                    | SegmentOutcome::AwaitsEvidence(_),
                ) => true,
                Some(_) => false,
            };
            let (Some(order_id), Some(segment_id)) = (Id::new(order), Id::new(segment)) else {
                continue;
            };
            let part = self.orders.get(order.as_str()).and_then(|t| {
                t.order
                    .parts
                    .iter()
                    .find(|p| p.segments.iter().any(|s| s.segment_id.as_str() == segment))
                    .and_then(|p| Id::new(p.part_id.as_str()))
            });
            let resolved = part
                .as_ref()
                .is_some_and(|p| self.ended_by_resolution(&order_id, p));
            if !held
                || self.retired_in_the_log(order, segment)
                || resolved
                || reversed(order, segment)
            {
                continue;
            }
            let Some(Hold::Route(held)) = holds.get(&(order.clone(), segment.clone())) else {
                continue;
            };
            let _ = exposures.hold(&order_id, &segment_id, held);
        }
        // A segment handed over whose operation can't be read back is held at the value it states, unless the log
        // says its value moved on: failing closed, the ceiling still counts it.
        for (order, segment) in self.unread.keys() {
            let Some(Hold::Stated {
                provider,
                asset,
                amount,
            }) = holds.get(&(order.clone(), segment.clone()))
            else {
                continue;
            };
            let moved_on = inputs.iter().any(|i| {
                i.subject.order.as_str() == order
                    && i.subject
                        .segment
                        .as_ref()
                        .is_some_and(|s| s.as_str() == segment)
                    && matches!(
                        i.observed,
                        Observed::SegmentSettled { .. } | Observed::ReversalCompleted { .. }
                    )
            });
            let (Some(order_id), Some(segment_id)) = (Id::new(order), Id::new(segment)) else {
                continue;
            };
            if moved_on || self.retired_in_the_log(order, segment) {
                continue;
            }
            let _ = exposures.hold_resting(&order_id, &segment_id, provider, asset, amount);
        }
        for tracked in self.orders.values() {
            if tracked
                .order
                .parts
                .iter()
                .any(|p| p.state == OrderState::Stuck && p.resolved_to.is_none())
                && let Some(order) = Id::new(tracked.order.order_id.as_str())
            {
                exposures.mark_stuck(&order);
            }
        }
    }

    /// Whether `segment` of `order` is one a reopened Core holds as acted on, though it couldn't read its operation
    /// back: it is never handed over again (`INV-8`).
    pub(super) fn unread_effect(&self, at: &(String, String)) -> Option<&Receipt> {
        self.unread.get(at)
    }
}

/// The provider's answer to a segment handed over, where the log holds it and nothing more of the segment than its
/// question, its effect, its `bound` input and that answer: its bytes, kept behind its stated entry's commitment, with the
/// `bound` and `stated` steps the log took put in `steps`. `None` where the log holds anything more of the segment (a
/// result, evidence, a statement kept for its grading), so nothing of it is appended twice: it stays in motion then.
fn resumed_answer(
    history: &[(Receipt, Entry)],
    reading: &mut dyn Reopening,
    handed: &Handed<'_>,
    transaction: &Id,
    steps: &mut BTreeMap<&'static str, Step>,
) -> Option<Answered> {
    let later = &history[handed.at + 1..];
    let about_it = |a: &Option<About>| {
        a.as_ref().is_some_and(|a| {
            a.order.as_ref() == Some(handed.order) && a.segment.as_ref() == Some(handed.segment)
        })
    };
    let mut answer = None;
    let mut bound = None;
    for (receipt, entry) in later {
        match entry {
            Entry::Stated(stated) if about_it(&stated.about) => {
                let is_the_answer = stated.what.statement.as_str() == RESULT_SHAPE
                    && stated.about.as_ref().and_then(|a| a.transaction.as_ref())
                        == Some(transaction)
                    && stated.attribution.maker().acts == *handed.provider;
                if !is_the_answer || answer.is_some() {
                    return None;
                }
                answer = Some((receipt.clone(), stated.clone()));
            }
            Entry::Verified(v) => match &v.what {
                What::Input { input }
                    if input.subject.order.as_str() == handed.order.as_str()
                        && input
                            .subject
                            .segment
                            .as_ref()
                            .is_some_and(|s| s.as_str() == handed.segment.as_str()) =>
                {
                    if !matches!(input.observed, Observed::ProviderBound {}) {
                        return None;
                    }
                    bound = Some((receipt.clone(), entry.clone()));
                }
                What::Result { effect } if effect == &handed.effect.receipt => return None,
                What::Evidence { order, segment, .. }
                    if order == handed.order && segment == handed.segment =>
                {
                    return None;
                }
                _ => {}
            },
            _ => {}
        }
    }
    let (receipt, mut stated) = answer?;
    let bytes = reading.kept(&stated.commitment)?;
    stated.attribution = rutent_contracts::core_internal::Attribution::Unverified {
        claimed_by: stated.attribution.maker().clone(),
    };
    steps.insert(
        "stated",
        Step {
            entry: GeneralEntry::try_from(Entry::Stated(stated)).ok()?,
            receipt: Some(receipt),
        },
    );
    if let Some((receipt, entry)) = bound {
        steps.insert(
            "bound",
            Step {
                entry: GeneralEntry::try_from(entry).ok()?,
                receipt: Some(receipt),
            },
        );
    }
    Some(Answered::Bytes(bytes))
}

/// The value a segment handed over states, for a segment whose decision record can't be read back: its provider and
/// asset as the order's plan names them, and the amount its signed operation (the operator's question, kept behind its
/// commitment) moves, or else the order's own amount as its `order_opened` asks it, where the segment moves the
/// order's own asset. `None` where neither is readable.
fn stated_value(
    history: &[(Receipt, Entry)],
    inputs: &[RecordedInput],
    reading: &mut dyn Reopening,
    handed: &Handed<'_>,
) -> Option<Hold> {
    let (_, _, plan) = plan_of(inputs, handed.order, handed.segment)?;
    let provider = Id::new(plan.provider.as_str())?;
    let asset = plan.from.asset.clone();
    let questioned = history[..handed.at]
        .iter()
        .rev()
        .find_map(|(_, entry)| match entry {
            Entry::Stated(stated)
                if stated.what.statement.as_str() == OPERATION_SHAPE
                    && stated.about.as_ref().is_some_and(|a| {
                        a.order.as_ref() == Some(handed.order)
                            && a.segment.as_ref() == Some(handed.segment)
                    }) =>
            {
                Some(stated.commitment.clone())
            }
            _ => None,
        });
    let moved = questioned
        .and_then(|commitment| reading.kept(&commitment))
        .and_then(|bytes| serde_json::from_slice::<Statement<ProviderOperation>>(&bytes).ok())
        .and_then(|statement| match statement.statement {
            ProviderOperation::Withdrawal(o) => Amount::new(o.amount),
            ProviderOperation::Deposit(o) => Amount::new(o.amount),
            ProviderOperation::Transfer(o) => Amount::new(o.amount),
            ProviderOperation::Conversion(o) => Amount::new(o.amount),
            _ => None,
        });
    let amount = moved.or_else(|| {
        history.iter().find_map(|(_, entry)| match entry {
            Entry::Verified(VerifiedEntry {
                what: What::OrderOpened { order, route, .. },
                ..
            }) if order == handed.order && route.from.asset == asset => {
                Some(route.amount.amount().clone())
            }
            _ => None,
        })
    })?;
    Some(Hold::Stated {
        provider,
        asset,
        amount,
    })
}

/// The decision record, the place in its part and the segment's plan, from the last input of `order` that planned
/// `segment`: its `route_chosen`, or a `route_recalculated` or `destination_changed`.
fn plan_of(inputs: &[RecordedInput], order: &Id, segment: &Id) -> Option<(Id, usize, SegmentPlan)> {
    inputs
        .iter()
        .rev()
        .filter(|i| i.subject.order.as_str() == order.as_str())
        .find_map(|input| {
            let (record, planned): (&stm::Id, Vec<&[SegmentPlan]>) = match &input.observed {
                Observed::RouteChosen {
                    decision_record,
                    parts,
                    ..
                } => (
                    decision_record,
                    parts.iter().map(|p| p.segments.as_slice()).collect(),
                ),
                Observed::RouteRecalculated {
                    decision_record,
                    segments,
                }
                | Observed::DestinationChanged {
                    decision_record,
                    segments,
                    ..
                } => (decision_record, vec![segments.as_slice()]),
                _ => return None,
            };
            planned.into_iter().find_map(|segments| {
                let place = segments
                    .iter()
                    .position(|s| s.segment_id.as_str() == segment.as_str())?;
                Some((Id::new(record.as_str())?, place, segments[place].clone()))
            })
        })
}

/// The chosen route's operation a segment's plan names: its provider, kind and ends.
fn operation_of<'a>(record: &'a DecisionRecord, plan: &SegmentPlan) -> Option<&'a Operation> {
    record
        .chosen()?
        .parts
        .iter()
        .flat_map(|p| p.operations.iter())
        .find(|o| {
            o.provider.as_str() == plan.provider.as_str()
                && o.kind == plan.kind
                && o.from == plan.from
                && o.to == plan.to
        })
}

/// A rebuilt segment's outcome, as the inputs recorded about it say: settled on what its provider's result gave
/// out, as the judge counted it when the result was received; failed; its withdrawal cancelled; its outcome unknown.
/// `None` where the log holds no final input: it may have been carried out, and is in motion.
fn outcome_of(
    catalogue: &Catalogue,
    history: &[(Receipt, Entry)],
    inputs: &[RecordedInput],
    reading: &mut dyn Reopening,
    (handed, transaction): (&Handed<'_>, &Id),
    (record, operation, statement): (&DecisionRecord, &Operation, &Statement<ProviderOperation>),
) -> Option<SegmentOutcome> {
    let about = |i: &&RecordedInput| {
        i.subject.order.as_str() == handed.order.as_str()
            && i.subject
                .segment
                .as_ref()
                .is_some_and(|s| s.as_str() == handed.segment.as_str())
    };
    let part = |i: &&RecordedInput| {
        i.subject.order.as_str() == handed.order.as_str()
            && i.subject.segment.is_none()
            && matches!(i.observed, Observed::WithdrawalCancelled {})
    };
    if inputs.iter().filter(part).count() > 0
        && inputs
            .iter()
            .filter(about)
            .all(|i| !matches!(i.observed, Observed::SegmentSettled { .. }))
    {
        return Some(SegmentOutcome::WithdrawalCancelled);
    }
    // An effect whose one result is `failed` with no input about its segment's outcome was refused before anything
    // passed out (the platform wouldn't begin it, or refused it): nothing moved, and it is final. The log doesn't say
    // which refusal, so it reads as the platform's refusal to begin.
    let failed_alone = history[handed.at..].iter().any(|(_, entry)| {
        matches!(entry, Entry::Verified(v)
            if matches!(&v.what, What::Result { effect } if effect == &handed.effect.receipt)
                && v.outcome == (Outcome::Failed {}))
    });
    let last = inputs.iter().rev().filter(about).find(|i| {
        matches!(
            i.observed,
            Observed::SegmentSettled { .. }
                | Observed::SegmentFailed {}
                | Observed::SegmentOutcomeUnknown {}
        )
    });
    let Some(last) = last else {
        return failed_alone.then_some(SegmentOutcome::NotHandedOver(PlatformRefusal::Unavailable));
    };
    match &last.observed {
        Observed::SegmentFailed {} => Some(SegmentOutcome::Failed),
        Observed::SegmentOutcomeUnknown {} => {
            let record = history.iter().find_map(|(receipt, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what:
                        What::HandOver {
                            item: HandedOver::OperationUnanswered { transaction: t, .. },
                            ..
                        },
                    ..
                }) if t == transaction => Some(receipt.clone()),
                _ => None,
            });
            Some(SegmentOutcome::Unknown(UnknownBy::Platform {
                refusal: PlatformRefusal::OutcomeUnknown,
                record,
            }))
        }
        Observed::SegmentSettled { evidence } => {
            let amount_out = settled_out(
                catalogue,
                history,
                reading,
                (handed, transaction),
                (record, operation, statement),
            )?;
            Some(SegmentOutcome::Settled {
                amount_out,
                evidence: Id::new(evidence.as_str())?,
            })
        }
        _ => None,
    }
}

/// What a settled segment gave out, net of its charge: its provider's result about its transaction, kept behind its
/// commitment, judged again at the time its stated entry was received, as the judge counted it then.
fn settled_out(
    catalogue: &Catalogue,
    history: &[(Receipt, Entry)],
    reading: &mut dyn Reopening,
    (handed, transaction): (&Handed<'_>, &Id),
    (record, operation, statement): (&DecisionRecord, &Operation, &Statement<ProviderOperation>),
) -> Option<Amount> {
    settled_result(
        catalogue,
        history,
        reading,
        (handed, transaction),
        (record, operation, statement),
    )
    .map(|judged| judged.0)
}

/// What a settled segment's result counted for: what it handed on, what it took in, its charge, and the receipt of
/// its stated entry, judged again at the time that entry was received.
fn settled_result(
    catalogue: &Catalogue,
    history: &[(Receipt, Entry)],
    reading: &mut dyn Reopening,
    (handed, transaction): (&Handed<'_>, &Id),
    (record, operation, statement): (&DecisionRecord, &Operation, &Statement<ProviderOperation>),
) -> Option<(
    Amount,
    String,
    rutent_contracts::provider_role::Fee,
    Receipt,
    String,
)> {
    let cited = sign::cited(record, operation)?;
    let units = Units {
        origin_decimals: operation.from.asset.decimals,
        out_decimals: operation.to.asset.decimals,
    };
    history[handed.at..].iter().find_map(|(receipt, entry)| {
        let Entry::Stated(stated) = entry else {
            return None;
        };
        if stated.what.statement.as_str() != RESULT_SHAPE
            || stated
                .about
                .as_ref()
                .is_none_or(|a| a.transaction.as_ref() != Some(transaction))
        {
            return None;
        }
        let bytes = reading.kept(&stated.commitment)?;
        let at = rutent_prov::catalogue::Instant(
            u64::try_from(stated.received_at.unix_nanos() / 1_000_000_000).ok()?,
        );
        match judge(
            catalogue,
            at,
            handed.provider,
            &statement.statement,
            cited.terms,
            cited.quoted.as_ref(),
            units,
            &bytes,
        ) {
            Ok(Counted::Succeeded {
                handed_on,
                amount_in,
                fee,
                amount_out,
                ..
            }) => Some((
                Amount::new(handed_on)?,
                amount_in,
                fee,
                receipt.clone(),
                amount_out,
            )),
            _ => None,
        }
    })
}

/// The charge of a segment rebuilt settled, as the check needs it (`Segments::note_charge`'s): what its result took in
/// and charged, when its `move_value` effect started it, and the formula its route was priced on, so a charge the log
/// holds a raise of is answered `AlreadyRaised` and one it doesn't is checked as a Core that ran on checks it
/// (`T-020-46`). `None` where its charge is in an asset its segment names at neither end.
fn charged_of(
    handed: &Handed<'_>,
    started_at: &Timestamp,
    (record, operation): (&DecisionRecord, &Operation),
    (amount_in, fee, result): (String, rutent_contracts::provider_role::Fee, Receipt),
) -> Option<Charged> {
    let cited = sign::cited(record, operation)?;
    let charged_in = if fee.asset == operation.from.asset.id {
        None
    } else if fee.asset == operation.to.asset.id {
        Some(operation.to.asset.clone())
    } else {
        return None;
    };
    Some(Charged {
        order: handed.order.clone(),
        segment: handed.segment.clone(),
        provider: operation.provider.clone(),
        tuple: rutent_contracts::router_api::TupleKey {
            provider: operation.provider.clone(),
            from: operation.from.clone(),
            to: operation.to.clone(),
        },
        started_at: started_at.clone(),
        amount_in: Amount::new(amount_in)?,
        charged: Amount::new(fee.amount)?,
        charged_in,
        advertised: cited.price?.clone(),
        result,
    })
}
