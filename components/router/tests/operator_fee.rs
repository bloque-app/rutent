//! The operator's fee and the slippage limits in the policy bounds (`T-020-02`): the floor is judged on what the
//! customer receives after the fee, ranking stays on the providers' amounts before it and the record says so, the
//! record names the formula and limits it ran under, and the quote module's argument types are router-api's.

mod common;

use common::{
    Net, WIDE, bounds, endpoint, fee, limited, on, parse, request, routed, shortlist, unit,
};
use rutent_conformance::router::recompute;
use rutent_contracts::provider_role::BasisPoints;
use rutent_contracts::router_api::{
    Candidate, CatalogueSnapshot, DecisionRecord, DropReason, FeeFormula, HeldQuote, PolicyBounds,
    Ranking, Request, Router, SettlementTimes, SlippageLimit, StatedTerms, Timestamp,
};
use rutent_router::RouteFinder;
use rutent_router::quote::{
    Binding, Commitment, HeldQuoteRefused, HoldRefused, Measured, NoSlippageLimit, PriceRefused,
    SegmentTime, ShownPrice, binding, commitments, hold, measure, quoted_price, redeem,
};
use serde_json::{Value, json};

const TAKEN_AT: &str = "2026-11-06T15:02:00Z";

/// One free ramp from `hub` to `hub-b`, so what arrives is exactly what leaves: 99,500 USDC units.
fn ramp() -> CatalogueSnapshot {
    let mut net = Net::new("snap-floor", TAKEN_AT, 60);
    net.ramp(
        "r0",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("0", "0", "1"), "settlement_seconds": 60 }),
    );
    net.build()
}

fn with_floor(request: Request, floor: &str) -> Request {
    let mut value = serde_json::to_value(request).unwrap();
    value["floor"] = json!(floor);
    parse("request", value)
}

fn operator_fee(bounds: PolicyBounds, formula: Value) -> PolicyBounds {
    let mut value = serde_json::to_value(bounds).unwrap();
    value["operator_fee"] = formula;
    parse("bounds", value)
}

fn floor_request(floor: &str) -> Request {
    with_floor(
        request(
            "r-floor",
            shortlist(),
            endpoint(json!("hub"), "USDC", 6),
            endpoint(json!("hub-b"), "USDC", 6),
            "99500",
        ),
        floor,
    )
}

// verifies: REQ-ORD-27, AC-12
#[test]
fn floor_is_judged_on_what_the_customer_receives_after_the_operators_fee() {
    // A floor of 99,000, an arrival of 99,500 and an operator's fee of 1,000: the customer would be shown 98,500.
    let bounds = operator_fee(bounds(1), fee("1000", "0", "1"));
    let record = routed(&floor_request("99000"), &ramp(), &bounds);
    let candidate = on(&record, &["r0"]);
    assert_eq!(
        candidate.parts[0].operations[0].amount_out.as_str(),
        "99500"
    );
    let shown = quoted_price(candidate, &record.bounds.operator_fee).unwrap();
    assert_eq!(shown.shown_out.as_str(), "98500");
    assert_eq!(candidate.dropped, Some(DropReason::BelowFloor {}));
    assert!(recompute(&record).passes(), "{:#?}", recompute(&record));

    // At a floor of 98,500, what is shown, the same route is kept.
    let at_floor = routed(&floor_request("98500"), &ramp(), &bounds);
    assert_eq!(on(&at_floor, &["r0"]).dropped, None);
    assert!(recompute(&at_floor).passes());
}

/// Two ramps from `hub` to `hub-b`, one cheaper, and an exchange `x0` at `hub-b`, so the bounds hold a limit.
fn two_ramps() -> CatalogueSnapshot {
    let mut net = Net::new("snap-two", TAKEN_AT, 60);
    for (id, numerator) in [("r-cheap", "1"), ("r-dear", "5")] {
        net.ramp(
            id,
            ("hub", "hub-b"),
            "USDC",
            (6, 6),
            ("1", WIDE),
            json!({ "fee": fee("0", numerator, "1000"), "settlement_seconds": 60 }),
        );
    }
    net.exchange(
        "x0",
        "hub-b",
        (unit("USDC", 6), unit("EURC", 6)),
        ("1", WIDE),
        json!({
            "fee": fee("0", "0", "1"),
            "rate": { "kind": "fixed", "rate": "0.9" },
            "settlement_seconds": 3600,
        }),
    );
    net.build()
}

fn two_ramps_request(id: &str) -> Request {
    request(
        id,
        shortlist(),
        endpoint(json!("hub"), "USDC", 6),
        endpoint(json!("hub-b"), "USDC", 6),
        "100000000",
    )
}

/// The operator's fee of 0.25 % with a flat 1 USDC and a maximum of 5 USDC, and every exchange limited to 30
/// basis points.
fn charged(snapshot: &CatalogueSnapshot) -> PolicyBounds {
    operator_fee(
        limited(bounds(2), snapshot, 30),
        json!({
            "flat": "1000000",
            "proportional": { "numerator": "25", "denominator": "10000" },
            "maximum": "5000000",
        }),
    )
}

// verifies: REQ-AUD-4, REQ-QTE-2, REQ-QTE-9
#[test]
fn decision_record_names_the_fee_formula_and_slippage_limits_it_ran_under() {
    let snapshot = two_ramps();
    let bounds = charged(&snapshot);
    let record = routed(&two_ramps_request("r-named"), &snapshot, &bounds);
    let bytes = serde_json::to_value(&record).unwrap();
    assert_eq!(
        bytes["bounds"]["operator_fee"],
        json!({
            "flat": "1000000",
            "proportional": { "numerator": "25", "denominator": "10000" },
            "maximum": "5000000",
        })
    );
    assert_eq!(
        bytes["bounds"]["slippage_limits"][0]["tuple"]["provider"],
        "p-x0"
    );
    assert_eq!(bytes["bounds"]["slippage_limits"][0]["max_bps"], 30);
    // The price shown and the exchange's commitment are recomputed from the record's own bounds.
    let cheap = on(&record, &["r-cheap"]);
    let shown = quoted_price(cheap, &record.bounds.operator_fee).unwrap();
    // 1 USDC + ceil(99,900,000 × 25 / 10,000) = 1,249,750, under the maximum.
    assert_eq!(shown.operator_fee.as_str(), "1249750");
    // Without the limit, the bounds are refused for this snapshot: the exchange settles within three days.
    let mut unlimited = record.bounds.clone();
    unlimited.slippage_limits.clear();
    assert!(unlimited.check_against(&record.snapshot).is_err());
}

// verifies: AC-2, REQ-AUD-4
#[test]
fn same_request_and_snapshot_still_give_the_same_result() {
    let snapshot = two_ramps();
    let bounds = charged(&snapshot);
    let request = with_floor(two_ramps_request("r-same"), "98000000");
    let first = RouteFinder::route(&request, &snapshot, &bounds);
    let second = RouteFinder::route(&request, &snapshot, &bounds);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    // Read back from its bytes alone, the record routes to itself again and recomputes.
    let read: DecisionRecord =
        serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    let again = RouteFinder::route(&read.request, &read.snapshot, &read.bounds);
    assert_eq!(again, read);
    assert!(recompute(&read).passes(), "{:#?}", recompute(&read));
    // The floor binds after the fee: the dear ramp delivers 99,500,000, so 98,251,250 is left after the fee, and
    // stays; at a floor of 98,300,000 it would be dropped.
    assert_eq!(on(&read, &["r-dear"]).dropped, None);
}

// verifies: REQ-AUD-4, REQ-QTE-11
#[test]
fn decision_record_says_its_ranking_is_before_the_operators_fee() {
    let snapshot = two_ramps();
    let record = routed(
        &two_ramps_request("r-ranked"),
        &snapshot,
        &charged(&snapshot),
    );
    assert_eq!(record.ranking, Ranking::BeforeOperatorFee);
    assert_eq!(
        serde_json::to_value(&record).unwrap()["ranking"],
        json!("before_operator_fee")
    );
}

// verifies: REQ-MOD-3, DEC-2
#[test]
fn routers_quote_checks_take_their_argument_types_from_router_api() {
    // Each check takes router-api's type, as Core holds it: these bindings don't compile against any other.
    let _: fn(&Candidate, &FeeFormula) -> Result<ShownPrice, PriceRefused> = quoted_price;
    let _: for<'a> fn(&'a [StatedTerms], Option<&Timestamp>) -> Binding<'a> = binding;
    let _: fn(&SettlementTimes) -> Option<Measured> = measure;
    let _: fn(DecisionRecord, Timestamp) -> Result<HeldQuote, HoldRefused> = hold;
    let _: for<'a> fn(&'a HeldQuote, &Timestamp) -> Result<&'a Candidate, HeldQuoteRefused> =
        redeem;
    type Commitments = Vec<(SegmentTime, Result<Commitment, NoSlippageLimit>)>;
    let _: fn(&DecisionRecord, &Candidate, &[SlippageLimit]) -> Commitments = commitments;

    // And they read the contract's own fixtures, as Core would read its records.
    let fixture = |name: &str| -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/router-api/fixtures/valid")
            .join(format!("{name}.json"));
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    };
    let terms: StatedTerms =
        serde_json::from_value(fixture("stated-terms-of-an-exchange")).unwrap();
    let at = |t: &str| Timestamp::new(t).unwrap();
    assert_eq!(
        binding(
            std::slice::from_ref(&terms),
            Some(&at("2026-11-06T15:00:00Z"))
        ),
        Binding::Binds(&terms)
    );
    let times: SettlementTimes =
        serde_json::from_value(fixture("settlement-times-of-a-segment")).unwrap();
    assert_eq!(
        measure(&times),
        Some(Measured {
            declared_seconds: 3600,
            actual_seconds: 3601,
            over_seconds: 1,
        })
    );
    let held: HeldQuote = serde_json::from_value(fixture("held-quote-of-a-choice-round")).unwrap();
    assert!(redeem(&held, &at("2026-11-06T15:30:00Z")).is_ok());
    assert!(redeem(&held, &at("2026-11-06T15:30:01Z")).is_err());
    let limit: SlippageLimit = SlippageLimit {
        tuple: held.record.bounds.slippage_limits[0].tuple.clone(),
        max_bps: BasisPoints::try_from(10).unwrap(),
    };
    let chosen = held.record.chosen().unwrap();
    assert!(
        commitments(&held.record, chosen, &[limit])
            .iter()
            .all(|(_, c)| c.is_ok())
    );
}
