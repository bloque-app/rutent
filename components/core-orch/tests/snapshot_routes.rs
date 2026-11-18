//! The route Core takes in, refused unless it runs from a reserve to a reserve, changes asset only at an exchange,
//! and is an ordered list of typed operations (`T-020-05`; `REQ-ORD-16`, `REQ-ORD-36`, `REQ-ORD-37`).

mod snapshot_support;

use rutent_contracts::core_internal::{Entry, OrderRefusalClass, What};
use rutent_core_orch::snapshot::{
    RouteRefusal, RouteRefused, check_part, check_route, record_route_refusal,
};
use serde_json::{Value, json};
use snapshot_support::*;

fn unit(id: &str) -> Value {
    json!({ "id": id, "decimals": 6 })
}

/// A route received: how many operations it has, or why it is refused.
fn check(route: &Value) -> Result<usize, RouteRefusal> {
    check_route(route).map(|s| s.len()).map_err(|r| r.reason)
}

fn at(node: Value, asset: &str) -> Value {
    json!({ "node": node, "asset": unit(asset) })
}

fn step(kind: &str, from: Value, to: Value) -> Value {
    json!({ "kind": kind, "from": from, "to": to })
}

fn out() -> Value {
    json!({ "network": "polygon" })
}

fn node(name: &str) -> Value {
    json!(name)
}

/// Withdraw USDC into `polygon`, ramp it to `base`, exchange it for EURC there, and deposit EURC out of `base`.
fn whole() -> Value {
    json!([
        step("withdrawal", at(out(), "USDC"), at(node("polygon"), "USDC")),
        step(
            "route",
            at(node("polygon"), "USDC"),
            at(node("base"), "USDC")
        ),
        step(
            "exchange",
            at(node("base"), "USDC"),
            at(node("base"), "EURC")
        ),
        step(
            "deposit",
            at(node("base"), "EURC"),
            at(json!({ "network": "base" }), "EURC")
        ),
    ])
}

/// Records the refusal and checks it went to the log as an `order_refusal` of its class, stamped by Core's clock.
fn recorded(refused: &RouteRefused, class: OrderRefusalClass) {
    assert_eq!(refused.class, class);
    let mut log = KeptLog::default();
    let clock = TestClock::at(4_000);
    record_route_refusal(&mut log, &clock, core_actor(), &id("req-1"), None, refused).unwrap();
    let [Entry::Verified(entry)] = log.entries.as_slice() else {
        panic!("one verified entry");
    };
    assert!(matches!(
        entry.what,
        What::OrderRefusal { class: c, order: None, .. } if c == class
    ));
    assert_eq!(entry.when.at.as_str(), "1970-01-01T01:06:40Z");
}

// verifies: REQ-ORD-16
#[test]
fn route_without_a_withdrawal_and_a_deposit_at_a_reserve_is_refused() {
    assert_eq!(check(&whole()), Ok(4));
    // One reserve, one account: withdraw and deposit back is a route of two segments.
    let at_one_reserve = json!([
        step("withdrawal", at(out(), "USDC"), at(node("polygon"), "USDC")),
        step("deposit", at(node("polygon"), "USDC"), at(out(), "USDC")),
    ]);
    assert_eq!(check(&at_one_reserve), Ok(2));

    // Starting inside the book, with no withdrawal from a reserve.
    let mut no_withdrawal = whole();
    no_withdrawal.as_array_mut().unwrap().remove(0);
    let refused = check_route(&no_withdrawal).unwrap_err();
    assert_eq!(refused.reason, RouteRefusal::NoWithdrawalFirst);
    recorded(&refused, OrderRefusalClass::MalformedRequest);

    // Ending inside the book, with no deposit into a reserve.
    let mut no_deposit = whole();
    no_deposit.as_array_mut().unwrap().pop();
    assert_eq!(check(&no_deposit), Err(RouteRefusal::NoDepositLast));

    // A transfer inside one book: no reserve at either end.
    let inside = json!([step(
        "route",
        at(node("polygon"), "USDC"),
        at(node("base"), "USDC")
    )]);
    assert_eq!(check(&inside), Err(RouteRefusal::NoWithdrawalFirst));
}

// verifies: REQ-ORD-36
#[test]
fn asset_change_outside_an_exchange_is_refused() {
    // The ramp changes USDC into EURC on its way to `base`.
    let ramp_converts = json!([
        step("withdrawal", at(out(), "USDC"), at(node("polygon"), "USDC")),
        step(
            "route",
            at(node("polygon"), "USDC"),
            at(node("base"), "EURC")
        ),
        step(
            "deposit",
            at(node("base"), "EURC"),
            at(json!({ "network": "base" }), "EURC")
        ),
    ]);
    let refused = check_route(&ramp_converts).unwrap_err();
    assert_eq!(refused.reason, RouteRefusal::AssetChangeOutsideExchange(1));
    recorded(&refused, OrderRefusalClass::MalformedRequest);

    // The deposit pays out another asset than it took in.
    let deposit_converts = json!([
        step("withdrawal", at(out(), "USDC"), at(node("polygon"), "USDC")),
        step("deposit", at(node("polygon"), "USDC"), at(out(), "EURC")),
    ]);
    assert_eq!(
        check(&deposit_converts),
        Err(RouteRefusal::AssetChangeOutsideExchange(1))
    );

    // A change of unit alone is no change of asset: a ramp or reserve counts the asset in its own decimals at
    // each end (`REQ-CAT-11`). USDC ramped from polygon's 6 decimals to bnb's 18, and paid out in 2.
    let other_units = json!([
        step("withdrawal", at(out(), "USDC"), at(node("polygon"), "USDC")),
        step(
            "route",
            at(node("polygon"), "USDC"),
            json!({ "node": "bnb", "asset": { "id": "USDC", "decimals": 18 } })
        ),
        step(
            "deposit",
            json!({ "node": "bnb", "asset": { "id": "USDC", "decimals": 18 } }),
            json!({ "node": { "network": "bnb" }, "asset": { "id": "USDC", "decimals": 2 } })
        ),
    ]);
    assert_eq!(check(&other_units), Ok(3));
}

// verifies: REQ-ORD-37
#[test]
fn route_that_isnt_an_ordered_list_of_typed_operations_is_refused() {
    // Not a list.
    let refused = check_route(&json!({ "operations": whole() })).unwrap_err();
    assert_eq!(refused.reason, RouteRefusal::NotAList);
    recorded(&refused, OrderRefusalClass::MalformedRequest);
    assert_eq!(check(&json!([])), Err(RouteRefusal::NotAList));

    // An operation of no kind of the four, or with no kind, or without its ends.
    let mut untyped = whole();
    untyped[1]["kind"] = json!("swap");
    assert_eq!(check(&untyped), Err(RouteRefusal::NotTyped(1)));
    let mut kindless = whole();
    kindless[2].as_object_mut().unwrap().remove("kind");
    assert_eq!(check(&kindless), Err(RouteRefusal::NotTyped(2)));
    let mut endless = whole();
    endless[0].as_object_mut().unwrap().remove("to");
    assert_eq!(check(&endless), Err(RouteRefusal::NotTyped(0)));

    // The same operations out of order: the list sets the order, so each starts where the one before ended.
    let mut reordered = whole();
    reordered.as_array_mut().unwrap().swap(1, 2);
    assert_eq!(check(&reordered), Err(RouteRefusal::OutOfOrder(1)));

    // A part the router chose is held to the same rules; refused, it leaves the order with no route right now.
    let part: rutent_contracts::router_api::Part = serde_json::from_value(json!({
        "operations": [withdrawal(RESERVE, "1000")]
    }))
    .unwrap();
    let refused = check_part(&part).unwrap_err();
    assert_eq!(refused.reason, RouteRefusal::NoDepositLast);
    recorded(&refused, OrderRefusalClass::NoRouteRightNow);
}
