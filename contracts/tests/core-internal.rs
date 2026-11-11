//! The core-internal contract's tests: its schemas against its fixtures, and its Rust types against its schemas
//! (`DEC-4`, `T-020-01`).

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::core_internal::calls::{
    AppendFailure, Authorisations, Charges, Comparisons, DECISION_ON_THE_GENERAL_HANDLE,
    DecisionEntry, Decisions, Delivery, Evidence, Fetched, GeneralEntry, Kept, Log, Measures,
    Operator, PersonQuestions, PolicyLog, RECORD_ON_THE_POLICY_HANDLE, Reads, Sessions,
    StuckOrders, is_decision,
};
use rutent_contracts::core_internal::{
    Action, ActorKind, CatalogueChange, KeptStatement, PermissionChange, RELATIONS, Role,
    Violation, What, admit_entry,
};
use rutent_contracts::core_internal::{
    Authorisation, Charge, ChargeAnswer, Decision, DecisionRequest, Effect, Entry, HandedOver,
    HoldingReading, MeasureRecord, PersonAnswer, PersonQuestion, Raise, ReadAnswer, ReadCheck,
    Receipt, ReportPair, SessionAnswer, SessionRequest, StatedEntry, StuckDecide, StuckMove,
    VERSION, VerifiedEntry,
};
use rutent_contracts::core_internal::{Quantifier, Through};
use rutent_contracts::router_api::Id;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The contract's shapes; each has `schemas/<shape>.schema.json` and the `$id` `urn:rutent:core-internal:<shape>`.
const SHAPES: [&str; 26] = [
    "common",
    "receipt",
    "decision-request",
    "decision",
    "effect",
    "authorisation",
    "stuck-decide",
    "stuck-move",
    "session-request",
    "session-answer",
    "read-check",
    "read-answer",
    "charge",
    "charge-answer",
    "person-question",
    "person-answer",
    "report-pair",
    "holding-reading",
    "measure-record",
    "handover",
    "raise",
    "verified-entry",
    "stated-entry",
    "entry",
    "kept-statement",
    "permission-change",
];

/// The other contracts whose shapes these schemas refer to by `$id`, directly or through another's.
const REFERRED: [&str; 4] = [
    "provider-role",
    "module-interface",
    "router-api",
    "state-machine",
];

/// Invalid fixtures the schemas accept and the Rust types refuse, each with the refusal it must give. Every other
/// invalid fixture is listed in `SCHEMA_RULES`.
const RUST_RULES: [(&str, &str); 28] = [
    (
        "permission-change-by-a-provider",
        "a read permission is granted or revoked by the operator",
    ),
    (
        "permission-change-granted-to-a-customer",
        "a read permission is a regulator's or a checker's",
    ),
    (
        "person-question-asking-off-its-shortlist",
        "a question on a shortlisted route asks only the shortlisted providers",
    ),
    (
        "verified-entry-of-a-customers-permitted-read-of-the-trail",
        "a customer's permitted read is of its own orders, never the trail",
    ),
    (
        "verified-entry-of-a-providers-permitted-read-of-an-order",
        "a provider's permitted read is of a segment, never an order or the trail",
    ),
    (
        "verified-entry-of-a-regulators-read-without-its-permission",
        "a regulator's or a checker's permitted read names its permission",
    ),
    (
        "verified-entry-of-a-shortlist-record-without-its-providers",
        "a decision record names shortlisted providers exactly when it is a shortlist's",
    ),
    (
        "verified-entry-of-an-acceptance-given-without-the-reserves-yes",
        "an acceptance given names the reserve's yes and the session it was asked in, and an automatic deposit neither",
    ),
    (
        "verified-entry-of-a-command-by-a-provider",
        "an operator command is the operator's",
    ),
    (
        "charge-to-another-party-than-its-callers",
        "work is charged to the accountable party its caller acts for",
    ),
    (
        "decision-request-marking-stale-without-a-version",
        "a catalogue change names a version exactly when it is about a module release",
    ),
    (
        "decision-request-setting-slippage-on-a-version",
        "a catalogue change names a version exactly when it is about a module release",
    ),
    (
        "decision-request-setting-slippage-without-a-tuple",
        "a catalogue change names a tuple exactly when it is about a tuple",
    ),
    (
        "effect-granting-without-a-tuple",
        "a catalogue change names a tuple exactly when it is about a tuple",
    ),
    (
        "handover-secret-use-of-the-statement-key-naming-an-operation",
        "a proxy use names its operation exactly when it asked for a secret",
    ),
    (
        "handover-segment-record-running-on-a-version-it-never-moved-to",
        "a segment record's moves lead from the version it began on to the one it runs on",
    ),
    (
        "handover-segment-record-whose-moves-skip-a-version",
        "a segment record's moves lead from the version it began on to the one it runs on",
    ),
    (
        "holding-reading-naming-an-order-twice",
        "a holding account reading names each order once",
    ),
    (
        "raise-of-a-charge-within-its-maximum",
        "a raise's difference is a real one: two amounts or times that differ, a charge above its maximum",
    ),
    (
        "raise-of-reports-that-agree",
        "a raise's difference is a real one: two amounts or times that differ, a charge above its maximum",
    ),
    (
        "report-pair-of-one-party-reporting-both",
        "a segment's two reports are by two different parties",
    ),
    (
        "session-request-ending-before-it-is-asked",
        "a session ends after the time it is asked for",
    ),
    (
        "stated-entry-relayed-by-its-own-maker",
        "a stated entry's relay is a party other than its maker",
    ),
    (
        "verified-entry-of-a-decision-observed",
        "an entry's outcome is one its call can have",
    ),
    (
        "verified-entry-of-a-raise-merely-observed",
        "an entry's outcome is one its call can have",
    ),
    (
        "verified-entry-of-a-session-outliving-its-until",
        "an opened session ends after it was asked for and no later than its until",
    ),
    (
        "verified-entry-of-an-effect-already-done",
        "an entry's outcome is one its call can have",
    ),
    (
        "verified-entry-of-an-opened-session-without-its-session",
        "a session entry names the session exactly when it was opened",
    ),
];

/// Invalid fixtures the schemas reject: each with the exact (instance location, keyword) pairs of the one rule it
/// breaks, without repeats, and the refusal the Rust types give it.
/// One invalid fixture the schemas reject: its name, its (instance location, keyword) pairs, and Rust's refusal.
type SchemaRule = (
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static str,
);

const SCHEMA_RULES: [SchemaRule; 77] = [
    (
        "permission-change-without-its-grantee",
        &[("", "required")],
        "missing field `grantee`",
    ),
    (
        "authorisation-naming-two-orders",
        &[("/order", "type")],
        "invalid type: sequence",
    ),
    (
        "authorisation-with-a-floor-of-zero",
        &[("/floor", "pattern")],
        "a positive amount is up to 78 decimal digits with no leading zeros, above zero",
    ),
    (
        "authorisation-without-its-origin-reserve",
        &[("", "required")],
        "missing field `origin_reserve`",
    ),
    (
        "authorisation-without-its-to-asset",
        &[("", "required")],
        "missing field `to_asset`",
    ),
    (
        "charge-of-no-work",
        &[("/count", "minimum")],
        "a count is 1 to 2^53 - 1",
    ),
    (
        "charge-to-no-party",
        &[("", "required")],
        "missing field `party`",
    ),
    (
        "decision-permitting-without-a-receipt",
        &[("", "required")],
        "missing field `receipt`",
    ),
    (
        "decision-refusing-without-its-class",
        &[("/outcome", "required")],
        "missing field `class`",
    ),
    (
        "decision-request-by-an-actor-of-no-kind",
        &[("/caller", "required")],
        "missing field `kind`",
    ),
    (
        "decision-request-moving-value-under-no-authorisation",
        &[("/action", "required")],
        "missing field `authorisation`",
    ),
    (
        "decision-request-moving-value-without-the-origin-reserve",
        &[("/action", "required")],
        "missing field `origin_reserve`",
    ),
    (
        "decision-request-moving-value-without-the-segments-provider",
        &[("/action", "required")],
        "missing field `provider`",
    ),
    (
        "decision-request-of-an-unknown-action",
        &[("/action/kind", "enum")],
        "unknown variant `pay_out`",
    ),
    (
        "decision-request-to-move-a-stuck-orders-value",
        &[("/action", "not")],
        "a decision request never asks to move a stuck order's value: only a stuck move does",
    ),
    (
        "verified-entry-of-a-command-that-is-a-catalogue-change",
        &[("/what/command", "enum")],
        "unknown variant `mark_stale`",
    ),
    (
        "verified-entry-of-a-command-under-a-synonym",
        &[("/what/command", "enum")],
        "unknown variant `move_funds`",
    ),
    (
        "effect-citing-a-permit-without-its-receipt",
        &[("/decision", "required")],
        "missing field `receipt`",
    ),
    (
        "effect-citing-a-refusal",
        &[("/decision/answer", "const")],
        "unknown variant `refuse`",
    ),
    (
        "effect-without-a-decision",
        &[("", "required")],
        "missing field `decision`",
    ),
    (
        "effect-without-a-log-receipt",
        &[("", "required")],
        "missing field `receipt`",
    ),
    (
        "entry-of-no-kind",
        &[("", "required")],
        "an entry's kind is stated or verified",
    ),
    (
        "handover-cross-module-call-without-its-transaction",
        &[("/call", "required")],
        "missing field `transaction`",
    ),
    (
        "handover-limit-without-its-transaction",
        &[("", "required")],
        "missing field `transaction`",
    ),
    (
        "handover-outbound-call-without-its-transaction",
        &[("/record", "required")],
        "missing field `transaction`",
    ),
    (
        "handover-secret-lookup-without-its-transaction",
        &[("/lookup", "required")],
        "missing field `transaction`",
    ),
    (
        "handover-secret-use-carrying-the-value",
        &[("/outcome", "additionalProperties")],
        "unknown field `value`",
    ),
    (
        "handover-secret-use-without-its-transaction",
        &[("", "required")],
        "missing field `transaction`",
    ),
    (
        "handover-segment-record-without-its-transaction",
        &[("", "required")],
        "missing field `transaction`",
    ),
    (
        "handover-segment-record-without-the-version-it-began-on",
        &[("", "required")],
        "missing field `began_on`",
    ),
    (
        "handover-state-read-without-its-transaction",
        &[("", "required")],
        "missing field `transaction`",
    ),
    (
        "holding-reading-without-its-order-attribution",
        &[("", "required")],
        "missing field `held_for`",
    ),
    (
        "kept-statement-with-a-short-salt",
        &[("/salt", "pattern")],
        "a salt is 32 bytes, base64url without padding, in its canonical form",
    ),
    (
        "kept-statement-with-padded-bytes",
        &[("/bytes", "pattern")],
        "bytes are base64url without padding, at least one, in their canonical form",
    ),
    (
        "measure-record-citing-nothing",
        &[("/cites", "minItems")],
        "a measure record cites the entries it rests on, at least one, each once",
    ),
    (
        "measure-record-without-its-provider",
        &[("", "required")],
        "missing field `provider`",
    ),
    (
        "person-question-asking-a-provider-twice",
        &[("/ask", "uniqueItems")],
        "a question names who it asks, at least one, each once",
    ),
    (
        "person-question-asking-no-one",
        &[("/ask", "minItems")],
        "a question names who it asks, at least one, each once",
    ),
    (
        "person-question-on-a-shortlist-naming-no-providers",
        &[("/authority/providers", "minItems")],
        "a shortlisted route names its providers, at least one, each once",
    ),
    (
        "person-question-without-its-authority",
        &[("", "required")],
        "missing field `authority`",
    ),
    (
        "person-question-without-the-providers-already-asked",
        &[("", "required")],
        "missing field `asked_before`",
    ),
    (
        "raise-carrying-an-order-state-change",
        &[("", "additionalProperties")],
        "unknown field `order_state`",
    ),
    (
        "raise-citing-nothing",
        &[("/cites", "minItems")],
        "a raise cites the entries it rests on, at least one, each once",
    ),
    (
        "read-answer-without-a-receipt",
        &[("", "required")],
        "missing field `receipt`",
    ),
    (
        "read-check-of-a-segment-without-its-provider",
        &[("/reads", "required")],
        "missing field `provider`",
    ),
    (
        "read-check-of-nothing",
        &[("/reads", "required")],
        "missing field `kind`",
    ),
    (
        "read-check-without-the-readers-role",
        &[("/reader", "required")],
        "missing field `role`",
    ),
    (
        "receipt-at-a-negative-index",
        &[("/index", "minimum")],
        "invalid value: integer `-1`",
    ),
    (
        "report-pair-without-the-receiving-partys-report",
        &[("", "required")],
        "missing field `received`",
    ),
    (
        "report-pair-without-the-sending-partys-report",
        &[("", "required")],
        "missing field `sent`",
    ),
    (
        "stated-entry-about-nothing",
        &[("/about", "minProperties")],
        "a stated entry's about names an order, a segment or a transaction",
    ),
    (
        "stated-entry-carrying-its-signatures-value",
        &[("/signed_with", "additionalProperties")],
        "unknown field `value`",
    ),
    (
        "stated-entry-carrying-its-statement",
        &[("", "additionalProperties")],
        "unknown field `statement`",
    ),
    (
        "stated-entry-timed-by-cores-clock",
        &[("/when/source", "const")],
        "unknown variant `core_clock`",
    ),
    (
        "stated-entry-unverified-but-attributed",
        &[
            ("/attribution", "additionalProperties"),
            ("/attribution", "required"),
        ],
        "unknown field `who`",
    ),
    (
        "stated-entry-without-its-commitment",
        &[("", "required")],
        "missing field `commitment`",
    ),
    (
        "stated-entry-without-the-key-it-was-signed-with",
        &[("", "required")],
        "missing field `signed_with`",
    ),
    (
        "stated-entry-without-the-source-of-its-time",
        &[("/when", "required")],
        "missing field `source`",
    ),
    (
        "stuck-decide-naming-no-evidence",
        &[("/evidence", "minItems")],
        "a stuck order's decision names its evidence, at least one item, each once",
    ),
    (
        "stuck-move-without-the-decision-it-carries-out",
        &[("", "required")],
        "missing field `carries_out`",
    ),
    (
        "verified-entry-carrying-a-statement",
        &[("", "additionalProperties")],
        "unknown field `statement`",
    ),
    (
        "verified-entry-of-a-decision-record-without-its-digest",
        &[("/what", "required")],
        "missing field `record`",
    ),
    (
        "verified-entry-of-a-decision-record-without-its-id",
        &[("/what", "required")],
        "missing field `decision_record`",
    ),
    (
        "verified-entry-of-a-decision-without-its-id",
        &[("/what", "required")],
        "missing field `decision`",
    ),
    (
        "verified-entry-of-a-fee-to-a-third-party",
        &[("/what/to", "enum")],
        "unknown variant `verifier`",
    ),
    (
        "verified-entry-of-a-hand-over-without-its-place-in-its-drain",
        &[("/what", "required")],
        "missing field `position`",
    ),
    (
        "verified-entry-of-a-holding-check-carrying-a-balance",
        &[("/what", "additionalProperties")],
        "unknown field `balance`",
    ),
    (
        "verified-entry-of-a-refusal-carrying-a-providers-text",
        &[("/what", "additionalProperties")],
        "unknown field `text`",
    ),
    (
        "verified-entry-of-a-settlement-without-its-declared-time",
        &[("/what", "required")],
        "missing field `declared_seconds`",
    ),
    (
        "verified-entry-of-an-acceptance-without-its-id",
        &[("/what", "required")],
        "missing field `acceptance`",
    ),
    (
        "verified-entry-of-an-acceptance-without-its-parties",
        &[("/what", "required")],
        "missing field `parties`",
    ),
    (
        "verified-entry-of-an-effect-without-a-permit",
        &[("/what", "required")],
        "missing field `decision`",
    ),
    (
        "verified-entry-of-graded-evidence-resting-on-nothing",
        &[("/what/rests_on", "minItems")],
        "graded evidence rests on stated entries, at least one, each once",
    ),
    (
        "verified-entry-of-graded-evidence-without-its-id",
        &[("/what", "required")],
        "missing field `evidence`",
    ),
    (
        "verified-entry-of-reliance-naming-no-judge",
        &[("/what", "required")],
        "missing field `judge`",
    ),
    (
        "verified-entry-timed-by-a-partys-statement",
        &[("/when/source", "enum")],
        "unknown variant `party_stated`",
    ),
    (
        "verified-entry-without-the-source-of-its-time",
        &[("/when", "required")],
        "missing field `source`",
    ),
];

fn contracts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn contract_dir() -> PathBuf {
    contracts_dir().join("core-internal")
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
    format!("urn:rutent:core-internal:{shape}")
}

/// Every schema of another contract's directory, by `$id`.
fn schemas_of(contract: &str) -> Vec<(String, Value)> {
    let dir = contracts_dir().join(contract).join("schemas");
    let mut out: Vec<(String, Value)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".schema.json"))
        .map(|p| {
            let schema = read_json(&p);
            let id = schema["$id"]
                .as_str()
                .expect("schema has an $id")
                .to_owned();
            (id, schema)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A validator for one shape, with this contract's schemas and those it refers to registered by `$id`, so every
/// `$ref` resolves offline.
fn validator(shape: &str) -> Validator {
    let mut registry = Registry::new();
    let others = REFERRED.iter().flat_map(|c| schemas_of(c));
    for (id, s) in others.chain(SHAPES.iter().map(|s| (urn(s), schema(s)))) {
        registry = registry
            .add(id.clone(), s)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let registry = registry.prepare().expect("registry");
    jsonschema::options()
        .offline()
        .with_registry(&registry)
        .build(&schema(shape))
        .unwrap_or_else(|e| panic!("{shape}: {e}"))
}

/// The shape a fixture belongs to: the longest shape its file name starts with, followed by `-`.
fn shape_of(name: &str) -> &'static str {
    SHAPES
        .iter()
        .filter(|s| name.starts_with(&format!("{s}-")))
        .max_by_key(|s| s.len())
        .unwrap_or_else(|| panic!("{name} names no shape"))
}

/// Every fixture of a kind, as (file stem, JSON), sorted by name.
fn fixtures(kind: &str) -> Vec<(String, Value)> {
    let dir = contract_dir().join("fixtures").join(kind);
    let mut out: Vec<(String, Value)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (name, read_json(&p))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn fixture(kind: &str, name: &str) -> Value {
    read_json(
        &contract_dir()
            .join("fixtures")
            .join(kind)
            .join(format!("{name}.json")),
    )
}

fn round_trip<T: Serialize + DeserializeOwned>(value: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

/// The fixture deserialised into its Rust type and serialised back.
fn through_rust(shape: &str, value: &Value) -> Result<Value, String> {
    match shape {
        "receipt" => round_trip::<Receipt>(value),
        "decision-request" => round_trip::<DecisionRequest>(value),
        "decision" => round_trip::<Decision>(value),
        "effect" => round_trip::<Effect>(value),
        "authorisation" => round_trip::<Authorisation>(value),
        "stuck-decide" => round_trip::<StuckDecide>(value),
        "stuck-move" => round_trip::<StuckMove>(value),
        "session-request" => round_trip::<SessionRequest>(value),
        "session-answer" => round_trip::<SessionAnswer>(value),
        "read-check" => round_trip::<ReadCheck>(value),
        "read-answer" => round_trip::<ReadAnswer>(value),
        "charge" => round_trip::<Charge>(value),
        "charge-answer" => round_trip::<ChargeAnswer>(value),
        "person-question" => round_trip::<PersonQuestion>(value),
        "person-answer" => round_trip::<PersonAnswer>(value),
        "report-pair" => round_trip::<ReportPair>(value),
        "holding-reading" => round_trip::<HoldingReading>(value),
        "measure-record" => round_trip::<MeasureRecord>(value),
        "handover" => round_trip::<HandedOver>(value),
        "raise" => round_trip::<Raise>(value),
        "verified-entry" => round_trip::<VerifiedEntry>(value),
        "stated-entry" => round_trip::<StatedEntry>(value),
        "entry" => round_trip::<Entry>(value),
        "kept-statement" => round_trip::<KeptStatement>(value),
        "permission-change" => round_trip::<PermissionChange>(value),
        other => Err(format!("no Rust type for {other}")),
    }
}

/// The schema errors for an instance, as sorted (instance location, keyword) pairs without repeats.
fn schema_errors(validator: &Validator, instance: &Value) -> Vec<(String, String)> {
    let mut errors: Vec<(String, String)> = validator
        .iter_errors(instance)
        .map(|e| {
            (
                e.instance_path().as_str().to_owned(),
                e.kind().keyword().to_owned(),
            )
        })
        .collect();
    errors.sort();
    errors.dedup();
    errors
}

/// Asserts that an invalid fixture is refused: by the schemas and the Rust types both, or, for a rule only Rust can
/// state (`RUST_RULES`), accepted by the schemas and refused by the Rust types with that rule.
fn assert_rejected(name: &str) {
    let shape = shape_of(name);
    let value = fixture("invalid", name);
    let refused = through_rust(shape, &value);
    match RUST_RULES.iter().find(|(n, _)| *n == name) {
        Some((_, rule)) => {
            assert!(
                validator(shape).is_valid(&value),
                "{name} breaks a rule only Rust states, so the {shape} schema should accept it"
            );
            assert!(
                refused.as_ref().is_err_and(|e| e.contains(rule)),
                "{name} should be refused by the Rust types for its rule: {refused:?}"
            );
        }
        None => {
            assert!(
                !validator(shape).is_valid(&value),
                "{name} should be rejected by the {shape} schema"
            );
            assert!(
                refused.is_err(),
                "{name} should be refused by the Rust types"
            );
        }
    }
}

/// Asserts that the schemas accept a valid fixture and the Rust types read it.
fn assert_accepted_by_both(name: &str) {
    let shape = shape_of(name);
    let value = fixture("valid", name);
    assert!(validator(shape).is_valid(&value), "{name} should validate");
    through_rust(shape, &value).unwrap_or_else(|e| panic!("{name}: {e}"));
}

// verifies: REQ-AUZ-1
#[test]
fn value_moving_action_without_a_policy_decision_is_invalid() {
    assert_accepted_by_both("effect-moving-value-on-a-permit");
    assert_rejected("effect-without-a-decision");
    assert_rejected("effect-citing-a-refusal");
    assert_rejected("effect-citing-a-permit-without-its-receipt");
    assert_rejected("verified-entry-of-a-decision-without-its-id");
    assert_rejected("verified-entry-of-an-effect-without-a-permit");
}

// verifies: REQ-AUD-1
#[test]
fn log_entry_without_the_source_of_its_time_is_invalid() {
    assert_rejected("verified-entry-without-the-source-of-its-time");
    assert_rejected("stated-entry-without-the-source-of-its-time");
    // Each kind of entry is timed by its own source: rutent's clock for what it did, the maker's for what it said.
    assert_rejected("verified-entry-timed-by-a-partys-statement");
    assert_rejected("stated-entry-timed-by-cores-clock");
    for (name, value) in fixtures("invalid")
        .into_iter()
        .filter(|(n, _)| n.ends_with("-without-the-source-of-its-time"))
    {
        assert!(
            !validator("entry").is_valid(&value),
            "{name} should be rejected as an entry"
        );
        assert!(
            serde_json::from_value::<Entry>(value).is_err(),
            "{name} should be refused as an entry"
        );
    }
}

// verifies: REQ-AUD-2
#[test]
fn stated_entry_and_verified_entry_are_distinct_shapes() {
    let valid = fixtures("valid");
    let of = |shape: &str| {
        valid
            .iter()
            .filter(|(n, _)| shape_of(n) == shape)
            .cloned()
            .collect::<Vec<_>>()
    };
    let stated = of("stated-entry");
    let verified = of("verified-entry");
    assert!(!stated.is_empty() && !verified.is_empty());
    for (name, value) in &stated {
        assert!(
            !validator("verified-entry").is_valid(value),
            "{name} passes as a verified entry"
        );
        assert!(
            serde_json::from_value::<VerifiedEntry>(value.clone()).is_err(),
            "{name} reads as a verified entry"
        );
        let entry: Entry = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(entry, Entry::Stated(_)), "{name}");
    }
    for (name, value) in &verified {
        assert!(
            !validator("stated-entry").is_valid(value),
            "{name} passes as a stated entry"
        );
        assert!(
            serde_json::from_value::<StatedEntry>(value.clone()).is_err(),
            "{name} reads as a stated entry"
        );
        let entry: Entry = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(entry, Entry::Verified(_)), "{name}");
    }
    // Neither carries a party's statement itself: a stated entry holds the key it says signed it and a salted
    // commitment to the bytes as received, which the evidence store keeps with the signature's value, and an
    // unverified one names only who it claims made it.
    assert_rejected("verified-entry-carrying-a-statement");
    assert_rejected("stated-entry-carrying-its-statement");
    assert_rejected("stated-entry-without-the-key-it-was-signed-with");
    assert_rejected("stated-entry-carrying-its-signatures-value");
    assert_rejected("stated-entry-without-its-commitment");
    assert_rejected("stated-entry-unverified-but-attributed");
    assert_rejected("stated-entry-relayed-by-its-own-maker");
}

// verifies: REQ-AUZ-3
#[test]
fn authorisation_naming_two_orders_is_invalid() {
    assert_accepted_by_both("authorisation-of-one-order-with-a-floor");
    assert_rejected("authorisation-naming-two-orders");
}

// verifies: REQ-ORD-9
#[test]
fn raise_carrying_an_order_state_change_is_invalid() {
    assert_rejected("raise-carrying-an-order-state-change");
    // A raise holds its id, its difference, what it cites and its time, and nothing else.
    let raise = schema("raise");
    let mut fields: Vec<&str> = raise["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort();
    assert_eq!(fields, ["at", "cites", "difference", "raise"]);
    assert_eq!(raise["additionalProperties"], Value::Bool(false));
}

// verifies: INV-7
#[test]
fn effect_without_a_log_receipt_is_invalid() {
    assert_rejected("effect-without-a-log-receipt");
}

// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn fanned_out_work_charged_to_no_party_is_invalid() {
    assert_accepted_by_both("charge-of-fanned-out-work");
    assert_rejected("charge-to-no-party");
    assert_rejected("charge-to-another-party-than-its-callers");
}

// verifies: REQ-CPL-2
#[test]
fn question_about_a_person_without_its_authority_is_invalid() {
    assert_rejected("person-question-without-its-authority");
}

// verifies: REQ-CPL-5
#[test]
fn question_without_the_providers_already_asked_about_its_order_is_invalid() {
    assert_accepted_by_both("person-question-on-the-persons-authority-first-asked");
    assert_rejected("person-question-without-the-providers-already-asked");
}

// verifies: REQ-AUZ-7, REQ-AUD-12
#[test]
fn read_check_without_the_readers_role_is_invalid() {
    assert_rejected("read-check-without-the-readers-role");
    assert_rejected("read-check-of-a-segment-without-its-provider");
}

// verifies: REQ-ORD-9
#[test]
fn segment_report_pair_without_both_parties_reports_is_invalid() {
    assert_rejected("report-pair-without-the-sending-partys-report");
    assert_rejected("report-pair-without-the-receiving-partys-report");
    assert_rejected("report-pair-of-one-party-reporting-both");
}

// verifies: REQ-AUD-15
#[test]
fn holding_account_reading_without_its_order_attribution_is_invalid() {
    assert_accepted_by_both("holding-reading-with-nothing-held");
    assert_rejected("holding-reading-without-its-order-attribution");
    assert_rejected("holding-reading-naming-an-order-twice");
}

// verifies: REQ-AUD-7
#[test]
fn measure_record_without_its_provider_is_invalid() {
    assert_rejected("measure-record-without-its-provider");
}

// verifies: REQ-MDL-8
#[test]
fn platform_event_handed_to_the_log_without_its_transaction_is_invalid() {
    let missing: Vec<String> = fixtures("invalid")
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| n.starts_with("handover-") && n.ends_with("-without-its-transaction"))
        .collect();
    // Every kind of item the platform or the proxy hands over.
    assert_eq!(missing.len(), 7, "{missing:?}");
    for name in &missing {
        assert_rejected(name);
    }
}

// verifies: REQ-MDL-5
#[test]
fn segment_record_without_the_version_it_began_on_is_invalid() {
    assert_accepted_by_both("handover-segment-record-moved-once");
    assert_rejected("handover-segment-record-without-the-version-it-began-on");
    assert_rejected("handover-segment-record-whose-moves-skip-a-version");
}

// verifies: REQ-AUZ-1
#[test]
fn a_permit_comes_from_a_permitting_decision_and_names_its_entry() {
    fn read<T: DeserializeOwned>(name: &str) -> T {
        serde_json::from_value(fixture("valid", name)).unwrap()
    }
    let permitting: Decision = read("decision-permitting");
    let permit = permitting.permit().expect("a permit");
    assert_eq!(permit.receipt(), &permitting.receipt);
    assert_eq!(
        (permit.request(), permit.decision()),
        (&permitting.request, &permitting.decision)
    );
    let effect: Effect = read("effect-moving-value-on-a-permit");
    assert_eq!(effect.decision, permit);
    for name in [
        "decision-refusing-outside-the-authorisation",
        "decision-not-known-as-policy-is-unreachable",
    ] {
        assert!(read::<Decision>(name).permit().is_none(), "{name}");
    }
}

/// Every case of `fixtures/admission/`, as (name, case).
fn admission_fixtures() -> Vec<(String, Value)> {
    let dir = contract_dir().join("fixtures").join("admission");
    let mut out: Vec<(String, Value)> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            (
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                read_json(&p),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The handle a case's entry comes through: the one it names, or else the one its kind belongs to.
fn through(case: &Value, entry: &Entry) -> Through {
    match case["through"].as_str() {
        Some("general") => Through::General,
        Some("policy") => Through::Policy,
        _ => match entry {
            Entry::Verified(v) if is_decision(&v.what) => Through::Policy,
            _ => Through::General,
        },
    }
}

/// What a case expects of [`admit_entry`]: admitted, or refused with the row's own refusal or its field's.
fn expected(name: &str, case: &Value) -> Result<(), Violation> {
    if case["breaks"] == "handle" {
        let appended: Entry = serde_json::from_value(case["entry"].clone()).unwrap();
        return Err(Violation(match through(case, &appended) {
            Through::General => DECISION_ON_THE_GENERAL_HANDLE,
            Through::Policy => RECORD_ON_THE_POLICY_HANDLE,
        }));
    }
    let Some(row) = case["row"].as_str() else {
        return Ok(());
    };
    let relation = RELATIONS
        .iter()
        .find(|r| r.name == row)
        .unwrap_or_else(|| panic!("{name}: no row {row}"));
    match case["breaks"].as_str() {
        None => Ok(()),
        Some("missing") | Some("conflict") => Err(Violation(relation.refusal)),
        Some(field) => {
            let (_, refusal) = relation
                .fields
                .iter()
                .find(|(f, _)| *f == field)
                .unwrap_or_else(|| panic!("{name}: row {row} has no field {field}"));
            Err(Violation(refusal))
        }
    }
}

// verifies: REQ-AUZ-1, REQ-AUZ-3, REQ-AUZ-4, REQ-AUZ-7, REQ-AUD-1, REQ-AUD-4, REQ-ORD-34, REQ-ORD-43, REQ-CPL-5,
// INV-8, INV-7
#[test]
fn every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say() {
    let entry = validator("entry");
    let cases = admission_fixtures();
    let mut failures = Vec::new();
    for (name, case) in &cases {
        let mut history = Vec::new();
        for item in case["history"].as_array().unwrap() {
            assert!(entry.is_valid(&item["entry"]), "{name}: a history entry");
            let receipt: Receipt = serde_json::from_value(item["receipt"].clone()).unwrap();
            let held: Entry = serde_json::from_value(item["entry"].clone()).unwrap();
            history.push((receipt, held));
        }
        assert!(entry.is_valid(&case["entry"]), "{name}: the entry");
        let appended: Entry = serde_json::from_value(case["entry"].clone()).unwrap();
        let got = admit_entry(&history, &appended, through(case, &appended));
        let want = expected(name, case);
        if got != want {
            failures.push(format!("{name}: got {got:?}, expected {want:?}"));
        }
    }
    // Every row is refused without what it needs (or with what it forbids), and on each field it matches.
    for row in &RELATIONS {
        let breaks: Vec<&str> = cases
            .iter()
            .filter(|(_, c)| c["row"] == row.name)
            .filter_map(|(_, c)| c["breaks"].as_str())
            .collect();
        let whole = match row.quantifier {
            Quantifier::Exists => "missing",
            Quantifier::NoneOther => "conflict",
        };
        for needed in std::iter::once(whole).chain(row.fields.iter().map(|(f, _)| *f)) {
            if !breaks.contains(&needed) {
                failures.push(format!("{}: no case breaking {needed}", row.name));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// verifies: REQ-AUD-1, INV-7
#[test]
fn every_entry_of_a_complete_log_is_admitted_on_what_came_before_it() {
    let world: Vec<_> = admission_fixtures()
        .into_iter()
        .filter(|(n, _)| n.starts_with("world--"))
        .collect();
    // The world covers every kind of entry that cites an earlier one, and each is admitted.
    assert!(world.len() >= 40, "{}", world.len());
    for (name, case) in world {
        assert_eq!(case["row"], Value::Null, "{name}");
        let history: Vec<(Receipt, Entry)> = case["history"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    serde_json::from_value(i["receipt"].clone()).unwrap(),
                    serde_json::from_value(i["entry"].clone()).unwrap(),
                )
            })
            .collect();
        let appended: Entry = serde_json::from_value(case["entry"].clone()).unwrap();
        assert_eq!(
            admit_entry(&history, &appended, through(&case, &appended)),
            Ok(()),
            "{name}"
        );
    }
}

// verifies: REQ-AUZ-1, INV-7
#[test]
fn a_decision_entry_reaches_the_log_only_through_policys_handle() {
    let verified: Vec<(String, Value)> = fixtures("valid")
        .into_iter()
        .filter(|(n, _)| shape_of(n) == "verified-entry")
        .collect();
    // The decision kinds, by their tag on the wire, written out apart from `is_decision`.
    const DECISION_CALLS: [&str; 10] = [
        "decide",
        "authorise",
        "stuck_decide",
        "stuck_move",
        "open_session",
        "check_read",
        "grant_permission",
        "revoke_permission",
        "charge",
        "ask_about_person",
    ];
    let mut seen = std::collections::BTreeSet::new();
    for (name, value) in &verified {
        let entry: VerifiedEntry = serde_json::from_value(value.clone()).unwrap();
        let general = serde_json::from_value::<GeneralEntry>(value.clone());
        let decision = serde_json::from_value::<DecisionEntry>(value.clone());
        let call = value["what"]["call"].as_str().unwrap();
        assert_eq!(
            is_decision(&entry.what),
            DECISION_CALLS.contains(&call),
            "{name}"
        );
        if is_decision(&entry.what) {
            seen.insert(call);
            // Neither built nor read from the wire can a decision kind be a general entry.
            assert!(
                general
                    .unwrap_err()
                    .to_string()
                    .contains(DECISION_ON_THE_GENERAL_HANDLE),
                "{name}"
            );
            assert_eq!(
                GeneralEntry::try_from(Entry::Verified(entry.clone())),
                Err(Violation(DECISION_ON_THE_GENERAL_HANDLE)),
                "{name}"
            );
            assert!(decision.is_ok(), "{name}");
        } else {
            assert!(general.is_ok(), "{name}");
            assert!(
                decision
                    .unwrap_err()
                    .to_string()
                    .contains(RECORD_ON_THE_POLICY_HANDLE),
                "{name}"
            );
        }
    }
    // Every decision kind has a fixture here, so none can leave `is_decision` unseen.
    assert_eq!(seen, DECISION_CALLS.into_iter().collect());
    // A stated entry is never a decision's.
    let stated = fixture("valid", "stated-entry-of-a-quote");
    assert!(serde_json::from_value::<GeneralEntry>(stated.clone()).is_ok());
    assert!(serde_json::from_value::<DecisionEntry>(stated).is_err());

    // The probe: orchestration writes its own permitted `decide` and an effect on a permit naming it. The decide
    // can't be a general entry, the log refuses it on the general path, and with it absent the effect has nothing
    // to rest on.
    let forged: Entry =
        serde_json::from_value(fixture("valid", "verified-entry-of-a-decision")).unwrap();
    assert!(GeneralEntry::try_from(forged.clone()).is_err());
    assert_eq!(
        admit_entry(&[], &forged, Through::General),
        Err(Violation(DECISION_ON_THE_GENERAL_HANDLE))
    );
    let effect: Entry =
        serde_json::from_value(fixture("valid", "verified-entry-of-an-effect")).unwrap();
    assert!(admit_entry(&[], &effect, Through::General).is_err());
}

// verifies: DEC-4
#[test]
fn the_readme_lists_every_relation_of_the_citation_table_with_its_spec_ids() {
    let readme = fs::read_to_string(contract_dir().join("README.md")).unwrap();
    let rows: Vec<&str> = readme.lines().filter(|l| l.starts_with("| `")).collect();
    for relation in &RELATIONS {
        let Some(line) = rows
            .iter()
            .find(|l| l.starts_with(&format!("| `{}` |", relation.name)))
        else {
            panic!("the README's citation table lacks {}", relation.name);
        };
        for id in relation.spec {
            assert!(line.contains(&format!("`{id}`")), "{}: {id}", relation.name);
        }
    }
}

// verifies: REQ-ORD-34, REQ-ORD-43, REQ-AUD-4, REQ-ORD-17
#[test]
fn each_record_a_recorded_input_cites_is_joined_to_it_by_its_id() {
    let valid = fixtures("valid");
    let entries: Vec<&Value> = valid
        .iter()
        .filter(|(n, _)| shape_of(n) == "verified-entry")
        .map(|(_, v)| v)
        .collect();
    let recorded = |call: &str, field: &str, id: &str| {
        entries
            .iter()
            .any(|e| e["what"]["call"] == call && e["what"][field] == id)
    };
    let mut joined = Vec::new();
    for entry in entries.iter().filter(|e| e["what"]["call"] == "input") {
        let observed = &entry["what"]["input"]["observed"];
        for (field, call) in [
            ("decision_record", "decision_record"),
            ("acceptance", "acceptance"),
            ("evidence", "evidence"),
        ] {
            if let Some(id) = observed[field].as_str() {
                assert!(
                    recorded(call, field, id),
                    "no {call} entry is the {id} a recorded input cites"
                );
                joined.push(field);
            }
        }
    }
    for field in ["decision_record", "acceptance", "evidence"] {
        assert!(joined.contains(&field), "no input cites a {field}");
    }
    // A settlement account's payees' checks rest with its reserve, and the record says so.
    let paying: VerifiedEntry = serde_json::from_value(fixture(
        "valid",
        "verified-entry-of-an-acceptance-paying-a-settlement-account",
    ))
    .unwrap();
    let What::Acceptance { parties, .. } = paying.what else {
        panic!("an acceptance");
    };
    assert_eq!(
        parties.settlement_account_at.as_ref().map(Id::as_str),
        Some("bank-co")
    );
}

// verifies: REQ-AUD-5, REQ-AUD-10, REQ-AUD-2
#[test]
fn a_kept_statement_is_the_one_its_stated_entry_names() {
    fn decode(text: &str) -> Vec<u8> {
        let value = |c: u8| match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            _ => 63,
        };
        let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
        for c in text.bytes() {
            acc = (acc << 6) | u32::from(value(c));
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        out
    }
    let kept: KeptStatement =
        serde_json::from_value(fixture("valid", "kept-statement-of-a-quote")).unwrap();
    let stated = fixture("valid", "stated-entry-of-a-quote");
    assert_eq!(
        serde_json::to_value(&kept.commitment).unwrap(),
        stated["commitment"]
    );
    assert_eq!(decode(kept.salt.as_str()).len(), 32);
    // The bytes are the envelope as received: the key and scheme the entry says signed it, its shape and its time.
    let envelope: Value = serde_json::from_slice(&decode(kept.bytes.as_str())).unwrap();
    assert_eq!(envelope["signature"]["key"], stated["signed_with"]["key"]);
    assert_eq!(
        envelope["signature"]["scheme"],
        stated["signed_with"]["scheme"]
    );
    assert_eq!(envelope["shape"], stated["what"]["statement"]);
    assert_eq!(envelope["made_at"], stated["when"]["at"]);
    // The log's entry holds neither the salt, the bytes nor the signature's value.
    let text = stated.to_string();
    let value = envelope["signature"]["value"].as_str().unwrap();
    assert!(!text.contains(kept.salt.as_str()) && !text.contains(value));
}

// verifies: DEC-4, REQ-ORD-9
#[test]
fn a_value_built_in_memory_is_held_to_the_same_rules_as_one_read() {
    let mut entry: VerifiedEntry =
        serde_json::from_value(fixture("valid", "verified-entry-of-a-hand-over")).unwrap();
    let pair: ReportPair =
        serde_json::from_value(fixture("valid", "report-pair-of-a-route-segment")).unwrap();
    entry.what = What::ReportPair { pair: pair.clone() };
    assert_eq!(Entry::Verified(entry.clone()).check(), Ok(()));

    // One party reporting both sides, built in memory and never serialised.
    let mut one_party = pair;
    one_party.received.party = one_party.sent.party.clone();
    entry.what = What::ReportPair { pair: one_party };
    assert_eq!(
        Entry::Verified(entry).check(),
        Err(Violation(
            "a segment's two reports are by two different parties"
        ))
    );

    // An effect whose action breaks an action's rule, built in memory: a grant naming no tuple.
    let mut effect: Effect =
        serde_json::from_value(fixture("valid", "effect-moving-value-on-a-permit")).unwrap();
    assert_eq!(effect.check(), Ok(()));
    effect.action = Action::ChangeCatalogue {
        change: CatalogueChange::Grant,
        provider: Id::new("fx-desk").unwrap(),
        version: None,
        tuple: None,
    };
    assert_eq!(
        effect.check(),
        Err(Violation(
            "a catalogue change names a tuple exactly when it is about a tuple"
        ))
    );
}

// verifies: REQ-AUD-1
#[test]
fn every_kind_of_record_a_verified_entry_holds_has_a_valid_fixture() {
    let calls: Vec<String> =
        schema("verified-entry")["$defs"]["what"]["properties"]["call"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_owned())
            .collect();
    let seen: Vec<String> = fixtures("valid")
        .into_iter()
        .filter(|(n, _)| shape_of(n) == "verified-entry")
        .map(|(_, v)| v["what"]["call"].as_str().unwrap().to_owned())
        .collect();
    let missing: Vec<&String> = calls.iter().filter(|c| !seen.contains(c)).collect();
    assert!(missing.is_empty(), "no valid fixture records {missing:?}");
}

// verifies: REQ-OPS-5, REQ-AUZ-7
#[test]
fn an_agent_reads_in_its_partys_role_and_policy_answers_it() {
    let check: ReadCheck = serde_json::from_value(fixture(
        "valid",
        "read-check-by-an-agent-in-its-partys-role",
    ))
    .unwrap();
    assert_eq!(
        (check.reader.role, check.reader.kind),
        (Role::Customer, ActorKind::Agent)
    );
    assert_ne!(check.reader.acts, check.reader.party);
}

// verifies: DEC-4
#[test]
fn every_valid_fixture_validates_and_every_invalid_one_is_rejected() {
    let valid = fixtures("valid");
    let invalid = fixtures("invalid");
    assert!(!valid.is_empty() && !invalid.is_empty(), "fixtures missing");

    let mut failures = Vec::new();
    for (name, value) in &valid {
        let shape = shape_of(name);
        if let Err(e) = validator(shape).validate(value) {
            failures.push(format!("{name} should validate as {shape}: {e}"));
        }
        match through_rust(shape, value) {
            Ok(back) if &back == value => {}
            Ok(back) => failures.push(format!("{name} changes through Rust: {back}")),
            Err(e) => failures.push(format!("{name} doesn't deserialise: {e}")),
        }
    }
    for (name, value) in &invalid {
        let shape = shape_of(name);
        let errors = schema_errors(&validator(shape), value);
        let schema_rule = SCHEMA_RULES.iter().find(|(n, _, _)| n == name);
        let rust_rule = RUST_RULES.iter().find(|(n, _)| n == name);
        match (schema_rule, rust_rule) {
            (Some((_, expected, refusal)), None) => {
                match through_rust(shape, value) {
                    Err(e) if e.contains(refusal) => {}
                    Err(e) => {
                        failures.push(format!("{name} refused by Rust for another reason: {e}"))
                    }
                    Ok(_) => failures.push(format!("{name} accepted by Rust")),
                }
                let mut expected: Vec<(String, String)> = expected
                    .iter()
                    .map(|(at, keyword)| ((*at).to_owned(), (*keyword).to_owned()))
                    .collect();
                expected.sort();
                if errors != expected {
                    failures.push(format!(
                        "{name}: schema errors {errors:?}, expected {expected:?}"
                    ));
                }
            }
            (None, Some((_, refusal))) => {
                if !errors.is_empty() {
                    failures.push(format!(
                        "{name} breaks a Rust rule, so the schema should accept it: {errors:?}"
                    ));
                }
                match through_rust(shape, value) {
                    Err(e) if e.contains(refusal) => {}
                    Err(e) => failures.push(format!("{name} refused for another reason: {e}")),
                    Ok(_) => failures.push(format!("{name} accepted by Rust")),
                }
            }
            _ => failures.push(format!(
                "{name} must name its rule exactly once in this test"
            )),
        }
    }
    let listed = SCHEMA_RULES
        .iter()
        .map(|(n, _, _)| *n)
        .chain(RUST_RULES.iter().map(|(n, _)| *n));
    for name in listed {
        if !invalid.iter().any(|(n, _)| n == name) {
            failures.push(format!("missing invalid fixture {name}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// verifies: DEC-4
#[test]
fn every_invalid_fixture_is_rejected_by_the_rust_types_too() {
    let accepted: Vec<String> = fixtures("invalid")
        .into_iter()
        .filter(|(name, value)| through_rust(shape_of(name), value).is_ok())
        .map(|(name, _)| name)
        .collect();
    assert!(accepted.is_empty(), "Rust accepts: {}", accepted.join(", "));
}

// verifies: DEC-4
#[test]
fn every_shape_but_the_shared_atoms_has_a_valid_fixture() {
    let valid = fixtures("valid");
    for shape in SHAPES.iter().filter(|s| **s != "common") {
        assert!(
            valid.iter().any(|(n, _)| shape_of(n) == *shape),
            "{shape} has no valid fixture"
        );
    }
    let on_disk = fs::read_dir(contract_dir().join("schemas"))
        .unwrap()
        .count();
    assert_eq!(on_disk, SHAPES.len(), "a schema file is not a listed shape");
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

/// One type implementing every call, so the test below holds each trait as a trait object.
struct Unbuilt;

fn receipt() -> Receipt {
    serde_json::from_value(fixture("valid", "receipt-of-an-appended-entry")).unwrap()
}

impl Log for Unbuilt {
    fn append(&mut self, _: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl PolicyLog for Unbuilt {
    fn append_decision(&mut self, _: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl Evidence for Unbuilt {
    fn keep(&mut self, _: &[u8]) -> Result<Kept, AppendFailure> {
        Ok(Kept {
            commitment: receipt().entry,
            restates: None,
        })
    }
    fn fetch(&mut self, _: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Ok(Fetched::Erased)
    }
}
impl Decisions for Unbuilt {
    fn decide(&mut self, _: &DecisionRequest) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl Authorisations for Unbuilt {
    fn authorise(&mut self, _: &Authorisation) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl Sessions for Unbuilt {
    fn open_session(&mut self, _: &SessionRequest) -> Result<SessionAnswer, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl Reads for Unbuilt {
    fn check_read(&mut self, _: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
    fn grant_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
    fn revoke_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl Charges for Unbuilt {
    fn charge(&mut self, _: &Charge) -> Result<ChargeAnswer, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl StuckOrders for Unbuilt {
    fn decide_stuck(&mut self, _: &StuckDecide) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
    fn move_stuck(&mut self, _: &StuckMove) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl PersonQuestions for Unbuilt {
    fn ask_about_person(&mut self, _: &PersonQuestion) -> Result<PersonAnswer, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
impl Comparisons for Unbuilt {
    fn report_pair(&mut self, _: &ReportPair) -> Result<Receipt, AppendFailure> {
        Ok(receipt())
    }
    fn holding_reading(&mut self, _: &HoldingReading) -> Result<Receipt, AppendFailure> {
        Ok(receipt())
    }
}
impl Measures for Unbuilt {
    fn measure(&mut self, _: &MeasureRecord) -> Result<Receipt, AppendFailure> {
        Ok(receipt())
    }
}
impl Operator for Unbuilt {
    fn raise(&mut self, _: &Raise) -> Delivery {
        Delivery::Undelivered
    }
}

// verifies: DEC-2, REQ-MOD-3
#[test]
fn every_call_between_cores_parts_can_be_held_as_a_trait_object() {
    fn read<T: DeserializeOwned>(name: &str) -> T {
        serde_json::from_value(fixture("valid", name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }
    let mut policy_log: Box<dyn PolicyLog> = Box::new(Unbuilt);
    assert_eq!(
        policy_log.append_decision(&read("verified-entry-of-a-decision")),
        Err(AppendFailure::Unavailable)
    );
    let mut log: Box<dyn Log> = Box::new(Unbuilt);
    assert_eq!(
        log.append(&read("entry-verified")),
        Err(AppendFailure::Unavailable)
    );
    let mut evidence: Box<dyn Evidence> = Box::new(Unbuilt);
    assert_eq!(
        evidence.keep(b"{}"),
        Ok(Kept {
            commitment: receipt().entry,
            restates: None,
        })
    );
    assert_eq!(
        evidence.fetch(&read("read-check-by-a-party-of-its-own-evidence")),
        Ok(Fetched::Erased)
    );
    let mut decisions: Box<dyn Decisions> = Box::new(Unbuilt);
    assert_eq!(
        decisions.decide(&read("decision-request-to-move-value")),
        Err(AppendFailure::Unavailable)
    );
    let mut authorisations: Box<dyn Authorisations> = Box::new(Unbuilt);
    assert_eq!(
        authorisations.authorise(&read("authorisation-of-one-order-with-a-floor")),
        Err(AppendFailure::Unavailable)
    );
    let mut stuck: Box<dyn StuckOrders> = Box::new(Unbuilt);
    assert_eq!(
        stuck.decide_stuck(&read("stuck-decide-naming-its-evidence")),
        Err(AppendFailure::Unavailable)
    );
    assert_eq!(
        stuck.move_stuck(&read("stuck-move-carrying-out-a-decision")),
        Err(AppendFailure::Unavailable)
    );
    let mut sessions: Box<dyn Sessions> = Box::new(Unbuilt);
    assert_eq!(
        sessions.open_session(&read("session-request-of-a-delegate")),
        Err(AppendFailure::Unavailable)
    );
    let mut reads: Box<dyn Reads> = Box::new(Unbuilt);
    assert_eq!(
        reads.grant_permission(&read("permission-change-granting-a-regulator-the-trail")),
        Err(AppendFailure::Unavailable)
    );
    assert_eq!(
        reads.check_read(&read("read-check-by-a-provider-of-its-segment")),
        Err(AppendFailure::Unavailable)
    );
    let mut charges: Box<dyn Charges> = Box::new(Unbuilt);
    assert_eq!(
        charges.charge(&read("charge-of-fanned-out-work")),
        Err(AppendFailure::Unavailable)
    );
    let mut questions: Box<dyn PersonQuestions> = Box::new(Unbuilt);
    assert_eq!(
        questions.ask_about_person(&read(
            "person-question-on-the-persons-authority-first-asked"
        )),
        Err(AppendFailure::Unavailable)
    );
    let mut comparisons: Box<dyn Comparisons> = Box::new(Unbuilt);
    assert_eq!(
        comparisons.report_pair(&read("report-pair-of-a-route-segment")),
        Ok(receipt())
    );
    assert_eq!(
        comparisons.holding_reading(&read("holding-reading-at-a-private-node")),
        Ok(receipt())
    );
    let mut measures: Box<dyn Measures> = Box::new(Unbuilt);
    assert_eq!(
        measures.measure(&read("measure-record-of-a-quote")),
        Ok(receipt())
    );
    let mut operator: Box<dyn Operator> = Box::new(Unbuilt);
    assert_eq!(
        operator.raise(&read("raise-of-a-report-difference")),
        Delivery::Undelivered
    );
}
