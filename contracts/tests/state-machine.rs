//! The state-machine contract's tests: its schemas against its fixtures, its state list against SPEC, and its
//! Rust types against its schemas (`DEC-4`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::state_machine::{
    Order, OrderState, RecordedInput, SegmentPlan, SegmentState, VERSION,
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
    for s in SHAPES {
        registry = registry
            .add(urn(s), schema(s))
            .unwrap_or_else(|e| panic!("{s}: {e}"));
    }
    let registry = registry.prepare().expect("registry");
    jsonschema::options()
        .with_registry(&registry)
        .should_validate_formats(formats)
        .build(&schema(shape))
        .unwrap_or_else(|e| panic!("{shape}: {e}"))
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
        if validator(shape).is_valid(&read_json(path)) {
            let name = path.file_name().unwrap().to_string_lossy();
            failures.push(format!("{name} should be rejected as {shape}"));
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
