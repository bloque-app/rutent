//! Objectives and the decision record (`T-011-04`): the customer's objective decides the choice, the record names
//! every candidate with its drop reason and recomputes the choice from itself alone, and every result is a split of
//! one part (`DEC-8`).

mod common;

use std::collections::BTreeSet;

use common::{
    Net, WIDE, bounds, choice_of, endpoint, fee, on, outer, parse, request, routed, shortlist,
    tuples,
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
    let (_, cheapest, snapshot) = rounds(&net, ac2_ends(), &bounds(3));
    let fastest = for_objective(&cheapest, "fastest_settlement");

    let by_cost = routed(&cheapest, &snapshot, &bounds(3));
    let by_time = routed(&fastest, &snapshot, &bounds(3));
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
    let (_, cheapest, snapshot) = rounds(&net, ac2_ends(), &bounds(3));
    for objective in ["lowest_total_cost", "fastest_settlement"] {
        let bytes = {
            let record =
                RouteFinder::route(&for_objective(&cheapest, objective), &snapshot, &bounds(3));
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

// verifies: REQ-AUD-4, AC-2
#[test]
fn tuples_no_operation_can_state_are_no_candidates_and_leave_the_choice_unchanged() {
    let ends = (
        endpoint(json!("n0"), "USDC", 6),
        endpoint(json!("n1"), "USDC", 6),
        "1000000000",
    );
    let plain = || {
        let mut net = Net::new("snap-unstated", SHORTLIST_AT, 10);
        net.ramp(
            "r-c",
            ("n0", "n1"),
            "USDC",
            (6, 6),
            ("1", WIDE),
            priced("9", 60),
        );
        net
    };
    // Beside the route: a ramp advertised with no price, and one whose flat fee is above what would enter it.
    let mut crowded = plain();
    crowded.advertised(
        "r-unpriced",
        "p-r-unpriced",
        json!({
            "kind": "ramp_tuple",
            "tuple": {
                "origin": "n0", "destination": "n1", "asset": "USDC",
                "decimals": { "origin": 6, "destination": 6 },
                "terms": common::terms("1", WIDE),
            },
        }),
    );
    crowded.ramp(
        "r-dear",
        ("n0", "n1"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("2000000000", "0", "1"), "settlement_seconds": 1 }),
    );
    let bounds = bounds(2);
    let decide = |net: &Net| {
        let (shortlisted, choice, snapshot) = rounds(net, ends.clone(), &bounds);
        let chosen = routed(&choice, &snapshot, &bounds);
        passes(&chosen);
        (shortlisted, chosen)
    };
    let (plain_short, plain_choice) = decide(&plain());
    let (short, choice) = decide(&crowded);
    for record in [&short, &choice] {
        for candidate in &record.candidates {
            let cited: Vec<String> = tuples(candidate);
            assert!(
                !cited.iter().any(|t| t == "r-unpriced" || t == "r-dear"),
                "{} takes a tuple no operation can state",
                candidate.candidate_id.as_str()
            );
        }
        // The record still holds both, verbatim, so a reader recomputing the choice sees why neither is a route.
        for id in ["r-unpriced", "r-dear", "r-dear-price"] {
            assert!(
                record
                    .snapshot
                    .figures
                    .iter()
                    .any(|f| f.figure_id.as_str() == id),
                "{id}"
            );
        }
    }
    assert_eq!(short.candidates, plain_short.candidates);
    assert_eq!(choice.candidates, plain_choice.candidates);
    assert_eq!(tuples(choice.chosen().unwrap()), ["r-c"]);
}
