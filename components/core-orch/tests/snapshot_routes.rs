//! The route Core takes in, refused unless it runs from a reserve to a reserve, changes asset only at an exchange,
//! and is an ordered list of typed operations (`T-020-05`; `REQ-ORD-16`, `REQ-ORD-36`, `REQ-ORD-37`).

mod snapshot_support;

use rutent_contracts::core_internal::{Entry, OrderRefusalClass, What};
use rutent_contracts::router_api::Timestamp;
use rutent_core_orch::snapshot::{RouteRefusal, check_part, check_route, record_route_refusal};
use rutent_prov::catalogue::Instant;
use serde_json::{Value, json};
use snapshot_support::*;

fn unit(id: &str) -> Value {
    json!({ "id": id, "decimals": 6 })
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

/// Records the refusal and checks it went to the log as an `order_refusal` of its class.
fn recorded(refusal: &RouteRefusal) {
    let mut log = KeptLog::default();
    record_route_refusal(
        &mut log,
        core_actor(),
        &id("req-1"),
        None,
        refusal,
        Timestamp::new(Instant(4_000).rfc3339().unwrap()).unwrap(),
    )
    .unwrap();
    let [Entry::Verified(entry)] = log.entries.as_slice() else {
        panic!("one verified entry");
    };
    assert!(matches!(
        entry.what,
        What::OrderRefusal {
            class: OrderRefusalClass::MalformedRequest,
            order: None,
            ..
        }
    ));
}

// verifies: REQ-ORD-16
#[test]
fn route_without_a_withdrawal_and_a_deposit_at_a_reserve_is_refused() {
    assert_eq!(check_route(&whole()).map(|s| s.len()), Ok(4));
    // One reserve, one account: withdraw and deposit back is a route of two segments.
    let at_one_reserve = json!([
        step("withdrawal", at(out(), "USDC"), at(node("polygon"), "USDC")),
        step("deposit", at(node("polygon"), "USDC"), at(out(), "USDC")),
    ]);
    assert!(check_route(&at_one_reserve).is_ok());

    // Starting inside the book, with no withdrawal from a reserve.
    let mut no_withdrawal = whole();
    no_withdrawal.as_array_mut().unwrap().remove(0);
    let refusal = check_route(&no_withdrawal).unwrap_err();
    assert_eq!(refusal, RouteRefusal::NoWithdrawalFirst);
    recorded(&refusal);

    // Ending inside the book, with no deposit into a reserve.
    let mut no_deposit = whole();
    no_deposit.as_array_mut().unwrap().pop();
    assert_eq!(check_route(&no_deposit), Err(RouteRefusal::NoDepositLast));

    // A transfer inside one book: no reserve at either end.
    let inside = json!([step(
        "route",
        at(node("polygon"), "USDC"),
        at(node("base"), "USDC")
    )]);
    assert_eq!(check_route(&inside), Err(RouteRefusal::NoWithdrawalFirst));
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
    let refusal = check_route(&ramp_converts).unwrap_err();
    assert_eq!(refusal, RouteRefusal::AssetChangeOutsideExchange(1));
    recorded(&refusal);

    // The deposit pays out another asset than it took in.
    let deposit_converts = json!([
        step("withdrawal", at(out(), "USDC"), at(node("polygon"), "USDC")),
        step("deposit", at(node("polygon"), "USDC"), at(out(), "EURC")),
    ]);
    assert_eq!(
        check_route(&deposit_converts),
        Err(RouteRefusal::AssetChangeOutsideExchange(1))
    );

    // A change of unit is a change of asset too: it needs its own exchange segment, even at one value.
    let mut other_unit = whole();
    other_unit[3]["to"]["asset"]["decimals"] = json!(2);
    assert_eq!(
        check_route(&other_unit),
        Err(RouteRefusal::AssetChangeOutsideExchange(3))
    );
}

// verifies: REQ-ORD-37
#[test]
fn route_that_isnt_an_ordered_list_of_typed_operations_is_refused() {
    // Not a list.
    let refusal = check_route(&json!({ "operations": whole() })).unwrap_err();
    assert_eq!(refusal, RouteRefusal::NotAList);
    recorded(&refusal);
    assert_eq!(check_route(&json!([])), Err(RouteRefusal::NotAList));

    // An operation of no kind of the four, or with no kind, or without its ends.
    let mut untyped = whole();
    untyped[1]["kind"] = json!("swap");
    assert_eq!(check_route(&untyped), Err(RouteRefusal::NotTyped(1)));
    let mut kindless = whole();
    kindless[2].as_object_mut().unwrap().remove("kind");
    assert_eq!(check_route(&kindless), Err(RouteRefusal::NotTyped(2)));
    let mut endless = whole();
    endless[0].as_object_mut().unwrap().remove("to");
    assert_eq!(check_route(&endless), Err(RouteRefusal::NotTyped(0)));

    // The same operations out of order: the list sets the order, so each starts where the one before ended.
    let mut reordered = whole();
    reordered.as_array_mut().unwrap().swap(1, 2);
    assert_eq!(check_route(&reordered), Err(RouteRefusal::OutOfOrder(1)));

    // A part the router chose is held to the same rules.
    let part: rutent_contracts::router_api::Part = serde_json::from_value(json!({
        "operations": [withdrawal(RESERVE, "1000")]
    }))
    .unwrap();
    assert_eq!(check_part(&part), Err(RouteRefusal::NoDepositLast));
}
