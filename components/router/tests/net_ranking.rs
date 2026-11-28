//! Ranking and the floor on what arrives (`T-011-07`): where a provider's accepted answer binds an operation that
//! keeps its asset at a tuple whose terms report `gross`, the answer's `amount_out` includes its charge, so the
//! operation states that amount less the charge (router-api's `Operation::stated_out`, `REQ-CAT-11`). Lowest total
//! cost, the floor after the operator's fee and the next operation's amount in all read what arrives.

mod common;

use common::{Net, WIDE, bounds, choice_of, endpoint, fee, on, parse, request, routed};
use rutent_conformance::router::recompute;
use rutent_contracts::router_api::{
    CatalogueSnapshot, DecisionRecord, DropReason, Id, Request, Router,
};
use rutent_router::RouteFinder;
use serde_json::{Value, json};

const SHORTLIST_AT: &str = "2026-11-06T15:00:00Z";
const CHOICE_AT: &str = "2026-11-06T15:01:00Z";
/// Each quote's making and validity, and so its age and validity in seconds at the choice round's `taken_at`.
const QUOTED: (&str, &str) = ("2026-11-06T15:00:30Z", "2026-11-06T15:10:00Z");
const QUOTE_AGE: (u64, u64) = (30, 570);

/// One ramp's terms in a choice round: its id, whether its terms report `gross`, and its quote's fee and amount out
/// for 1,000 in.
struct Ramp {
    id: &'static str,
    gross: bool,
    fee: &'static str,
    out: &'static str,
}

/// Sets a tuple's terms to report `gross`.
fn report_gross(net: &mut Net, tuple: &str) {
    for figure in net.figures.iter_mut() {
        if figure["figure_id"] == tuple {
            figure["value"]["tuple"]["terms"]["reporting"] = json!("gross");
        }
    }
}

/// Ramps from `hub` to `hub-b`, each advertised at its quote's fee and quoted for 1,000 USDC units in.
fn ramps(ramps: &[Ramp]) -> CatalogueSnapshot {
    let mut net = Net::new("snap-net-ranking", CHOICE_AT, 10);
    for ramp in ramps {
        net.ramp(
            ramp.id,
            ("hub", "hub-b"),
            "USDC",
            (6, 6),
            ("1", WIDE),
            json!({ "fee": fee(ramp.fee, "0", "1"), "settlement_seconds": 60 }),
        );
        if ramp.gross {
            report_gross(&mut net, ramp.id);
        }
        net.transfer_quote(
            &format!("q-{}", ramp.id),
            ramp.id,
            ("1000", "USDC", ramp.fee, ramp.out),
            60,
            QUOTED,
            QUOTE_AGE,
        );
    }
    net.build()
}

fn choice(id: &str, to: Value, floor: Option<&str>) -> Request {
    let mut value = serde_json::to_value(request(
        id,
        choice_of("r-short", SHORTLIST_AT),
        endpoint(json!("hub"), "USDC", 6),
        to,
        "1000",
    ))
    .unwrap();
    if let Some(floor) = floor {
        value["floor"] = json!(floor);
    }
    parse("request", value)
}

fn to_hub_b() -> Value {
    endpoint(json!("hub-b"), "USDC", 6)
}

fn passes(record: &DecisionRecord) {
    let recomputed = recompute(record);
    assert!(recomputed.passes(), "{:#?}", recomputed.findings);
}

fn out_of(record: &DecisionRecord, route: &[&str]) -> Vec<String> {
    on(record, route).parts[0]
        .operations
        .iter()
        .map(|o| o.amount_out.as_str().to_owned())
        .collect()
}

/// A gives out 1,000 with its charge of 10 inside it, reporting `gross`; B gives out 995, net. A delivers 990, so
/// B is chosen. Routed on through a second ramp, the gross one's next segment takes in 990, what arrived.
// verifies: REQ-QTE-11, REQ-CAT-11
#[test]
fn gross_reported_charge_is_taken_out_before_ranking() {
    let snapshot = ramps(&[
        Ramp {
            id: "r-a",
            gross: true,
            fee: "10",
            out: "1000",
        },
        Ramp {
            id: "r-b",
            gross: false,
            fee: "5",
            out: "995",
        },
    ]);
    let record = routed(&choice("r-rank", to_hub_b(), None), &snapshot, &bounds(1));
    assert_eq!(out_of(&record, &["r-a"]), ["990"]);
    assert_eq!(out_of(&record, &["r-b"]), ["995"]);
    let b = on(&record, &["r-b"]).candidate_id.clone();
    assert_eq!(record.chosen.as_ref(), Some(&b));
    assert_eq!(
        on(&record, &["r-a"]).dropped,
        Some(DropReason::Outranked { by: b })
    );
    passes(&record);

    // On to `hub-c` through a net ramp quoted at what A delivers: it is priced on 990, not 1,000.
    let mut net = Net::new("snap-net-ranking-chain", CHOICE_AT, 10);
    for (id, ends, gross, (amount_in, charge, out)) in [
        ("r-a", ("hub", "hub-b"), true, ("1000", "10", "1000")),
        ("r-c", ("hub-b", "hub-c"), false, ("990", "0", "990")),
    ] {
        net.ramp(
            id,
            ends,
            "USDC",
            (6, 6),
            ("1", WIDE),
            json!({ "fee": fee(charge, "0", "1"), "settlement_seconds": 60 }),
        );
        if gross {
            report_gross(&mut net, id);
        }
        net.transfer_quote(
            &format!("q-{id}"),
            id,
            (amount_in, "USDC", charge, out),
            60,
            QUOTED,
            QUOTE_AGE,
        );
    }
    let chained = routed(
        &choice("r-chain", endpoint(json!("hub-c"), "USDC", 6), None),
        &net.build(),
        &bounds(2),
    );
    let route = on(&chained, &["r-a", "r-c"]);
    assert_eq!(
        route.parts[0].operations[1].amount_in.amount().as_str(),
        "990"
    );
    assert_eq!(out_of(&chained, &["r-a", "r-c"]), ["990", "990"]);
    assert_eq!(chained.chosen.as_ref(), Some(&route.candidate_id));
    passes(&chained);
}

/// A floor of 995 with no operator's fee: A, whose gross 1,000 holds a charge of 10, leaves the customer 990 and is
/// below it; B's 995 meets it and is chosen.
// verifies: REQ-ORD-27, REQ-CAT-11
#[test]
fn gross_reported_charge_is_taken_out_before_the_floor() {
    let snapshot = ramps(&[
        Ramp {
            id: "r-a",
            gross: true,
            fee: "10",
            out: "1000",
        },
        Ramp {
            id: "r-b",
            gross: false,
            fee: "5",
            out: "995",
        },
    ]);
    let record = routed(
        &choice("r-floor", to_hub_b(), Some("995")),
        &snapshot,
        &bounds(1),
    );
    assert_eq!(
        on(&record, &["r-a"]).dropped,
        Some(DropReason::BelowFloor {})
    );
    assert_eq!(
        record.chosen.as_ref(),
        Some(&on(&record, &["r-b"]).candidate_id)
    );
    passes(&record);
}

/// A gross ramp giving out 1,000 with a charge of 5 inside it and a net one giving out 995 deliver the same, so
/// they tie, and the tie goes to the lowest `candidate_id`, whichever of the two reports `gross`.
// verifies: REQ-QTE-11
#[test]
fn net_and_gross_candidates_with_equal_arrivals_tie_to_the_lowest_candidate_id() {
    for gross_first in [true, false] {
        let (first, second) = if gross_first {
            ((true, "1000"), (false, "995"))
        } else {
            ((false, "995"), (true, "1000"))
        };
        let snapshot = ramps(&[
            Ramp {
                id: "r-a",
                gross: first.0,
                fee: "5",
                out: first.1,
            },
            Ramp {
                id: "r-b",
                gross: second.0,
                fee: "5",
                out: second.1,
            },
        ]);
        let record = routed(&choice("r-tie", to_hub_b(), None), &snapshot, &bounds(1));
        assert_eq!(out_of(&record, &["r-a"]), ["995"]);
        assert_eq!(out_of(&record, &["r-b"]), ["995"]);
        let (a, b) = (on(&record, &["r-a"]), on(&record, &["r-b"]));
        assert_eq!(a.candidate_id, Id::new("c001").unwrap());
        assert_eq!(
            record.chosen.as_ref(),
            Some(&a.candidate_id),
            "gross first: {gross_first}"
        );
        assert_eq!(
            b.dropped,
            Some(DropReason::Outranked {
                by: a.candidate_id.clone()
            })
        );
        passes(&record);
    }
}

/// A gross answer that delivers the most, 998 from 1,000 with a charge of 2, is chosen and confirmed. Read back
/// from its bytes alone, the record routes to itself again, and the suite recomputes its amounts and its choice
/// from the record, with no provider asked.
// verifies: AC-2, REQ-AUD-4
#[test]
fn decision_record_with_a_gross_answer_recomputes_the_choice_with_no_provider_call() {
    let snapshot = ramps(&[
        Ramp {
            id: "r-a",
            gross: true,
            fee: "2",
            out: "1000",
        },
        Ramp {
            id: "r-b",
            gross: false,
            fee: "5",
            out: "995",
        },
    ]);
    let record = routed(&choice("r-again", to_hub_b(), None), &snapshot, &bounds(1));
    let a = on(&record, &["r-a"]);
    assert_eq!(out_of(&record, &["r-a"]), ["998"]);
    assert_eq!(record.chosen.as_ref(), Some(&a.candidate_id));
    passes(&record);
    let read: DecisionRecord =
        serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
    let again = RouteFinder::route(&read.request, &read.snapshot, &read.bounds);
    assert_eq!(again, read);
    passes(&read);
}

/// A gross ramp advertised with no fee, whose quote for 1,000 in charges 10 and gives out 5: the charge is above
/// what it gives out, so nothing arrives by it. The operation cites the quote, as the contract asks of every usable
/// answer at its amount in, states its fee and 0 out, and its candidate is dropped: as arriving with nothing where
/// it ends at the destination, and as delivering nothing where a segment would follow. Either record recomputes.
// verifies: REQ-QTE-13, REQ-CAT-11
#[test]
fn gross_answer_charging_more_than_it_gives_out_delivers_nothing() {
    let charging_more = |net: &mut Net| {
        net.ramp(
            "r-a",
            ("hub", "hub-b"),
            "USDC",
            (6, 6),
            ("1", WIDE),
            json!({ "fee": fee("0", "0", "1"), "settlement_seconds": 60 }),
        );
        report_gross(net, "r-a");
        net.transfer_quote(
            "q-a",
            "r-a",
            ("1000", "USDC", "10", "5"),
            60,
            QUOTED,
            QUOTE_AGE,
        );
    };
    let mut net = Net::new("snap-net-ranking-more", CHOICE_AT, 10);
    charging_more(&mut net);
    let record = routed(
        &choice("r-more", to_hub_b(), None),
        &net.build(),
        &bounds(1),
    );
    let a = on(&record, &["r-a"]);
    let operation = &a.parts[0].operations[0];
    assert_eq!(
        (operation.fee.amount.as_str(), operation.amount_out.as_str()),
        ("10", "0")
    );
    assert!(operation.figures.iter().any(|f| f.as_str() == "q-a"));
    assert_eq!(a.dropped, Some(DropReason::NoPositiveOutcome {}));
    assert_eq!(record.chosen, None);
    passes(&record);

    // Mid-route, short of `hub-c`: the path ends at the ramp, which delivers nothing.
    let mut net = Net::new("snap-net-ranking-more-chain", CHOICE_AT, 10);
    charging_more(&mut net);
    net.ramp(
        "r-c",
        ("hub-b", "hub-c"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("0", "0", "1"), "settlement_seconds": 60 }),
    );
    let chained = routed(
        &choice("r-more-chain", endpoint(json!("hub-c"), "USDC", 6), None),
        &net.build(),
        &bounds(2),
    );
    assert_eq!(
        on(&chained, &["r-a"]).dropped,
        Some(DropReason::NothingDelivered {
            part: 0,
            operation: 0
        })
    );
    passes(&chained);
}
