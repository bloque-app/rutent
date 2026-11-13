//! What the routing scenarios share (`T-024-03`): a world of the reserve `sim-reserve-a` and reserves beside it, which
//! serve its two tuples at other prices, so a route runs from `a`'s bank into its ledger and out again, with more than
//! one provider serving its deposit.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::time::Duration;

use rutent_conformance::sim::{Role, Spec};
use rutent_contracts::core_internal::{Destination, Entry, Parties, What};
use rutent_contracts::provider_role::OuterNode;
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_core_orch::intake::{OrderRequest, Status, Told};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, DESTINATION_ACCOUNT, Staged};
use rutent_scenarios::harness::{Given, Scenario};
use rutent_scenarios::kit::{self, id};
use serde_json::json;

pub const A: &str = "sim-reserve-a";

/// What the routes move: above the fees the scenarios' reserves take.
pub const AMOUNT: &str = "5000000";

/// What a scenario is given: the reserve `a`, the reserves named beside it, staging at once.
pub fn given(reserves: &[&str]) -> Given {
    let mut providers = vec![Spec::of(Role::Reserve, A)];
    providers.extend(reserves.iter().map(|name| Spec::of(Role::Reserve, name)));
    Given {
        providers,
        staging: Staged::AtOnce,
        deadline: Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    }
}

/// Core, put together with `reserves` besides `a` and a session open for the customer.
pub fn core_with(accepts: &'static str, reserves: &[&str]) -> (Core, Id) {
    let (mut core, _) = Scenario::new("routing", accepts, given(reserves)).core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    (core, session)
}

/// The customer's request to move `amount` from `a`'s bank into its ledger and back out to the account, ranked by
/// `objective`.
pub fn request(name: &str, amount: &str, session: &Id, objective: &str) -> OrderRequest {
    request_at(A, name, amount, session, objective)
}

/// [`request`], at the reserve `reserve`.
pub fn request_at(
    reserve: &str,
    name: &str,
    amount: &str,
    session: &Id,
    objective: &str,
) -> OrderRequest {
    let customer = kit::customer();
    OrderRequest {
        request: id(name),
        caller: customer.clone(),
        parties: Parties {
            requester: customer.party.clone(),
            beneficiary: customer.party.clone(),
            settlement_account_at: None,
            payer: customer.party.clone(),
        },
        origin_reserve: id(reserve),
        session: session.clone(),
        destination: Destination {
            reserve: id(reserve),
            node: OuterNode {
                network: format!("{reserve}-bank"),
            },
            account: id(DESTINATION_ACCOUNT),
        },
        route: serde_json::from_value(json!({
            "request_id": format!("{name}-route"),
            "round": {"kind": "shortlist"},
            "from": kit::bank(reserve),
            "to": kit::bank(reserve),
            "amount": amount,
            "objective": {"kind": objective},
        }))
        .expect("a route request"),
        inputs: BTreeMap::from([
            ("payer.bank_account".to_owned(), "acct-1".to_owned()),
            (
                "beneficiary.bank_account".to_owned(),
                DESTINATION_ACCOUNT.to_owned(),
            ),
        ]),
        needs: Vec::new(),
    }
}

/// The order `request` opens and what quoting it told.
pub fn quoted(core: &mut Core, request: OrderRequest) -> (Id, Told) {
    let told = core.request(request).expect("the request opens an order");
    let order = Core::order_of(&told).expect("an order");
    let told = core.quote(&kit::customer(), &order).expect("quoted");
    (order, told)
}

/// The quote shown, where one is.
pub fn quote_of(told: &Told) -> Option<&rutent_core_orch::intake::Quote> {
    match told {
        Told::Exists {
            status: Status::Quoted { quote, .. },
            ..
        } => Some(quote),
        _ => None,
    }
}

/// The providers a candidate's operations run through, in order.
pub fn providers_of(
    record: &DecisionRecord,
    candidate: &rutent_contracts::router_api::Candidate,
) -> Vec<String> {
    let _ = record;
    candidate
        .parts
        .iter()
        .flat_map(|p| p.operations.iter())
        .map(|o| o.provider.as_str().to_owned())
        .collect()
}

/// Every decision record Core signed for `order`'s request, in the order the log holds them.
pub fn decisions(core: &Core) -> Vec<DecisionRecord> {
    core.decision_records()
}

/// The verified entries of the log, with their positions.
pub fn verified(core: &Core) -> Vec<(usize, What)> {
    core.log()
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(i, (_, e))| match e {
            Entry::Verified(v) => Some((i, v.what.clone())),
            Entry::Stated(_) => None,
        })
        .collect()
}

/// How many statements the log holds about `order`: every question asked of a provider and every answer.
pub fn statements_about(core: &Core, order: &Id) -> usize {
    core.log()
        .entries()
        .iter()
        .filter(|(_, e)| {
            matches!(e, Entry::Stated(s)
                if s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order))
        })
        .count()
}

/// The decision record `id`, as Core signed it.
pub fn record_named(core: &Core, id: &Id) -> DecisionRecord {
    decisions(core)
        .into_iter()
        .find(|r| r.record_id() == id)
        .expect("Core signed the record")
}

/// The providers of the route the quote `told` shows, in order.
pub fn route_shown(core: &Core, told: &Told) -> Vec<String> {
    let quote = quote_of(told).expect("a quote is shown");
    let record = record_named(core, &quote.decision_record);
    let chosen = record.chosen().expect("the record chose a route");
    assert_eq!(chosen.candidate_id, quote.candidate);
    providers_of(&record, chosen)
}
