//! A route is never ended on a reserve that batches payouts (`T-020-43`, item `shared-movement-reading`): one final
//! movement evidences one order's close, so a reserve that pays several orders with one movement could never close a
//! route that ends on its deposit. The search drops such a candidate as `batched_payout`; a reserve that declares
//! `per_order`, or nothing, is routed to as before.

mod common;

use common::{Net, WIDE, bounds, choice_of, endpoint, fee, on, outer, request, routed, shortlist};
use rutent_conformance::router::recompute;
use rutent_contracts::router_api::{DecisionRecord, DropReason};
use serde_json::{Value, json};

const TAKEN_AT: &str = "2026-11-06T15:02:00Z";
const MADE_AT: &str = "2026-11-06T15:01:55Z";
const VALID_UNTIL: &str = "2026-11-06T15:03:55Z";

fn free() -> Value {
    json!({ "fee": fee("0", "0", "1"), "settlement_seconds": 60 })
}

/// A withdrawal from `chain-a` to the `hub`, and three deposits from the hub to `chain-b`: one that declares
/// `batched` payouts, one `per_order`, and one that declares nothing.
fn network(taken_at: &str) -> Net {
    let mut net = Net::new("snap-batched", taken_at, 60);
    net.reserve(
        "f-in",
        (outer("chain-a"), json!("hub")),
        "USDC",
        (6, 6),
        ("1", WIDE),
        free(),
    );
    for id in ["f-out-batched", "f-out-per-order", "f-out-undeclared"] {
        net.reserve(
            id,
            (json!("hub"), outer("chain-b")),
            "USDC",
            (6, 6),
            ("1", WIDE),
            free(),
        );
    }
    declare(&mut net, "f-out-batched", "batched");
    declare(&mut net, "f-out-per-order", "per_order");
    net
}

/// States `payouts` on the tuple figure `id`, as the advertisement's tuple carries it into the snapshot.
fn declare(net: &mut Net, id: &str, payouts: &str) {
    let figure = net
        .figures
        .iter_mut()
        .find(|f| f["figure_id"] == id)
        .expect("the tuple figure");
    figure["value"]["tuple"]["payouts"] = json!(payouts);
}

fn to_chain_b() -> Value {
    endpoint(outer("chain-b"), "USDC", 6)
}

fn shortlisted(net: &Net) -> DecisionRecord {
    routed(
        &request(
            "r-batched",
            shortlist(),
            endpoint(outer("chain-a"), "USDC", 6),
            to_chain_b(),
            "100000000",
        ),
        &net.build(),
        &bounds(2),
    )
}

// verifies: REQ-ORD-33
#[test]
fn route_is_never_ended_on_a_reserve_that_batches_payouts() {
    let record = shortlisted(&network(TAKEN_AT));
    assert_eq!(record.candidates.len(), 3, "each deposit is a candidate");
    for candidate in &record.candidates {
        let ends_batched = candidate
            .parts
            .iter()
            .any(|p| p.operations.last().figures.first().as_str() == "f-out-batched");
        if ends_batched {
            assert!(
                matches!(candidate.dropped, Some(DropReason::BatchedPayout { .. })),
                "a route ending on the batching reserve is dropped: {:?}",
                candidate.dropped
            );
        } else {
            assert_eq!(candidate.dropped, None, "the others are kept");
        }
    }
    // Whatever round asks, a batching reserve ends no kept candidate and no choice.
    let mut net = network(TAKEN_AT);
    answer(&mut net, "f-in", "f-out-per-order");
    let choice = routed(
        &request(
            "r-batched-choice",
            choice_of("r-batched", "2026-11-06T15:00:00Z"),
            endpoint(outer("chain-a"), "USDC", 6),
            to_chain_b(),
            "100000000",
        ),
        &net.build(),
        &bounds(2),
    );
    let chosen = choice.chosen().expect("a route that ends per order");
    assert_eq!(
        chosen.parts[0].operations.last().figures.first().as_str(),
        "f-out-per-order"
    );
}

// verifies: REQ-AUD-4
#[test]
fn candidate_ending_on_a_batching_reserve_is_recorded_with_its_drop_reason() {
    let record = shortlisted(&network(TAKEN_AT));
    let batched = on(&record, &["f-in", "f-out-batched"]);
    assert_eq!(
        batched.dropped,
        Some(DropReason::BatchedPayout {
            part: 0,
            operation: 1
        }),
        "the part's last operation, its deposit"
    );
    let recomputed = recompute(&record);
    assert!(recomputed.passes(), "{:#?}", recomputed.findings);
}

// verifies: REQ-QTE-11
#[test]
fn route_ended_on_a_reserve_that_pays_per_order_is_still_chosen() {
    for only in ["f-out-per-order", "f-out-undeclared"] {
        let mut net = network("2026-11-06T15:02:00Z");
        answer(&mut net, "f-in", only);
        // The batching reserve is advertised and answered, and would deliver the same.
        answer(&mut net, "f-in", "f-out-batched");
        let choice = routed(
            &request(
                "r-per-order",
                choice_of("r-batched", "2026-11-06T15:00:00Z"),
                endpoint(outer("chain-a"), "USDC", 6),
                to_chain_b(),
                "100000000",
            ),
            &net.build(),
            &bounds(2),
        );
        let chosen = choice
            .chosen()
            .unwrap_or_else(|| panic!("{only} is chosen"));
        assert_eq!(
            chosen.parts[0].operations.last().figures.first().as_str(),
            only
        );
        let recomputed = recompute(&choice);
        assert!(recomputed.passes(), "{:#?}", recomputed.findings);
    }
}

/// Accepted dry runs of the withdrawal `from` and the deposit `to`, each at the amount that reaches it.
fn answer(net: &mut Net, from: &str, to: &str) {
    for (id, tuple, kind) in [(from, from, "withdrawal"), (to, to, "deposit")] {
        if net
            .figures
            .iter()
            .any(|f| f["figure_id"] == format!("dr-{id}"))
        {
            continue;
        }
        let body = json!({
            "kind": kind,
            "transaction": format!("tx-{id}"),
            "outcome": {
                "status": "dry_run_accepted",
                "amount_in": "100000000",
                "amount_out": "100000000",
                "fee": { "asset": "USDC", "amount": "0" },
                "valid_until": VALID_UNTIL,
                "voided_by": ["validity_passes", "amount_changes"],
                "settlement_seconds": 60,
            },
        });
        let provider = format!("p-{tuple}");
        net.figures.push(json!({
            "figure_id": format!("dr-{id}"),
            "origin": Net::origin(&provider),
            "age_seconds": 5,
            "valid_for_seconds": 120,
            "value": {
                "kind": "dry_run",
                "tuple": tuple,
                "amount_in": "100000000",
                "result": Net::envelope(&provider, MADE_AT, "urn:rutent:provider-role:result", body),
            },
        }));
    }
}
