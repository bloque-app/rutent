//! The start rule across two orders (`REQ-ORD-43`, `INV-4`; `F-005`'s notes, finding 4).
//!
//! One order's check can't see this rule, so it is a function Core calls with both orders: a route handed on
//! starts at the inner node where the first order's value sits, which is where its final deposit failed.

use rutent_contracts::state_machine::{Order, OrderState, Part, value_at};

/// Why a handed-on order's route is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandOnRefusal {
    /// The two orders don't point to each other: the second doesn't name the first as `handed_on_from`, or is the
    /// first itself.
    NotLinked,
    /// No part of the first order holds value to hand on: none is waiting for the customer or stuck with its
    /// value at an inner node, and none was handed on to the second order.
    NoValueToHandOn,
    /// A part of the second order starts anywhere but where the first order's value sits (`INV-4`).
    StartsAwayFromTheValue,
}

/// Whether the first order's part holds value for the second order: waiting for the customer, or stuck (the
/// operator may hand a stuck order on, `REQ-ORD-43`), or already handed on to this very order.
fn holds_value_for(part: &Part, second: &Order) -> bool {
    let for_second = match part.state {
        OrderState::WaitingForCustomer | OrderState::Stuck => true,
        OrderState::HandedOn => part.linked_order.as_ref() == Some(&second.order_id),
        _ => false,
    };
    for_second && value_at(part).is_some()
}

/// The start rule across two orders: the route of `second`, handed on from `first`, starts where the value of
/// one of `first`'s parts sits.
///
/// - `second` names `first` as `handed_on_from`, and is another order than `first`.
/// - A part of `first` holds value for `second`, by [`value_at`]: it waits for the customer or is stuck with its
///   value at an inner node, or it was handed on to `second`. Where it failed its final deposit, that node is
///   `deposit_failed_at`, which [`value_at`] reads from the segments, and the part's own check keeps equal.
/// - Every part of `second` starts at that endpoint: the same inner node, in the same unit.
///
/// Both orders' own checks hold the rest: that a route handed on starts at an inner node with no withdrawal.
/// Which amount the route moves is `F-020`'s.
pub fn handed_on_route_starts_where_value_sits(
    first: &Order,
    second: &Order,
) -> Result<(), HandOnRefusal> {
    if second.handed_on_from.as_ref() != Some(&first.order_id) || second.order_id == first.order_id
    {
        return Err(HandOnRefusal::NotLinked);
    }
    let holders: alloc::vec::Vec<_> = first
        .parts
        .iter()
        .filter(|p| holds_value_for(p, second))
        .filter_map(value_at)
        .collect();
    if holders.is_empty() {
        return Err(HandOnRefusal::NoValueToHandOn);
    }
    for part in &second.parts {
        let starts_at = part.segments.first().map(|s| &s.from);
        if !holders.iter().any(|at| Some(*at) == starts_at) {
            return Err(HandOnRefusal::StartsAwayFromTheValue);
        }
    }
    Ok(())
}
