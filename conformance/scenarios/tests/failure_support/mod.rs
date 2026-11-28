//! What the failure, stuck, reversal and ending scenarios share (`T-024-06`), over [`execution_support`]'s world: an
//! order with a floor, the inputs Core recorded for it, the notices its user and the operator were sent, and the
//! operator's staff as the register lists them.

#![allow(dead_code)]

use rutent_contracts::core_internal::{Actor, ActorKind, Role};
use rutent_contracts::router_api::{Amount, FeeFormula};
use rutent_contracts::state_machine::{Observed, OrderState};
use rutent_core_orch::intake::OrderRequest;
use rutent_core_orch::notice::{About, Change, Notice};
use rutent_core_orch::segments::{Accounted, Ran};
use rutent_scenarios::kit::id;

/// `request`, the customer holding that it takes no less than `floor` at the destination.
pub fn floored(mut request: OrderRequest, floor: &str) -> OrderRequest {
    request.route.floor = Some(Amount::new(floor).expect("a floor"));
    request
}

/// A fee of `flat` on each movement and nothing in proportion.
pub fn flat_fee(flat: &str) -> FeeFormula {
    serde_json::from_value(serde_json::json!({
        "flat": flat, "proportional": { "numerator": "0", "denominator": "1" }
    }))
    .expect("a fee formula")
}

/// The segments of the one part `ran` ran.
pub fn segments_of(ran: &[Ran]) -> &[Accounted] {
    let [Ran::Segments(segments)] = ran else {
        panic!("one part ran its segments: {ran:?}");
    };
    segments
}

/// What the log recorded as observed of `order`, in its order: each input the state machine was handed.
pub fn observed(
    core: &rutent_scenarios::assembly::Core,
    order: &rutent_contracts::router_api::Id,
) -> Vec<Observed> {
    core.inputs()
        .into_iter()
        .filter(|i| i.subject.order.as_str() == order.as_str())
        .map(|i| i.observed)
        .collect()
}

/// How many inputs of `order` `pick` says yes to.
pub fn count_inputs(
    core: &rutent_scenarios::assembly::Core,
    order: &rutent_contracts::router_api::Id,
    pick: impl Fn(&Observed) -> bool,
) -> usize {
    observed(core, order).iter().filter(|o| pick(o)).count()
}

/// The kind each change in `notices` names, in order, with the state the order was in after it.
pub fn changes(notices: &[Notice]) -> Vec<(String, OrderState)> {
    notices
        .iter()
        .filter_map(|n| match &n.about {
            About::Order {
                change: Change::Input { input, .. },
                view,
                ..
            } => Some((input.clone(), view.state)),
            About::Order {
                change: Change::Afterwards { event },
                view,
                ..
            } => Some((format!("afterwards:{event:?}"), view.state)),
            _ => None,
        })
        .collect()
}

/// The part of the operator's staff on the desk `desk`, a person: `login` is the one that acts, `desk` the party it
/// states, which the register lists as acting for the operator.
pub fn staff(desk: &str, login: &str) -> Actor {
    Actor {
        party: id(desk),
        acts: id(login),
        role: Role::Operator,
        kind: ActorKind::Person,
    }
}
