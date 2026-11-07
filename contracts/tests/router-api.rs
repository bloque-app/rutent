//! The router-api contract's tests: its schemas against its fixtures, and its Rust types against its schemas
//! (`DEC-4`, `T-004-01`).

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::router_api::{
    Candidate, CatalogueSnapshot, FigureValue, Id, NonEmpty, Operation, Part, PolicyBounds,
    RecordedDecision, Request, VERSION,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The contract's shapes; each has `schemas/<shape>.schema.json` and the `$id` `urn:rutent:router-api:<shape>`.
const SHAPES: [&str; 7] = [
    "common",
    "request",
    "catalogue-snapshot",
    "policy-bounds",
    "operation",
    "candidate",
    "decision-record",
];

/// Invalid fixtures the schemas accept and the Rust types refuse, each with the refusal it must give. Every other
/// invalid fixture is listed in `SCHEMA_RULES`.
const RUST_RULES: [(&str, &str); 101] = [
    (
        "operation-exchange-across-two-nodes",
        "an exchange stays at one node",
    ),
    (
        "operation-route-within-one-node",
        "a route runs between two different nodes",
    ),
    (
        "operation-route-changing-its-asset",
        "a route keeps its asset",
    ),
    (
        "operation-fee-above-amount-in",
        "an operation's fee is at most its amount in",
    ),
    (
        "operation-fee-in-another-asset",
        "an operation's fee is in its from asset",
    ),
    (
        "candidate-with-a-gap-between-operations",
        "each operation of a part starts where the one before it ended",
    ),
    (
        "candidate-taking-in-more-than-delivered",
        "each operation of a part takes in what the one before it delivered",
    ),
    (
        "candidate-outside-band-naming-no-operation",
        "an outside-band or unconfirmed reason names an operation of its candidate",
    ),
    (
        "candidate-outranked-by-itself",
        "a candidate isn't outranked by itself",
    ),
    (
        "policy-bounds-two-ceilings-for-one-provider-and-asset",
        "there is at most one ceiling per provider or publisher and asset",
    ),
    (
        "catalogue-snapshot-figure-id-twice",
        "figure ids are unique in a snapshot",
    ),
    (
        "catalogue-snapshot-two-exposures-for-one-provider-and-asset",
        "there is at most one exposure per provider and asset",
    ),
    (
        "catalogue-snapshot-price-for-a-missing-figure",
        "a figure names a tuple figure of its snapshot",
    ),
    (
        "catalogue-snapshot-price-of-a-price",
        "a figure names a tuple figure of its snapshot",
    ),
    (
        "catalogue-snapshot-price-from-another-provider",
        "a figure about a tuple is from its tuple's provider and publisher",
    ),
    (
        "catalogue-snapshot-ramp-price-with-a-rate",
        "a price states a rate exactly when its tuple is an exchange's",
    ),
    (
        "catalogue-snapshot-exchange-price-without-a-rate",
        "a price states a rate exactly when its tuple is an exchange's",
    ),
    (
        "catalogue-snapshot-levels-out-of-order",
        "levels' bounds strictly increase",
    ),
    (
        "catalogue-snapshot-quote-by-another-maker",
        "a statement's maker is its figure's provider",
    ),
    (
        "catalogue-snapshot-quote-fee-in-another-asset",
        "a quote's fee is in its tuple's input asset",
    ),
    (
        "catalogue-snapshot-quote-fee-above-amount-in",
        "a quote's fee is at most its amount in",
    ),
    (
        "catalogue-snapshot-dry-run-for-another-kind-of-operation",
        "a dry run answers for its tuple's kind of operation",
    ),
    (
        "catalogue-snapshot-accepted-dry-run-without-validity",
        "a statement figure's validity runs from its making to its valid_until, rounded down",
    ),
    (
        "catalogue-snapshot-refused-dry-run-stating-a-validity",
        "a statement figure's validity runs from its making to its valid_until, rounded down",
    ),
    (
        "catalogue-snapshot-quote-age-unlike-its-making",
        "a statement figure's age runs from its making to the snapshot, rounded up",
    ),
    (
        "catalogue-snapshot-quote-validity-unlike-its-valid-until",
        "a statement figure's validity runs from its making to its valid_until, rounded down",
    ),
    (
        "catalogue-snapshot-quote-made-after-the-snapshot",
        "a statement figure's age runs from its making to the snapshot, rounded up",
    ),
    (
        "catalogue-snapshot-quote-age-rounded-down",
        "a statement figure's age runs from its making to the snapshot, rounded up",
    ),
    (
        "catalogue-snapshot-quote-validity-rounded-up",
        "a statement figure's validity runs from its making to its valid_until, rounded down",
    ),
    (
        "catalogue-snapshot-dry-run-by-another-maker",
        "a statement's maker is its figure's provider",
    ),
    (
        "decision-record-candidate-id-twice",
        "candidate ids are unique in a decision record",
    ),
    (
        "decision-record-chosen-not-a-candidate",
        "the chosen candidate is in the record",
    ),
    (
        "decision-record-chosen-candidate-with-a-drop-reason",
        "every candidate but the chosen one has a drop reason, and the chosen one has none",
    ),
    (
        "decision-record-candidate-without-a-drop-reason",
        "every candidate but the chosen one has a drop reason, and the chosen one has none",
    ),
    (
        "decision-record-part-not-starting-at-the-origin",
        "every part starts at the request's origin",
    ),
    (
        "decision-record-part-not-ending-at-the-destination",
        "every part ends at the request's destination",
    ),
    (
        "decision-record-parts-not-adding-up",
        "a candidate's parts add up to the request's amount",
    ),
    (
        "decision-record-operation-citing-a-missing-figure",
        "every figure an operation names is in the snapshot",
    ),
    (
        "decision-record-stale-reason-naming-a-missing-figure",
        "every figure a drop reason names is in the snapshot",
    ),
    (
        "decision-record-outranked-by-a-missing-candidate",
        "an outranking candidate is in the record",
    ),
    (
        "decision-record-void-reason-naming-a-statement-valid-here",
        "a drop reason's figure supports the drop",
    ),
    (
        "decision-record-chosen-candidate-using-a-void-statement",
        "a kept candidate uses no void statement",
    ),
    (
        "decision-record-stale-reason-naming-a-fresh-figure",
        "a drop reason's figure supports the drop",
    ),
    (
        "decision-record-drop-reason-naming-a-figure-its-candidate-does-not-cite",
        "a drop reason names a figure its candidate cites",
    ),
    (
        "decision-record-ineligible-on-a-yes-judgement",
        "a drop reason's figure supports the drop",
    ),
    (
        "decision-record-dry-run-refused-on-an-accepted-dry-run",
        "a drop reason's figure supports the drop",
    ),
    (
        "decision-record-quote-voided-by-a-condition-it-does-not-name",
        "a drop reason's figure supports the drop",
    ),
    (
        "decision-record-quote-voided-by-an-unchanged-amount",
        "a drop reason's figure supports the drop",
    ),
    (
        "decision-record-unconfirmed-naming-a-confirmed-operation",
        "an unconfirmed reason names an operation the choice round finds unconfirmed",
    ),
    (
        "decision-record-unconfirmed-in-a-shortlist-round",
        "an unconfirmed reason names an operation the choice round finds unconfirmed",
    ),
    (
        "decision-record-choosing-an-unconfirmed-candidate",
        "the choice round chooses only a candidate whose every operation is confirmed",
    ),
    (
        "decision-record-chosen-candidate-using-a-stale-figure",
        "a kept candidate uses no figure past its validity",
    ),
    (
        "decision-record-operation-citing-another-providers-tuple",
        "every operation cites a tuple of its own provider",
    ),
    (
        "decision-record-operation-citing-a-tuple-with-other-ends",
        "every operation cites a tuple with its own two ends",
    ),
    (
        "decision-record-operation-citing-no-tuple",
        "every operation cites one tuple figure",
    ),
    (
        "decision-record-over-a-ceiling-at-other-decimals",
        "every ceiling a drop reason names is in the bounds",
    ),
    (
        "decision-record-refused-dry-run-moved-to-the-deposit",
        "every other figure an operation cites names the operation's own tuple",
    ),
    (
        "decision-record-ineligible-judgement-moved-to-the-withdrawal",
        "every other figure an operation cites names the operation's own tuple",
    ),
    (
        "decision-record-price-of-another-tuple",
        "every other figure an operation cites names the operation's own tuple",
    ),
    (
        "decision-record-chosen-candidate-citing-a-judgement-of-no",
        "a kept candidate cites no refused dry run and no judgement of no",
    ),
    (
        "decision-record-shortlisted-candidate-citing-a-refused-dry-run",
        "a kept candidate cites no refused dry run and no judgement of no",
    ),
    (
        "decision-record-operation-citing-no-price",
        "every operation cites one price, for its own tuple",
    ),
    (
        "decision-record-dry-run-cited-at-another-amount",
        "every dry run an operation cites is for its amount in",
    ),
    (
        "catalogue-snapshot-exposure-under-another-publisher",
        "every provider has one publisher across the snapshot",
    ),
    (
        "catalogue-snapshot-disagreeing-dry-runs",
        "dry-run answers for one tuple and amount agree",
    ),
    (
        "catalogue-snapshot-disagreeing-judgements",
        "eligibility judgements of one subject for one tuple agree",
    ),
    (
        "catalogue-snapshot-tuple-listed-twice",
        "a snapshot holds one tuple figure per provider and ends",
    ),
    (
        "catalogue-snapshot-second-price-for-a-tuple",
        "a snapshot holds at most one price per tuple",
    ),
    (
        "catalogue-snapshot-one-relying-providers-judgements-about-two-subjects",
        "all judgements about one relying provider's tuples are about one subject",
    ),
    (
        "catalogue-snapshot-second-verifier-bringing-another-subject-to-a-tuple",
        "all judgements about one relying provider's tuples are about one subject",
    ),
    (
        "catalogue-snapshot-fresh-quote-meeting-a-refused-dry-run",
        "dry-run answers for one tuple and amount agree",
    ),
    (
        "catalogue-snapshot-accepted-dry-run-for-another-amount",
        "an accepted dry run states the amount its figure records",
    ),
    (
        "decision-record-chosen-operation-claiming-a-lower-fee-than-its-quote",
        "the choice round chooses only a candidate whose every operation is confirmed",
    ),
    (
        "decision-record-chosen-operation-claiming-more-than-its-quote-delivers",
        "the choice round chooses only a candidate whose every operation is confirmed",
    ),
    (
        "decision-record-below-floor-without-a-floor",
        "a below-floor reason names a candidate that arrives below the floor",
    ),
    (
        "decision-record-no-positive-outcome-for-a-candidate-that-arrives",
        "a no-positive-outcome reason names a candidate with a part that arrives with nothing",
    ),
    (
        "decision-record-over-a-ceiling-it-stays-under",
        "an over-ceiling reason names a ceiling the candidate passes",
    ),
    (
        "decision-record-excluded-without-an-exclusion",
        "an excluded-by-policy reason names a candidate that uses an excluded provider or tuple",
    ),
    (
        "decision-record-outside-band-for-an-operation-within-it",
        "an outside-band reason names an operation outside its tuple's bands or levels",
    ),
    (
        "decision-record-chosen-candidate-below-the-floor",
        "a kept candidate arrives at or above the floor",
    ),
    (
        "decision-record-chosen-candidate-over-a-ceiling",
        "a kept candidate passes no ceiling",
    ),
    (
        "decision-record-chosen-candidate-using-an-excluded-provider",
        "a kept candidate uses no excluded provider or tuple",
    ),
    (
        "decision-record-chosen-candidate-outside-a-band",
        "a kept candidate's every operation is within its tuple's bands and levels",
    ),
    (
        "decision-record-chosen-candidate-over-the-hop-limit",
        "a kept candidate keeps within the part and hop limits",
    ),
    (
        "decision-record-over-a-ceiling-the-bounds-lack",
        "every ceiling a drop reason names is in the bounds",
    ),
    (
        "decision-record-shortlisted-candidate-with-a-part-arriving-with-nothing",
        "a kept candidate arrives with something in every part",
    ),
    (
        "decision-record-chosen-candidate-over-a-ceiling-on-what-it-produces",
        "a kept candidate passes no ceiling",
    ),
    (
        "decision-record-unconfirmed-leaving-its-quote-uncited",
        "every operation cites every usable answer and judgement about its tuple at its amount in",
    ),
    (
        "decision-record-choice-leaving-a-judgement-of-no-uncited",
        "every operation cites every usable answer and judgement about its tuple at its amount in",
    ),
    (
        "decision-record-operation-citing-a-quote-at-another-amount",
        "a fresh quote an operation cites is for its amount in",
    ),
    (
        "decision-record-usable-answers-for-one-amount-disagreeing",
        "usable accepted answers for one tuple and amount agree on their fee and amount out",
    ),
    (
        "decision-record-stale-reason-on-an-operation-a-usable-answer-covers",
        "a stale, void or voided statement drops its candidate only where no usable answer covers its operation",
    ),
    (
        "decision-record-quote-voided-on-an-operation-a-usable-answer-covers",
        "a stale, void or voided statement drops its candidate only where no usable answer covers its operation",
    ),
    (
        "decision-record-quote-voided-citing-a-second-quote-at-another-amount",
        "a fresh quote an operation cites is for its amount in",
    ),
    (
        "decision-record-beyond-levels-naming-an-operation-before-the-last",
        "a beyond-levels reason names its part's last operation, which arrives with nothing",
    ),
    (
        "decision-record-beyond-levels-with-something-out",
        "a beyond-levels reason names its part's last operation, which arrives with nothing",
    ),
    (
        "decision-record-beyond-levels-within-the-levels",
        "a beyond-levels reason names an operation whose net is above its price's last level",
    ),
    (
        "decision-record-confirmed-by-an-answer-made-before-the-shortlist",
        "a confirming answer is made at or after the shortlist round it refines",
    ),
    (
        "decision-record-judgement-of-no-made-before-the-shortlist-left-uncited",
        "every operation cites every usable answer and judgement about its tuple at its amount in",
    ),
    (
        "decision-record-judgement-of-no-between-the-shortlist-and-the-answers-left-uncited",
        "every operation cites every usable answer and judgement about its tuple at its amount in",
    ),
    (
        "decision-record-refining-a-shortlist-taken-after-its-snapshot",
        "a choice round refines a shortlist round taken at or before its own snapshot",
    ),
];

/// Invalid fixtures the schemas reject, each with the exact (instance location, keyword) pairs of the one rule it
/// breaks, without repeats.
const SCHEMA_RULES: [(&str, &[(&str, &str)]); 61] = [
    (
        "operation-citing-a-figure-twice",
        &[("/figures", "uniqueItems")],
    ),
    (
        "candidate-dropped-for-an-unknown-reason",
        &[("/dropped/reason", "enum")],
    ),
    ("candidate-dropped-null", &[("/dropped", "type")]),
    (
        "candidate-over-a-ceiling-of-no-scope",
        &[("/dropped", "required")],
    ),
    (
        "candidate-part-without-operations",
        &[("/parts/0/operations", "minItems")],
    ),
    (
        "candidate-with-an-untyped-operation",
        &[("/parts/0/operations/1", "required")],
    ),
    ("candidate-without-parts", &[("/parts", "minItems")]),
    (
        "catalogue-snapshot-age-beyond-a-safe-integer",
        &[("/figures/0/age_seconds", "maximum")],
    ),
    (
        "catalogue-snapshot-age-negative",
        &[("/figures/0/age_seconds", "minimum")],
    ),
    (
        "catalogue-snapshot-dry-run-of-an-unknown-outcome",
        &[("/figures/15/value/result/statement/outcome/status", "enum")],
    ),
    (
        "catalogue-snapshot-eligibility-figure-without-validity",
        &[("/figures/17", "required")],
    ),
    (
        "catalogue-snapshot-exposure-from-a-provider",
        &[("/figures/14/origin/kind", "const")],
    ),
    (
        "catalogue-snapshot-figure-of-an-unknown-kind",
        &[("/figures/1/value/kind", "enum")],
    ),
    (
        "catalogue-snapshot-figure-validity-null",
        &[("/figures/0/valid_for_seconds", "type")],
    ),
    (
        "catalogue-snapshot-figure-without-age",
        &[("/figures/0", "required")],
    ),
    (
        "catalogue-snapshot-figure-without-origin",
        &[("/figures/0", "required")],
    ),
    (
        "catalogue-snapshot-fresh-quote-of-a-refusal",
        &[("/figures/6/value/quote/statement/outcome/status", "const")],
    ),
    (
        "catalogue-snapshot-levels-empty",
        &[("/figures/9/value/rate/levels", "minItems")],
    ),
    (
        "catalogue-snapshot-origin-without-module-version",
        &[("/figures/0/origin", "required")],
    ),
    (
        "catalogue-snapshot-origin-without-publisher",
        &[("/figures/0/origin", "required")],
    ),
    (
        "catalogue-snapshot-quote-figure-without-validity",
        &[("/figures/6", "required")],
    ),
    (
        "catalogue-snapshot-quote-of-another-shape",
        &[("/figures/6/value/quote/shape", "const")],
    ),
    (
        "catalogue-snapshot-quote-slippage-beyond-ten-thousand-bps",
        &[(
            "/figures/6/value/quote/statement/outcome/slippage/max_bps",
            "maximum",
        )],
    ),
    (
        "catalogue-snapshot-quote-voided-by-an-unknown-condition",
        &[(
            "/figures/6/value/quote/statement/outcome/voided_by/0",
            "enum",
        )],
    ),
    (
        "catalogue-snapshot-quote-voided-by-nothing",
        &[(
            "/figures/6/value/quote/statement/outcome/voided_by",
            "minItems",
        )],
    ),
    (
        "catalogue-snapshot-quote-voided-twice-by-one-condition",
        &[(
            "/figures/6/value/quote/statement/outcome/voided_by",
            "uniqueItems",
        )],
    ),
    (
        "catalogue-snapshot-quote-with-a-rate-but-no-slippage",
        &[("/figures/6/value/quote/statement/outcome", "required")],
    ),
    (
        "catalogue-snapshot-quote-without-direction",
        &[("/figures/6/value/quote/statement/outcome", "required")],
    ),
    (
        "catalogue-snapshot-quote-without-envelope",
        &[
            ("/figures/6/value/quote", "additionalProperties"),
            ("/figures/6/value/quote", "required"),
        ],
    ),
    (
        "catalogue-snapshot-quote-without-settlement-time",
        &[("/figures/6/value/quote/statement/outcome", "required")],
    ),
    (
        "catalogue-snapshot-quote-without-signature",
        &[("/figures/6/value/quote", "required")],
    ),
    (
        "catalogue-snapshot-quote-without-valid-until",
        &[("/figures/6/value/quote/statement/outcome", "required")],
    ),
    (
        "catalogue-snapshot-quote-without-void-conditions",
        &[("/figures/6/value/quote/statement/outcome", "required")],
    ),
    (
        "catalogue-snapshot-rate-as-a-bare-number",
        &[
            ("/figures/5/value/rate/rate", "not"),
            ("/figures/5/value/rate/rate", "type"),
        ],
    ),
    (
        "catalogue-snapshot-taken-at-second-sixty",
        &[("/taken_at", "pattern")],
    ),
    (
        "catalogue-snapshot-tuple-from-the-operator",
        &[("/figures/0/origin/kind", "const")],
    ),
    ("catalogue-snapshot-without-taken-at", &[("", "required")]),
    (
        "decision-record-chosen-null",
        &[("/statement/chosen", "type")],
    ),
    ("decision-record-of-another-shape", &[("/shape", "const")]),
    (
        "decision-record-shortlist-round-choosing",
        &[("/statement", "not")],
    ),
    ("decision-record-without-envelope", &[("", "required")]),
    (
        "decision-record-without-router-version",
        &[("/statement", "required")],
    ),
    (
        "decision-record-without-snapshot",
        &[("/statement", "required")],
    ),
    (
        "operation-amount-with-a-fraction",
        &[("/amount_in", "pattern")],
    ),
    ("operation-citing-no-figure", &[("/figures", "minItems")]),
    ("operation-fee-as-a-bare-amount", &[("/fee", "type")]),
    ("operation-of-an-unknown-kind", &[("/kind", "enum")]),
    (
        "operation-with-an-extra-field",
        &[("", "unevaluatedProperties")],
    ),
    (
        "operation-withdrawal-into-an-outer-node",
        &[("/to/node", "type")],
    ),
    (
        "policy-bounds-excluding-a-provider-twice",
        &[("/excluded_providers", "uniqueItems")],
    ),
    ("policy-bounds-with-no-hops", &[("/max_hops", "minimum")]),
    ("policy-bounds-without-ceilings", &[("", "required")]),
    ("policy-bounds-without-deployment", &[("", "required")]),
    (
        "policy-bounds-without-protocol-version",
        &[("", "required")],
    ),
    ("request-amount-zero", &[("/amount", "pattern")]),
    (
        "request-choice-round-refining-nothing",
        &[("/round", "required")],
    ),
    ("request-floor-null", &[("/floor", "type")]),
    (
        "request-objective-as-a-bare-string",
        &[("/objective", "type")],
    ),
    (
        "request-of-an-unknown-objective",
        &[("/objective/kind", "enum")],
    ),
    ("request-without-objective", &[("", "required")]),
    ("request-without-round", &[("", "required")]),
];

/// Invalid fixtures the Rust types accept on purpose, each with its reason. Empty: every rule the schemas state
/// is also held by a Rust field type or a `check` that deserialising runs (`DEC-4`).
const ACCEPTED_BY_RUST_ON_PURPOSE: [&str; 0] = [];

fn contracts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn contract_dir() -> PathBuf {
    contracts_dir().join("router-api")
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
    format!("urn:rutent:router-api:{shape}")
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

/// A validator for one shape, with this contract's schemas and those it refers to registered by `$id`, so
/// every `$ref` resolves offline.
fn validator(shape: &str) -> Validator {
    let mut registry = Registry::new();
    let others = schemas_of("provider-role").into_iter();
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

/// The fixture deserialised into its Rust type, its statement shape checked, and serialised back.
fn through_rust(shape: &str, value: &Value) -> Result<Value, String> {
    match shape {
        "request" => round_trip::<Request>(value),
        "catalogue-snapshot" => round_trip::<CatalogueSnapshot>(value),
        "policy-bounds" => round_trip::<PolicyBounds>(value),
        "operation" => round_trip::<Operation>(value),
        "candidate" => round_trip::<Candidate>(value),
        "decision-record" => {
            let record: RecordedDecision =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            record.check_shape().map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(&record).map_err(|e| e.to_string())
        }
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

/// Asserts that the schemas and the Rust types both reject an invalid fixture.
fn assert_rejected_by_both(name: &str) {
    let shape = shape_of(name);
    let value = fixture("invalid", name);
    assert!(
        !validator(shape).is_valid(&value),
        "{name} should be rejected by the {shape} schema"
    );
    assert!(
        through_rust(shape, &value).is_err(),
        "{name} should be refused by the Rust types"
    );
}

fn valid_record(name: &str) -> RecordedDecision {
    let value = fixture("valid", name);
    assert!(validator("decision-record").is_valid(&value), "{name}");
    serde_json::from_value(value).unwrap_or_else(|e| panic!("{name}: {e}"))
}

// verifies: REQ-QTE-4
#[test]
fn figure_without_age_or_origin_is_invalid() {
    assert_rejected_by_both("catalogue-snapshot-figure-without-age");
    assert_rejected_by_both("catalogue-snapshot-figure-without-origin");
}

// verifies: REQ-QTE-16
#[test]
fn split_of_one_part_is_valid() {
    let record = valid_record("decision-record-split-of-one-part");
    assert_eq!(record.statement.chosen().unwrap().parts.len(), 1);
}

// verifies: REQ-QTE-16
#[test]
fn split_of_three_parts_is_valid() {
    let record = valid_record("decision-record-split-of-three-parts");
    assert_eq!(record.statement.chosen().unwrap().parts.len(), 3);
}

// verifies: REQ-ORD-37
#[test]
fn route_with_an_untyped_operation_is_invalid() {
    assert_rejected_by_both("candidate-with-an-untyped-operation");
}

// verifies: REQ-PRT-5
#[test]
fn operation_of_an_unknown_kind_is_refused() {
    assert_rejected_by_both("operation-of-an-unknown-kind");
}

// verifies: REQ-QTE-1, REQ-QTE-4, REQ-QTE-16, REQ-AUD-4, REQ-ORD-37, DEC-4, DEC-8
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
        let schema_rule = SCHEMA_RULES.iter().find(|(n, _)| n == name);
        let rust_rule = RUST_RULES.iter().find(|(n, _)| n == name);
        match (schema_rule, rust_rule) {
            (Some((_, expected)), None) => {
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
        .map(|(n, _)| *n)
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
        .filter(|(name, value)| {
            through_rust(shape_of(name), value).is_ok()
                && !ACCEPTED_BY_RUST_ON_PURPOSE.contains(&name.as_str())
        })
        .map(|(name, _)| name)
        .collect();
    assert!(accepted.is_empty(), "Rust accepts: {}", accepted.join(", "));
}

// verifies: REQ-QTE-4
#[test]
fn a_quote_past_its_validity_is_not_within_it() {
    let record = valid_record("decision-record-split-of-one-part");
    let figure = |id: &str| {
        record
            .statement
            .snapshot
            .figures
            .iter()
            .find(|f| f.figure_id.as_str() == id)
            .unwrap()
    };
    assert!(figure("f-ex-a-quote").is_within_validity());
    assert!(!figure("f-ex-a-quote-old").is_within_validity());
    assert!(figure("f-w").is_within_validity());
    assert!(matches!(
        figure("f-ex-a-quote").value,
        FigureValue::FreshQuote { .. }
    ));
}

// verifies: REQ-AUD-4, DEC-4
#[test]
fn a_record_built_in_rust_is_checked_whole() {
    let mut record = valid_record("decision-record-split-of-one-part").statement;
    assert_eq!(record.check(), Ok(()));

    let mut deep = record.clone();
    let mut parts: Vec<Part> = deep.candidates[1].parts.to_vec();
    let mut operations: Vec<Operation> = parts[0].operations.to_vec();
    operations[2].fee.asset = "USDT".to_owned();
    parts[0].operations = NonEmpty::new(operations).unwrap();
    deep.candidates[1].parts = NonEmpty::new(parts).unwrap();
    assert!(
        deep.check().is_err(),
        "an operation deep in a candidate is checked"
    );

    record.snapshot.figures[1].figure_id = Id::new("f-w").unwrap();
    assert!(record.check().is_err(), "the snapshot is checked");
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
