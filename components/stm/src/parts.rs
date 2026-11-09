//! How an order's state follows from its parts' states (`REQ-ORD-42`, `REQ-STM-1`, `REQ-STM-6`).
//!
//! Each part is a route that ends by itself, without changing any other part, and the order closes only when
//! every part has ended. The rule is a function of the parts' states alone, so the order has exactly one state at
//! every moment, and an order of one part is in its part's state.

use rutent_contracts::state_machine::{OrderState, Part};

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
