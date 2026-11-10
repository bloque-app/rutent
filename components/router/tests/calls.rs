//! The calls a route makes (`T-011-06`, router-api 3.0.0, `REQ-MDL-21`, `REQ-MDL-9`): each segment names the
//! cross-module calls its provider's module declares, in the snapshot's calls figure of its release, to the
//! providers of the other segments of its part, and no other, so a module may call only a provider on its route.

mod common;

use common::{Net, WIDE, bounds, choice_of, endpoint, fee, parse, request, routed};
use rutent_contracts::router_api::{CallKind, DecisionRecord, Id, RecordedDecision, RouteCall};
use serde_json::json;

const TAKEN_AT: &str = "2026-11-06T15:02:00Z";

/// The protocol version the record's statements and bounds name, pinned as the other decision-record fixtures pin
/// theirs, so a provider-role bump leaves the fixture standing.
const PROTOCOL: &str = "2.0.1";

/// The fixture the platform's tests fill a route's calls from: this router's own record over these inputs.
const FIXTURE: &str = "decision-record-naming-the-calls-each-segment-makes";

fn id(text: &str) -> Id {
    Id::new(text).expect("an id")
}

/// A choice round over two ramps, `r1` from `hub` to `hub-b` at `p-r1` and `r2` from `hub-b` to `hub-c` at
/// `p-r2`, each confirmed by its fresh quote. `p-r1`'s module declares calls to `p-r2`, for withdrawals and
/// transfers, and to `p-z`, which is on no route; `p-r2`'s declares one to `p-z` only.
fn two_ramps() -> DecisionRecord {
    let mut net = Net::new("snap-calls", TAKEN_AT, 120);
    for (tuple, (from, to)) in [("r1", ("hub", "hub-b")), ("r2", ("hub-b", "hub-c"))] {
        net.ramp(
            tuple,
            (from, to),
            "USDC",
            (6, 6),
            ("1", WIDE),
            json!({ "fee": fee("0", "1", "100"), "settlement_seconds": 60 }),
        );
    }
    net.calls(
        "r1-calls",
        "p-r1",
        &[
            ("p-r2", &["withdrawal", "transfer"]),
            ("p-z", &["withdrawal"]),
        ],
    );
    net.calls("r2-calls", "p-r2", &[("p-z", &["withdrawal"])]);
    net.transfer_quote(
        "q-r1",
        "r1",
        ("100000000", "USDC", "1000000", "99000000"),
        30,
        ("2026-11-06T15:01:40Z", "2026-11-06T15:03:40Z"),
        (20, 120),
    );
    net.transfer_quote(
        "q-r2",
        "r2",
        ("99000000", "USDC", "990000", "98010000"),
        30,
        ("2026-11-06T15:01:40Z", "2026-11-06T15:03:40Z"),
        (20, 120),
    );
    for figure in &mut net.figures {
        if let Some(quote) = figure.pointer_mut("/value/quote") {
            quote["protocol_version"] = json!(PROTOCOL);
        }
    }
    let mut bounds = serde_json::to_value(bounds(2)).expect("JSON");
    bounds["protocol_version"] = json!(PROTOCOL);
    routed(
        &request(
            "r-calls",
            choice_of("r-calls-shortlist", "2026-11-06T15:00:00Z"),
            endpoint(json!("hub"), "USDC", 6),
            endpoint(json!("hub-c"), "USDC", 6),
            "100000000",
        ),
        &net.build(),
        &parse("bounds", bounds),
    )
}

fn call(callee: &str, kind: CallKind) -> RouteCall {
    RouteCall {
        callee: id(callee),
        kind,
    }
}

// verifies: REQ-MDL-9, REQ-MDL-21
#[test]
fn route_names_the_calls_each_segment_makes() {
    let record = two_ramps();
    let chosen = record.chosen().expect("the two ramps are chosen");
    let [r1, r2] = chosen.parts[0].operations.as_slice() else {
        panic!("not the two ramps: {chosen:?}");
    };
    // `p-r1`'s segment names its declared calls to `p-r2`, the other segment's provider, and not the one to `p-z`,
    // which is on no segment of the part; it cites the calls figure they come from.
    assert_eq!(
        r1.calls,
        vec![
            call("p-r2", CallKind::Withdrawal),
            call("p-r2", CallKind::Transfer)
        ]
    );
    assert!(r1.figures.contains(&id("r1-calls")), "{:?}", r1.figures);
    // `p-r2` declares a call only to `p-z`, off the route, so its segment names none and cites no calls figure.
    assert!(r2.calls.is_empty(), "{:?}", r2.calls);
    assert!(!r2.figures.contains(&id("r2-calls")), "{:?}", r2.figures);
    // Every candidate's segments name calls by the same rule, dropped ones included.
    for candidate in &record.candidates {
        for operation in candidate.parts.iter().flat_map(|p| p.operations.iter()) {
            assert!(
                operation.calls.iter().all(|c| c.callee.as_str() != "p-z"),
                "{}: a call to a provider on no segment",
                candidate.candidate_id
            );
        }
    }
}

// verifies: REQ-MDL-9, DEC-4
#[test]
fn the_fixture_naming_calls_is_this_router_s_own_record() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/router-api/fixtures/valid")
        .join(format!("{FIXTURE}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let fixture: RecordedDecision = serde_json::from_str(&text).expect("a valid fixture");
    assert_eq!(fixture.statement, two_ramps());
}
