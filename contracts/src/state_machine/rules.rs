//! The state machine's transition rules, published (`T-020-01`): every rule an order follows, one function each, so
//! that `CMP-STM` and core-internal's reading of a part's standing apply the same ones (`DEC-4`). Each dispatches
//! an input to them by its kind itself; nothing else of a transition is left to either.
//!
//! - [`open`] opens an order on a `route_chosen`.
//! - [`guard`] holds an input to the order: another order, a repeat, an input that breaks its contract's rules, an
//!   ended order or part (but for a complete part's starting segment proven undone, [`undoes_a_complete_part`]),
//!   an unknown part. It names what the input is about.
//! - The rules of each kind: [`after_segment_input`], [`after_restart`] (`route_recalculated`,
//!   `destination_changed`), [`after_no_route`], [`after_waiting_at_floor`], [`after_stuck_declared`],
//!   [`after_stuck_resolved`], [`after_withdrawal_cancelled`], [`after_handed_on`] and [`after_end_requested`].
//! - [`finish`] makes the order a transition returns, with [`order_state_of`] and the order's check.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use super::{
    Destination, Id, LastInput, NotApplicableReason, Observed, OperationKind, Order, OrderState,
    Outcome, Part, RecordedInput, Scope, Segment, SegmentPlan, SegmentState, after_no_route,
    after_stuck_resolved, after_waiting_at_floor, deposit_failed_at_after,
    destination_change_applies, recalculation_keeps_destination,
    recalculation_starts_where_value_sits, value_at, waiting_since_after,
};

/// Not applicable, for the reason given.
pub fn not_applicable(reason: NotApplicableReason) -> Outcome {
    Outcome::NotApplicable { reason }
}

/// Not applicable from this state.
pub fn not_from_this_state() -> Outcome {
    not_applicable(NotApplicableReason::NotFromThisState)
}

/// Whether a state is one an order or a part has ended in (`REQ-STM-6`): complete, failed, handed on, or ended
/// without completing. Stuck and waiting for the customer are not endings: someone can still resolve them.
pub fn is_ended(state: OrderState) -> bool {
    matches!(
        state,
        OrderState::Complete
            | OrderState::Failed
            | OrderState::HandedOn
            | OrderState::EndedWithoutCompleting
    )
}

/// The order's state, from its parts' states (`REQ-ORD-42`):
///
/// - When every part has ended, the order has ended, and its state is the worst ending of its parts: failed, then
///   handed on, then ended without completing, then complete. So an order is complete only when every part is,
///   and an order that holds a handed-on part is handed on, as the contract requires.
/// - Otherwise it is open: stuck if any part is stuck, else waiting for the customer if any part waits, else
///   first segment settled if any part is past its first segment, else not past its first segment.
///
/// An order of one part is in its part's state.
pub fn order_state_of(parts: &[Part]) -> OrderState {
    let any = |state: OrderState| parts.iter().any(|p| p.state == state);
    if parts.is_empty() {
        return OrderState::NotPastFirstSegment;
    }
    if parts.iter().all(|p| is_ended(p.state)) {
        return if any(OrderState::Failed) {
            OrderState::Failed
        } else if any(OrderState::HandedOn) {
            OrderState::HandedOn
        } else if any(OrderState::EndedWithoutCompleting) {
            OrderState::EndedWithoutCompleting
        } else {
            OrderState::Complete
        };
    }
    if any(OrderState::Stuck) {
        OrderState::Stuck
    } else if any(OrderState::WaitingForCustomer) {
        OrderState::WaitingForCustomer
    } else if parts.iter().any(|p| {
        matches!(
            p.state,
            OrderState::FirstSegmentSettled
                | OrderState::Complete
                | OrderState::Failed
                | OrderState::HandedOn
        )
    }) {
        OrderState::FirstSegmentSettled
    } else {
        OrderState::NotPastFirstSegment
    }
}

/// The order a `route_chosen` input opens (`REQ-STM-7`): every segment pending, every part not past its first
/// segment; `None` for any other input, or one that breaks its contract's rules.
pub fn open(input: &RecordedInput) -> Option<Order> {
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
                waiting_since: None,
                resolved_to: None,
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

/// The order a transition returns, from the order before it, the order its input's rule built (`next`) and that
/// rule's outcome: on `applied`, the order's state follows its parts ([`order_state_of`]) and the input is the last
/// that reached the state; an order that would break its check is not applied. Otherwise the order is unchanged.
/// Either way the input is the last delivered, with its outcome (`REQ-AUD-1`).
pub fn finish(state: &Order, next: Order, input: &RecordedInput, outcome: Outcome) -> Order {
    let mut next = next;
    let mut outcome = outcome;
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

/// What an input is about, once [`guard`] lets it through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The order as a whole.
    Order,
    /// The order's part at this index, with every segment id the order has, current or retired, which no new route
    /// may reuse.
    Part {
        /// The part's index in the order.
        index: usize,
        /// The order's segment ids, current and retired.
        used: BTreeSet<Id>,
    },
}

/// Holds `input` to `order`, before any rule of its kind: `Err` with the outcome where it can't reach a rule, `Ok`
/// with what it is about where it can.
pub fn guard(order: &Order, input: &RecordedInput) -> Result<Target, Outcome> {
    if input.subject.order != order.order_id {
        return Err(not_applicable(NotApplicableReason::OtherOrder));
    }
    if input.record_id == order.last_record || input.record_id == order.last_input.record_id {
        return Err(Outcome::Repeat {});
    }
    // An input that breaks its own contract's rules (a route that doesn't chain, a subject that names the wrong
    // things) is not one the order can use.
    if input.check().is_err() {
        return Err(not_from_this_state());
    }
    let order_ended = is_ended(order.state);
    let part_index = input
        .subject
        .part
        .as_ref()
        .and_then(|id| order.parts.iter().position(|p| &p.part_id == id));
    let exception = order.state == OrderState::Complete
        && part_index.is_some_and(|i| undoes_a_complete_part(&order.parts[i], input));
    match input.observed.scope() {
        Scope::Order => {
            if order_ended {
                return Err(not_applicable(NotApplicableReason::Ended));
            }
            Ok(Target::Order)
        }
        Scope::Part | Scope::Segment => {
            if order_ended && !exception {
                return Err(not_applicable(NotApplicableReason::Ended));
            }
            let Some(index) = part_index else {
                return Err(not_applicable(NotApplicableReason::UnknownSubject));
            };
            if is_ended(order.parts[index].state) && !exception {
                return Err(not_applicable(NotApplicableReason::Ended));
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
            Ok(Target::Part { index, used })
        }
    }
}

/// Whether the input is a complete part's starting segment proven undone, the one change an ended part takes
/// (`REQ-STM-6`), and only in an order that is complete: a handed-on or failed order, or one still open, does not
/// change. The caller checks the order.
pub fn undoes_a_complete_part(part: &Part, input: &RecordedInput) -> bool {
    part.state == OrderState::Complete && undoes_starting_segment(part, input)
}

fn undoes_starting_segment(part: &Part, input: &RecordedInput) -> bool {
    matches!(input.observed, Observed::ReversalCompleted { .. })
        && part.segments.first().is_some_and(|first| {
            first.state == SegmentState::Settled
                && input.subject.segment.as_ref() == Some(&first.segment_id)
        })
}

/// `end_requested`: each part that hasn't passed its first segment ends without completing (`REQ-ORD-12`). The
/// parts past it go on.
pub fn after_end_requested(order: &mut Order) -> Outcome {
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

/// A part's state by whether its first segment settled.
pub fn by_first_segment(part: &Part) -> OrderState {
    if part.segments[0].state == SegmentState::Settled {
        OrderState::FirstSegmentSettled
    } else {
        OrderState::NotPastFirstSegment
    }
}

/// `stuck_declared`: a part not stuck already becomes stuck; over time or in dispute, time arriving as a recorded
/// statement, never a clock (`REQ-ORD-29`, `REQ-STM-5`).
pub fn after_stuck_declared(part: &mut Part) -> Outcome {
    if part.state == OrderState::Stuck {
        not_from_this_state()
    } else {
        part.state = OrderState::Stuck;
        Outcome::Applied {}
    }
}

/// `withdrawal_cancelled`: a part whose withdrawal hasn't settled ends without completing (`REQ-ORD-40`).
pub fn after_withdrawal_cancelled(part: &mut Part) -> Outcome {
    let withdrawing = part.segments[0].kind == OperationKind::Withdrawal
        && part.segments[0].state != SegmentState::Settled;
    if part.state == OrderState::NotPastFirstSegment && withdrawing {
        part.state = OrderState::EndedWithoutCompleting;
        Outcome::Applied {}
    } else {
        not_from_this_state()
    }
}

/// `handed_on`: a part waiting or stuck, whose value sits at a node, is handed on to another order
/// (`REQ-ORD-43`).
pub fn after_handed_on(part: &mut Part, order_id: &Id, linked_order: &Id) -> Outcome {
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

/// A segment's input on its part (`REQ-STM-1`), along the published edges and no others (the table in the module's
/// docs): the segment moves, and the part's state follows. A report about a retired segment is `late`; one about a
/// segment the part never had is not applicable. A deposit has no reversal: a failed deposit leaves the value where
/// it was (`INV-2`, `REQ-ORD-43`).
pub fn after_segment_input(part: &mut Part, input: &RecordedInput) -> Outcome {
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
            if undoes_a_complete_part(part, input) =>
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
pub fn after_restart(
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

/// The transition (`REQ-STM-3`): `input` on `order`, dispatched to the rule of its kind, and the order [`finish`]
/// makes of it. `CMP-STM`'s `StateMachine::transition` is this, and core-internal's reading of a part's standing
/// steps an order by it, so the two dispatch alike.
pub fn transition(order: &Order, input: &RecordedInput) -> Order {
    let mut next = order.clone();
    let outcome = apply(&mut next, input);
    finish(order, next, input, outcome)
}

/// Dispatches `input` to the published rule of its kind.
fn apply(order: &mut Order, input: &RecordedInput) -> Outcome {
    let (index, used) = match guard(order, input) {
        Err(outcome) => return outcome,
        Ok(Target::Order) => {
            return match &input.observed {
                Observed::EndRequested {} => after_end_requested(order),
                _ => not_from_this_state(),
            };
        }
        Ok(Target::Part { index, used }) => (index, used),
    };
    let order_id = order.order_id.clone();
    let before = order.parts[index].clone();
    let part = &mut order.parts[index];
    let outcome = match &input.observed {
        Observed::ProviderBound {}
        | Observed::SegmentSettled { .. }
        | Observed::SegmentFailed {}
        | Observed::SegmentOutcomeUnknown {}
        | Observed::ReversalCompleted { .. } => after_segment_input(part, input),
        Observed::RouteRecalculated { segments, .. } => {
            after_restart(part, &used, input, segments, false)
        }
        Observed::DestinationChanged { segments, .. } => {
            after_restart(part, &used, input, segments, true)
        }
        Observed::NoRoute { .. } => match after_no_route(part) {
            Some(state) if state != part.state => {
                part.state = state;
                Outcome::Applied {}
            }
            _ => not_from_this_state(),
        },
        Observed::WaitingAtFloor {} => match after_waiting_at_floor(part) {
            Some(state) => {
                part.state = state;
                Outcome::Applied {}
            }
            None => not_from_this_state(),
        },
        Observed::StuckResolved { to, .. } => match after_stuck_resolved(part, *to) {
            Some(state) => {
                part.state = state;
                part.resolved_to = Some(*to);
                Outcome::Applied {}
            }
            None => not_from_this_state(),
        },
        Observed::StuckDeclared { .. } => after_stuck_declared(part),
        Observed::WithdrawalCancelled {} => after_withdrawal_cancelled(part),
        Observed::HandedOn { linked_order, .. } => after_handed_on(part, &order_id, linked_order),
        // Order-scope kinds never reach a part.
        Observed::RouteChosen { .. } | Observed::EndRequested {} => not_from_this_state(),
    };
    if outcome == (Outcome::Applied {}) {
        // The published update rule: a part says since when it waits, exactly while it waits.
        part.waiting_since = waiting_since_after(&before, part.state, input);
    }
    outcome
}
