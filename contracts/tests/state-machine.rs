//! The state-machine contract's tests: its schemas against its fixtures, its state list against SPEC, and its
//! Rust types against its schemas (`DEC-4`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::state_machine::{
    Destination, Id, NodeId, NodeRef, Observed, Order, OrderState, Part, RecordedInput, Segment,
    SegmentPlan, SegmentState, VERSION, after_no_route, deposit_failed_at_after,
    destination_change_applies, recalculation_keeps_destination, value_at,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The contract's shapes; each has `schemas/<shape>.schema.json` and the `$id` `urn:rutent:state-machine:<shape>`.
const SHAPES: [&str; 5] = [
    "order-state",
    "segment-state",
    "segment-plan",
    "recorded-input",
    "order",
];

/// `REQ-STM-2`'s table of states that must be expressible, `SPEC.md` lines 107 to 121, each row with the
/// published state that expresses it.
const SPEC_STATES: [(&str, &str); 13] = [
    (
        "Order: not yet past its first segment",
        "not_past_first_segment",
    ),
    (
        "Order: ended without completing, including cancelled by its reserve",
        "ended_without_completing",
    ),
    ("Order: first segment settled", "first_segment_settled"),
    ("Order: waiting for the customer", "waiting_for_customer"),
    ("Order: stuck", "stuck"),
    ("Order: complete", "complete"),
    ("Order: failed", "failed"),
    (
        "Order: handed on, its value passed to a linked order",
        "handed_on",
    ),
    (
        "Segment: started, from when its provider is bound",
        "started",
    ),
    ("Segment: settled", "settled"),
    (
        "Segment: failed, with the reversal owed",
        "failed_reversal_owed",
    ),
    ("Segment: outcome unknown", "outcome_unknown"),
    ("Segment: reversal complete", "reversal_complete"),
];

fn contract_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("state-machine")
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn schema(shape: &str) -> Value {
    read_json(
        &contract_dir()
            .join("schemas")
            .join(format!("{shape}.schema.json")),
    )
}

fn urn(shape: &str) -> String {
    format!("urn:rutent:state-machine:{shape}")
}

/// A validator for one shape, with every shape of the contract registered so `$ref`s by `$id` resolve offline.
fn validator(shape: &str) -> Validator {
    validator_with_formats(shape, true)
}

/// The same, with `format` asserted or left as an annotation, the 2020-12 default.
fn validator_with_formats(shape: &str, formats: bool) -> Validator {
    let mut registry = Registry::new();
    let others = schemas_of("provider-role")
        .into_iter()
        .chain(schemas_of("router-api"));
    for (id, s) in others.chain(SHAPES.iter().map(|s| (urn(s), schema(s)))) {
        registry = registry
            .add(id.clone(), s)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let registry = registry.prepare().expect("registry");
    jsonschema::options()
        .with_registry(&registry)
        .should_validate_formats(formats)
        .build(&schema(shape))
        .unwrap_or_else(|e| panic!("{shape}: {e}"))
}

/// Every schema of another contract this one refers to, by its own `$id`: the router's typed operation and the
/// provider-role shapes it is built from.
fn schemas_of(contract: &str) -> Vec<(String, Value)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(contract)
        .join("schemas");
    let mut out: Vec<(String, Value)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| read_json(&entry.unwrap().path()))
        .map(|s| {
            (
                s["$id"]
                    .as_str()
                    .expect("a schema names its $id")
                    .to_owned(),
                s,
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The shape a fixture belongs to: the longest shape its file name starts with, followed by `-`.
fn shape_of(fixture: &Path) -> &'static str {
    let name = fixture.file_stem().unwrap().to_str().unwrap();
    SHAPES
        .iter()
        .filter(|s| name.starts_with(&format!("{s}-")))
        .max_by_key(|s| s.len())
        .unwrap_or_else(|| panic!("{name} names no shape"))
}

fn fixtures(kind: &str) -> Vec<PathBuf> {
    let dir = contract_dir().join("fixtures").join(kind);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    paths
}

fn round_trip<T: Serialize + DeserializeOwned>(value: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

/// The fixture deserialised into its Rust type and serialised back.
fn through_rust(shape: &str, value: &Value) -> Result<Value, String> {
    match shape {
        "order-state" => round_trip::<OrderState>(value),
        "segment-state" => round_trip::<SegmentState>(value),
        "segment-plan" => round_trip::<SegmentPlan>(value),
        "recorded-input" => round_trip::<RecordedInput>(value),
        "order" => round_trip::<Order>(value),
        other => Err(format!("no Rust type for {other}")),
    }
}

fn schema_enum(shape: &str) -> BTreeSet<String> {
    schema(shape)["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("{shape} has no enum"))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn names<T: Serialize>(states: &[T]) -> BTreeSet<String> {
    states
        .iter()
        .map(|s| {
            serde_json::to_value(s)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

/// The first cell of every row of SPEC's table of states that must be expressible.
fn spec_table_rows() -> Vec<String> {
    let spec = Path::new(env!("CARGO_MANIFEST_DIR")).join("../SPEC.md");
    let text = fs::read_to_string(&spec).unwrap_or_else(|e| panic!("{}: {e}", spec.display()));
    let mut lines = text
        .lines()
        .skip_while(|l| !l.starts_with("| States that must be expressible |"));
    assert!(lines.next().is_some(), "SPEC has no table of states");
    lines
        .skip(1)
        .take_while(|l| l.starts_with('|'))
        .map(|l| l.split('|').nth(1).unwrap().trim().to_owned())
        .collect()
}

// verifies: REQ-STM-2
#[test]
fn state_list_covers_every_state_spec_requires() {
    let cited: Vec<String> = SPEC_STATES.iter().map(|(row, _)| row.to_string()).collect();
    assert_eq!(
        spec_table_rows(),
        cited,
        "the cited list has drifted from SPEC's table"
    );

    let order_names = schema_enum("order-state");
    let segment_names = schema_enum("segment-state");
    assert_eq!(
        names(&OrderState::ALL),
        order_names,
        "Rust and schema order lists differ"
    );
    assert_eq!(
        names(&SegmentState::ALL),
        segment_names,
        "Rust and schema segment lists differ"
    );

    for (row, state) in SPEC_STATES {
        let published = if row.starts_with("Order: ") {
            &order_names
        } else {
            &segment_names
        };
        assert!(
            published.contains(state),
            "SPEC requires \"{row}\", but `{state}` is not in the published list"
        );
    }
}

// verifies: REQ-STM-3
#[test]
fn input_from_outside_without_a_record_is_invalid() {
    let validator = validator("recorded-input");
    for case in [
        "recorded-input-without-record-id",
        "recorded-input-with-empty-record-id",
        "recorded-input-bare-observation-without-envelope",
    ] {
        let path = contract_dir()
            .join("fixtures/invalid")
            .join(format!("{case}.json"));
        let input = read_json(&path);
        assert!(!validator.is_valid(&input), "{case}: the schema accepts it");
        assert!(
            serde_json::from_value::<RecordedInput>(input).is_err(),
            "{case}: the Rust type accepts it"
        );
    }
}

// verifies: REQ-STM-1, REQ-STM-2, REQ-STM-3, REQ-STM-7, REQ-ORD-42, DEC-4, DEC-8
#[test]
fn every_valid_fixture_validates_and_every_invalid_one_is_rejected() {
    let valid = fixtures("valid");
    let invalid = fixtures("invalid");
    assert!(!valid.is_empty() && !invalid.is_empty(), "fixtures missing");

    let mut failures = Vec::new();
    for path in &valid {
        let shape = shape_of(path);
        let value = read_json(path);
        let name = path.file_name().unwrap().to_string_lossy();
        if let Err(e) = validator(shape).validate(&value) {
            failures.push(format!("{name} should validate as {shape}: {e}"));
        }
        match through_rust(shape, &value) {
            Ok(back) if back == value => {}
            Ok(back) => failures.push(format!("{name} changes through Rust: {back}")),
            Err(e) => failures.push(format!("{name} doesn't deserialise: {e}")),
        }
    }
    for path in &invalid {
        let shape = shape_of(path);
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let schema_valid = validator(shape).is_valid(&read_json(path));
        if schema_valid && !rust_only(stem) {
            let name = path.file_name().unwrap().to_string_lossy();
            failures.push(format!("{name} should be rejected as {shape}"));
        }
        if !schema_valid && rust_only(stem) {
            failures.push(format!(
                "{stem} is listed as Rust-only, but the schema rejects it"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// verifies: REQ-ORD-42, DEC-8
#[test]
fn an_order_of_three_parts_is_valid() {
    let order =
        read_json(&contract_dir().join("fixtures/valid/order-of-three-parts-ending-apart.json"));
    assert!(validator("order").is_valid(&order));
    let order: Order = serde_json::from_value(order).unwrap();
    assert_eq!(order.parts.len(), 3);
}

// verifies: DEC-4
#[test]
fn version_matches_the_changelog() {
    let changelog = fs::read_to_string(contract_dir().join("CHANGELOG.md")).unwrap();
    let heading = changelog
        .lines()
        .find_map(|l| l.strip_prefix("## "))
        .expect("the changelog has no version heading");
    assert_eq!(heading.trim(), VERSION);
}

// verifies: DEC-4
#[test]
fn every_schema_names_draft_2020_12_and_its_urn() {
    for shape in SHAPES {
        let s = schema(shape);
        assert_eq!(
            s["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "{shape}"
        );
        assert_eq!(s["$id"], urn(shape).as_str(), "{shape}");
    }
}

/// Invalid fixtures the Rust types accept on purpose, each with its reason. Every other invalid fixture must fail
/// to deserialise, so a Rust value can't hold what the schema rejects (`DEC-4`).
///
/// Empty: every rule the schemas state is also checked in Rust, by a field type or by `Order::check` and
/// `RecordedInput::check`, which deserialisation runs. A fixture added here needs its reason beside it.
const ACCEPTED_BY_RUST_ON_PURPOSE: [&str; 0] = [];

/// Invalid fixtures that break a rule JSON Schema can't state, so the schema accepts them and only the Rust types
/// refuse them: each with the rule, as the text of the violation Rust gives.
const REJECTED_BY_RUST_ONLY: [(&str, &str); 6] = [
    // INV-10: the route's last deposit is by another reserve than the part's destination, to the same network.
    (
        "order-part-depositing-through-another-reserve",
        "a part's route ends with a deposit at its destination",
    ),
    // Each segment starts where the one before it ended (the router's Part::check).
    (
        "order-segments-that-do-not-chain",
        "each segment starts where the one before it ended",
    ),
    // While the last segment is a failed deposit, deposit_failed_at is its from node.
    (
        "order-failed-final-deposit-without-deposit-failed-at",
        "deposit_failed_at is that deposit's from node",
    ),
    // deposit_failed_at only where a final deposit failed: the last segment, or one a recalculation retired.
    (
        "order-deposit-failed-at-with-no-failed-deposit",
        "deposit_failed_at only where a final deposit failed",
    ),
    // A recalculated route chains, segment to segment.
    (
        "recorded-input-route-recalculated-that-does-not-chain",
        "each segment starts where the one before it ended",
    ),
    // A recalculated route ends with a deposit (REQ-ORD-16).
    (
        "recorded-input-route-recalculated-not-ending-with-a-deposit",
        "a route ends with a deposit",
    ),
];

fn rust_only(stem: &str) -> bool {
    REJECTED_BY_RUST_ONLY.iter().any(|(name, _)| *name == stem)
}

// verifies: DEC-4, INV-10, REQ-ORD-16, REQ-ORD-43
#[test]
fn each_rule_only_rust_can_check_refuses_its_fixture_with_its_own_violation() {
    for (name, rule) in REJECTED_BY_RUST_ONLY {
        let path = contract_dir()
            .join("fixtures/invalid")
            .join(format!("{name}.json"));
        match through_rust(shape_of(&path), &read_json(&path)) {
            Err(e) => assert!(
                e.contains(rule),
                "{name} was refused for another reason: {e}"
            ),
            Ok(_) => panic!("{name}: Rust accepts it"),
        }
    }
}

// verifies: DEC-4, REQ-STM-1, REQ-STM-3, REQ-ORD-43
#[test]
fn every_invalid_fixture_is_rejected_by_the_rust_types_too() {
    let mut accepted = Vec::new();
    for path in fixtures("invalid") {
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let shape = shape_of(&path);
        if through_rust(shape, &read_json(&path)).is_ok()
            && !ACCEPTED_BY_RUST_ON_PURPOSE.contains(&name.as_str())
        {
            accepted.push(name);
        }
    }
    assert!(accepted.is_empty(), "Rust accepts: {}", accepted.join(", "));
}

fn valid_fixture(name: &str) -> Value {
    read_json(&contract_dir().join(format!("fixtures/valid/{name}.json")))
}

// verifies: REQ-STM-4, DEC-4
#[test]
fn a_segment_id_is_never_reused_in_an_order_s_life() {
    let mut order = valid_fixture("order-last-input-late");
    assert!(serde_json::from_value::<Order>(order.clone()).is_ok());

    // A current segment takes the id of a retired one: the schema can't see it, Rust refuses it.
    order["parts"][0]["segments"][2]["segment_id"] = "p1-s2-old".into();
    assert!(validator("order").is_valid(&order));
    assert!(serde_json::from_value::<Order>(order).is_err());
}

// verifies: REQ-AUD-1, DEC-4
#[test]
fn an_applied_last_input_is_the_one_that_reached_the_state() {
    let mut order = valid_fixture("order-last-input-applied");
    order["last_input"]["record_id"] = "rec-0099".into();
    assert!(validator("order").is_valid(&order));
    assert!(serde_json::from_value::<Order>(order).is_err());
}

// verifies: REQ-AUD-1
#[test]
fn each_outcome_of_an_input_has_a_valid_order() {
    let mut seen = BTreeSet::new();
    for path in fixtures("valid") {
        if shape_of(&path) == "order" {
            let order: Order = serde_json::from_value(read_json(&path)).unwrap();
            seen.insert(
                serde_json::to_value(order.last_input.outcome).unwrap()["outcome"].to_string(),
            );
        }
    }
    for outcome in ["applied", "repeat", "late", "not_applicable"] {
        assert!(
            seen.contains(&format!("\"{outcome}\"")),
            "no valid order whose last input is {outcome}"
        );
    }
}

// verifies: REQ-AUD-1
#[test]
fn a_time_that_is_not_a_date_time_is_rejected_without_format_assertion() {
    let input = read_json(
        &contract_dir().join("fixtures/invalid/recorded-input-with-time-not-a-date-time.json"),
    );
    assert!(!validator_with_formats("recorded-input", false).is_valid(&input));
}

fn part_of(fixture: &str) -> Part {
    let order: Order = serde_json::from_value(valid_fixture(fixture)).unwrap();
    order.parts.into_iter().next().unwrap()
}

/// The waiting part after a recalculation from the node where its final deposit failed: a route to `other`, then a
/// deposit from there, with the route's state given.
fn recalculated(route_state: &str, deposit_state: &str) -> Part {
    let mut order = valid_fixture("order-waiting-for-the-customer-after-a-failed-final-deposit");
    let part = &mut order["parts"][0];
    let failed = part["segments"][2].clone();
    let at = failed["from"].clone();
    let mut route = failed.clone();
    route["segment_id"] = "p1-s4".into();
    route["kind"] = "route".into();
    route["to"] = serde_json::json!({ "node": "other-ledger", "asset": at["asset"] });
    route["state"] = route_state.into();
    let mut deposit = failed.clone();
    deposit["segment_id"] = "p1-s5".into();
    deposit["from"] = route["to"].clone();
    deposit["state"] = deposit_state.into();
    let settled: Vec<Value> = part["segments"].as_array().unwrap()[..2].to_vec();
    part["segments"] = Value::Array(settled.into_iter().chain([route, deposit]).collect());
    part["retired_segments"] = serde_json::json!(["p1-s3"]);
    serde_json::from_value::<Order>(order)
        .unwrap()
        .parts
        .remove(0)
}

// verifies: REQ-ORD-28
#[test]
fn a_part_whose_value_cannot_reach_the_destination_reserve_s_node_becomes_stuck() {
    let part = part_of("order-stuck-with-no-route-and-no-failed-final-deposit");
    assert_eq!(after_no_route(&part), Some(OrderState::Stuck));

    // Its final deposit failed, but a recalculation's route moved the value on to another node.
    let away = recalculated("settled", "pending");
    assert_eq!(
        after_no_route(&away),
        Some(OrderState::Stuck),
        "the value left the deposit's node"
    );
}

// verifies: REQ-ORD-43
#[test]
fn a_part_whose_value_sits_at_the_destination_node_after_a_failed_final_deposit_waits_for_the_customer()
 {
    let part = part_of("order-waiting-for-the-customer-after-a-failed-final-deposit");
    assert_eq!(after_no_route(&part), Some(OrderState::WaitingForCustomer));

    // A recalculation from that node failed and was reversed: the value is back where the deposit failed.
    let back = recalculated("reversal_complete", "pending");
    assert_eq!(after_no_route(&back), Some(OrderState::WaitingForCustomer));
    let mut forgotten = back.clone();
    forgotten.deposit_failed_at = None;
    assert_eq!(
        after_no_route(&forgotten),
        Some(OrderState::Stuck),
        "without deposit_failed_at the segments can't show where the deposit failed"
    );
}

fn input_on(part: &str, segment: Option<&str>, observed: Value) -> RecordedInput {
    let mut input = valid_fixture("recorded-input-no-route");
    input["record_id"] = "rec-0090".into();
    input["subject"] = serde_json::json!({ "order": "ord-1", "part": part });
    if let Some(segment) = segment {
        input["subject"]["segment"] = segment.into();
    }
    input["observed"] = observed;
    serde_json::from_value(input).unwrap()
}

// verifies: REQ-ORD-28, REQ-ORD-43, INV-10
#[test]
fn deposit_failed_at_is_overwritten_by_each_failed_final_deposit_cleared_by_a_destination_change_and_kept_by_a_recalculation()
 {
    let waiting = part_of("order-waiting-for-the-customer-after-a-failed-final-deposit");
    let node = |text: &str| Some(NodeId::new(text).unwrap());
    assert_eq!(waiting.deposit_failed_at, node("ledger-in"));

    // A recalculation keeps it.
    let usdc = serde_json::json!({ "id": "USDC", "decimals": 6 });
    let route = |to: Value| {
        serde_json::json!([
            { "segment_id": "p1-s7", "provider": "ramp-b", "kind": "route",
              "from": { "node": "ledger-in", "asset": usdc }, "to": { "node": "ledger-b", "asset": usdc } },
            { "segment_id": "p1-s8", "provider": "reserve-b", "kind": "deposit",
              "from": { "node": "ledger-b", "asset": usdc }, "to": to }
        ])
    };
    let recalculation = input_on(
        "p1",
        None,
        serde_json::json!({ "kind": "route_recalculated", "decision_record": "dr-1",
            "segments": route(serde_json::json!({ "node": { "network": "polygon" }, "asset": usdc })) }),
    );
    assert_eq!(
        deposit_failed_at_after(&waiting, &recalculation),
        node("ledger-in")
    );

    // The customer accepts another destination, from ledger-in through ledger-b to the bank: it is cleared.
    let changed = input_on(
        "p1",
        None,
        serde_json::json!({ "kind": "destination_changed", "decision_record": "dr-2", "acceptance": "acc-1",
            "segments": route(serde_json::json!({ "node": { "network": "bank" }, "asset": usdc })) }),
    );
    assert_eq!(deposit_failed_at_after(&waiting, &changed), None);

    // On the new route, ledger-in to ledger-b and out to the bank, the first segment is reversed: the value is
    // back at ledger-in, but the destination reserve now lives at ledger-b, so the part is stuck, not waiting.
    let Observed::DestinationChanged { segments, .. } = &changed.observed else {
        unreachable!()
    };
    let mut moved = waiting.clone();
    moved.segments.truncate(2);
    moved.segments.push(Segment::new(
        segments[0].clone(),
        SegmentState::ReversalComplete,
    ));
    moved
        .segments
        .push(Segment::new(segments[1].clone(), SegmentState::Pending));
    moved.retired_segments = vec![Id::new("p1-s3").unwrap()];
    moved.destination = Destination::of(segments).unwrap();
    moved.deposit_failed_at = deposit_failed_at_after(&waiting, &changed);
    assert_eq!(
        value_at(&moved).map(|e| &e.node),
        Some(&NodeRef::Inner("ledger-in".into()))
    );
    assert_eq!(after_no_route(&moved), Some(OrderState::Stuck));
    let mut kept = moved.clone();
    kept.deposit_failed_at = waiting.deposit_failed_at.clone();
    assert_eq!(
        after_no_route(&kept),
        Some(OrderState::WaitingForCustomer),
        "had the change kept deposit_failed_at, the part would wait where it should be stuck"
    );

    // The new route's deposit, at other-ledger, fails too: that failure overwrites it.
    let mut second = recalculated("settled", "started");
    let failed = input_on(
        "p1",
        Some("p1-s5"),
        serde_json::json!({ "kind": "segment_failed" }),
    );
    assert_eq!(
        deposit_failed_at_after(&second, &failed),
        node("other-ledger")
    );
    second.deposit_failed_at = deposit_failed_at_after(&second, &failed);
    second.segments[3].state = SegmentState::FailedReversalOwed;
    assert_eq!(
        after_no_route(&second),
        Some(OrderState::WaitingForCustomer)
    );

    // A failure of a segment that isn't the final deposit leaves it as it was.
    let earlier = input_on(
        "p1",
        Some("p1-s4"),
        serde_json::json!({ "kind": "segment_failed" }),
    );
    assert_eq!(
        deposit_failed_at_after(&waiting, &earlier),
        node("ledger-in")
    );
}

// verifies: REQ-ORD-28
#[test]
fn a_part_whose_value_sits_away_from_every_deposit_node_becomes_stuck() {
    let part = part_of("order-stuck-with-its-value-away-from-the-deposit-node");
    assert_eq!(
        value_at(&part).map(|e| &e.node),
        Some(&NodeRef::Inner("ledger-in".into()))
    );
    assert_eq!(after_no_route(&part), Some(OrderState::Stuck));
}

// verifies: REQ-ORD-12, REQ-ORD-40, REQ-ORD-28
#[test]
fn a_part_whose_value_is_still_outside_the_book_is_not_placed() {
    let mut part = part_of("order-stuck-with-no-route-and-no-failed-final-deposit");
    for segment in &mut part.segments {
        segment.state = SegmentState::Pending;
    }
    assert_eq!(
        value_at(&part),
        None,
        "nothing has moved: the value is still at the outer node"
    );
    assert_eq!(after_no_route(&part), None);
}

// verifies: REQ-ORD-28, REQ-ORD-43
#[test]
fn a_failed_segment_that_isn_t_a_deposit_leaves_the_value_in_flight() {
    let mut part = part_of("order-stuck-with-no-route-and-no-failed-final-deposit");
    part.segments[1].state = SegmentState::FailedReversalOwed;
    assert_eq!(
        value_at(&part),
        None,
        "the exchange owes its reversal: the value isn't placed yet"
    );
    assert_eq!(after_no_route(&part), None);
}

// verifies: INV-10
#[test]
fn a_recalculation_keeps_the_destination_reserve_its_node_and_its_asset() {
    let part = part_of("order-stuck-with-no-route-and-no-failed-final-deposit");
    let route: Vec<SegmentPlan> = part.segments.iter().map(|s| s.plan()).collect();
    assert!(recalculation_keeps_destination(&part, &route));
    let mut other_reserve = route.clone();
    other_reserve.last_mut().unwrap().provider =
        rutent_contracts::state_machine::Id::new("reserve-other").unwrap();
    assert!(
        !recalculation_keeps_destination(&part, &other_reserve),
        "another reserve, the same network"
    );
    let mut other_unit = route.clone();
    let last = other_unit.last_mut().unwrap();
    last.to.asset.decimals = 2;
    assert!(
        !recalculation_keeps_destination(&part, &other_unit),
        "the same asset in another unit"
    );
}

// verifies: REQ-ORD-28, REQ-ORD-43, REQ-STM-4
#[test]
fn no_route_while_the_value_is_in_flight_places_nothing() {
    let mut part = part_of("order-stuck-with-no-route-and-no-failed-final-deposit");
    part.segments[1].state = SegmentState::Started;
    assert_eq!(after_no_route(&part), None);
    part.segments[1].state = SegmentState::OutcomeUnknown;
    assert_eq!(after_no_route(&part), None);
}

// verifies: REQ-ORD-37, REQ-STM-7, DEC-4
#[test]
fn the_router_s_operation_rules_the_schema_cannot_state_hold_on_every_segment() {
    let plan = valid_fixture("segment-plan-changes-no-value");
    type Break = fn(&mut Value);
    let cases: [(&str, Break); 3] = [
        ("a route that changes its asset", |p| {
            p["to"]["asset"]["id"] = "EURC".into()
        }),
        ("a route to its own node", |p| {
            p["to"]["node"] = p["from"]["node"].clone()
        }),
        ("an exchange across two nodes", |p| {
            p["kind"] = "exchange".into();
            p["to"]["asset"]["id"] = "EURC".into();
        }),
    ];
    let schema = validator("segment-plan");
    for (case, breaks) in cases {
        let mut value = plan.clone();
        breaks(&mut value);
        assert!(
            schema.is_valid(&value),
            "{case}: the schema already rejects it"
        );
        assert!(
            serde_json::from_value::<SegmentPlan>(value).is_err(),
            "{case}: Rust accepts it"
        );
    }
}

// verifies: INV-10, REQ-ORD-43
#[test]
fn a_destination_change_that_keeps_the_destination_is_a_recalculation_and_not_applicable() {
    let waiting = part_of("order-waiting-for-the-customer-after-a-failed-final-deposit");
    let mut route: Vec<SegmentPlan> = waiting.segments[2..].iter().map(|s| s.plan()).collect();
    route[0].segment_id = Id::new("p1-s9").unwrap();
    let mut input = valid_fixture("recorded-input-destination-changed-with-acceptance");
    input["subject"] = serde_json::json!({ "order": "ord-1", "part": "p1" });
    input["observed"]["segments"] = serde_json::to_value(&route).unwrap();
    let same: RecordedInput = serde_json::from_value(input).unwrap();

    assert!(recalculation_keeps_destination(&waiting, &route));
    assert!(
        !destination_change_applies(&waiting, &route),
        "the route ends at the part's destination"
    );
    assert_eq!(
        deposit_failed_at_after(&waiting, &same),
        waiting.deposit_failed_at,
        "a change that isn't one leaves deposit_failed_at as it was"
    );
    assert_eq!(
        after_no_route(&waiting),
        Some(OrderState::WaitingForCustomer)
    );
}
