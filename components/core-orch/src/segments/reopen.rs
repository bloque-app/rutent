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
//! - **The values reversals brought back to held nodes** (`T-020-57`, `INV-1`): each linked order's first move out
//!   of a held node whose `reversal_completed` the log holds, what the confirmation its evidence rests on says came
//!   back (read back from the evidence store), never more than the move took in, also once a recalculation retired the
//!   move ([`Segments::rebuild_returns`]). One whose confirmation can't be read back is rebuilt as nothing confirmed:
//!   nothing is landed or restored blind.
//!
//! What a reopened Core doesn't hold, it fails closed on: a segment whose decision record, question or kept bytes it
//! can't read back is never handed over again, and is held in flight where its operation can be read.

use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{OPERATION_SHAPE, RESULT_SHAPE};
use rutent_contracts::router_api::Operation;
use rutent_contracts::state_machine::SegmentPlan;

use super::*;
use rutent_contracts::core_internal::ReadBy;

/// What a Core that reopens reads beside its log (`T-020-46`): the decision records it signed, and the statements the
/// evidence store keeps.
pub trait Reopening {
    /// The decision record `id`, as Core signed it, held to the digest the log names it by: `None` where Core holds
    /// no such record, or the one it holds isn't the one the log names.
    fn record(&self, id: &Id) -> Option<DecisionRecord>;

    /// The bytes the evidence store keeps behind `commitment`, as the log reads them: `None` where it keeps none.
    fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>>;

    /// The authorisation `authorisation` of `order` with the terms policy's register holds in force for it, read from
    /// the log (`T-020-61`): what a reopened Core's started order runs on. `None` where policy holds it in force no
    /// more (the customer replaced its terms) or holds none: the order then runs on nothing, failing closed. A reading
    /// that answers none rebuilds no order's run.
    fn in_force(
        &mut self,
        order: &Id,
        authorisation: &Id,
    ) -> Option<rutent_contracts::core_internal::Authorisation> {
        let _ = (order, authorisation);
        None
    }
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
    /// The log's cutovers, to read what the segment began under.
    timeline: &'a crate::settings::CutoverTimeline,
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
        // The reversals Core carried out for each order: the operator's questions the log holds about an order, whose
        // statement kept behind their commitment is a reversal. A private node's record that lists one is read again
        // after a restart (`REQ-ORD-41`, `T-020-47`).
        for (_, entry) in history {
            let Entry::Stated(stated) = entry else {
                continue;
            };
            let Some(about) = &stated.about else {
                continue;
            };
            let (Some(order), Some(transaction)) = (&about.order, &about.transaction) else {
                continue;
            };
            if stated.what.statement.as_str() != rutent_contracts::provider_role::OPERATION_SHAPE {
                continue;
            }
            let Some(bytes) = reading.kept(&stated.commitment) else {
                continue;
            };
            if let Ok(question) = serde_json::from_slice::<Statement<ProviderOperation>>(&bytes)
                && matches!(question.statement, ProviderOperation::Reversal(_))
            {
                segments.note_reversal_ran(order, transaction);
            }
        }
        // The publishers' records of private nodes the log holds, by what they say: a record read again after the restart,
        // signed afresh, isn't kept a second time (`T-020-58`).
        for (receipt, entry) in history {
            let Entry::Stated(stated) = entry else {
                continue;
            };
            if stated.what.statement.as_str()
                != rutent_contracts::provider_role::INNER_SEGMENTS_SHAPE
            {
                continue;
            }
            let Some(order) = stated.about.as_ref().and_then(|a| a.order.as_ref()) else {
                continue;
            };
            let Some(node) = stated.about.as_ref().and_then(|a| a.node.as_ref()) else {
                continue;
            };
            let Some(bytes) = reading.kept(&stated.commitment) else {
                continue;
            };
            if let Ok(record) = serde_json::from_slice::<
                Statement<rutent_contracts::provider_role::InnerSegments>,
            >(&bytes)
            {
                segments.records_held.push(super::HeldRecord {
                    order: order.as_str().to_owned(),
                    node: node.as_str().to_owned(),
                    maker: record.maker,
                    body: record.statement,
                    receipt: receipt.clone(),
                });
            }
        }
        // The reads of private nodes' records the log holds, each a try for its segment, node and provider: a Core that
        // reopens doesn't ask past the cap (`T-020-67`). It asks again at once, once, as before (`T-020-58`): the books the
        // reads fed, and a record the log couldn't take, were lost with the restart, so no time is kept from the log and
        // the cadence counts from that read. Of the reads of one segment, node and provider, the last one doesn't count
        // where the log holds a record of the node for the order from that provider, since that read is the one that
        // produced it; the other segment's reads still count (`rebuilt_tries`).
        let held: Vec<(String, String, String)> = segments
            .records_held
            .iter()
            .map(|h| (h.order.clone(), h.node.clone(), h.maker.clone()))
            .collect();
        let reads: Vec<(ReadKey, ReadBy)> = history
            .iter()
            .filter_map(|(_, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what:
                        What::Effect {
                            action:
                                Action::ReadNodeRecord {
                                    order,
                                    segment,
                                    node,
                                    provider,
                                    by,
                                    ..
                                },
                            ..
                        },
                    ..
                }) => Some((
                    (
                        order.as_str().to_owned(),
                        segment.as_str().to_owned(),
                        node.as_str().to_owned(),
                        provider.as_str().to_owned(),
                    ),
                    *by,
                )),
                _ => None,
            })
            .collect();
        for (key, count) in rebuilt_tries(&reads, &held) {
            for _ in 0..count {
                segments.note_record_try(key.clone(), 0);
            }
        }
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
        // The log's cutovers, folded once: each segment began under what stood before its start effect.
        let timeline = crate::settings::Settings::cutover_timeline(history);
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
                            timeline: &timeline,
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
        segments.rebuild_returns(history, &inputs, reading);
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

    /// The values reversals brought back to the held nodes linked orders' first moves left, rebuilt from the log
    /// (`T-020-57`, `INV-1`, `NFR-6`): what [`Segments::note_reversed`] kept when the reversal completed, so a value a
    /// recalculation retired the move of is still landed, restored and released as a Core that ran on would. For each
    /// `reversal_completed` of a move the order's plan put first in its part, out of an inner node, of an order handed
    /// on: the confirmation its evidence rests on, as the evidence store keeps it (a reserve's confirmed credit, or a
    /// publisher's word that the value is back), capped at what the move took in.
    fn rebuild_returns(
        &mut self,
        history: &[(Receipt, Entry)],
        inputs: &[RecordedInput],
        reading: &mut dyn Reopening,
    ) {
        for input in inputs {
            let Observed::ReversalCompleted { evidence } = &input.observed else {
                continue;
            };
            let (Some(order), Some(segment)) = (
                Id::new(input.subject.order.as_str()),
                input
                    .subject
                    .segment
                    .as_ref()
                    .and_then(|s| Id::new(s.as_str())),
            ) else {
                continue;
            };
            let handed_on = self
                .orders
                .get(order.as_str())
                .is_some_and(|t| t.order.handed_on_from.is_some());
            let Some((_, 0, plan)) = plan_of(inputs, &order, &segment) else {
                continue;
            };
            let rutent_contracts::provider_role::NodeRef::Inner(node) = &plan.from.node else {
                continue;
            };
            if !handed_on || plan.kind == stm::OperationKind::Withdrawal {
                continue;
            }
            // What the confirmation says came back, capped at what the move took in; a confirmation or an operation
            // that can't be read back is kept as nothing confirmed, so the order's reading doesn't stand in for it and
            // nothing is landed or restored blind (failing closed).
            let at = (order.as_str().to_owned(), segment.as_str().to_owned());
            let back = confirmed_back(history, reading, evidence.as_str(), &plan.from.asset.id);
            let taken_in = self
                .flights
                .get(&at)
                .and_then(|f| moved_in(&f.statement.statement));
            let amount = match (back, taken_in) {
                (Some(back), Some(taken_in)) if back < taken_in => back,
                (Some(_), Some(taken_in)) => taken_in,
                _ => Amount::zero(),
            };
            self.returned.insert(
                at,
                (
                    node.as_str().to_owned(),
                    plan.from.asset.id.as_str().to_owned(),
                    amount,
                ),
            );
        }
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
        let answered = match &outcome {
            None => resumed_answer(history, reading, handed, &transaction, &mut steps),
            // Its operation was handed over and came to an outcome the log holds: it was answered, as a Core that ran on
            // holds it, so a late answer, or one to Core's status question, is taken for it after the restart as before
            // (`T-020-61`: seed 117, and `T-020-63`'s gap 1). The answer's bytes went with the Core that stopped; a
            // segment that ended with none (`unanswered`) is held as the platform ended it, with no answer, and one with
            // a final outcome is never taken on again ([`Segments::run_part`] stands it as it is).
            Some(SegmentOutcome::NotHandedOver(_)) => None,
            Some(_) => Some(Answered::Refused(
                rutent_contracts::module_interface::PlatformRefusal::OutcomeUnknown,
            )),
        };
        // Whether the log holds the provider's answer, or evidence, of the segment since its effect: then it is no segment
        // begun with nothing back (item 8), whatever was taken up of it.
        let answer_logged = answered.is_none()
            && outcome.is_none()
            && history[handed.at..].iter().any(|(_, entry)| match entry {
                Entry::Stated(stated) => {
                    stated.what.statement.as_str() == RESULT_SHAPE
                        && stated.about.as_ref().is_some_and(|a| {
                            a.order.as_ref() == Some(handed.order)
                                && a.segment.as_ref() == Some(handed.segment)
                                && a.transaction.as_ref() == Some(&transaction)
                        })
                }
                Entry::Verified(v) => matches!(&v.what,
                    What::Evidence { order, segment, .. }
                        if order == handed.order && segment == handed.segment),
            });
        // The inputs the log took of the segment since its effect, each the segment's own step, so a segment taken on
        // again appends neither a second time (`INV-8`): the provider bound, and its outcome left unknown.
        for (receipt, entry) in &history[handed.at..] {
            let Entry::Verified(VerifiedEntry {
                what: What::Input { input },
                ..
            }) = entry
            else {
                continue;
            };
            if input.subject.order.as_str() != handed.order.as_str()
                || input
                    .subject
                    .segment
                    .as_ref()
                    .is_none_or(|s| s.as_str() != handed.segment.as_str())
            {
                continue;
            }
            let name = match input.observed {
                Observed::ProviderBound {} => "bound",
                Observed::SegmentOutcomeUnknown {} => "unknown",
                _ => continue,
            };
            if let Ok(built) = GeneralEntry::try_from(entry.clone()) {
                steps.entry(name).or_insert(Step {
                    entry: built,
                    receipt: Some(receipt.clone()),
                });
            }
        }
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
            // What it began under, derived from the log's settings as they stood before its start effect.
            began: Began::from_log(
                &statement.made_at,
                handed.timeline,
                handed.at,
                statement.protocol_version.as_str(),
                clock.now().floor(),
            ),
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
            answer_logged,
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

/// What came back, by the confirmation the evidence `evidence` rests on, in `asset`: the reserve's confirmed credit, or
/// the publisher's word that the value is back, read back from the evidence store behind its stated entry's commitment.
/// `None` where the log holds no such evidence or the store no such statement, or it is in another asset.
fn confirmed_back(
    history: &[(Receipt, Entry)],
    reading: &mut dyn Reopening,
    evidence: &str,
    asset: &str,
) -> Option<Amount> {
    use rutent_contracts::provider_role::{
        ResultBody as Body, ReversalConfirmationOutcome, ReversalOutcome,
    };
    let rests_on = history.iter().find_map(|(_, e)| match e {
        Entry::Verified(VerifiedEntry {
            what:
                What::Evidence {
                    evidence: e,
                    rests_on,
                    ..
                },
            ..
        }) if e.as_str() == evidence => rests_on.first().cloned(),
        _ => None,
    })?;
    let commitment = history.iter().find_map(|(r, e)| match e {
        Entry::Stated(s) if *r == rests_on => Some(s.commitment.clone()),
        _ => None,
    })?;
    let bytes = reading.kept(&commitment)?;
    let result: Statement<Body> = serde_json::from_slice(&bytes).ok()?;
    let (in_asset, amount) = match &result.statement {
        Body::ReversalConfirmation(answer) => match &answer.outcome {
            ReversalConfirmationOutcome::Confirmed(confirmed) => (
                confirmed.confirmation.statement.asset.clone(),
                confirmed.confirmation.statement.amount.clone(),
            ),
            _ => return None,
        },
        Body::Reversal(answer) => match &answer.outcome {
            ReversalOutcome::Succeeded(back) => (back.asset.clone(), back.amount.clone()),
            _ => return None,
        },
        _ => return None,
    };
    (in_asset == asset).then(|| Amount::new(amount)).flatten()
}

/// What an operation Core signed takes in: Core writes every operation `exact_in`.
fn moved_in(operation: &ProviderOperation) -> Option<Amount> {
    let amount = match operation {
        ProviderOperation::Withdrawal(o) => &o.amount,
        ProviderOperation::Deposit(o) => &o.amount,
        ProviderOperation::Transfer(o) => &o.amount,
        ProviderOperation::Conversion(o) => &o.amount,
        _ => return None,
    };
    Amount::new(amount.clone())
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
            (
                at,
                &Began::from_log(
                    &statement.made_at,
                    handed.timeline,
                    handed.at,
                    statement.protocol_version.as_str(),
                    at,
                ),
            ),
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

/// A read's key: order, segment, node and provider.
type ReadKey = (String, String, String, String);

/// How many tries each read key (order, segment, node, provider) has in the log: its `read_node_record` effects, less one
/// for the key's last where `held` (order, node, maker) holds the record that read produced. Keyed by segment, so a second
/// segment's reads at the same node and provider count in full (`T-020-67`).
fn rebuilt_tries(
    reads: &[(ReadKey, ReadBy)],
    held: &[(String, String, String)],
) -> BTreeMap<(String, String, String, String), u32> {
    let mut tries: BTreeMap<(String, String, String, String), u32> = BTreeMap::new();
    // Only the grading's reads count toward its tries: the effect says who asked, so the tick's, which ask under the first
    // segment of an order at a node (which the grading may be waiting on too), spend none of them (`T-020-67`).
    for (key, _) in reads.iter().filter(|(_, by)| *by == ReadBy::Grading) {
        *tries.entry(key.clone()).or_default() += 1;
    }
    for (key, count) in &mut tries {
        if held
            .iter()
            .any(|(order, node, maker)| *order == key.0 && *node == key.2 && *maker == key.3)
        {
            *count = count.saturating_sub(1);
        }
    }
    tries.retain(|_, count| *count > 0);
    tries
}

#[cfg(test)]
mod rebuilt_tries_tests {
    use super::{ReadBy, rebuilt_tries};

    fn key(segment: &str) -> (String, String, String, String) {
        ("o".into(), segment.into(), "n".into(), "p".into())
    }

    /// A record held for the order, node and provider exempts one read of each key, not all of them: a second segment at the
    /// same node doesn't get fresh tries on every restart (`REQ-ORD-41`, `NFR-5`, `T-020-67`).
    // verifies: REQ-ORD-41, NFR-5
    #[test]
    fn restart_exemption_of_a_held_record_is_one_read_of_each_segment_and_not_all_of_a_second() {
        let held = [("o".to_owned(), "n".to_owned(), "p".to_owned())];
        let grading = |segment: &str, n: usize| -> Vec<_> {
            (0..n).map(|_| (key(segment), ReadBy::Grading)).collect()
        };
        let few: Vec<_> = grading("s2", 10)
            .into_iter()
            .chain(grading("s1", 1))
            .collect();
        let tries = rebuilt_tries(&few, &held);
        assert_eq!(
            tries.get(&key("s2")),
            Some(&9),
            "the second segment keeps its tries"
        );
        assert_eq!(
            tries.get(&key("s1")),
            None,
            "the first's one read produced the record"
        );
        assert_eq!(rebuilt_tries(&few, &[]).get(&key("s2")), Some(&10));
    }

    /// The tick's reads, however many, under the waiting segment's own key, leave the grading its full remaining tries
    /// after a restart: only the grading's own reads are counted (`REQ-ORD-41`, `NFR-5`, `T-020-67`).
    // verifies: REQ-ORD-41, NFR-5
    #[test]
    fn restart_after_many_tick_reads_leaves_the_grading_its_tries() {
        let mut reads: Vec<_> = (0..40).map(|_| (key("s1"), ReadBy::Tick)).collect();
        reads.extend((0..3).map(|_| (key("s1"), ReadBy::Grading)));
        assert_eq!(rebuilt_tries(&reads, &[]).get(&key("s1")), Some(&3));
        let only_tick: Vec<_> = (0..40).map(|_| (key("s1"), ReadBy::Tick)).collect();
        assert_eq!(rebuilt_tries(&only_tick, &[]).get(&key("s1")), None);
    }
}
