//! Recalculation's rules, as pure functions over the order Core follows and the router's record, so each can be
//! held to its invariant by a property test (`INV-4`, `INV-10`).

use rutent_contracts::core_internal::{Authorisation, Destination, RouteEnd};
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::{
    Amount, Endpoint, FeeFormula, Operation, OperationKind, Part as RoutePart, PositiveAmount,
};
use rutent_contracts::state_machine::{
    self as stm, Observed, OrderState, RecordedInput, SegmentPlan, SegmentState,
};
use rutent_router::quote::received_after_operator_fee;

/// Why a part's route isn't recalculated now: its value is evidenced at no node (`REQ-ORD-2`, `INV-4`). Nothing is
/// asked of anyone and nothing is recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRecalculable {
    /// Core follows no such order, or the order has no such part.
    Unknown,
    /// The part has ended: complete, ended without completing, failed or handed on.
    Ended(OrderState),
    /// The part waits for the customer or is stuck: only the customer's choice, or the operator's resolution under
    /// `REQ-AUZ-4`, moves it now (`REQ-ORD-27`, `REQ-ORD-28`, `REQ-ORD-43`).
    Held(OrderState),
    /// The part's withdrawal hasn't settled: nothing has moved, and its value is outside the book (`REQ-ORD-40`).
    NotWithdrawn,
    /// A segment holds the value in flight: started with no outcome that counts, failed with its reversal owed, or
    /// of unknown outcome. Silence, a stopped advertisement and a stale version leave it so: none of them is
    /// evidence of where the value is (`REQ-ORD-2`, `THR-12`).
    InFlight {
        /// The segment.
        segment: stm::Id,
        /// Its state.
        state: SegmentState,
    },
    /// A segment of the part was handed over and has no final outcome: its effect may be in the log, its value
    /// about to move, though the state machine still has it `pending` (`INV-4`).
    HandedOver {
        /// The segment.
        segment: stm::Id,
    },
    /// The value sits at a node no settled segment of the part ends at, so nothing evidences it there
    /// (`recalculation-starts-from-evidenced-value`).
    NoSettledEnd,
}

/// Where `part`'s value is evidenced: the endpoint the state machine's `value_at` places it at, and the settled
/// segment of the part that ended there, whose evidence the recalculation rests on (`REQ-ORD-2`, `INV-4`,
/// core-internal's `recalculation-starts-from-evidenced-value`).
///
/// The value is evidenced only at the start of the part's first segment that hasn't settled, where that segment
/// hasn't started, has been reversed in full, or is a deposit that failed (a failed deposit leaves the value where it
/// was, `INV-2`), and the segment before it settled there. A segment started, failed with a reversal owed, or of
/// unknown outcome holds it in flight; before the withdrawal settles it isn't in the book.
pub fn evidenced(part: &stm::Part) -> Result<(&Endpoint, &stm::Segment), NotRecalculable> {
    match part.state {
        OrderState::Complete
        | OrderState::EndedWithoutCompleting
        | OrderState::Failed
        | OrderState::HandedOn => return Err(NotRecalculable::Ended(part.state)),
        OrderState::WaitingForCustomer | OrderState::Stuck => {
            return Err(NotRecalculable::Held(part.state));
        }
        OrderState::NotPastFirstSegment | OrderState::FirstSegmentSettled => {}
    }
    let index = part
        .segments
        .iter()
        .position(|s| s.state != SegmentState::Settled)
        .ok_or(NotRecalculable::Ended(OrderState::Complete))?;
    let next = &part.segments[index];
    let in_flight = match next.state {
        SegmentState::Started | SegmentState::OutcomeUnknown => true,
        SegmentState::FailedReversalOwed => next.kind != OperationKind::Deposit,
        SegmentState::Pending | SegmentState::ReversalComplete | SegmentState::Settled => false,
    };
    if in_flight {
        return Err(NotRecalculable::InFlight {
            segment: next.segment_id.clone(),
            state: next.state,
        });
    }
    if index == 0 {
        return Err(match next.from.node {
            NodeRef::Outer(_) => NotRecalculable::NotWithdrawn,
            NodeRef::Inner(_) => NotRecalculable::NoSettledEnd,
        });
    }
    let settled = &part.segments[index - 1];
    let at = stm::value_at(part).ok_or(NotRecalculable::NoSettledEnd)?;
    if settled.to != *at || next.from != *at {
        return Err(NotRecalculable::NoSettledEnd);
    }
    Ok((at, settled))
}

/// Why Core refuses the route the router chose for a recalculation, before anything is recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// The router's choice has more parts than one: a part is recalculated as one route.
    Split,
    /// It doesn't start where the value is evidenced (`REQ-ORD-2`, `INV-4`).
    StartsElsewhere,
    /// It doesn't move the whole of what is evidenced there: the failed provider covers only the return to its
    /// segment's origin, and nothing of the value is left behind (`REQ-ORD-22`, `INV-2`).
    MovesAnotherAmount,
    /// It ends at another destination than the authorisation names (`INV-10`, `REQ-AUZ-3`).
    ChangesDestination,
    /// It delivers another asset than the authorisation names (`INV-10`, `REQ-AUZ-3`).
    ChangesToAsset,
    /// What it leaves the customer after the operator's fee is below the floor the authorisation names
    /// (`REQ-ORD-4`, `REQ-AUZ-3`).
    BelowFloor,
    /// The state machine wouldn't take it from the part as it stands.
    NotApplicable(stm::Outcome),
}

/// Holds the route the router chose to recalculate a part from `at`, moving `amount`, to `authorisation`: one part,
/// starting where the value is evidenced and moving all of it, ending in a deposit at the authorisation's
/// destination reserve to its node, in its to asset, and leaving the customer at least its floor after
/// `operator_fee`, the record's (`REQ-ORD-2`, `REQ-ORD-4`, `REQ-ORD-22`, `INV-10`, `REQ-AUZ-3`). The route's ends are read from
/// the authorisation, never from the router's record, so a record can't move them.
pub fn check_route(
    operator_fee: &FeeFormula,
    route: &[RoutePart],
    at: &Endpoint,
    amount: &PositiveAmount,
    authorisation: &Authorisation,
) -> Result<(), Rejected> {
    let [part] = route else {
        return Err(Rejected::Split);
    };
    let operations = part.operations.as_slice();
    let (first, last) = (part.operations.first(), part.operations.last());
    if first.from != *at {
        return Err(Rejected::StartsElsewhere);
    }
    if first.amount_in != *amount {
        return Err(Rejected::MovesAnotherAmount);
    }
    let destination = &authorisation.destination;
    let keeps_destination = last.kind == OperationKind::Deposit
        && last.provider == destination.reserve
        && last.to.node == NodeRef::Outer(destination.node.clone());
    if !keeps_destination {
        return Err(Rejected::ChangesDestination);
    }
    if last.to.asset != authorisation.to_asset {
        return Err(Rejected::ChangesToAsset);
    }
    debug_assert!(!operations.is_empty());
    if let Some(floor) = &authorisation.floor {
        let left = received_after_operator_fee(operator_fee, &last.amount_out);
        if left < *floor.amount() {
            return Err(Rejected::BelowFloor);
        }
    }
    Ok(())
}

/// What the customer is left with after `operator_fee` if `arrived` sits at the end of the operation at `after` of
/// `operations`, a part's route as the router priced it, by the quotes still ahead (`REQ-ORD-27`, `AC-42`). `None`
/// when an amount is past what Core counts in: a check that can't be made fails closed.
///
/// Each operation still ahead is read from its quote as the router priced it: one that keeps its asset and unit
/// takes off what its quote charged, never below nothing; one that converts, or counts in another unit, gives out in
/// the proportion its quote gave out, rounded down. The operator's fee comes off what arrives at the destination,
/// by `quote::received_after_operator_fee`, the function the router judges the floor with, so the two agree.
pub fn left_after(
    operations: &[Operation],
    operator_fee: &FeeFormula,
    after: usize,
    arrived: &PositiveAmount,
) -> Option<Amount> {
    let mut amount = whole(arrived.amount().as_str())?;
    for operation in operations.iter().skip(after + 1) {
        let quoted_in = whole(operation.amount_in.amount().as_str())?;
        let quoted_out = whole(operation.amount_out.as_str())?;
        amount = if operation.from.asset == operation.to.asset {
            amount.saturating_sub(quoted_in.saturating_sub(quoted_out))
        } else {
            amount.checked_mul(quoted_out)?.checked_div(quoted_in)?
        };
    }
    let arrives = Amount::new(amount.to_string())?;
    Some(received_after_operator_fee(operator_fee, &arrives))
}

/// What each move on a recalculated route ends at, read from the route itself, never from the authorisation: the
/// reserve and outer node of its last deposit, the asset it delivers there, and what it leaves the customer after
/// `operator_fee`; only the account, which a route doesn't name, is the authorisation's. So the log's
/// `move-rests-on-its-authorisation` holds the route's own ends against the authorisation, a guard after
/// [`check_route`] and the state machine's (`REQ-AUZ-3`, `INV-10`). `None` where the route ends in no deposit to an
/// outer node.
pub fn route_end(
    route: &RoutePart,
    operator_fee: &FeeFormula,
    authorisation: &Authorisation,
) -> Option<RouteEnd> {
    let last = route.operations.last();
    let NodeRef::Outer(node) = &last.to.node else {
        return None;
    };
    (last.kind == OperationKind::Deposit).then(|| RouteEnd {
        destination: Destination {
            reserve: last.provider.clone(),
            node: node.clone(),
            account: authorisation.destination.account.clone(),
        },
        to_asset: last.to.asset.clone(),
        delivers: received_after_operator_fee(operator_fee, &last.amount_out),
    })
}

/// Whether what `left` leaves the customer holds its floor; `None` left, a check that couldn't be made, doesn't.
pub fn holds_floor(left: Option<&Amount>, floor: Option<&PositiveAmount>) -> bool {
    match (left, floor) {
        (_, None) => true,
        (Some(left), Some(floor)) => left >= floor.amount(),
        (None, Some(_)) => false,
    }
}

/// A provider a recalculated route passes again without the value having moved on since it had it: the value sits
/// at the node that provider's earlier segment started from, and that segment never settled (`REQ-ORD-4`,
/// `THR-28`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassedAgain {
    /// The provider.
    pub provider: stm::Id,
    /// Its earlier segments of the part, handed over from that node, that never settled.
    pub before: Vec<stm::Id>,
}

/// The providers `route`, a recalculation of `part` of `order` starting at `at`, passes again without progress, read
/// from the order's recorded inputs alone: its plans (`route_chosen`, `route_recalculated`), which segments were
/// handed over (`provider_bound`) and which settled (`segment_settled`). So it is visible from the log, not only
/// from Core's memory (`REQ-ORD-4`).
pub fn passed_again(
    inputs: &[RecordedInput],
    part: &stm::Id,
    at: &Endpoint,
    route: &[SegmentPlan],
) -> Vec<PassedAgain> {
    let about_part = |i: &&RecordedInput| i.subject.part.as_ref() == Some(part);
    let mut plans: Vec<&SegmentPlan> = Vec::new();
    for input in inputs {
        match &input.observed {
            Observed::RouteChosen { parts, .. } => plans.extend(
                parts
                    .iter()
                    .filter(|p| &p.part_id == part)
                    .flat_map(|p| &p.segments),
            ),
            Observed::RouteRecalculated { segments, .. }
            | Observed::DestinationChanged { segments, .. }
                if input.subject.part.as_ref() == Some(part) =>
            {
                plans.extend(segments)
            }
            _ => {}
        }
    }
    let marked = |kind: fn(&Observed) -> bool, segment: &stm::Id| {
        inputs
            .iter()
            .filter(about_part)
            .any(|i| i.subject.segment.as_ref() == Some(segment) && kind(&i.observed))
    };
    let mut passed: Vec<PassedAgain> = Vec::new();
    for again in route {
        let before: Vec<stm::Id> = plans
            .iter()
            .filter(|p| p.provider == again.provider && p.from == *at)
            .filter(|p| marked(|o| matches!(o, Observed::ProviderBound {}), &p.segment_id))
            .filter(|p| {
                !marked(
                    |o| matches!(o, Observed::SegmentSettled { .. }),
                    &p.segment_id,
                )
            })
            .map(|p| p.segment_id.clone())
            .collect();
        if before.is_empty() || passed.iter().any(|p| p.provider == again.provider) {
            continue;
        }
        passed.push(PassedAgain {
            provider: again.provider.clone(),
            before,
        });
    }
    passed
}

/// A decimal amount Core counts in, or `None` past it.
fn whole(text: &str) -> Option<u128> {
    text.parse().ok()
}
