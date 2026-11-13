//! Objectives and the decision record (`T-011-04`): the customer's objective decides the choice, the record names
//! every candidate with its drop reason and recomputes the choice from itself alone, and every result is a split of
//! one part (`DEC-8`).

mod common;

use std::collections::BTreeSet;

use common::{
    Net, WIDE, bounds, choice_of, endpoint, fee, limited, on, outer, parse, request, routed,
    shortlist, tuples,
};
use rutent_conformance::router::recompute;
use rutent_contracts::router_api::{
    CatalogueSnapshot, DecisionRecord, DropReason, FigureValue, OperationKind, PolicyBounds,
    Request, Router,
};
use rutent_router::RouteFinder;
use serde_json::{Value, json};

const SHORTLIST_AT: &str = "2026-11-06T15:00:00Z";
const CHOICE_AT: &str = "2026-11-06T15:01:00Z";
const MADE_AT: &str = "2026-11-06T15:00:30Z";
const VALID_UNTIL: &str = "2026-11-06T15:05:30Z";

fn priced(numerator: &str, seconds: u64) -> Value {
    json!({ "fee": fee("0", numerator, "1000"), "settlement_seconds": seconds })
}

fn exchanged(numerator: &str, rate: &str, seconds: u64) -> Value {
    json!({
        "fee": fee("0", numerator, "1000"),
        "rate": { "kind": "fixed", "rate": rate },
        "settlement_seconds": seconds,
    })
}

/// The declared settlement seconds of `tuple`'s price in `snapshot`.
fn declared(snapshot: &CatalogueSnapshot, tuple: &str) -> u64 {
    snapshot
        .figures
        .iter()
        .find_map(|f| match &f.value {
            FigureValue::Price {
                tuple: t,
                settlement_seconds,
                ..
            } if t.as_str() == tuple => Some(settlement_seconds.get()),
            _ => None,
        })
        .expect("a priced tuple")
}

/// Every kept operation of a shortlist round answered by its provider, accepted on the terms the round evaluated
/// and declaring its price's settlement time: a reserve's dry run, a ramp's or exchange's fresh quote. Each tuple is
/// answered at every amount a kept candidate takes it at, so the choice round can choose any of them.
fn answered(shortlisted: &DecisionRecord) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut figures = Vec::new();
    for operation in shortlisted
        .shortlist()
        .into_iter()
        .flat_map(|c| c.parts[0].operations.iter())
    {
        let tuple = operation.figures.first().as_str();
        let amount_in = operation.amount_in.amount().as_str();
        if !seen.insert((tuple.to_owned(), amount_in.to_owned())) {
            continue;
        }
        let id = format!("a-{tuple}-{amount_in}");
        let provider = operation.provider.as_str();
        let mut outcome = json!({
            "status": "dry_run_accepted",
            "amount_in": amount_in,
            "amount_out": operation.amount_out.as_str(),
            "fee": { "asset": operation.fee.asset, "amount": operation.fee.amount },
            "valid_until": VALID_UNTIL,
            "voided_by": ["validity_passes", "amount_changes"],
            "settlement_seconds": declared(&shortlisted.snapshot, tuple),
        });
        let kind = match operation.kind {
            OperationKind::Withdrawal => "withdrawal",
            OperationKind::Deposit => "deposit",
            OperationKind::Route => "transfer",
            OperationKind::Exchange => "conversion",
        };
        if operation.kind == OperationKind::Exchange {
            outcome["rate"] = json!("1");
            outcome["slippage"] = json!({ "regime": "firm" });
        }
        let quoted = matches!(
            operation.kind,
            OperationKind::Route | OperationKind::Exchange
        );
        if quoted {
            outcome["direction"] = json!("exact_in");
        }
        let body = json!({ "kind": kind, "transaction": format!("tx-{id}"), "outcome": outcome });
        let statement = Net::envelope(provider, MADE_AT, "urn:rutent:provider-role:result", body);
        let value = if quoted {
            json!({ "kind": "fresh_quote", "tuple": tuple, "quote": statement })
        } else {
            json!({ "kind": "dry_run", "tuple": tuple, "amount_in": amount_in, "result": statement })
        };
        figures.push(json!({
            "figure_id": id,
            "origin": Net::origin(provider),
            "age_seconds": 30,
            "valid_for_seconds": 300,
            "value": value,
        }));
    }
    figures
}

/// The choice round's snapshot: the shortlist round's advertised figures, a minute older, and every kept operation
/// answered.
fn choice_snapshot(net: &Net, shortlisted: &DecisionRecord) -> CatalogueSnapshot {
    let mut figures: Vec<Value> = net
        .figures
        .iter()
        .cloned()
        .map(|mut f| {
            f["age_seconds"] = json!(f["age_seconds"].as_u64().unwrap() + 60);
            f
        })
        .collect();
    figures.extend(answered(shortlisted));
    parse(
        "choice snapshot",
        json!({ "snapshot_id": format!("{}-choice", net.id), "taken_at": CHOICE_AT, "figures": figures }),
    )
}

/// A request with another objective.
fn for_objective(request: &Request, kind: &str) -> Request {
    let mut value = serde_json::to_value(request).unwrap();
    value["objective"] = json!({ "kind": kind });
    parse("request", value)
}

fn passes(record: &DecisionRecord) {
    let recomputed = recompute(record);
    assert!(recomputed.passes(), "{:#?}", recomputed.findings);
}

/// Both rounds over `net`, the choice round over every kept operation answered.
fn rounds(
    net: &Net,
    (from, to, amount): (Value, Value, &str),
    bounds: &PolicyBounds,
) -> (DecisionRecord, Request, CatalogueSnapshot) {
    let shortlisted = routed(
        &request("r-short", shortlist(), from.clone(), to.clone(), amount),
        &net.build(),
        bounds,
    );
    passes(&shortlisted);
    let choice = request(
        "r-choice",
        choice_of("r-short", SHORTLIST_AT),
        from,
        to,
        amount,
    );
    let snapshot = choice_snapshot(net, &shortlisted);
    (shortlisted, choice, snapshot)
}

/// `AC-2`'s shape: a reserve, an exchange and a ramp, from USDC on a chain to EURC at `n2`. Two exchanges at `n1`
/// at one rate: `ex-slow` charges 1/1000 and settles in two hours, `ex-fast` charges 5/1000 and settles in a
/// minute. So the slow route arrives with more and the fast one sooner.
fn reserve_exchange_ramp() -> Net {
    let mut net = Net::new("snap-ac2", SHORTLIST_AT, 10);
    net.reserve(
        "w",
        (outer("chain-a"), json!("n1")),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("1", 600),
    );
    let (usdc, eurc) = (common::unit("USDC", 6), common::unit("EURC", 6));
    net.exchange(
        "ex-fast",
        "n1",
        (usdc.clone(), eurc.clone()),
        ("1", WIDE),
        exchanged("5", "0.95", 60),
    );
    net.exchange(
        "ex-slow",
        "n1",
        (usdc, eurc),
        ("1", WIDE),
        exchanged("1", "0.95", 7_200),
    );
    net.ramp(
        "ramp",
        ("n1", "n2"),
        "EURC",
        (6, 6),
        ("1", WIDE),
        priced("1", 300),
    );
    net
}

fn ac2_ends() -> (Value, Value, &'static str) {
    (
        endpoint(outer("chain-a"), "USDC", 6),
        endpoint(json!("n2"), "EURC", 6),
        "1000000000",
    )
}

// verifies: AC-2, REQ-QTE-11
#[test]
fn changed_objective_can_change_the_chosen_route() {
    let net = reserve_exchange_ramp();
    let (_, cheapest, snapshot) = rounds(&net, ac2_ends(), &limited(bounds(3), &net.build(), 50));
    let fastest = for_objective(&cheapest, "fastest_settlement");

    let by_cost = routed(&cheapest, &snapshot, &limited(bounds(3), &net.build(), 50));
    let by_time = routed(&fastest, &snapshot, &limited(bounds(3), &net.build(), 50));
    passes(&by_cost);
    passes(&by_time);

    // The default objective, lowest total cost, takes the cheaper exchange; the fastest takes the quicker one.
    assert_eq!(tuples(by_cost.chosen().unwrap()), ["w", "ex-slow", "ramp"]);
    assert_eq!(tuples(by_time.chosen().unwrap()), ["w", "ex-fast", "ramp"]);
    // Over the same snapshot, with no provider asked again: the same candidates, each beaten by the other's choice.
    assert_eq!(by_cost.snapshot, by_time.snapshot);
    let ids = |r: &DecisionRecord| -> Vec<String> {
        r.candidates
            .iter()
            .map(|c| format!("{}:{:?}", c.candidate_id.as_str(), tuples(c)))
            .collect()
    };
    assert_eq!(ids(&by_cost), ids(&by_time));
    assert_eq!(
        on(&by_cost, &["w", "ex-fast", "ramp"]).dropped,
        Some(DropReason::Outranked {
            by: by_cost.chosen.clone().unwrap()
        })
    );
    assert_eq!(
        on(&by_time, &["w", "ex-slow", "ramp"]).dropped,
        Some(DropReason::Outranked {
            by: by_time.chosen.clone().unwrap()
        })
    );
}

// verifies: AC-2, REQ-AUD-4
#[test]
fn decision_record_recomputes_the_choice_with_no_provider_call() {
    let net = reserve_exchange_ramp();
    let (_, cheapest, snapshot) = rounds(&net, ac2_ends(), &limited(bounds(3), &net.build(), 50));
    for objective in ["lowest_total_cost", "fastest_settlement"] {
        let bytes = {
            let record = RouteFinder::route(
                &for_objective(&cheapest, objective),
                &snapshot,
                &limited(bounds(3), &net.build(), 50),
            );
            serde_json::to_vec(&record).unwrap()
        };
        // Only the record's bytes from here: no request, snapshot or bounds of the test's, and no router.
        let record: DecisionRecord = serde_json::from_slice(&bytes).unwrap();
        let recomputed = recompute(&record);
        assert!(
            recomputed.passes(),
            "{objective}: {:#?}",
            recomputed.findings
        );
        assert!(record.chosen.is_some(), "{objective}");
        assert_eq!(recomputed.choice, record.chosen, "{objective}");
        // Every candidate but the chosen one names why it was dropped, and the record names the objective.
        for candidate in &record.candidates {
            assert_eq!(
                candidate.dropped.is_none(),
                Some(&candidate.candidate_id) == record.chosen.as_ref(),
                "{objective}: {}",
                candidate.candidate_id.as_str()
            );
        }
        assert_eq!(
            serde_json::to_value(record.request.objective).unwrap()["kind"],
            objective
        );
        // Routing the record's own inputs again gives the record, byte for byte.
        let again = RouteFinder::route(&record.request, &record.snapshot, &record.bounds);
        assert_eq!(serde_json::to_vec(&again).unwrap(), bytes, "{objective}");
    }
}

/// Bounds that allow a split of up to `max_parts` parts.
fn splitting(max_hops: u8, max_parts: u8) -> PolicyBounds {
    let mut value = serde_json::to_value(bounds(max_hops)).unwrap();
    value["max_parts"] = json!(max_parts);
    parse("bounds", value)
}

// verifies: REQ-QTE-16, DEC-8
#[test]
fn every_result_is_a_split_of_one_part() {
    // Two cheap ramps that each serve at most 600 of the 1,000 asked, and a dear one that serves it all. A router
    // that split would send 500 through each cheap ramp; v0's sends the whole amount as one part.
    let mut net = Net::new("snap-split", SHORTLIST_AT, 10);
    net.ramp(
        "r-a",
        ("n0", "n1"),
        "USDC",
        (6, 6),
        ("1", "600000000"),
        priced("1", 60),
    );
    net.ramp(
        "r-b",
        ("n0", "n1"),
        "USDC",
        (6, 6),
        ("1", "600000000"),
        priced("1", 60),
    );
    net.ramp(
        "r-c",
        ("n0", "n1"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("9", 60),
    );
    let ends = (
        endpoint(json!("n0"), "USDC", 6),
        endpoint(json!("n1"), "USDC", 6),
        "1000000000",
    );
    let bounds = splitting(2, 3);
    let (shortlisted, choice, snapshot) = rounds(&net, ends, &bounds);
    let chosen = routed(&choice, &snapshot, &bounds);
    passes(&chosen);
    for record in [&shortlisted, &chosen] {
        assert!(!record.candidates.is_empty());
        for candidate in &record.candidates {
            assert_eq!(
                candidate.parts.len(),
                1,
                "{} has {} parts",
                candidate.candidate_id.as_str(),
                candidate.parts.len()
            );
        }
    }
    assert_eq!(tuples(chosen.chosen().unwrap()), ["r-c"]);
    assert_eq!(
        on(&chosen, &["r-a"]).dropped,
        Some(DropReason::OutsideBand {
            part: 0,
            operation: 0
        })
    );
}

/// The owner's example of 2026-11-09 (item `aud4-unstateable-candidates`), in whole cents: 100 USD leave `n0`
/// for EUR at `n1`.
///
/// - Route A: ramp `r-a` (1/1000) to `n1`, then exchange `ex-a` at 0.9: 99.90 USD, so 89.91 EUR.
/// - Route B: ramp `r-b` (2/1000), then `ex-a`: 99.80 USD, so 89.82 EUR.
/// - Ramp Q, `r-q`, is advertised with no price.
/// - Ramp `r-thin` keeps a flat 88 USD, so only 12 USD reach `n2`, where exchange E, `ex-e`, charges a flat 20 USD
///   before `r-eur` would take the EUR on to `n1`.
///
/// Before router-api 3.0.0 the record listed A and B alone, and couldn't say why Q or E wasn't used.
fn owners_example() -> Net {
    owners_example_with(
        &json!({ "fee": fee("2000", "0", "1"), "rate": { "kind": "fixed", "rate": "0.9" }, "settlement_seconds": 60 }),
    )
}

/// The owner's example with exchange E priced by `e`.
fn owners_example_with(e: &Value) -> Net {
    let mut net = Net::new("snap-owner", SHORTLIST_AT, 10);
    let (usd, eur) = (common::unit("USD", 2), common::unit("EUR", 2));
    net.ramp(
        "r-a",
        ("n0", "n1"),
        "USD",
        (2, 2),
        ("1", WIDE),
        priced("1", 60),
    );
    net.ramp(
        "r-b",
        ("n0", "n1"),
        "USD",
        (2, 2),
        ("1", WIDE),
        priced("2", 60),
    );
    net.exchange(
        "ex-a",
        "n1",
        (usd.clone(), eur.clone()),
        ("1", WIDE),
        exchanged("0", "0.9", 60),
    );
    net.advertised(
        "r-q",
        "p-r-q",
        json!({
            "kind": "ramp_tuple",
            "tuple": {
                "origin": "n0", "destination": "n1", "asset": "USD",
                "decimals": { "origin": 2, "destination": 2 },
                "terms": common::terms("1", WIDE),
            },
        }),
    );
    net.ramp(
        "r-thin",
        ("n0", "n2"),
        "USD",
        (2, 2),
        ("1", WIDE),
        json!({ "fee": fee("8800", "0", "1"), "settlement_seconds": 60 }),
    );
    net.exchange("ex-e", "n2", (usd, eur), ("1", WIDE), e.clone());
    net.ramp(
        "r-eur",
        ("n2", "n1"),
        "EUR",
        (2, 2),
        ("1", WIDE),
        priced("0", 60),
    );
    net
}

fn owners_ends() -> (Value, Value, &'static str) {
    (
        endpoint(json!("n0"), "USD", 2),
        endpoint(json!("n1"), "EUR", 2),
        "10000",
    )
}

/// Both rounds of the owner's example, each passing the contract and the router suite, the choice round choosing
/// route A.
fn owners_rounds() -> [DecisionRecord; 2] {
    owners_rounds_over(&owners_example())
}

fn owners_rounds_over(net: &Net) -> [DecisionRecord; 2] {
    let bounds = limited(bounds(3), &net.build(), 50);
    let (shortlisted, choice, snapshot) = rounds(net, owners_ends(), &bounds);
    let chosen = routed(&choice, &snapshot, &bounds);
    passes(&chosen);
    assert_eq!(tuples(chosen.chosen().unwrap()), ["r-a", "ex-a"]);
    on(&chosen, &["r-b", "ex-a"]);
    [shortlisted, chosen]
}

// verifies: REQ-AUD-4
#[test]
fn path_through_a_tuple_with_no_price_is_recorded_with_its_drop_reason() {
    for record in owners_rounds() {
        let q = on(&record, &["r-q"]);
        assert_eq!(
            q.dropped,
            Some(DropReason::NoPrice {
                part: 0,
                operation: 0
            })
        );
        let operation = q.parts[0].operations.last();
        // It cites its tuple and no price, states nothing it can't, and arrives with nothing.
        let cited: Vec<&str> = operation.figures.iter().map(|f| f.as_str()).collect();
        assert_eq!(cited, ["r-q"]);
        assert_eq!(operation.fee.amount, "0");
        assert!(operation.amount_out.is_zero());
        assert_eq!(operation.amount_in.amount().as_str(), "10000");
    }
}

// verifies: REQ-AUD-4
#[test]
fn operation_whose_fee_is_above_what_reaches_it_is_recorded_with_its_drop_reason() {
    for record in owners_rounds() {
        let e = on(&record, &["r-thin", "ex-e"]);
        assert_eq!(
            e.dropped,
            Some(DropReason::FeeAboveAmount {
                part: 0,
                operation: 1
            })
        );
        // 12 USD reach the exchange, whose flat 20 USD is above them: the fee is stated as all that enters.
        let operation = e.parts[0].operations.last();
        assert_eq!(operation.amount_in.amount().as_str(), "1200");
        assert_eq!(operation.fee.amount, "1200");
        assert!(operation.amount_out.is_zero());
        // The record names every path the search reaches: A and B, Q, and E's.
        let mut named: Vec<Vec<String>> = record.candidates.iter().map(tuples).collect();
        named.sort();
        assert_eq!(
            named,
            [
                vec!["r-a", "ex-a"],
                vec!["r-b", "ex-a"],
                vec!["r-q"],
                vec!["r-thin", "ex-e"],
            ]
        );
    }
}

// verifies: REQ-AUD-4
#[test]
fn path_whose_step_delivers_nothing_before_the_destination_is_recorded_with_its_drop_reason() {
    // Exchange E now charges exactly the 12 USD that reach it: its operation can be stated, but delivers nothing,
    // short of the destination, so nothing after it can be.
    let exact = json!({
        "fee": fee("1200", "0", "1"),
        "rate": { "kind": "fixed", "rate": "0.9" },
        "settlement_seconds": 60,
    });
    for record in owners_rounds_over(&owners_example_with(&exact)) {
        let e = on(&record, &["r-thin", "ex-e"]);
        assert_eq!(
            e.dropped,
            Some(DropReason::NothingDelivered {
                part: 0,
                operation: 1
            })
        );
        let operation = e.parts[0].operations.last();
        assert_eq!(operation.amount_in.amount().as_str(), "1200");
        assert_eq!(operation.fee.amount, "1200");
        assert!(operation.amount_out.is_zero());
        let mut named: Vec<Vec<String>> = record.candidates.iter().map(tuples).collect();
        named.sort();
        assert_eq!(
            named,
            [
                vec!["r-a", "ex-a"],
                vec!["r-b", "ex-a"],
                vec!["r-q"],
                vec!["r-thin", "ex-e"],
            ]
        );
    }
}

// verifies: REQ-AUD-4
#[test]
fn path_that_delivers_nothing_where_no_tuple_goes_on_is_still_recorded_with_its_drop_reason() {
    // As above, but with no `r-eur`: from E's EUR at `n2` no tuple goes on, so the path ends there either way, and
    // it is still named for delivering nothing.
    let exact = json!({
        "fee": fee("1200", "0", "1"),
        "rate": { "kind": "fixed", "rate": "0.9" },
        "settlement_seconds": 60,
    });
    let mut net = owners_example_with(&exact);
    net.figures
        .retain(|f| !["r-eur", "r-eur-price"].contains(&f["figure_id"].as_str().unwrap_or("")));
    for record in owners_rounds_over(&net) {
        assert_eq!(
            on(&record, &["r-thin", "ex-e"]).dropped,
            Some(DropReason::NothingDelivered {
                part: 0,
                operation: 1
            })
        );
    }
}
