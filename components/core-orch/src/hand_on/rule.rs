//! The hand-on's rules, as pure functions over the order Core follows, so each can be held to its invariant by a
//! test of its own (`REQ-ORD-43`, `REQ-ORD-40`, `REQ-ORD-13`, `INV-4`, `INV-10`).

use rutent_contracts::core_internal::{Authorisation, Entry, Role, What};
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::{Amount, Id};
use rutent_contracts::state_machine::{self as stm, OrderState, SegmentPlan, SegmentState};
use rutent_prov::holding::wallet::WithdrawalRecord;

/// Why Core refuses to resolve a part waiting for the customer, before anything is asked or recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Core follows no such order, or the order has no such part.
    Unknown,
    /// The part doesn't wait for the customer: only a part a failed final deposit left with no route to its
    /// destination waits, after recalculation to that destination came first (`REQ-ORD-43`); a stuck part is the
    /// operator's, under `REQ-AUZ-4`.
    NotWaiting(OrderState),
    /// The part waits, but not at a final deposit that failed: only the customer's choice there changes the
    /// destination or the to asset (`INV-10`).
    NoFailedDeposit,
    /// The new terms aren't the order's customer's: only the customer sets new terms (`REQ-AUZ-3`, `INV-10`).
    NotTheCustomer,
    /// The new terms are for another order than the one in force.
    OtherOrder,
    /// The new terms are given under the id of the terms in force: an id names one authorisation.
    SameId,
    /// The new terms name nothing new: no destination, to asset or floor of their own. A restatement isn't the
    /// customer's choice, and goes to policy as any restatement does.
    NothingNew,
    /// The new terms change the origin reserve: the value was withdrawn already, and its origin stays.
    ChangesOrigin,
    /// The new terms cover a part that doesn't wait for the customer, or every part while one doesn't: a split
    /// order's other part goes on under its own terms (`T-021-04`).
    CoversAPartThatGoesOn(Id),
    /// The new route keeps the part's destination: that is a recalculation, not the customer's choice
    /// (`keeps_destination`).
    KeepsDestination,
    /// The new route starts anywhere but where the part's value sits (`INV-4`).
    StartsAwayFromTheValue,
}

/// Why a holding account's record doesn't prove a handed-on route's start (`REQ-ORD-40`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unproven {
    /// The record is another provider's than the route's starting reserve: the holding account's own record is the
    /// proof, and no one else's.
    NotTheStartingReserves,
    /// The record is of another node than the one the route starts at.
    AnotherNode,
    /// The record is of another asset than the route starts in.
    AnotherAsset,
    /// The record is of another order's value than the order handed on.
    AnotherOrdersValue,
    /// The record withdrew another amount than the part held: the handed-on route moves all of it.
    AnotherAmount,
    /// The record names no reference of the holding account's own.
    NoReference,
}

/// Whether `part`'s value sits where a failed final deposit left it: the part's last segment is a deposit that
/// failed, from the node the part remembers in `deposit_failed_at` (`REQ-ORD-43`).
pub fn after_failed_final_deposit(part: &stm::Part) -> bool {
    part.deposit_failed_at.is_some()
        && part.segments.last().is_some_and(|s| {
            s.kind == stm::OperationKind::Deposit && s.state == SegmentState::FailedReversalOwed
        })
}

/// The parts `authorisation` speaks for in `order`: those it names, or every part where it names none.
fn covered<'a>(order: &'a stm::Order, authorisation: &Authorisation) -> Vec<&'a stm::Part> {
    order
        .parts
        .iter()
        .filter(|p| {
            authorisation
                .parts
                .as_ref()
                .is_none_or(|named| named.iter().any(|n| n.as_str() == p.part_id.as_str()))
        })
        .collect()
}

/// Whether `new` may go to policy as the customer's new terms for `order`, whose terms in force are `current` and
/// whose customer is `requester` (the gate `T-021-04` hands to orchestration: policy holds who gives new terms,
/// not when).
///
/// - `new` is for the same order, under an id of its own, given by the order's customer acting as such;
/// - it names something new (destination, to asset or floor) and keeps the origin reserve;
/// - every part it speaks for, and at least one, is in the order and waits for the customer; one naming no part
///   speaks for every part, so every part must wait;
/// - where it changes the destination or the to asset, each of those parts waits at a final deposit that failed
///   (`INV-10`); a new floor alone may resolve any wait (`T-021-04`'s default, `REQ-ORD-27`).
pub fn new_terms(
    order: &stm::Order,
    current: &Authorisation,
    new: &Authorisation,
    requester: &Id,
) -> Result<(), Refused> {
    if new.order != current.order || new.order.as_str() != order.order_id.as_str() {
        return Err(Refused::OtherOrder);
    }
    if new.given_by.role != Role::Customer || new.given_by.party != *requester {
        return Err(Refused::NotTheCustomer);
    }
    if new.authorisation == current.authorisation {
        return Err(Refused::SameId);
    }
    if new.origin_reserve != current.origin_reserve {
        return Err(Refused::ChangesOrigin);
    }
    let moves_ends = new.destination != current.destination || new.to_asset != current.to_asset;
    if !moves_ends && new.floor == current.floor {
        return Err(Refused::NothingNew);
    }
    if let Some(named) = &new.parts
        && let Some(missing) = named
            .iter()
            .find(|n| !order.parts.iter().any(|p| p.part_id.as_str() == n.as_str()))
    {
        return Err(Refused::CoversAPartThatGoesOn(missing.clone()));
    }
    let parts = covered(order, new);
    if parts.is_empty() {
        return Err(Refused::Unknown);
    }
    for part in parts {
        if part.state != OrderState::WaitingForCustomer {
            return Err(Refused::CoversAPartThatGoesOn(
                Id::new(part.part_id.as_str()).ok_or(Refused::Unknown)?,
            ));
        }
        if moves_ends && !after_failed_final_deposit(part) {
            return Err(Refused::NoFailedDeposit);
        }
    }
    Ok(())
}

/// Whether `plan` is the customer's new route for `part`, as the state machine would take it as a
/// `destination_changed` (`REQ-ORD-43`, `INV-4`, `INV-10`): the part waits for the customer at a final deposit that
/// failed; the route ends at another destination than the part's (one that keeps it is a recalculation); and it
/// starts where the value sits, in the same unit. Whether the route ends where the customer's new authorisation
/// names is [`check_route`](crate::recalc::check_route)'s, against that authorisation.
pub fn destination_change(part: &stm::Part, plan: &[SegmentPlan]) -> Result<(), Refused> {
    if part.state != OrderState::WaitingForCustomer {
        return Err(Refused::NotWaiting(part.state));
    }
    if !after_failed_final_deposit(part) {
        return Err(Refused::NoFailedDeposit);
    }
    if !stm::destination_change_applies(part, plan) {
        return Err(Refused::KeepsDestination);
    }
    if !stm::recalculation_starts_where_value_sits(part, plan) {
        return Err(Refused::StartsAwayFromTheValue);
    }
    Ok(())
}

/// The requester the log holds for `order`: the parties of its first acceptance in `recorded`, the log's entries.
pub fn requester_of<'a>(recorded: &'a [Entry], order: &Id) -> Option<&'a Id> {
    recorded.iter().find_map(|entry| match entry {
        Entry::Verified(v) => match &v.what {
            What::Acceptance {
                order: o, parties, ..
            } if o == order => Some(&parties.requester),
            _ => None,
        },
        Entry::Stated(_) => None,
    })
}

/// What the log holds of an acceptance: its order, its requester and the authorisation it accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accepted<'a> {
    /// The order accepted.
    pub order: &'a Id,
    /// Its requester.
    pub requester: &'a Id,
    /// The authorisation it accepts.
    pub authorisation: &'a Id,
}

/// The acceptance `acceptance` in `recorded`, the log's entries.
pub fn acceptance_of<'a>(recorded: &'a [Entry], acceptance: &Id) -> Option<Accepted<'a>> {
    recorded.iter().find_map(|entry| match entry {
        Entry::Verified(v) => match &v.what {
            What::Acceptance {
                acceptance: a,
                order,
                parties,
                authorisation,
                ..
            } if a == acceptance => Some(Accepted {
                order,
                requester: &parties.requester,
                authorisation,
            }),
            _ => None,
        },
        Entry::Stated(_) => None,
    })
}

/// Whether `record`, a holding account's own record of a withdrawal, proves the start of a route handed on from
/// `from` (`REQ-ORD-40`, `INV-5`): made by `reserve`, the linked order's starting reserve, the holding account's
/// provider; at the inner node the route's first segment starts from, in its asset; of `from`'s value; and of
/// `amount`, all the part held. At a private node the record is its publisher's word (`REQ-ORD-40`), and Core
/// holds it to the same fields.
pub fn proves_start(
    record: &WithdrawalRecord,
    reserve: &Id,
    first: &SegmentPlan,
    from: &Id,
    amount: &Amount,
) -> Result<(), Unproven> {
    if record.by != reserve.as_str() {
        return Err(Unproven::NotTheStartingReserves);
    }
    match &first.from.node {
        NodeRef::Inner(node) if *node == record.node => {}
        _ => return Err(Unproven::AnotherNode),
    }
    if record.asset != first.from.asset.id {
        return Err(Unproven::AnotherAsset);
    }
    if record.order != from.as_str() {
        return Err(Unproven::AnotherOrdersValue);
    }
    if amount.as_str().parse::<u128>().ok() != Some(record.amount) {
        return Err(Unproven::AnotherAmount);
    }
    if record.reference.is_empty() {
        return Err(Unproven::NoReference);
    }
    Ok(())
}
