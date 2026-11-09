//! One destination per route, as the assumption it is (`DEC-13`, ADR-0007).

use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::state_machine::{Destination, OperationKind, SegmentPlan};

/// How a route breaks the one-destination assumption that movement-to-segment matching rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationViolation {
    /// No segment ends at an outer node.
    NoDestination,
    /// More than one segment ends at an outer node.
    MoreThanOneDestination,
    /// The one outer end is not the route's last segment, a deposit.
    DestinationNotLast,
}

/// The route's destination, when exactly one segment ends at an outer node and that is its last, a deposit.
/// A route that breaks this is reported, never matched by amount instead.
pub fn one_destination(route: &[SegmentPlan]) -> Result<Destination, DestinationViolation> {
    let outer_ends = route
        .iter()
        .filter(|s| matches!(s.to.node, NodeRef::Outer(_)))
        .count();
    match outer_ends {
        0 => Err(DestinationViolation::NoDestination),
        1 => match route.last() {
            Some(last) if last.kind == OperationKind::Deposit => {
                Destination::of(route).ok_or(DestinationViolation::DestinationNotLast)
            }
            _ => Err(DestinationViolation::DestinationNotLast),
        },
        _ => Err(DestinationViolation::MoreThanOneDestination),
    }
}
