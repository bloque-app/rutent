//! The route Core takes in, refused unless it is one it may run (`REQ-ORD-16`, `REQ-ORD-36`, `REQ-ORD-37`).
//!
//! A route reaches Core as received: one part of a split, an ordered list of typed operations. Core checks it on
//! its own terms before it uses it, whatever produced it, so a rule here doesn't rest on a contract type having
//! checked it on the way in. Each rule has one owning check, run in this order:
//!
//! 1. **Typed and ordered** (`REQ-ORD-37`): a list, not empty, of operations each of the four kinds, withdrawal,
//!    route, exchange or deposit, with its two ends. Each starts where the one before it ended, node, asset and
//!    unit, so the list sets the order.
//! 2. **An asset changes only at an exchange** (`REQ-ORD-36`): an operation of any other kind keeps its asset.
//! 3. **From a reserve, to a reserve** (`REQ-ORD-16`): the first operation is a withdrawal from an outer node into an
//!    inner one, and the last a deposit from an inner node out to an outer one. It may start and end at one reserve.
//!
//! The node rules of each kind ([`check_typed_operation`]) follow. A route handed on under `REQ-ORD-43` starts
//! instead from the holding account where the value sits; that start is `T-020-13`'s, and this guard refuses it.
//!
//! A refused route is recorded as an `order_refusal` ([`record_route_refusal`]).

use rutent_contracts::core_internal::calls::AppendFailure;

use super::journal::{self, OrchestrationLog};
use rutent_contracts::core_internal::{
    Actor, Entry, OrderRefusalClass, Outcome, Receipt, TimeSource, VerifiedEntry, VerifiedKind,
    What, When,
};
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::{
    Endpoint, Id, OperationKind, Part, Timestamp, Violation, check_typed_operation,
};
use serde::Deserialize;
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
    /// The operation at this place breaks its kind's node rules.
    Malformed(usize, Violation),
}

impl RouteRefusal {
    /// The class the caller is told: the route breaks the deployment's rules for one.
    pub fn class(&self) -> OrderRefusalClass {
        OrderRefusalClass::MalformedRequest
    }
}

#[derive(Deserialize)]
struct Typed {
    kind: OperationKind,
    from: Endpoint,
    to: Endpoint,
}

/// Checks a route as received: one part, an ordered list of typed operations. Fields beyond an operation's kind and
/// ends, its amounts and figures, are the router's and aren't read here.
pub fn check_route(route: &Value) -> Result<Vec<Step>, RouteRefusal> {
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
    check_steps(&steps)?;
    Ok(steps)
}

/// [`check_route`] of a part the router chose, as the list of its operations.
pub fn check_part(part: &Part) -> Result<Vec<Step>, RouteRefusal> {
    let route =
        serde_json::to_value(part.operations.as_slice()).map_err(|_| RouteRefusal::NotAList)?;
    check_route(&route)
}

fn check_steps(steps: &[Step]) -> Result<(), RouteRefusal> {
    for (i, pair) in steps.windows(2).enumerate() {
        if pair[0].to != pair[1].from {
            return Err(RouteRefusal::OutOfOrder(i + 1));
        }
    }
    for (i, step) in steps.iter().enumerate() {
        if step.kind != OperationKind::Exchange && step.from.asset != step.to.asset {
            return Err(RouteRefusal::AssetChangeOutsideExchange(i));
        }
    }
    let outer = |node: &NodeRef| matches!(node, NodeRef::Outer(_));
    let first = steps.first().ok_or(RouteRefusal::NotAList)?;
    if first.kind != OperationKind::Withdrawal || !outer(&first.from.node) || outer(&first.to.node)
    {
        return Err(RouteRefusal::NoWithdrawalFirst);
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

/// Records a refused route as an `order_refusal` with its class (`REQ-ORD-18`), by `who`, at `at` by Core's clock.
/// `order` is absent where no order exists.
pub fn record_route_refusal(
    log: &mut dyn OrchestrationLog,
    who: Actor,
    request: &Id,
    order: Option<&Id>,
    refusal: &RouteRefusal,
    at: Timestamp,
) -> Result<Receipt, AppendFailure> {
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who,
        what: What::OrderRefusal {
            request: request.clone(),
            order: order.cloned(),
            class: refusal.class(),
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
