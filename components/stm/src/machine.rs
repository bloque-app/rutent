//! The transition function: [`Stm`], the contract's [`StateMachine`].
//!
//! # The states a segment moves through
//!
//! A segment only moves forward, along these edges and no others, so an input delivered again, or out of order,
//! finds the segment where it can't apply and changes nothing (`REQ-STM-4`, `INV-8`):
//!
//! ```text
//! pending -> started -> settled
//!                    -> failed_reversal_owed -> reversal_complete
//!                    -> outcome_unknown -> settled | failed_reversal_owed | reversal_complete
//! settled -> reversal_complete          (the starting segment only: the part, and so the order, fails)
//! ```
//!
//! A deposit has no `reversal_completed`: a failed deposit leaves the value where it was, at its `from` node
//! (`INV-2`, `REQ-ORD-43`), so it owes nothing back. Elapsed time is not an input of any of these (`REQ-STM-5`).
//!
//! # What each part does
//!
//! Every part keeps its own state, and the order's follows from its parts ([`order_state_of`]). A part that has
//! ended (`is_ended`) takes no input, except that a complete part fails when its starting segment is proven undone
//! (`REQ-STM-6`).
//!
//! No transition reads an amount.

use alloc::{collections::BTreeSet, vec::Vec};

use rutent_contracts::state_machine::{
    Destination, Id, LastInput, NotApplicableReason, Observed, OperationKind, Order, OrderState,
    Outcome, Part, RecordedInput, Scope, Segment, SegmentPlan, SegmentState, StateMachine,
    after_no_route, deposit_failed_at_after, destination_change_applies,
    recalculation_keeps_destination, recalculation_starts_where_value_sits, value_at,
};

use crate::parts::{is_ended, order_state_of};

/// The state machine of rutent's orders: `open` from the input that accepted a route, `transition` on every
/// recorded input after. Pure and deterministic: no clock, no randomness, no I/O, no amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stm;

fn not_applicable(reason: NotApplicableReason) -> Outcome {
    Outcome::NotApplicable { reason }
}

fn not_from_this_state() -> Outcome {
    not_applicable(NotApplicableReason::NotFromThisState)
}

impl StateMachine for Stm {
    fn open(input: &RecordedInput) -> Option<Order> {
        let Observed::RouteChosen {
            parts,
            handed_on_from,
            ..
        } = &input.observed
        else {
            return None;
        };
        input.check().ok()?;
        let parts = parts
            .iter()
            .map(|p| {
                Some(Part {
                    part_id: p.part_id.clone(),
                    state: OrderState::NotPastFirstSegment,
                    linked_order: None,
                    segments: p
                        .segments
                        .iter()
                        .map(|s| Segment::new(s.clone(), SegmentState::Pending))
                        .collect(),
                    retired_segments: Vec::new(),
                    destination: Destination::of(&p.segments)?,
                    deposit_failed_at: None,
                })
            })
            .collect::<Option<Vec<Part>>>()?;
        let order = Order {
            order_id: input.subject.order.clone(),
            state: order_state_of(&parts),
            last_record: input.record_id.clone(),
            last_input: LastInput {
                record_id: input.record_id.clone(),
                outcome: Outcome::Applied {},
            },
            handed_on_from: handed_on_from.clone(),
            parts,
        };
        order.check().ok()?;
        Some(order)
    }

    fn transition(state: &Order, input: &RecordedInput) -> Order {
        let mut next = state.clone();
        let mut outcome = apply(&mut next, input);
        if outcome == (Outcome::Applied {}) {
            next.state = order_state_of(&next.parts);
            next.last_record = input.record_id.clone();
            next.last_input = LastInput {
                record_id: input.record_id.clone(),
                outcome: Outcome::Applied {},
            };
            // The contract's promise: every order returned passes its check. A change that would break it is
            // not applied.
            let checked = next.check();
            debug_assert!(
                checked.is_ok(),
                "a transition built an invalid order: {checked:?}"
            );
            if checked.is_err() {
                next = state.clone();
                outcome = not_from_this_state();
            }
        } else {
            next = state.clone();
        }
        next.last_input = LastInput {
            record_id: input.record_id.clone(),
            outcome,
        };
        next
    }
}

/// Whether the input is a complete part's starting segment proven undone, the one change an ended part takes
/// (`REQ-STM-6`).
fn undoes_a_complete_part(part: &Part, input: &RecordedInput) -> bool {
    part.state == OrderState::Complete && undoes_starting_segment(part, input)
}

fn undoes_starting_segment(part: &Part, input: &RecordedInput) -> bool {
    matches!(input.observed, Observed::ReversalCompleted { .. })
        && part.segments.first().is_some_and(|first| {
            first.state == SegmentState::Settled
                && input.subject.segment.as_ref() == Some(&first.segment_id)
        })
}

fn apply(order: &mut Order, input: &RecordedInput) -> Outcome {
    if input.subject.order != order.order_id {
        return not_applicable(NotApplicableReason::OtherOrder);
    }
    if input.record_id == order.last_record || input.record_id == order.last_input.record_id {
        return Outcome::Repeat {};
    }
    // An input that breaks its own contract's rules (a route that doesn't chain, a subject that names the wrong
    // things) is not one the order can use.
    if input.check().is_err() {
        return not_from_this_state();
    }
    let order_ended = is_ended(order.state);
    let part_index = input
        .subject
        .part
        .as_ref()
        .and_then(|id| order.parts.iter().position(|p| &p.part_id == id));
    let exception = part_index.is_some_and(|i| undoes_a_complete_part(&order.parts[i], input));

    match input.observed.scope() {
        Scope::Order => {
            if order_ended {
                return not_applicable(NotApplicableReason::Ended);
            }
            match &input.observed {
                Observed::EndRequested {} => end_requested(order),
                _ => not_from_this_state(),
            }
        }
        Scope::Part | Scope::Segment => {
            if order_ended && !exception {
                return not_applicable(NotApplicableReason::Ended);
            }
            let Some(index) = part_index else {
                return not_applicable(NotApplicableReason::UnknownSubject);
            };
            if is_ended(order.parts[index].state) && !exception {
                return not_applicable(NotApplicableReason::Ended);
            }
            let used: BTreeSet<Id> = order
                .parts
                .iter()
                .flat_map(|p| {
                    p.segments
                        .iter()
                        .map(|s| &s.segment_id)
                        .chain(&p.retired_segments)
                })
                .cloned()
                .collect();
            let order_id = order.order_id.clone();
            apply_to_part(&mut order.parts[index], &order_id, &used, input)
        }
    }
}

/// `end_requested`: each part that hasn't passed its first segment ends without completing (`REQ-ORD-12`). The
/// parts past it go on.
fn end_requested(order: &mut Order) -> Outcome {
    let mut any = false;
    for part in &mut order.parts {
        if part.state == OrderState::NotPastFirstSegment {
            part.state = OrderState::EndedWithoutCompleting;
            any = true;
        }
    }
    if any {
        Outcome::Applied {}
    } else {
        not_from_this_state()
    }
}

/// A part's state once its segments moved: complete when every segment settled; otherwise the state it was in if
/// that holds (waiting or stuck), else by whether its first segment settled.
fn after_segments_moved(part: &mut Part) {
    part.state = if part
        .segments
        .iter()
        .all(|s| s.state == SegmentState::Settled)
    {
        OrderState::Complete
    } else if matches!(
        part.state,
        OrderState::WaitingForCustomer | OrderState::Stuck
    ) {
        part.state
    } else {
        by_first_segment(part)
    };
}

fn by_first_segment(part: &Part) -> OrderState {
    if part.segments[0].state == SegmentState::Settled {
        OrderState::FirstSegmentSettled
    } else {
        OrderState::NotPastFirstSegment
    }
}

fn apply_to_part(
    part: &mut Part,
    order_id: &Id,
    used: &BTreeSet<Id>,
    input: &RecordedInput,
) -> Outcome {
    match &input.observed {
        Observed::ProviderBound {}
        | Observed::SegmentSettled { .. }
        | Observed::SegmentFailed {}
        | Observed::SegmentOutcomeUnknown {}
        | Observed::ReversalCompleted { .. } => apply_to_segment(part, input),
        Observed::RouteRecalculated { segments, .. } => restart(part, used, input, segments, false),
        Observed::DestinationChanged { segments, .. } => restart(part, used, input, segments, true),
        Observed::NoRoute { .. } => match after_no_route(part) {
            Some(state) if state != part.state => {
                part.state = state;
                Outcome::Applied {}
            }
            _ => not_from_this_state(),
        },
        Observed::StuckDeclared { .. } => {
            if part.state == OrderState::Stuck {
                not_from_this_state()
            } else {
                part.state = OrderState::Stuck;
                Outcome::Applied {}
            }
        }
        Observed::WithdrawalCancelled {} => {
            let withdrawing = part.segments[0].kind == OperationKind::Withdrawal
                && part.segments[0].state != SegmentState::Settled;
            if part.state == OrderState::NotPastFirstSegment && withdrawing {
                part.state = OrderState::EndedWithoutCompleting;
                Outcome::Applied {}
            } else {
                not_from_this_state()
            }
        }
        Observed::HandedOn { linked_order, .. } => {
            let held = matches!(
                part.state,
                OrderState::WaitingForCustomer | OrderState::Stuck
            );
            if held && linked_order != order_id && value_at(part).is_some() {
                part.state = OrderState::HandedOn;
                part.linked_order = Some(linked_order.clone());
                Outcome::Applied {}
            } else {
                not_from_this_state()
            }
        }
        // Order-scope kinds never reach a part.
        Observed::RouteChosen { .. } | Observed::EndRequested {} => not_from_this_state(),
    }
}

fn apply_to_segment(part: &mut Part, input: &RecordedInput) -> Outcome {
    let Some(segment_id) = &input.subject.segment else {
        return not_from_this_state();
    };
    let Some(index) = part
        .segments
        .iter()
        .position(|s| &s.segment_id == segment_id)
    else {
        return if part.retired_segments.contains(segment_id) {
            Outcome::Late {}
        } else {
            not_applicable(NotApplicableReason::UnknownSubject)
        };
    };
    let kind = part.segments[index].kind;
    let current = part.segments[index].state;
    // The published update rule, read before the part changes.
    let failed_at = deposit_failed_at_after(part, input);
    let next = match (&input.observed, current) {
        // No new provider starts on a part that is stuck or waiting: the operator or the customer resolves it.
        (Observed::ProviderBound {}, SegmentState::Pending)
            if !matches!(
                part.state,
                OrderState::Stuck | OrderState::WaitingForCustomer
            ) && part.segments[..index]
                .iter()
                .all(|s| s.state == SegmentState::Settled) =>
        {
            SegmentState::Started
        }
        (Observed::SegmentSettled { .. }, SegmentState::Started | SegmentState::OutcomeUnknown) => {
            SegmentState::Settled
        }
        (Observed::SegmentFailed {}, SegmentState::Started | SegmentState::OutcomeUnknown) => {
            part.deposit_failed_at = failed_at;
            SegmentState::FailedReversalOwed
        }
        (Observed::SegmentOutcomeUnknown {}, SegmentState::Started) => SegmentState::OutcomeUnknown,
        (
            Observed::ReversalCompleted { .. },
            SegmentState::FailedReversalOwed | SegmentState::OutcomeUnknown,
        ) if kind != OperationKind::Deposit => SegmentState::ReversalComplete,
        (Observed::ReversalCompleted { .. }, SegmentState::Settled)
            if undoes_starting_segment(part, input) =>
        {
            // The starting segment is proven undone: the part fails, and so the order.
            part.segments[index].state = SegmentState::ReversalComplete;
            part.state = OrderState::Failed;
            return Outcome::Applied {};
        }
        _ => return not_from_this_state(),
    };
    part.segments[index].state = next;
    after_segments_moved(part);
    Outcome::Applied {}
}

/// A `route_recalculated` or a `destination_changed`: a new route for the part, from where its value sits.
/// The reasons a route is refused come in the contract's order: the destination, then a reused segment id, then
/// where the route starts (`INV-10`, `INV-4`).
fn restart(
    part: &mut Part,
    used: &BTreeSet<Id>,
    input: &RecordedInput,
    route: &[SegmentPlan],
    changes_destination: bool,
) -> Outcome {
    if changes_destination
        && !matches!(
            part.state,
            OrderState::WaitingForCustomer | OrderState::Stuck
        )
    {
        // The customer's choice follows a failed final deposit with no route left; the operator may process a
        // stuck part the same way.
        return not_from_this_state();
    }
    if changes_destination {
        if !destination_change_applies(part, route) {
            return not_applicable(NotApplicableReason::KeepsDestination);
        }
    } else if !recalculation_keeps_destination(part, route) {
        return not_applicable(NotApplicableReason::ChangesDestination);
    }
    if route.iter().any(|s| used.contains(&s.segment_id)) {
        return not_applicable(NotApplicableReason::ReusesSegmentId);
    }
    if !recalculation_starts_where_value_sits(part, route) {
        return not_applicable(NotApplicableReason::ValueNotAtStart);
    }
    let Some(new_destination) = Destination::of(route) else {
        return not_from_this_state();
    };
    let failed_at = deposit_failed_at_after(part, input);
    // The settled prefix stays; from the first segment that hasn't settled, the old route is retired.
    let keep = part
        .segments
        .iter()
        .position(|s| s.state != SegmentState::Settled)
        .unwrap_or(part.segments.len());
    let retired: Vec<Segment> = part.segments.drain(keep..).collect();
    part.retired_segments
        .extend(retired.into_iter().map(|s| s.segment_id));
    part.segments.extend(
        route
            .iter()
            .cloned()
            .map(|p| Segment::new(p, SegmentState::Pending)),
    );
    if changes_destination {
        part.destination = new_destination;
    }
    part.deposit_failed_at = failed_at;
    part.state = by_first_segment(part);
    Outcome::Applied {}
}
