//! The core-internal contract's tests: its schemas against its fixtures, and its Rust types against its schemas
//! (`DEC-4`, `T-020-01`).

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::core_internal::Attribution;
use rutent_contracts::core_internal::calls::{
    ATTRIBUTED_BY_ITS_WRITER, AppendFailure, Authorisations, Charges, Comparisons,
    DECISION_ON_THE_GENERAL_HANDLE, DecisionEntry, Decisions, Delivery, Evidence, Fetched,
    GeneralEntry, Kept, KeptStatements, Log, Measures, Operator, PersonQuestions, PolicyLog,
    RECORD_ON_THE_POLICY_HANDLE, Reads, Sessions, Signatures, StatementKeys, StuckOrders,
    is_decision,
};
use rutent_contracts::core_internal::{
    Action, ActorKind, CatalogueChange, KeptStatement, PermissionChange, RELATIONS, Role,
    STATED_WITHOUT_ITS_KEPT_BYTES, SignedWith, Violation, What, admit_entry, attribute,
};
use rutent_contracts::core_internal::{
    Authorisation, Charge, ChargeAnswer, Decision, DecisionRequest, Effect, Entry, HandedOver,
    HoldingReading, MeasureRecord, PersonAnswer, PersonQuestion, Raise, ReadAnswer, ReadCheck,
    Receipt, ReportPair, SessionAnswer, SessionRequest, StatedEntry, StuckDecide, StuckMove,
    VERSION, VerifiedEntry,
};
use rutent_contracts::core_internal::{Quantifier, STATED_RELATIONS, Through};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::Deployment;
use rutent_contracts::router_api::Id;
use rutent_contracts::router_api::Timestamp;
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

/// The party the world's operator makes its statements as, Core's questions among them.
fn operator() -> Id {
    Id::new("operator").unwrap()
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

/// A row of either table, as the tests read it: its name, ids, quantifier, refusal and fields.
type Row = (
    &'static str,
    &'static [&'static str],
    Quantifier,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// Every row of the citation table, the verified entries' and the stated entries'.
fn rows() -> Vec<Row> {
    let verified = RELATIONS
        .iter()
        .map(|r| (r.name, r.spec, r.quantifier, r.refusal, r.fields));
    let stated = STATED_RELATIONS
        .iter()
        .map(|r| (r.name, r.spec, r.quantifier, r.refusal, r.fields));
    verified.chain(stated).collect()
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
    let (_, _, _, row_refusal, fields) = rows()
        .into_iter()
        .find(|r| r.0 == row)
        .unwrap_or_else(|| panic!("{name}: no row {row}"));
    match case["breaks"].as_str() {
        None => Ok(()),
        Some("missing") | Some("conflict") => Err(Violation(row_refusal)),
        Some(field) => {
            let (_, refusal) = fields
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
        let got = admit_entry(&history, &appended, through(case, &appended), &operator());
        let want = expected(name, case);
        if got != want {
            failures.push(format!("{name}: got {got:?}, expected {want:?}"));
        }
    }
    // Every row is refused without what it needs (or with what it forbids), and on each field it matches.
    for (row, _, quantifier, _, fields) in rows() {
        let breaks: Vec<&str> = cases
            .iter()
            .filter(|(_, c)| c["row"] == row)
            .filter_map(|(_, c)| c["breaks"].as_str())
            .collect();
        let whole = match quantifier {
            Quantifier::Exists => "missing",
            Quantifier::NoneOther => "conflict",
        };
        for needed in std::iter::once(whole).chain(fields.iter().map(|(f, _)| *f)) {
            if !breaks.contains(&needed) {
                failures.push(format!("{row}: no case breaking {needed}"));
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
            admit_entry(&history, &appended, through(&case, &appended), &operator()),
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
    // A stated entry is never a decision's, and reaches the general handle only unverified: the log attributes it.
    let unverified = fixture("valid", "stated-entry-relayed-and-unverified");
    assert!(serde_json::from_value::<GeneralEntry>(unverified.clone()).is_ok());
    assert!(serde_json::from_value::<DecisionEntry>(unverified).is_err());
    let stated = fixture("valid", "stated-entry-of-a-quote");
    let error = serde_json::from_value::<GeneralEntry>(stated.clone())
        .unwrap_err()
        .to_string();
    assert!(error.contains(ATTRIBUTED_BY_ITS_WRITER), "{error}");
    let entry: Entry = serde_json::from_value(stated.clone()).unwrap();
    assert_eq!(
        GeneralEntry::try_from(entry),
        Err(Violation(ATTRIBUTED_BY_ITS_WRITER))
    );
    assert!(serde_json::from_value::<DecisionEntry>(stated).is_err());

    // The probe: orchestration writes its own permitted `decide` and an effect on a permit naming it. The decide
    // can't be a general entry, the log refuses it on the general path, and with it absent the effect has nothing
    // to rest on.
    let forged: Entry =
        serde_json::from_value(fixture("valid", "verified-entry-of-a-decision")).unwrap();
    assert!(GeneralEntry::try_from(forged.clone()).is_err());
    assert_eq!(
        admit_entry(&[], &forged, Through::General, &operator()),
        Err(Violation(DECISION_ON_THE_GENERAL_HANDLE))
    );
    let effect: Entry =
        serde_json::from_value(fixture("valid", "verified-entry-of-an-effect")).unwrap();
    assert!(admit_entry(&[], &effect, Through::General, &operator()).is_err());
}

/// The log's sources for an attribution, as a case states them.
struct Sources {
    kept: Option<KeptStatement>,
    deployment: Deployment,
    keys: Vec<(String, SignedWith, Timestamp)>,
    /// The party each maker states for; a maker not listed states for itself.
    publishers: Vec<(String, String)>,
    signature: bool,
}

impl KeptStatements for Sources {
    fn kept(&mut self, _: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        Ok(self.kept.clone())
    }
}

impl StatementKeys for Sources {
    fn deployment(&mut self) -> Result<Deployment, AppendFailure> {
        Ok(self.deployment.clone())
    }

    fn operator(&mut self) -> Result<Id, AppendFailure> {
        Ok(operator())
    }

    fn publisher(&mut self, maker: &Id) -> Result<Option<Id>, AppendFailure> {
        let party = self
            .publishers
            .iter()
            .find(|(m, _)| m == maker.as_str())
            .map_or(maker.as_str(), |(_, p)| p.as_str());
        Ok(Id::new(party))
    }

    fn signs_for(
        &mut self,
        maker: &Id,
        key: &SignedWith,
        at: &Timestamp,
    ) -> Result<bool, AppendFailure> {
        Ok(self
            .keys
            .iter()
            .any(|(m, k, t)| m == maker.as_str() && k == key && t == at))
    }
}

impl Signatures for Sources {
    fn verifies(&self, _: &[u8]) -> bool {
        self.signature
    }
}

fn sources(case: &Value) -> Sources {
    let scheme = serde_json::json!("ed25519");
    Sources {
        kept: serde_json::from_value(case["kept"].clone()).unwrap(),
        deployment: serde_json::from_value(case["deployment"].clone()).unwrap(),
        keys: case["keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| {
                let with = serde_json::json!({"scheme": scheme, "key": k["key"]});
                (
                    k["maker"].as_str().unwrap().to_string(),
                    serde_json::from_value(with).unwrap(),
                    serde_json::from_value(k["at"].clone()).unwrap(),
                )
            })
            .collect(),
        publishers: case["publishers"]
            .as_array()
            .map(|p| {
                p.iter()
                    .map(|p| {
                        (
                            p["maker"].as_str().unwrap().to_string(),
                            p["party"].as_str().unwrap().to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        signature: case["signature"].as_bool().unwrap(),
    }
}

fn attribution_cases() -> Vec<(String, Value)> {
    let dir = contract_dir().join("fixtures").join("attribution");
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

/// Each case's claim, attributed over its sources, comes out as the case says: verified only where the kept bytes
/// bear every part of the claim out, unverified where one part fails, and refused where the writer attributed it
/// or no statement is kept under its commitment.
// verifies: REQ-AUD-2, REQ-PRT-1, THR-3
#[test]
fn the_log_not_the_writer_attributes_a_stated_entry() {
    let cases = attribution_cases();
    assert!(cases.len() >= 14, "{}", cases.len());
    // The log holds each source as a trait object, as it holds the others.
    let (_, case) = &cases[0];
    let _: [Box<dyn std::any::Any>; 3] = [
        Box::new(Box::new(sources(case)) as Box<dyn KeptStatements>),
        Box::new(Box::new(sources(case)) as Box<dyn StatementKeys>),
        Box::new(Box::new(sources(case)) as Box<dyn Signatures>),
    ];
    for (name, case) in &cases {
        let claim: StatedEntry = serde_json::from_value(case["claim"].clone()).unwrap();
        let mut from = sources(case);
        let mut keys = sources(case);
        let signatures = sources(case);
        let got = attribute(&claim, &mut from, &mut keys, &signatures);
        let maker = claim.attribution.maker().clone();
        match case["expect"].as_str().unwrap() {
            "verified" => {
                let entry = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                assert_eq!(
                    entry.attribution,
                    Attribution::Verified { who: maker },
                    "{name}"
                );
                let mut rest = entry.clone();
                rest.attribution = claim.attribution.clone();
                assert_eq!(rest, claim, "{name}: only the attribution changes");
            }
            "unverified" => assert_eq!(got, Ok(claim.clone()), "{name}"),
            "attributed" => assert_eq!(
                got,
                Err(AppendFailure::Refused(Violation(ATTRIBUTED_BY_ITS_WRITER))),
                "{name}"
            ),
            "no_kept" => assert_eq!(
                got,
                Err(AppendFailure::Refused(Violation(
                    STATED_WITHOUT_ITS_KEPT_BYTES
                ))),
                "{name}"
            ),
            other => panic!("{name}: no expectation {other}"),
        }
    }
}

/// The reviewer's probe: orchestration logs a yes from the origin reserve as verified, which the reserve never
/// signed. Attributed `verified` by its writer, it is no general entry. Handed in as the claim it can be, with bytes
/// whose signature doesn't verify, the log records it unverified, and an acceptance resting on it is refused.
// verifies: REQ-AUD-2, REQ-ORD-32
#[test]
fn a_reserves_yes_its_writer_calls_verified_is_not_the_reserves() {
    let case = read_json(
        &contract_dir()
            .join("fixtures/admission/acceptance-rests-on-the-reserves-yes--verified.json"),
    );
    let mut history: Vec<(Receipt, Entry)> = case["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                serde_json::from_value(h["receipt"].clone()).unwrap(),
                serde_json::from_value(h["entry"].clone()).unwrap(),
            )
        })
        .collect();
    let acceptance: Entry = serde_json::from_value(case["entry"].clone()).unwrap();
    let Entry::Verified(VerifiedEntry {
        what: What::Acceptance {
            reserve_yes: Some(at),
            ..
        },
        ..
    }) = &acceptance
    else {
        panic!("an acceptance resting on a yes");
    };
    let slot = history.iter().position(|(r, _)| r == at).unwrap();
    let Entry::Stated(yes) = history[slot].1.clone() else {
        panic!("the yes is a stated entry");
    };
    let reserve = yes.attribution.maker().clone();

    // The forgery: the yes, attributed verified by its writer.
    let mut forged = yes.clone();
    forged.attribution = Attribution::Verified {
        who: reserve.clone(),
    };
    assert_eq!(
        GeneralEntry::try_from(Entry::Stated(forged.clone())),
        Err(Violation(ATTRIBUTED_BY_ITS_WRITER))
    );
    let wire = serde_json::to_value(Entry::Stated(forged.clone())).unwrap();
    assert!(serde_json::from_value::<GeneralEntry>(wire).is_err());

    // Handed in as a claim, with bytes the reserve's key never signed: the log records it unverified.
    let claim = StatedEntry {
        attribution: Attribution::Unverified {
            claimed_by: reserve.clone(),
        },
        ..yes.clone()
    };
    let envelope = serde_json::json!({
        "deployment": {"name": "rutent-test", "operator_key": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA"},
        "protocol_version": "2.0.1",
        "shape": claim.what.statement.as_str(),
        "maker": reserve.acts.as_str(),
        "made_at": claim.when.at.as_str(),
        "signature": {"scheme": "ed25519", "key": claim.signed_with.key.as_str(), "value": "AA"},
        "statement": {
            "kind": "balance_verification",
            "transaction": claim.about.as_ref().and_then(|a| a.transaction.as_ref()).unwrap().as_str(),
            "outcome": {"status": "answered", "sufficient": true},
        },
    });
    let bytes = base64url(&serde_json::to_vec(&envelope).unwrap());
    let kept = serde_json::json!({
        "commitment": claim.commitment,
        "salt": base64url(&[7; 32]),
        "bytes": bytes,
    });
    let mut source = Sources {
        kept: Some(serde_json::from_value(kept).unwrap()),
        deployment: serde_json::from_value(envelope["deployment"].clone()).unwrap(),
        keys: vec![(
            reserve.acts.as_str().to_string(),
            claim.signed_with.clone(),
            claim.when.at.clone(),
        )],
        publishers: vec![(
            reserve.acts.as_str().to_string(),
            reserve.party.as_str().to_string(),
        )],
        signature: false,
    };
    let signatures = sources_like(&source);
    let mut keys = sources_like(&source);
    let recorded = attribute(&claim, &mut source, &mut keys, &signatures).unwrap();
    assert_eq!(recorded, claim);
    history[slot].1 = Entry::Stated(recorded);
    assert!(admit_entry(&history, &acceptance, Through::General, &operator()).is_err());

    // Where the reserve's key did sign it, the log attributes it to the reserve, and the acceptance is admitted.
    let mut signed = sources_like(&source);
    signed.signature = true;
    let mut keys = sources_like(&signed);
    let signatures = sources_like(&signed);
    let recorded = attribute(&claim, &mut signed, &mut keys, &signatures).unwrap();
    assert_eq!(recorded.attribution, Attribution::Verified { who: reserve });
    history[slot].1 = Entry::Stated(recorded);
    assert_eq!(
        admit_entry(&history, &acceptance, Through::General, &operator()),
        Ok(())
    );
}

/// The kept bytes of `yes` as its maker signed them, about the transaction it names, and sources that verify them.
fn signed_sources(yes: &StatedEntry, transaction: &str) -> Sources {
    let envelope = serde_json::json!({
        "deployment": {"name": "rutent-test", "operator_key": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA"},
        "protocol_version": "2.0.1",
        "shape": yes.what.statement.as_str(),
        "maker": yes.attribution.maker().acts.as_str(),
        "made_at": yes.when.at.as_str(),
        "signature": {"scheme": "ed25519", "key": yes.signed_with.key.as_str(), "value": "AA"},
        "statement": {
            "kind": "balance_verification",
            "transaction": transaction,
            "outcome": {"status": "answered", "sufficient": true},
        },
    });
    let kept = serde_json::json!({
        "commitment": yes.commitment,
        "salt": base64url(&[7; 32]),
        "bytes": base64url(&serde_json::to_vec(&envelope).unwrap()),
    });
    Sources {
        kept: Some(serde_json::from_value(kept).unwrap()),
        deployment: serde_json::from_value(envelope["deployment"].clone()).unwrap(),
        keys: vec![(
            yes.attribution.maker().acts.as_str().to_string(),
            yes.signed_with.clone(),
            yes.when.at.clone(),
        )],
        publishers: vec![(
            yes.attribution.maker().acts.as_str().to_string(),
            yes.attribution.maker().party.as_str().to_string(),
        )],
        signature: true,
    }
}

fn attributed(claim: &StatedEntry, source: &Sources) -> StatedEntry {
    let (mut kept, mut keys, signatures) = (
        sources_like(source),
        sources_like(source),
        sources_like(source),
    );
    attribute(claim, &mut kept, &mut keys, &signatures).unwrap()
}

/// A genuine yes of the origin reserve about one order, re-appended for another: with its transaction left out the
/// log records it unverified and an acceptance on it is refused; with its transaction kept the log refuses it, since
/// the first entry naming the transaction fixed its order; and the genuine yes still admits its own acceptance.
// verifies: REQ-ORD-32, REQ-ORD-38, REQ-AUD-2
#[test]
fn a_reserves_yes_is_bound_to_its_order_through_its_signed_transaction() {
    let case = read_json(&contract_dir().join("fixtures/admission/world--acc1.json"));
    let history: Vec<(Receipt, Entry)> = case["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                serde_json::from_value(h["receipt"].clone()).unwrap(),
                serde_json::from_value(h["entry"].clone()).unwrap(),
            )
        })
        .collect();
    let acceptance: Entry = serde_json::from_value(case["entry"].clone()).unwrap();
    let Entry::Verified(VerifiedEntry {
        what:
            What::Acceptance {
                reserve_yes: Some(at),
                order,
                ..
            },
        ..
    }) = &acceptance
    else {
        panic!("an acceptance resting on a yes");
    };
    // The genuine yes admits its own acceptance.
    assert_eq!(
        admit_entry(&history, &acceptance, Through::General, &operator()),
        Ok(())
    );
    let Some((_, Entry::Stated(yes))) = history.iter().find(|(r, _)| r == at).cloned() else {
        panic!("the yes is a stated entry");
    };
    let transaction = yes
        .about
        .as_ref()
        .and_then(|a| a.transaction.clone())
        .expect("a yes names its signed transaction");
    let signed = signed_sources(&yes, transaction.as_str());
    let as_claim = |about: serde_json::Value| StatedEntry {
        attribution: Attribution::Unverified {
            claimed_by: yes.attribution.maker().clone(),
        },
        about: Some(serde_json::from_value(about).unwrap()),
        ..yes.clone()
    };
    let other = if order.as_str() == "ord-1" {
        "ord-2"
    } else {
        "ord-1"
    };

    // Re-appended for another order with its transaction left out: recorded unverified, so no acceptance rests on it.
    let left_out = attributed(&as_claim(serde_json::json!({"order": other})), &signed);
    assert!(matches!(
        left_out.attribution,
        Attribution::Unverified { .. }
    ));
    let slot = history.iter().position(|(r, _)| r == at).unwrap();
    let mut replayed = history.clone();
    let mut moved = left_out.clone();
    moved.about = yes.about.clone().map(|mut a| {
        a.transaction = None;
        a
    });
    replayed[slot].1 = Entry::Stated(moved);
    assert!(admit_entry(&replayed, &acceptance, Through::General, &operator()).is_err());

    // Re-appended for another order with its transaction kept: the bytes bear it out, but Core asked the question
    // under that transaction for the yes's own order, so the log refuses it.
    let kept = attributed(
        &as_claim(serde_json::json!({"order": other, "transaction": transaction})),
        &signed,
    );
    assert!(matches!(kept.attribution, Attribution::Verified { .. }));
    assert_eq!(
        admit_entry(
            &history,
            &Entry::Stated(kept.clone()),
            Through::General,
            &operator()
        ),
        Err(Violation(
            "an answer is about the order its question asked for"
        ))
    );
    // Behind a question asked again under the same transaction with a false label, the first entry naming the
    // transaction still fixed its order.
    let question_at = history
        .iter()
        .position(|(_, e)| {
            matches!(e, Entry::Stated(q) if q.what.statement.as_str() == "urn:rutent:provider-role:operation"
                && q.about.as_ref().and_then(|a| a.transaction.as_ref()) == Some(&transaction))
        })
        .expect("Core asked the yes's question before it");
    assert!(question_at < slot, "the question comes before its answer");
    let Entry::Stated(question) = history[question_at].1.clone() else {
        unreachable!()
    };
    let mut relabelled = question.clone();
    relabelled.about = Some(
        serde_json::from_value(serde_json::json!({"order": other, "transaction": transaction}))
            .unwrap(),
    );
    let mut with_false_label = history.clone();
    with_false_label.push((
        serde_json::from_value(serde_json::json!({
            "log": "log-main", "index": 9000,
            "entry": {"algorithm": "sha-256", "value": "fe".repeat(32)},
        }))
        .unwrap(),
        Entry::Stated(relabelled),
    ));
    assert_eq!(
        admit_entry(
            &with_false_label,
            &Entry::Stated(kept.clone()),
            Through::General,
            &operator()
        ),
        Err(Violation(stated_row("transaction-belongs-to-one-order")))
    );

    // The reviewer's order of events: the genuine yes appended first, before Core's question is in the log, under
    // another order with its transaction kept. With no question asked under it for that order, it is refused, and
    // the genuine question and answer that follow are admitted.
    let before: Vec<(Receipt, Entry)> = history[..question_at].to_vec();
    assert_eq!(
        admit_entry(&before, &Entry::Stated(kept), Through::General, &operator()),
        Err(Violation(stated_row("answer-follows-its-question")))
    );
    let mut genuine = before.clone();
    assert_eq!(
        admit_entry(
            &genuine,
            &Entry::Stated(question.clone()),
            Through::General,
            &operator()
        ),
        Ok(())
    );
    genuine.push(history[question_at].clone());
    let again = attributed(&yes_claim(&yes), &signed);
    assert_eq!(again.attribution, yes.attribution);
    assert_eq!(
        admit_entry(
            &genuine,
            &Entry::Stated(again),
            Through::General,
            &operator()
        ),
        Ok(())
    );
}

/// The reviewer's probe: a provider's result appended about nothing, though its body names the transaction Core
/// asked it under. The log records it unverified, so the report pair citing it is refused, and it can't stand for
/// another order's segment by saying nothing.
// verifies: REQ-AUD-2, REQ-ORD-9, REQ-ORD-38
#[test]
fn a_result_about_nothing_whose_body_names_a_transaction_is_not_verified() {
    let case = read_json(&contract_dir().join("fixtures/admission/world--pair.json"));
    let mut history: Vec<(Receipt, Entry)> = case["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                serde_json::from_value(h["receipt"].clone()).unwrap(),
                serde_json::from_value(h["entry"].clone()).unwrap(),
            )
        })
        .collect();
    let pair: Entry = serde_json::from_value(case["entry"].clone()).unwrap();
    assert_eq!(
        admit_entry(&history, &pair, Through::General, &operator()),
        Ok(())
    );
    let Entry::Verified(VerifiedEntry {
        what: What::ReportPair { pair: reports },
        ..
    }) = &pair
    else {
        panic!("a report pair");
    };
    let slot = history
        .iter()
        .position(|(r, _)| *r == reports.sent.statement)
        .unwrap();
    let Entry::Stated(sent) = history[slot].1.clone() else {
        panic!("the sent report is a stated entry");
    };
    let transaction = sent
        .about
        .as_ref()
        .and_then(|a| a.transaction.clone())
        .expect("a result names the transaction it answers");
    let about_nothing = StatedEntry {
        about: None,
        ..yes_claim(&sent)
    };
    let recorded = attributed(&about_nothing, &signed_sources(&sent, transaction.as_str()));
    assert!(matches!(
        recorded.attribution,
        Attribution::Unverified { .. }
    ));
    history[slot].1 = Entry::Stated(recorded);
    assert_eq!(
        admit_entry(&history, &pair, Through::General, &operator()),
        Err(Violation("a sent report is a verified statement"))
    );
}

/// A provider signs an `operation` with its own key and claims it is the operator's question. The log binds the party
/// a statement states for to its maker's publisher, so it records the forgery unverified, and the answer it would
/// license is refused: a question is a verified statement of the operator's party.
// verifies: REQ-AUD-2, REQ-ORD-32
#[test]
fn a_providers_operation_claiming_to_be_the_operators_question_licenses_no_answer() {
    let case = read_json(&contract_dir().join("fixtures/admission/world--s_usbank1.json"));
    let mut history: Vec<(Receipt, Entry)> = case["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                serde_json::from_value(h["receipt"].clone()).unwrap(),
                serde_json::from_value(h["entry"].clone()).unwrap(),
            )
        })
        .collect();
    let answer: Entry = serde_json::from_value(case["entry"].clone()).unwrap();
    assert_eq!(
        admit_entry(&history, &answer, Through::General, &operator()),
        Ok(())
    );
    let Entry::Stated(yes) = &answer else {
        panic!("the yes is a stated entry");
    };
    let transaction = yes
        .about
        .as_ref()
        .and_then(|a| a.transaction.clone())
        .unwrap();
    let slot = history
        .iter()
        .position(|(_, e)| {
            matches!(e, Entry::Stated(q) if q.what.statement.as_str() == "urn:rutent:provider-role:operation")
        })
        .unwrap();
    let Entry::Stated(question) = history[slot].1.clone() else {
        unreachable!()
    };
    let provider = yes.attribution.maker().clone();
    let forged = StatedEntry {
        attribution: Attribution::Unverified {
            claimed_by: rutent_contracts::core_internal::Actor {
                party: operator(),
                ..provider.clone()
            },
        },
        signed_with: yes.signed_with.clone(),
        ..question
    };
    let mut sources = signed_sources(&forged, transaction.as_str());
    sources.publishers = vec![(
        provider.acts.as_str().to_string(),
        provider.party.as_str().to_string(),
    )];
    let recorded = attributed(&forged, &sources);
    assert!(matches!(
        recorded.attribution,
        Attribution::Unverified { .. }
    ));
    history[slot].1 = Entry::Stated(recorded);
    assert_eq!(
        admit_entry(&history, &answer, Through::General, &operator()),
        Err(Violation("a question is a verified statement"))
    );
    // With the party it claims bound to its key, the same statement is verified to the provider, not the operator.
    sources.publishers.clear();
    let as_its_own = StatedEntry {
        attribution: Attribution::Unverified {
            claimed_by: provider.clone(),
        },
        ..forged.clone()
    };
    let recorded = attributed(&as_its_own, &sources);
    assert_eq!(
        recorded.attribution,
        Attribution::Verified { who: provider }
    );
    history[slot].1 = Entry::Stated(recorded);
    assert_eq!(
        admit_entry(&history, &answer, Through::General, &operator()),
        Err(Violation("a question is the operator's statement"))
    );
}

/// The refusal of the stated entries' row `name`.
fn stated_row(name: &str) -> &'static str {
    STATED_RELATIONS
        .iter()
        .find(|r| r.name == name)
        .unwrap()
        .refusal
}

fn yes_claim(yes: &StatedEntry) -> StatedEntry {
    StatedEntry {
        attribution: Attribution::Unverified {
            claimed_by: yes.attribution.maker().clone(),
        },
        ..yes.clone()
    }
}

fn sources_like(s: &Sources) -> Sources {
    Sources {
        kept: s.kept.clone(),
        deployment: s.deployment.clone(),
        keys: s.keys.clone(),
        publishers: s.publishers.clone(),
        signature: s.signature,
    }
}

fn base64url(raw: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

// verifies: DEC-4
#[test]
fn the_readme_lists_every_relation_of_the_citation_table_with_its_spec_ids() {
    let readme = fs::read_to_string(contract_dir().join("README.md")).unwrap();
    let rows: Vec<&str> = readme.lines().filter(|l| l.starts_with("| `")).collect();
    for (name, spec, ..) in self::rows() {
        let Some(line) = rows
            .iter()
            .find(|l| l.starts_with(&format!("| `{name}` |")))
        else {
            panic!("the README's citation table lacks {name}");
        };
        for id in spec {
            assert!(line.contains(&format!("`{id}`")), "{name}: {id}");
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
