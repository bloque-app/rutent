//! The route Core takes in, refused unless it is one it may run (`REQ-ORD-16`, `REQ-ORD-36`, `REQ-ORD-37`).
//!
//! A route reaches Core as received: one part of a split, an ordered list of typed operations. Core checks it on
//! its own terms before it uses it, whatever produced it, so a rule here doesn't rest on a contract type having
//! checked it on the way in. Each rule has one owning check, run in this order:
//!
//! 1. **Typed and ordered** (`REQ-ORD-37`): a list, not empty, of operations each of the four kinds, withdrawal,
//!    route, exchange or deposit, with its two ends. Each starts where the one before it ended, node, asset and
//!    unit, so the list sets the order.
//! 2. **An asset changes only at an exchange** (`REQ-ORD-36`): an operation of any other kind keeps its asset, by
//!    its id. Its unit may change: a ramp or reserve counts it in its own decimals at each end (`REQ-CAT-11`).
//! 3. **From a reserve, to a reserve** (`REQ-ORD-16`): the first operation is a withdrawal from an outer node into an
//!    inner one, and the last a deposit from an inner node out to an outer one. It may start and end at one reserve.
//!
//! The node rules of each kind ([`check_typed_operation`]) follow. A route handed on under `REQ-ORD-43` starts
//! instead from the holding account where the value sits; that start is `T-020-13`'s, and this guard refuses it.
//!
//! A part's route recalculated from where its value is evidenced (`REQ-ORD-2`, `T-020-10`) starts at that inner node,
//! not with a withdrawal: [`check_recalculated_part`] holds it to every rule above but the start, which it replaces
//! with "from an inner node, by no withdrawal" (rule 3 still ends it with a deposit to an outer node). Added by
//! `T-020-10`, the one change to this module outside its owning row.
//!
//! A refused route is recorded as an `order_refusal` ([`record_route_refusal`]), of the class its source gives: a
//! route Core received breaks the deployment's rules for one (`malformed_request`); a part the router chose that
//! Core can't run leaves the order with no route right now (`no_route_right_now`).

use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::core_internal::{
    Actor, Entry, OrderRefusalClass, Outcome, Receipt, TimeSource, VerifiedEntry, VerifiedKind,
    What, When,
};
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::{
    Endpoint, Id, OperationKind, Part, Violation, check_typed_operation,
};
use serde::Deserialize;

use super::clock::Clock;
use super::journal::{self, OrchestrationLog};
use serde_json::Value;

/// One operation of a route, as Core reads it: its kind and its two ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The kind.
    pub kind: OperationKind,
    /// Where the value starts.
    pub from: Endpoint,
    /// Where it ends.
    pub to: Endpoint,
}

/// Why a route was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteRefusal {
    /// It isn't a list of operations, or the list is empty (`REQ-ORD-37`).
    NotAList,
    /// The operation at this place isn't typed: no kind of the four, or no two ends (`REQ-ORD-37`).
    NotTyped(usize),
    /// The operation at this place doesn't start where the one before it ended (`REQ-ORD-37`).
    OutOfOrder(usize),
    /// The operation at this place changes asset and isn't an exchange (`REQ-ORD-36`).
    AssetChangeOutsideExchange(usize),
    /// The route doesn't start with a withdrawal from a reserve, from an outer node to an inner one
    /// (`REQ-ORD-16`).
    NoWithdrawalFirst,
    /// The route doesn't end with a deposit into a reserve, from an inner node to an outer one (`REQ-ORD-16`).
    NoDepositLast,
    /// A recalculated route doesn't start at an inner node, where value is evidenced, by something other than a
    /// withdrawal (`REQ-ORD-2`, `INV-4`).
    NotFromAnInnerNode,
    /// The operation at this place breaks its kind's node rules.
    Malformed(usize, Violation),
    /// The operation at this place is at a provider whose declared evidence is below the floor the operator sets
    /// on its tuple now, or on a tuple with no floor: it isn't routed through (`REQ-ORD-39`, `T-020-09`).
    BelowEvidenceFloor(usize),
    /// The operation at this place leaves the value at an inner node with no holding account: between segments
    /// value rests only in a node's holding account (`INV-5`, `REQ-CAT-23`, `T-020-09`).
    NoHoldingAccount(usize),
    /// The operation at this place is at a provider the catalogue doesn't list now (`T-020-09`).
    ProviderNotListed(usize),
    /// The part's final deposit, at this place, is to a destination with no public record Core reads, which no
    /// publisher declares a private rail: nothing could close the part (`REQ-ORD-33`, `T-020-09`).
    NoPublicRecord(usize),
}

/// A refused route: why, and the class the caller is told.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a refused route is recorded with `record_route_refusal`"]
pub struct RouteRefused {
    /// Why.
    pub reason: RouteRefusal,
    /// The class: `malformed_request` for a route received, `no_route_right_now` for a part the router chose.
    pub class: OrderRefusalClass,
}

#[derive(Deserialize)]
struct Typed {
    kind: OperationKind,
    from: Endpoint,
    to: Endpoint,
}

/// Checks a route as received: one part, an ordered list of typed operations. Fields beyond an operation's kind and
/// ends, its amounts and figures, are the router's and aren't read here.
pub fn check_route(route: &Value) -> Result<Vec<Step>, RouteRefused> {
    steps_of(route).map_err(|reason| RouteRefused {
        reason,
        class: OrderRefusalClass::MalformedRequest,
    })
}

/// [`check_route`] of a part the router chose, as the list of its operations. A refusal leaves the order with no
/// route right now: the router's output is not the caller's request.
pub fn check_part(part: &Part) -> Result<Vec<Step>, RouteRefused> {
    serde_json::to_value(part.operations.as_slice())
        .map_err(|_| RouteRefusal::NotAList)
        .and_then(|route| steps_of(&route))
        .map_err(|reason| RouteRefused {
            reason,
            class: OrderRefusalClass::NoRouteRightNow,
        })
}

/// [`check_part`] of a part recalculated from where its value sits (`T-020-10`): the same rules, but the route
/// starts at an inner node by an operation other than a withdrawal, since its value is already in the book
/// (`REQ-ORD-2`, `INV-4`). A refusal leaves the order with no route right now.
pub fn check_recalculated_part(part: &Part) -> Result<Vec<Step>, RouteRefused> {
    serde_json::to_value(part.operations.as_slice())
        .map_err(|_| RouteRefusal::NotAList)
        .and_then(|route| steps_from(&route, Start::Inner))
        .map_err(|reason| RouteRefused {
            reason,
            class: OrderRefusalClass::NoRouteRightNow,
        })
}

/// Where a route starts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Start {
    /// With a withdrawal from a reserve, from an outer node into an inner one (`REQ-ORD-16`).
    Withdrawal,
    /// At an inner node, where the value already is, by anything but a withdrawal (`T-020-10`).
    Inner,
}

fn steps_of(route: &Value) -> Result<Vec<Step>, RouteRefusal> {
    steps_from(route, Start::Withdrawal)
}

fn steps_from(route: &Value, start: Start) -> Result<Vec<Step>, RouteRefusal> {
    let items = route
        .as_array()
        .filter(|i| !i.is_empty())
        .ok_or(RouteRefusal::NotAList)?;
    let mut steps = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let typed = Typed::deserialize(item).map_err(|_| RouteRefusal::NotTyped(i))?;
        steps.push(Step {
            kind: typed.kind,
            from: typed.from,
            to: typed.to,
        });
    }
    check_steps(&steps, start)?;
    Ok(steps)
}

fn check_steps(steps: &[Step], start: Start) -> Result<(), RouteRefusal> {
    for (i, pair) in steps.windows(2).enumerate() {
        if pair[0].to != pair[1].from {
            return Err(RouteRefusal::OutOfOrder(i + 1));
        }
    }
    for (i, step) in steps.iter().enumerate() {
        if step.kind != OperationKind::Exchange && step.from.asset.id != step.to.asset.id {
            return Err(RouteRefusal::AssetChangeOutsideExchange(i));
        }
    }
    let outer = |node: &NodeRef| matches!(node, NodeRef::Outer(_));
    let first = steps.first().ok_or(RouteRefusal::NotAList)?;
    match start {
        Start::Withdrawal => {
            if first.kind != OperationKind::Withdrawal
                || !outer(&first.from.node)
                || outer(&first.to.node)
            {
                return Err(RouteRefusal::NoWithdrawalFirst);
            }
        }
        Start::Inner => {
            if first.kind == OperationKind::Withdrawal || outer(&first.from.node) {
                return Err(RouteRefusal::NotFromAnInnerNode);
            }
        }
    }
    let last = steps.last().ok_or(RouteRefusal::NotAList)?;
    if last.kind != OperationKind::Deposit || outer(&last.from.node) || !outer(&last.to.node) {
        return Err(RouteRefusal::NoDepositLast);
    }
    for (i, step) in steps.iter().enumerate() {
        check_typed_operation(step.kind, &step.from, &step.to)
            .map_err(|v| RouteRefusal::Malformed(i, v))?;
    }
    Ok(())
}

/// Records a refused route as an `order_refusal` with its class (`REQ-ORD-18`), by `who`, stamped by Core's clock.
/// `order` is absent where no order exists.
pub fn record_route_refusal(
    log: &mut dyn OrchestrationLog,
    clock: &dyn Clock,
    who: Actor,
    request: &Id,
    order: Option<&Id>,
    refused: &RouteRefused,
) -> Result<Receipt, AppendFailure> {
    let at = clock.now().timestamp().ok_or(AppendFailure::Refused(
        rutent_contracts::core_internal::Violation(
            "Core's clock reads a time a timestamp can hold",
        ),
    ))?;
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who,
        what: What::OrderRefusal {
            request: request.clone(),
            order: order.cloned(),
            class: refused.class,
        },
        when: When {
            at,
            source: TimeSource::CoreClock,
        },
        outcome: Outcome::Observed {},
    });
    entry.check().map_err(AppendFailure::Refused)?;
    journal::append(log, &entry)
}
