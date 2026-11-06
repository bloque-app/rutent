//! The provider-role contract's schemas, fixtures and Rust validation (`T-002-01`).

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::provider_role::{self, Advertisement, Category, RuleViolation};
use serde_json::Value;

const SCHEMAS: [&str; 4] = [
    "common",
    "declarations",
    "reserve-declaration",
    "advertisement",
];

const RAMP_0: &str = "/offers/ramp/tuples/0";
const EXCHANGE_0: &str = "/offers/exchange/tuples/0";
const RESERVE_0: &str = "/offers/reserve/tuples/0";

/// Invalid fixtures the schemas must reject, each with the exact (instance location, keyword) errors of the one
/// rule it breaks.
fn schema_rule_fixtures() -> Vec<(&'static str, Vec<(String, &'static str)>)> {
    let at = |path: &str, suffix: &str| format!("{path}{suffix}");
    vec![
        (
            "advertisement-without-offers",
            vec![("/offers".into(), "minProperties")],
        ),
        (
            "band-amount-with-a-fraction",
            vec![(at(RAMP_0, "/terms/bands/0/min"), "pattern")],
        ),
        (
            "evidence-availability-beyond-sixty-four-bits",
            vec![("/declarations/evidence/availability".into(), "oneOf")],
        ),
        (
            "evidence-grade-out-of-range",
            vec![("/declarations/evidence/grade".into(), "maximum")],
        ),
        (
            "evidence-without-availability",
            vec![("/declarations/evidence".into(), "required")],
        ),
        (
            "exchange-asset-without-units",
            vec![(at(EXCHANGE_0, "/to"), "required")],
        ),
        (
            "exchange-outside-an-inner-node",
            vec![(at(EXCHANGE_0, "/node"), "type")],
        ),
        (
            "exchange-spanning-two-nodes",
            vec![
                (EXCHANGE_0.into(), "additionalProperties"),
                (EXCHANGE_0.into(), "required"),
            ],
        ),
        (
            "offer-under-an-unknown-category",
            vec![("/offers".into(), "additionalProperties")],
        ),
        (
            "offer-without-tuples",
            vec![("/offers/ramp/tuples".into(), "minItems")],
        ),
        (
            "outer-node-naming-its-reserve",
            vec![(RESERVE_0.into(), "oneOf")],
        ),
        (
            "private-node-declared-twice",
            vec![("/private_nodes".into(), "uniqueItems")],
        ),
        (
            "provider-without-identifier-or-status-query",
            vec![("/declarations/tracking".into(), "minItems")],
        ),
        ("provider-without-publisher", vec![("".into(), "required")]),
        (
            "provider-without-safe-retry-declaration",
            vec![("/declarations".into(), "required")],
        ),
        (
            "ramp-changing-its-asset",
            vec![
                (RAMP_0.into(), "additionalProperties"),
                (RAMP_0.into(), "required"),
                (RAMP_0.into(), "required"),
            ],
        ),
        (
            "ramp-to-an-outer-node",
            vec![(at(RAMP_0, "/destination"), "type")],
        ),
        (
            "reserve-between-two-inner-nodes",
            vec![(RESERVE_0.into(), "oneOf")],
        ),
        (
            "reserve-between-two-outer-nodes",
            vec![(RESERVE_0.into(), "oneOf")],
        ),
        (
            "reserve-declaration-without-automatic-deposit-answer",
            vec![("/offers/reserve/declaration".into(), "required")],
        ),
        (
            "reserve-declaration-without-maker",
            vec![("/offers/reserve/declaration".into(), "required")],
        ),
        (
            "reserve-without-declaration",
            vec![("/offers/reserve".into(), "required")],
        ),
        (
            "tuple-without-amount-bands",
            vec![(at(RAMP_0, "/terms/bands"), "minItems")],
        ),
        (
            "tuple-without-gross-or-net",
            vec![(at(RAMP_0, "/terms"), "required")],
        ),
        (
            "tuple-without-required-inputs-declared",
            vec![(at(RAMP_0, "/terms"), "required")],
        ),
        (
            "tuple-without-units-at-its-destination",
            vec![(at(RAMP_0, "/decimals"), "required")],
        ),
        (
            "value-moving-provider-without-evidence-declaration",
            vec![("/declarations".into(), "required")],
        ),
        (
            "verifier-advertising-a-tuple",
            vec![("/offers/verifier".into(), "additionalProperties")],
        ),
        (
            "verifier-without-classes",
            vec![("/offers/verifier/classes".into(), "minItems")],
        ),
        (
            "verifier-without-identifier-or-status-query",
            vec![("/declarations/tracking".into(), "minItems")],
        ),
        (
            "zero-fill-increment",
            vec![(at(EXCHANGE_0, "/terms/increment"), "pattern")],
        ),
    ]
}

/// Invalid fixtures that pass the schemas and must be rejected by `provider_role::validate`, with the rule each
/// breaks.
fn rust_rule_fixtures() -> Vec<(&'static str, RuleViolation)> {
    vec![
        (
            "ramp-within-one-node",
            RuleViolation::RampWithinOneNode { tuple: 0 },
        ),
        (
            "band-with-min-above-max",
            RuleViolation::BandMinAboveMax {
                category: Category::Ramp,
                tuple: 0,
                band: 0,
            },
        ),
        (
            "ramp-tuple-declared-twice",
            RuleViolation::TupleDeclaredTwice {
                category: Category::Ramp,
                tuple: 1,
                first: 0,
            },
        ),
        (
            "reserve-tuple-declared-twice",
            RuleViolation::TupleDeclaredTwice {
                category: Category::Reserve,
                tuple: 2,
                first: 0,
            },
        ),
        (
            "exchange-tuple-declared-twice",
            RuleViolation::TupleDeclaredTwice {
                category: Category::Exchange,
                tuple: 1,
                first: 0,
            },
        ),
    ]
}

fn contract_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("provider-role")
}

fn read_json(path: &Path) -> Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

/// A validator for `urn:rutent:provider-role:advertisement`, with every schema of the contract registered by its
/// `$id` and nothing fetched from elsewhere.
fn advertisement_validator() -> Validator {
    let schemas: Vec<(String, Value)> = SCHEMAS
        .iter()
        .map(|name| {
            let schema = read_json(
                &contract_dir()
                    .join("schemas")
                    .join(format!("{name}.schema.json")),
            );
            let id = schema["$id"]
                .as_str()
                .expect("schema has an $id")
                .to_owned();
            assert_eq!(
                id,
                format!("urn:rutent:provider-role:{name}"),
                "$id of {name}"
            );
            assert_eq!(
                schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
                "$schema of {name}"
            );
            (id, schema)
        })
        .collect();
    let registry = Registry::new()
        .extend(schemas.iter().map(|(id, schema)| (id.as_str(), schema)))
        .expect("schema ids are valid URIs")
        .prepare()
        .expect("registry prepares");
    let root = serde_json::json!({ "$ref": "urn:rutent:provider-role:advertisement" });
    jsonschema::options()
        .offline()
        .with_registry(&registry)
        .build(&root)
        .expect("advertisement schema builds")
}

fn fixture(kind: &str, name: &str) -> Value {
    read_json(
        &contract_dir()
            .join("fixtures")
            .join(kind)
            .join(format!("{name}.json")),
    )
}

fn fixtures(kind: &str) -> Vec<(String, Value)> {
    let dir = contract_dir().join("fixtures").join(kind);
    let mut out: Vec<(String, Value)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| {
            let name = path
                .file_stem()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            (name, read_json(&path))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The schema errors for an instance, as sorted (instance location, keyword) pairs.
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
    errors
}

/// Asserts the schemas reject `name` with exactly the errors its entry in `schema_rule_fixtures` names.
fn assert_schema_rejects(name: &str) {
    let table = schema_rule_fixtures();
    let (_, expected) = table
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is not a schema-rule fixture"));
    let mut expected: Vec<(String, String)> = expected
        .iter()
        .map(|(at, keyword)| (at.clone(), (*keyword).to_owned()))
        .collect();
    expected.sort();
    let errors = schema_errors(&advertisement_validator(), &fixture("invalid", name));
    assert!(
        !errors.is_empty(),
        "invalid fixture {name} was accepted by the schemas"
    );
    assert_eq!(errors, expected, "schema errors of invalid fixture {name}");
}

/// Asserts `name` passes the schemas and that `validate` reports exactly `expected`.
fn assert_rule_rejects(name: &str, expected: &RuleViolation) {
    let instance = fixture("invalid", name);
    let errors = schema_errors(&advertisement_validator(), &instance);
    assert!(
        errors.is_empty(),
        "fixture {name} should pass the schemas, got {errors:?}"
    );
    let advertisement: Advertisement = serde_json::from_value(instance)
        .unwrap_or_else(|e| panic!("fixture {name} deserialises: {e}"));
    match provider_role::validate(&advertisement) {
        Ok(()) => panic!("invalid fixture {name} was accepted by validate"),
        Err(violations) => {
            assert_eq!(violations, vec![expected.clone()], "violations of {name}")
        }
    }
}

// verifies: REQ-CAT-13
#[test]
fn exchange_spanning_two_nodes_is_invalid() {
    assert_schema_rejects("exchange-spanning-two-nodes");
}

// verifies: REQ-CAT-13
#[test]
fn ramp_within_one_node_is_invalid() {
    assert_rule_rejects(
        "ramp-within-one-node",
        &RuleViolation::RampWithinOneNode { tuple: 0 },
    );
}

// verifies: REQ-CAT-13, DEC-16
#[test]
fn verifier_advertising_a_tuple_is_invalid() {
    assert_schema_rejects("verifier-advertising-a-tuple");
}

// verifies: REQ-CAT-16
#[test]
fn provider_without_an_identifier_or_status_query_is_invalid() {
    assert_schema_rejects("provider-without-identifier-or-status-query");
    assert_schema_rejects("verifier-without-identifier-or-status-query");
}

// verifies: REQ-CAT-6, REQ-CAT-8, REQ-CAT-10, REQ-CAT-11, REQ-CAT-12, REQ-CAT-13, REQ-CAT-16, REQ-CAT-20, REQ-CAT-22, REQ-ORD-39
#[test]
fn every_valid_fixture_validates_and_every_invalid_one_is_rejected() {
    let validator = advertisement_validator();

    let valid = fixtures("valid");
    assert!(!valid.is_empty(), "no valid fixtures");
    for (name, instance) in &valid {
        let errors = schema_errors(&validator, instance);
        assert!(
            errors.is_empty(),
            "valid fixture {name} was rejected by the schemas: {errors:?}"
        );
        let advertisement: Advertisement = serde_json::from_value(instance.clone())
            .unwrap_or_else(|e| panic!("valid fixture {name} doesn't deserialise: {e}"));
        assert_eq!(
            provider_role::validate(&advertisement),
            Ok(()),
            "valid fixture {name} broke a rule"
        );
        let round_trip = serde_json::to_value(&advertisement).expect("serialises");
        assert_eq!(
            &round_trip, instance,
            "valid fixture {name} changes through the Rust types"
        );
    }

    let schema_rules = schema_rule_fixtures();
    let rust_rules = rust_rule_fixtures();
    let invalid = fixtures("invalid");
    assert!(!invalid.is_empty(), "no invalid fixtures");
    for (name, _) in &invalid {
        let listed = schema_rules.iter().filter(|(n, _)| n == name).count()
            + rust_rules.iter().filter(|(n, _)| n == name).count();
        assert_eq!(
            listed, 1,
            "invalid fixture {name} must name its rule exactly once in this test"
        );
    }
    for (name, _) in &schema_rules {
        assert!(
            invalid.iter().any(|(n, _)| n == name),
            "missing invalid fixture {name}"
        );
        assert_schema_rejects(name);
    }
    for (name, rule) in &rust_rules {
        assert!(
            invalid.iter().any(|(n, _)| n == name),
            "missing invalid fixture {name}"
        );
        assert_rule_rejects(name, rule);
    }
}

// verifies: DEC-4
#[test]
fn version_matches_the_changelog() {
    let changelog = fs::read_to_string(contract_dir().join("CHANGELOG.md")).expect("changelog");
    let top = changelog
        .lines()
        .find_map(|line| line.strip_prefix("## "))
        .expect("changelog has a version heading");
    assert_eq!(top.trim(), provider_role::VERSION);
}
