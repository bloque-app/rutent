//! The search over a snapshot (`T-011-01`): each segment judged on the amount that reaches it, no figure past its
//! validity used, the route's price its segments' prices, any reachable origin to any reachable destination, and
//! the same inputs always giving the same record.

mod common;

use common::{
    Net, WIDE, bounds, choice_of, endpoint, fee, on, outer, request, routed, shortlist, tuples,
};
use rutent_conformance::router::{cases, recompute};
use rutent_contracts::router_api::{DropReason, Id, OperationKind, Router};
use rutent_router::RouteFinder;
use serde_json::json;

const TAKEN_AT: &str = "2026-11-06T15:02:00Z";

fn free() -> serde_json::Value {
    json!({ "fee": fee("0", "0", "1"), "settlement_seconds": 60 })
}

// verifies: REQ-QTE-12
#[test]
fn amount_that_clears_the_first_band_and_misses_a_later_one_is_dropped() {
    // 100 USDC leave chain-a in units of 10^-6 and reach the hub, less 1%, in units of 10^-18: 99 × 10^18.
    let mut net = Net::new("snap-bands", TAKEN_AT, 60);
    net.reserve(
        "f-in",
        (outer("chain-a"), json!("hub")),
        "USDC",
        (6, 18),
        ("1", "1000000000000"),
        json!({ "fee": fee("0", "1", "100"), "settlement_seconds": 60 }),
    );
    // Serves 99.5 USDC and up: the 100 that left chain-a would clear it, the 99 after the fee does not.
    net.ramp(
        "f-ramp-tight",
        ("hub", "hub-b"),
        "USDC",
        (18, 18),
        ("99500000000000000000", WIDE),
        free(),
    );
    // Serves 50 to 200 units of 10^-18: 99,000,000 counted in chain-a's unit would clear it; in its own it does not.
    net.ramp(
        "f-ramp-unscaled",
        ("hub", "hub-b"),
        "USDC",
        (18, 18),
        ("50000000", "200000000"),
        free(),
    );
    net.ramp(
        "f-ramp-wide",
        ("hub", "hub-b"),
        "USDC",
        (18, 18),
        ("1", WIDE),
        free(),
    );
    let record = routed(
        &request(
            "r-bands",
            shortlist(),
            endpoint(outer("chain-a"), "USDC", 6),
            endpoint(json!("hub-b"), "USDC", 18),
            "100000000",
        ),
        &net.build(),
        &bounds(3),
    );
    let missed = DropReason::OutsideBand {
        part: 0,
        operation: 1,
    };
    assert_eq!(
        on(&record, &["f-in", "f-ramp-tight"]).dropped,
        Some(missed.clone())
    );
    assert_eq!(
        on(&record, &["f-in", "f-ramp-unscaled"]).dropped,
        Some(missed)
    );
    let kept = on(&record, &["f-in", "f-ramp-wide"]);
    assert_eq!(kept.dropped, None);
    let operations = &kept.parts[0].operations;
    assert_eq!(operations[0].fee.amount, "1000000");
    assert_eq!(
        operations[1].amount_in.amount().as_str(),
        "99000000000000000000"
    );
    assert!(
        recompute(&record).passes(),
        "{:#?}",
        recompute(&record).findings
    );
}

// verifies: REQ-QTE-4
#[test]
fn expired_figure_is_never_used() {
    // An advertised price past its stated validity drops its route in the shortlist round.
    let mut net = Net::new("snap-stale-price", TAKEN_AT, 120);
    net.ramp(
        "f-ramp-stale",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("0", "0", "1"), "settlement_seconds": 60 }),
    );
    net.figures[1]["valid_for_seconds"] = json!(60);
    net.ramp(
        "f-ramp-fresh",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("0", "1", "100"), "settlement_seconds": 60 }),
    );
    let to = endpoint(json!("hub-b"), "USDC", 6);
    let from = endpoint(json!("hub"), "USDC", 6);
    let record = routed(
        &request(
            "r-stale-price",
            shortlist(),
            from.clone(),
            to.clone(),
            "100000000",
        ),
        &net.build(),
        &bounds(1),
    );
    assert_eq!(
        on(&record, &["f-ramp-stale"]).dropped,
        Some(DropReason::StaleFigure {
            figure: Id::new("f-ramp-stale-price").unwrap()
        })
    );
    let shortlisted: Vec<_> = record.shortlist().iter().map(|c| tuples(c)).collect();
    assert_eq!(shortlisted, vec![vec!["f-ramp-fresh".to_owned()]]);

    // In the choice round, a quote past its validity binds nothing: its route is priced from its advertised price,
    // not the quote's better terms, and dropped for it; the route with a usable quote is chosen.
    let mut net = Net::new("snap-stale-quote", TAKEN_AT, 120);
    let one_percent = json!({ "fee": fee("0", "1", "100"), "settlement_seconds": 60 });
    net.ramp(
        "f-r1",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        one_percent.clone(),
    );
    net.ramp(
        "f-r2",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        one_percent,
    );
    net.transfer_quote(
        "q-r1-expired",
        "f-r1",
        ("100000000", "USDC", "0", "100000000"),
        30,
        ("2026-11-06T15:00:00Z", "2026-11-06T15:01:00Z"),
        (120, 60),
    );
    net.transfer_quote(
        "q-r2",
        "f-r2",
        ("100000000", "USDC", "500000", "99500000"),
        30,
        ("2026-11-06T15:01:40Z", "2026-11-06T15:03:40Z"),
        (20, 120),
    );
    let record = routed(
        &request(
            "r-stale-quote",
            choice_of("r-stale-price", "2026-11-06T15:00:00Z"),
            from,
            to,
            "100000000",
        ),
        &net.build(),
        &bounds(1),
    );
    let expired = on(&record, &["f-r1"]);
    assert_eq!(
        expired.dropped,
        Some(DropReason::StaleFigure {
            figure: Id::new("q-r1-expired").unwrap()
        })
    );
    let operation = &expired.parts[0].operations[0];
    assert_eq!(
        (operation.fee.amount.as_str(), operation.amount_out.as_str()),
        ("1000000", "99000000")
    );
    let chosen = record.chosen().expect("the route with a usable quote");
    assert_eq!(tuples(chosen), vec!["f-r2"]);
    assert_eq!(
        chosen.parts[0].operations[0].amount_out.as_str(),
        "99500000"
    );
    assert!(
        recompute(&record).passes(),
        "{:#?}",
        recompute(&record).findings
    );
}

// verifies: REQ-QTE-8
#[test]
fn route_price_is_the_sum_of_its_segments_prices() {
    // One asset in one unit all the way, so the segments' fees add up in it.
    let mut net = Net::new("snap-price", TAKEN_AT, 60);
    net.reserve(
        "f-in",
        (outer("chain-a"), json!("hub")),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("1000", "0", "1"), "settlement_seconds": 60 }),
    );
    net.ramp(
        "f-ramp",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("0", "1", "1000"), "settlement_seconds": 60 }),
    );
    net.reserve(
        "f-out",
        (json!("hub-b"), outer("chain-b")),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("500", "3", "10000"), "settlement_seconds": 60 }),
    );
    let record = routed(
        &request(
            "r-price",
            shortlist(),
            endpoint(outer("chain-a"), "USDC", 6),
            endpoint(outer("chain-b"), "USDC", 6),
            "100000000",
        ),
        &net.build(),
        &bounds(3),
    );
    let route = on(&record, &["f-in", "f-ramp", "f-out"]);
    let fees: Vec<u128> = route.parts[0]
        .operations
        .iter()
        .map(|o| o.fee.amount.parse().unwrap())
        .collect();
    // Each segment's price on what reaches it: a flat 1,000; ceil(99,999,000 / 1000) = 99,999; and
    // 500 + ceil(99,899,001 × 3 / 10,000) = 500 + ceil(29,969.7003) = 30,470.
    assert_eq!(fees, vec![1000, 99_999, 30_470]);
    let arrives: u128 = route.parts[0]
        .operations
        .last()
        .amount_out
        .as_str()
        .parse()
        .unwrap();
    assert_eq!(arrives, 99_868_531);
    assert_eq!(100_000_000 - arrives, fees.iter().sum::<u128>());
    assert!(
        recompute(&record).passes(),
        "{:#?}",
        recompute(&record).findings
    );
}

// verifies: REQ-QTE-14
#[test]
fn any_reachable_origin_reaches_any_reachable_destination_without_an_exchange() {
    // AC-26's shape: an on-chain origin's reserve, two ramps across other networks, a reserve at the destination;
    // no exchange and no local money. A ramp and a reserve back make some endpoints reachable both ways.
    let mut net = Net::new("snap-reach", TAKEN_AT, 60);
    let zero = free();
    let wide = ("1", WIDE);
    net.reserve(
        "f-eth-in",
        (outer("ethereum"), json!("hub-eth")),
        "USDC",
        (6, 18),
        wide,
        zero.clone(),
    );
    net.ramp(
        "f-eth-mid",
        ("hub-eth", "hub-mid"),
        "USDC",
        (18, 18),
        wide,
        zero.clone(),
    );
    net.ramp(
        "f-mid-sol",
        ("hub-mid", "hub-sol"),
        "USDC",
        (18, 18),
        wide,
        zero.clone(),
    );
    net.reserve(
        "f-sol-out",
        (json!("hub-sol"), outer("solana")),
        "USDC",
        (18, 6),
        wide,
        zero.clone(),
    );
    net.ramp(
        "f-sol-eth",
        ("hub-sol", "hub-eth"),
        "USDC",
        (18, 18),
        wide,
        zero.clone(),
    );
    net.reserve(
        "f-eth-out",
        (json!("hub-eth"), outer("ethereum")),
        "USDC",
        (18, 6),
        wide,
        zero,
    );
    let snapshot = net.build();
    let ends = [
        (outer("ethereum"), 6u8),
        (json!("hub-eth"), 18),
        (json!("hub-mid"), 18),
        (json!("hub-sol"), 18),
        (outer("solana"), 6),
    ];
    let edges = [(0, 1), (1, 2), (2, 3), (3, 4), (3, 1), (1, 0)];
    // Which ends reach which, through the tuples alone.
    let mut reach = [[false; 5]; 5];
    for &(a, b) in &edges {
        reach[a][b] = true;
    }
    for k in 0..5 {
        for i in 0..5 {
            for j in 0..5 {
                reach[i][j] |= reach[i][k] && reach[k][j];
            }
        }
    }
    let mut routes = 0;
    for (i, (from, from_decimals)) in ends.iter().enumerate() {
        for (j, (to, to_decimals)) in ends.iter().enumerate() {
            if i == j {
                continue;
            }
            let amount = if *from_decimals == 6 {
                "1000000"
            } else {
                "1000000000000000000"
            };
            let record = routed(
                &request(
                    &format!("r-reach-{i}-{j}"),
                    shortlist(),
                    endpoint(from.clone(), "USDC", *from_decimals),
                    endpoint(to.clone(), "USDC", *to_decimals),
                    amount,
                ),
                &snapshot,
                &bounds(4),
            );
            let kept = record.shortlist();
            assert_eq!(
                !kept.is_empty(),
                reach[i][j],
                "from {from} to {to}: {kept:#?}"
            );
            for candidate in kept {
                assert!(
                    candidate.parts[0]
                        .operations
                        .iter()
                        .all(|o| o.kind != OperationKind::Exchange && o.from.asset.id == "USDC")
                );
                routes += 1;
            }
            assert!(
                recompute(&record).passes(),
                "{:#?}",
                recompute(&record).findings
            );
        }
    }
    // The AC-26 route itself: ethereum to solana through both ramps.
    let record = routed(
        &request(
            "r-ac26",
            shortlist(),
            endpoint(outer("ethereum"), "USDC", 6),
            endpoint(outer("solana"), "USDC", 6),
            "1000000",
        ),
        &snapshot,
        &bounds(4),
    );
    let route = on(
        &record,
        &["f-eth-in", "f-eth-mid", "f-mid-sol", "f-sol-out"],
    );
    assert_eq!(route.dropped, None);
    assert_eq!(
        route.parts[0].operations.last().amount_out.as_str(),
        "1000000"
    );
    assert!(routes >= 10);
}

// verifies: AC-2, REQ-AUD-4
#[test]
fn same_request_and_snapshot_always_give_the_same_result() {
    for case in cases() {
        let first = serde_json::to_vec(&RouteFinder::route(
            &case.request,
            &case.snapshot,
            &case.bounds,
        ))
        .unwrap();
        let second = serde_json::to_vec(&RouteFinder::route(
            &case.request,
            &case.snapshot,
            &case.bounds,
        ))
        .unwrap();
        assert_eq!(first, second, "{}", case.name);
        // The record alone replays: its own request, snapshot and bounds give it again.
        let record: rutent_contracts::router_api::DecisionRecord =
            serde_json::from_slice(&first).unwrap();
        let replayed = serde_json::to_vec(&RouteFinder::route(
            &record.request,
            &record.snapshot,
            &record.bounds,
        ))
        .unwrap();
        assert_eq!(first, replayed, "{}", case.name);
        // Nothing but the answers' order depends on the snapshot's: its advertised figures listed backwards give
        // the same candidates and the same choice.
        if matches!(
            case.request.round,
            rutent_contracts::router_api::Round::Shortlist {}
        ) {
            let mut reversed = case.snapshot.clone();
            reversed.figures.reverse();
            let again = RouteFinder::route(&case.request, &reversed, &case.bounds);
            assert_eq!(again.candidates, record.candidates, "{}", case.name);
            assert_eq!(again.chosen, record.chosen, "{}", case.name);
        }
    }
}
