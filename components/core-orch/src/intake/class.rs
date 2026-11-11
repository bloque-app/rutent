//! The stable class a caller is told when an order can't proceed (`REQ-ORD-18`). Every class is read from what Core
//! recorded, the router's record or policy's answer, never from a provider's text: a provider's own words stay in
//! its stated entry and reach no caller.

use rutent_contracts::core_internal::{OrderRefusalClass, RefusalClass};
use rutent_contracts::router_api::{DecisionRecord, DropReason};

use crate::rounds::{NotOffered, Routed};

/// Whether a candidate was dropped for its amount: at another amount the same route may run. Every other reason,
/// a refusal, a silence, a stale or voided figure, a ceiling or a policy's exclusion, is about now, not the amount.
/// A reason router-api adds later counts as now until it is named here (`DropReason` is non-exhaustive).
fn for_the_amount(reason: &DropReason) -> bool {
    matches!(
        reason,
        DropReason::OutsideBand { .. }
            | DropReason::BeyondLevels { .. }
            | DropReason::FeeAboveAmount { .. }
            | DropReason::NothingDelivered { .. }
            | DropReason::NoPositiveOutcome {}
            | DropReason::BelowFloor {}
    )
}

/// The record that decided there is no route: the last choice round's, or the shortlist's where no choice round
/// ran.
fn deciding(routed: &Routed) -> &DecisionRecord {
    routed
        .choices
        .last()
        .map_or(&routed.shortlist.record, |c| &c.record)
}

/// "No route for this amount" where the deciding record considered at least one candidate and dropped every one
/// for its amount, and "no route right now" otherwise: a record with no candidate at all, or one dropping any
/// candidate for a reason that may pass.
pub(super) fn no_route(routed: &Routed) -> OrderRefusalClass {
    let record = deciding(routed);
    let candidates = record.candidates();
    let all_for_amount = !candidates.is_empty()
        && candidates
            .iter()
            .all(|c| c.dropped.as_ref().is_some_and(for_the_amount));
    if all_for_amount {
        OrderRefusalClass::NoRouteForAmount
    } else {
        OrderRefusalClass::NoRouteRightNow
    }
}

/// The class a routing that offered nothing gives its caller.
pub(super) fn not_offered(routed: &Routed, why: &NotOffered) -> OrderRefusalClass {
    match why {
        NotOffered::NoRoute => no_route(routed),
        NotOffered::OverBound { .. } => OrderRefusalClass::OverBound,
        // Policy's bound on who is asked about a person for the order, not the caller's party's bound.
        NotOffered::PersonQuestionsPastBound => OrderRefusalClass::RefusedByPolicy,
        // The reserve didn't say yes now; the same request may get one later.
        NotOffered::NoReservesYes => OrderRefusalClass::NoRouteRightNow,
        // A judgement the route needs isn't cited; a verifier silent now may answer later.
        NotOffered::NotEligible { .. } => OrderRefusalClass::NoRouteRightNow,
    }
}

/// The class a policy refusal of the order's authorisation gives its caller.
pub(super) fn refused(class: RefusalClass) -> OrderRefusalClass {
    match class {
        RefusalClass::OverBound => OrderRefusalClass::OverBound,
        RefusalClass::NotPermitted
        | RefusalClass::MissingInput
        | RefusalClass::OutsideAuthorisation
        | RefusalClass::NoAccountableParty
        | RefusalClass::SameActor => OrderRefusalClass::RefusedByPolicy,
    }
}
