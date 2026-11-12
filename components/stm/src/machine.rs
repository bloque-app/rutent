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
//! # One source for the rules
//!
//! Every rule is the contract's, published in [`rules`](rutent_contracts::state_machine::rules): this module only
//! dispatches each input to the rule of its kind, as core-internal's reading of a part's standing does, so the two
//! can't drift apart (`T-020-01`).
//!
//! No transition reads an amount.

use rutent_contracts::state_machine::rules::{
    Target, after_end_requested, after_handed_on, after_restart, after_segment_input,
    after_stuck_declared, after_withdrawal_cancelled, finish, guard, not_from_this_state, open,
};
use rutent_contracts::state_machine::{
    Observed, Order, Outcome, RecordedInput, StateMachine, after_no_route, after_stuck_resolved,
    after_waiting_at_floor, waiting_since_after,
};

/// The state machine of rutent's orders: `open` from the input that accepted a route, `transition` on every
/// recorded input after. Pure and deterministic: no clock, no randomness, no I/O, no amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stm;

impl StateMachine for Stm {
    fn open(input: &RecordedInput) -> Option<Order> {
        open(input)
    }

    fn transition(state: &Order, input: &RecordedInput) -> Order {
        let mut next = state.clone();
        let outcome = apply(&mut next, input);
        finish(state, next, input, outcome)
    }
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
