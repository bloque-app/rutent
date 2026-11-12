//! A part's standing as this log's inputs show it: the one reading the citation table's part rows share, so each row
//! reads where a part stands the same way (`T-020-01`, `T-020-10`, `T-020-11`).
//!
//! The reading is the state machine's own: each of the order's inputs goes, in the log's order, through the
//! transition rules the state-machine contract publishes ([`rules`](crate::state_machine::rules)), dispatched by
//! kind as `CMP-STM` dispatches them. An input those rules don't take (one `CMP-STM` would answer not applicable)
//! is one the log refuses ([`state_machine_takes`]), so an admitted log holds only inputs the state machine took,
//! and what a row reads of a part is what the state machine holds. The differential oracle in `CMP-STM`'s tests
//! holds the two to each other. A stuck resolution's move counts once its result is `done`, which the state machine
//! doesn't see.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};

use super::relations::History;
use super::types::{Action, Entry, Outcome as EntryOutcome, VerifiedEntry, What};
use crate::state_machine::rules::{
    Target, after_end_requested, after_handed_on, after_restart, after_segment_input,
    after_stuck_declared, after_withdrawal_cancelled, finish, guard, not_from_this_state, open,
};
use crate::state_machine::{
    Observed, OperationKind, Order, OrderState, Outcome, Part, RecordedInput, SegmentState,
    after_no_route, after_stuck_resolved, after_waiting_at_floor, waiting_since_after,
};

/// Where a part stands after a history.
pub(crate) struct Standing {
    /// The part, as the state machine holds it.
    pub part: Part,
    /// The index in the history of the entry that put the part where it stands: the input that moved it to its
    /// state, or the `done` result of its stuck resolution.
    pub since: usize,
    /// Whether a stuck resolution's move for the part has a `done` result: the part is resolved.
    pub resolved: bool,
    /// Every segment the part ever had, retired ones included, with its kind and its last state.
    pub segments: BTreeMap<String, (OperationKind, SegmentState)>,
}

impl Standing {
    /// Whether the part has ended (`REQ-STM-6`), or was resolved while stuck, its value moved on its resolution.
    pub fn ended(&self) -> bool {
        self.resolved || crate::state_machine::rules::is_ended(self.part.state)
    }

    /// Whether the part is held: waiting for the customer or stuck, where no provider starts on it.
    pub fn held(&self) -> bool {
        matches!(
            self.part.state,
            OrderState::WaitingForCustomer | OrderState::Stuck
        )
    }

    /// Whether the part's value has left `segment`: it settled, was reversed, or is a deposit that failed, which
    /// leaves the value where it was (`INV-2`, `REQ-ORD-43`).
    pub fn released(&self, segment: &str) -> bool {
        matches!(
            self.segments.get(segment),
            Some((_, SegmentState::Settled | SegmentState::ReversalComplete))
                | Some((OperationKind::Deposit, SegmentState::FailedReversalOwed))
        )
    }
}

fn verified(entry: &Entry) -> Option<&VerifiedEntry> {
    match entry {
        Entry::Verified(v) => Some(v),
        Entry::Stated(_) => None,
    }
}

fn input(entry: &VerifiedEntry) -> Option<&RecordedInput> {
    match &entry.what {
        What::Input { input } => Some(input),
        _ => None,
    }
}

/// One transition: `input` on `order`, through the published rules, dispatched by its kind as `CMP-STM` does.
pub(crate) fn step(order: &Order, input: &RecordedInput) -> Order {
    let mut next = order.clone();
    let outcome = apply(&mut next, input);
    finish(order, next, input, outcome)
}

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
    let placed = |part: &mut Part, state: Option<OrderState>| match state {
        Some(state) => {
            part.state = state;
            Outcome::Applied {}
        }
        None => not_from_this_state(),
    };
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
        Observed::NoRoute { .. } => {
            let state = after_no_route(part).filter(|s| *s != part.state);
            placed(part, state)
        }
        Observed::WaitingAtFloor {} => {
            let state = after_waiting_at_floor(part);
            placed(part, state)
        }
        Observed::StuckResolved { to, .. } => {
            let state = after_stuck_resolved(part, *to);
            let outcome = placed(part, state);
            if outcome == (Outcome::Applied {}) {
                part.resolved_to = Some(*to);
            }
            outcome
        }
        Observed::StuckDeclared { .. } => after_stuck_declared(part),
        Observed::WithdrawalCancelled {} => after_withdrawal_cancelled(part),
        Observed::HandedOn { linked_order, .. } => after_handed_on(part, &order_id, linked_order),
        Observed::RouteChosen { .. } | Observed::EndRequested {} => not_from_this_state(),
    };
    if outcome == (Outcome::Applied {}) {
        part.waiting_since = waiting_since_after(&before, part.state, input);
    }
    outcome
}

/// Whether the state machine takes `outcome`: it applied the input, or recorded it late or as a repeat.
fn taken(outcome: &Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Applied {} | Outcome::Late {} | Outcome::Repeat {}
    )
}

/// The order `order` after the inputs of `history` the state machine took, with the index of each entry that
/// changed it, or `None` before its route is chosen.
fn order_after(order: &str, history: &History) -> Option<Order> {
    let mut current: Option<Order> = None;
    for (_, entry) in history {
        let Some(input) = verified(entry).and_then(input) else {
            continue;
        };
        if input.subject.order.as_str() != order {
            continue;
        }
        current = match current {
            None => open(input),
            Some(state) => {
                let next = step(&state, input);
                Some(if taken(&next.last_input.outcome) {
                    next
                } else {
                    state
                })
            }
        };
    }
    current
}

/// Whether the state machine takes `input` after `history`: the log admits a state-machine input only where it
/// does (`input-is-one-the-state-machine-takes`). An input before its order's route is recorded with no state, as
/// the host contract records it, and taken; a `route_chosen` is taken where it opens a valid order.
pub fn state_machine_takes(history: &History, input: &RecordedInput) -> bool {
    match order_after(input.subject.order.as_str(), history) {
        None => !matches!(input.observed, Observed::RouteChosen { .. }) || open(input).is_some(),
        Some(order) => match step(&order, input).last_input.outcome {
            // A repeat is the same input sent again: another input under a used record id is the host contract's
            // refusal, never a repeat.
            Outcome::Repeat {} => history.iter().any(|(_, e)| {
                matches!(verified(e).map(|v| &v.what), Some(What::Input { input: earlier })
                    if earlier.record_id == input.record_id && earlier == input)
            }),
            outcome => taken(&outcome),
        },
    }
}

/// The part of `order` a stuck decision of `history` names, by the request and decision `carries` names.
pub(crate) fn decided_part<'a>(
    order: &str,
    carries: &super::types::Decided,
    history: &'a History,
) -> Option<&'a str> {
    history.iter().find_map(|(_, e)| match &verified(e)?.what {
        What::StuckDecide { request, decision }
            if request.order.as_str() == order
                && request.request == carries.request
                && *decision == carries.decision =>
        {
            Some(request.part.as_str())
        }
        _ => None,
    })
}

/// Whether `entry` is the `done` result of a stuck resolution's move for `part` of `order`.
fn resolves(entry: &VerifiedEntry, order: &str, part: &str, history: &History) -> bool {
    let (What::Result { effect }, EntryOutcome::Done {}) = (&entry.what, &entry.outcome) else {
        return false;
    };
    history.iter().any(|(r, e)| {
        r == effect
            && verified(e).is_some_and(|v| match &v.what {
                What::Effect {
                    action:
                        Action::ResolveStuck {
                            order: o,
                            carries_out,
                            ..
                        },
                    ..
                } => o.as_str() == order && decided_part(order, carries_out, history) == Some(part),
                _ => false,
            })
    })
}

/// Where `part` of `order` stands after `history`, or `None` if no `route_chosen` of the order planned it.
pub(crate) fn standing(order: &str, part: &str, history: &History) -> Option<Standing> {
    let mut current: Option<Order> = None;
    let mut standing: Option<Standing> = None;
    for (index, (_, entry)) in history.iter().enumerate() {
        let Some(entry) = verified(entry) else {
            continue;
        };
        let Some(input) = input(entry) else {
            if let Some(s) = &mut standing
                && !s.resolved
                && resolves(entry, order, part, history)
            {
                s.resolved = true;
                s.since = index;
            }
            continue;
        };
        if input.subject.order.as_str() != order {
            continue;
        }
        let next = match &current {
            None => open(input),
            Some(state) => {
                let next = step(state, input);
                if !taken(&next.last_input.outcome) {
                    continue;
                }
                Some(next)
            }
        };
        let Some(next) = next else {
            continue;
        };
        if let Some(now) = next.parts.iter().find(|p| p.part_id.as_str() == part) {
            let s = standing.get_or_insert_with(|| Standing {
                part: now.clone(),
                since: index,
                resolved: false,
                segments: BTreeMap::new(),
            });
            if s.part.state != now.state {
                s.since = index;
            }
            s.part = now.clone();
            for segment in &now.segments {
                s.segments.insert(
                    segment.segment_id.as_str().to_string(),
                    (segment.kind, segment.state),
                );
            }
        }
        current = Some(next);
    }
    standing
}
