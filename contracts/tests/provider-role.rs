//! The provider-role contract's tests (`T-002-01`, `T-002-02`, `T-002-03`): its schemas against its fixtures, its Rust types
//! against its schemas, and its Rust validation.

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::provider_role::{
    self, Advertisement, AmountMismatch, Category, ContextMismatch, Deployment, NodeRef, Operation,
    OuterNode, Remainder, ResultBody, RuleViolation, Statement, StatementShape, Terms,
    TransferOutcome,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Every schema of the contract, registered by `$id` as `urn:rutent:provider-role:<name>`.
const SCHEMAS: [&str; 9] = [
    "common",
    "declarations",
    "reserve-declaration",
    "advertisement",
    "statement-envelope",
    "operation",
    "result",
    "judgement",
    "reversal-confirmation",
];

/// The shapes fixtures are written in. A fixture's shape is the longest of these its file name starts with,
/// followed by `-`. `successive-advertisements` and `answered-operation` are bundles of values, not schemas of
/// their own: see [`SuccessiveAdvertisements`] and [`AnsweredOperation`].
const SHAPES: [&str; 6] = [
    "advertisement",
    "statement-envelope",
    "operation",
    "result",
    "successive-advertisements",
    "answered-operation",
];

/// The members of a `successive-advertisements` bundle: the provider's accepted advertisement and the one that
/// follows it, each an enveloped advertisement, for the rules that relate them (`validate_successor`).
const SUCCESSIVE_ADVERTISEMENTS_MEMBERS: [(&str, &str); 2] = [
    ("previous", "urn:rutent:provider-role:advertisement"),
    ("next", "urn:rutent:provider-role:advertisement"),
];

/// The members of an `answered-operation` bundle: an enveloped operation, the enveloped result that answers it,
/// and the terms of the tuple it runs through, for the rules that relate their amounts (`check_amounts`).
const ANSWERED_OPERATION_MEMBERS: [(&str, &str); 3] = [
    ("operation", "urn:rutent:provider-role:operation"),
    ("result", "urn:rutent:provider-role:result"),
    (
        "terms",
        "urn:rutent:provider-role:advertisement#/$defs/terms",
    ),
];

/// A `successive-advertisements` fixture.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuccessiveAdvertisements {
    previous: Statement<Advertisement>,
    next: Statement<Advertisement>,
}

/// An `answered-operation` fixture.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnsweredOperation {
    operation: Statement<Operation>,
    result: Statement<ResultBody>,
    terms: Terms,
}

impl AnsweredOperation {
    fn check(&self) -> Result<Option<Remainder>, Vec<AmountMismatch>> {
        provider_role::check_amounts(
            &self.operation.statement,
            &self.result.statement,
            &self.terms,
        )
    }
}

/// Invalid fixtures the schemas must reject, each with the exact (instance location, keyword) errors of the one
/// rule it breaks.
fn schema_rule_fixtures() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            "advertisement-key-without-a-purpose",
            vec![("/statement/keys/0", "required")],
        ),
        (
            "advertisement-key-of-an-unknown-purpose",
            vec![("/statement/keys/0/purpose", "enum")],
        ),
        (
            "operation-signed-with-a-non-canonical-alias-of-the-operator-key",
            vec![("/signature/key", "pattern")],
        ),
        (
            "advertisement-without-offers",
            vec![("/statement/offers", "minProperties")],
        ),
        (
            "advertisement-band-amount-with-a-fraction",
            vec![(
                "/statement/offers/ramp/tuples/0/terms/bands/0/min",
                "pattern",
            )],
        ),
        (
            "advertisement-evidence-availability-beyond-two-to-the-fifty-three",
            vec![("/statement/declarations/evidence/availability", "oneOf")],
        ),
        (
            "advertisement-evidence-grade-out-of-range",
            vec![("/statement/declarations/evidence/grade", "maximum")],
        ),
        (
            "advertisement-evidence-without-availability",
            vec![("/statement/declarations/evidence", "required")],
        ),
        (
            "advertisement-exchange-asset-without-units",
            vec![("/statement/offers/exchange/tuples/0/to", "required")],
        ),
        (
            "advertisement-exchange-outside-an-inner-node",
            vec![("/statement/offers/exchange/tuples/0/node", "type")],
        ),
        (
            "advertisement-exchange-spanning-two-nodes",
            vec![
                (
                    "/statement/offers/exchange/tuples/0",
                    "additionalProperties",
                ),
                ("/statement/offers/exchange/tuples/0", "required"),
            ],
        ),
        (
            "advertisement-offer-under-an-unknown-category",
            vec![("/statement/offers", "additionalProperties")],
        ),
        (
            "advertisement-offer-without-tuples",
            vec![("/statement/offers/ramp/tuples", "minItems")],
        ),
        (
            "advertisement-outer-node-naming-its-reserve",
            vec![("/statement/offers/reserve/tuples/0", "oneOf")],
        ),
        (
            "advertisement-private-node-declared-twice",
            vec![("/statement/private_nodes", "uniqueItems")],
        ),
        (
            "advertisement-provider-without-identifier-or-status-query",
            vec![("/statement/declarations/tracking", "minItems")],
        ),
        (
            "advertisement-provider-without-publisher",
            vec![("/statement", "required")],
        ),
        (
            "advertisement-provider-without-safe-retry-declaration",
            vec![("/statement/declarations", "required")],
        ),
        (
            "advertisement-ramp-changing-its-asset",
            vec![
                ("/statement/offers/ramp/tuples/0", "additionalProperties"),
                ("/statement/offers/ramp/tuples/0", "required"),
                ("/statement/offers/ramp/tuples/0", "required"),
            ],
        ),
        (
            "advertisement-ramp-to-an-outer-node",
            vec![("/statement/offers/ramp/tuples/0/destination", "type")],
        ),
        (
            "advertisement-reserve-between-two-inner-nodes",
            vec![("/statement/offers/reserve/tuples/0", "oneOf")],
        ),
        (
            "advertisement-reserve-between-two-outer-nodes",
            vec![("/statement/offers/reserve/tuples/0", "oneOf")],
        ),
        (
            "advertisement-reserve-declaration-without-automatic-deposit-answer",
            vec![("/statement/offers/reserve/declaration", "required")],
        ),
        (
            "advertisement-reserve-declaration-without-maker",
            vec![("/statement/offers/reserve/declaration", "required")],
        ),
        (
            "advertisement-reserve-without-declaration",
            vec![("/statement/offers/reserve", "required")],
        ),
        (
            "advertisement-tuple-without-amount-bands",
            vec![("/statement/offers/ramp/tuples/0/terms/bands", "minItems")],
        ),
        (
            "advertisement-tuple-without-gross-or-net",
            vec![("/statement/offers/ramp/tuples/0/terms", "required")],
        ),
        (
            "advertisement-tuple-without-required-inputs-declared",
            vec![("/statement/offers/ramp/tuples/0/terms", "required")],
        ),
        (
            "advertisement-tuple-without-units-at-its-destination",
            vec![("/statement/offers/ramp/tuples/0/decimals", "required")],
        ),
        (
            "advertisement-under-another-shape",
            vec![("/shape", "const")],
        ),
        (
            "advertisement-value-moving-provider-without-evidence-declaration",
            vec![("/statement/declarations", "required")],
        ),
        (
            "advertisement-verifier-advertising-a-tuple",
            vec![("/statement/offers/verifier", "additionalProperties")],
        ),
        (
            "advertisement-verifier-without-classes",
            vec![("/statement/offers/verifier/classes", "minItems")],
        ),
        (
            "advertisement-verifier-without-identifier-or-status-query",
            vec![("/statement/declarations/tracking", "minItems")],
        ),
        (
            "advertisement-without-signing-keys",
            vec![("/statement/keys", "minItems")],
        ),
        (
            "advertisement-zero-fill-increment",
            vec![(
                "/statement/offers/exchange/tuples/0/terms/increment",
                "pattern",
            )],
        ),
        (
            "statement-envelope-without-deployment",
            vec![("", "required")],
        ),
        (
            "statement-envelope-without-protocol-version",
            vec![("", "required")],
        ),
        (
            "statement-envelope-without-signature",
            vec![("", "required")],
        ),
        (
            "statement-envelope-made-at-not-a-utc-time",
            vec![("/made_at", "pattern")],
        ),
        (
            "statement-envelope-naming-no-shape",
            vec![("/shape", "pattern")],
        ),
        (
            "statement-envelope-deployment-without-operator-key",
            vec![("/deployment", "required")],
        ),
        (
            "statement-envelope-protocol-version-not-major-minor-patch",
            vec![("/protocol_version", "pattern")],
        ),
        (
            "statement-envelope-signed-with-another-scheme",
            vec![("/signature/scheme", "enum")],
        ),
        (
            "statement-envelope-signature-with-padding",
            vec![("/signature/value", "pattern")],
        ),
        (
            "operation-of-an-unknown-kind",
            vec![("/statement/kind", "enum")],
        ),
        (
            "operation-withdrawal-into-an-outer-node",
            vec![("/statement/destination", "type")],
        ),
        (
            "operation-withdrawal-dry-run-without-a-session",
            vec![("/statement", "required")],
        ),
        (
            "operation-without-a-transaction",
            vec![("/statement", "required")],
        ),
        (
            "operation-conversion-without-a-direction",
            vec![("/statement", "required")],
        ),
        (
            "operation-balance-verification-as-a-dry-run",
            vec![("/statement", "additionalProperties")],
        ),
        (
            "operation-balance-verification-without-an-amount",
            vec![("/statement", "required")],
        ),
        (
            "operation-with-a-fractional-amount",
            vec![("/statement/amount", "pattern")],
        ),
        (
            "operation-with-an-input-of-a-bad-name",
            vec![("/statement/inputs", "propertyNames")],
        ),
        (
            "operation-judgement-question-carrying-personal-data",
            vec![("/statement", "additionalProperties")],
        ),
        (
            "operation-judgement-question-carrying-a-transaction",
            vec![("/statement", "additionalProperties")],
        ),
        ("operation-under-another-shape", vec![("/shape", "const")]),
        (
            "result-balance-answer-carrying-an-amount",
            vec![("/statement/outcome", "additionalProperties")],
        ),
        (
            "result-balance-answer-neither-yes-nor-no",
            vec![("/statement/outcome/sufficient", "type")],
        ),
        (
            "result-balance-refusal-saying-when-it-lifts",
            vec![("/statement/outcome/refusal/class", "enum")],
        ),
        (
            "result-refusal-without-its-class",
            vec![("/statement/outcome/refusal", "required")],
        ),
        (
            "result-refusal-of-an-unknown-class",
            vec![("/statement/outcome/refusal/class", "enum")],
        ),
        (
            "result-refusal-lifting-at-no-stated-time",
            vec![("/statement/outcome/refusal", "required")],
        ),
        (
            "result-final-refusal-carrying-a-time",
            vec![("/statement/outcome/refusal", "additionalProperties")],
        ),
        (
            "result-outcome-unknown-carrying-amounts",
            vec![("/statement/outcome", "additionalProperties")],
        ),
        (
            "result-success-without-evidence",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-success-with-no-evidence-listed",
            vec![("/statement/outcome/evidence", "minItems")],
        ),
        (
            "result-failure-without-evidence",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-evidence-not-saying-whose-it-is",
            vec![("/statement/outcome/evidence/0", "required")],
        ),
        (
            "result-evidence-graded-five",
            vec![("/statement/outcome/evidence/0/grade", "maximum")],
        ),
        (
            "result-conversion-success-without-a-rate",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-transfer-success-carrying-a-rate",
            vec![("/statement/outcome", "additionalProperties")],
        ),
        (
            "result-dry-run-quote-without-validity",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-quote-without-what-voids-it",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-quote-without-a-direction",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-conversion-quote-without-a-slippage-regime",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-transfer-quote-with-a-slippage-regime",
            vec![("/statement/outcome", "additionalProperties")],
        ),
        (
            "result-transfer-quote-without-a-settlement-time",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-settlement-time-beyond-two-to-the-fifty-three",
            vec![("/statement/outcome/settlement_seconds", "maximum")],
        ),
        (
            "result-of-an-unknown-status",
            vec![("/statement/outcome/status", "enum")],
        ),
        (
            "result-judgement-answering-not-known",
            vec![("/statement/outcome/judgement/statement/answer", "enum")],
        ),
        (
            "result-judgement-without-validity",
            vec![("/statement/outcome/judgement/statement", "required")],
        ),
        (
            "result-judgement-under-another-shape",
            vec![("/statement/outcome/judgement/shape", "const")],
        ),
        (
            "result-judgement-carrying-a-transaction",
            vec![("/statement", "additionalProperties")],
        ),
        ("result-under-another-shape", vec![("/shape", "const")]),
        (
            "result-reserve-dry-run-answer-without-what-voids-it",
            vec![("/statement/outcome", "required")],
        ),
        (
            "operation-reversal-as-a-dry-run",
            vec![("/statement", "additionalProperties")],
        ),
        (
            "operation-reversal-without-the-segment-it-reverses",
            vec![("/statement", "required")],
        ),
        (
            "operation-reversal-of-a-zero-amount",
            vec![("/statement/amount", "pattern")],
        ),
        (
            "operation-reversal-confirmation-asking-about-an-account",
            vec![("/statement", "additionalProperties")],
        ),
        (
            "operation-withdrawal-with-a-null-session",
            vec![("/statement/session", "type")],
        ),
        (
            "result-reversal-succeeded-without-evidence",
            vec![("/statement/outcome", "required")],
        ),
        (
            "result-reversal-succeeded-with-nothing-back",
            vec![("/statement/outcome/amount", "pattern")],
        ),
        (
            "result-reversal-failed",
            vec![("/statement/outcome/status", "enum")],
        ),
        (
            "result-reversal-confirmation-saying-whether-funds-suffice",
            vec![("/statement/outcome", "additionalProperties")],
        ),
        (
            "result-reversal-confirmation-naming-a-balance",
            vec![(
                "/statement/outcome/confirmation/statement",
                "additionalProperties",
            )],
        ),
        (
            "result-reversal-confirmation-under-another-shape",
            vec![("/statement/outcome/confirmation/shape", "const")],
        ),
        (
            "advertisement-tuple-with-a-null-increment",
            vec![("/statement/offers/ramp/tuples/0/terms/increment", "type")],
        ),
        (
            "advertisement-with-a-null-evidence-declaration",
            vec![("/statement/declarations/evidence", "type")],
        ),
        (
            "advertisement-with-a-null-safe-retry-declaration",
            vec![("/statement/declarations/safe_retry", "type")],
        ),
        (
            "advertisement-with-a-null-offer",
            vec![("/statement/offers/exchange", "type")],
        ),
        (
            "result-judgement-with-a-null-holder-key",
            vec![("/statement/outcome/judgement/statement/holder_key", "type")],
        ),
        (
            "statement-envelope-key-in-a-non-canonical-encoding",
            vec![("/signature/key", "pattern")],
        ),
        (
            "statement-envelope-signature-in-a-non-canonical-encoding",
            vec![("/signature/value", "pattern")],
        ),
    ]
}

/// Invalid fixtures that pass the schemas and must be rejected by a Rust rule, with the rule each breaks, as
/// `rust_rules` names it.
fn rust_rule_fixtures() -> Vec<(&'static str, String)> {
    let advertisement = |v: RuleViolation| format!("{:?}", vec![v]);
    let amounts = |v: AmountMismatch| format!("{:?}", vec![v]);
    vec![
        (
            "advertisement-ramp-within-one-node",
            advertisement(RuleViolation::RampWithinOneNode { tuple: 0 }),
        ),
        (
            "advertisement-band-with-min-above-max",
            advertisement(RuleViolation::BandMinAboveMax {
                category: Category::Ramp,
                tuple: 0,
                band: 0,
            }),
        ),
        (
            "advertisement-ramp-tuple-declared-twice",
            advertisement(RuleViolation::TupleDeclaredTwice {
                category: Category::Ramp,
                tuple: 1,
                first: 0,
            }),
        ),
        (
            "advertisement-reserve-tuple-declared-twice",
            advertisement(RuleViolation::TupleDeclaredTwice {
                category: Category::Reserve,
                tuple: 2,
                first: 0,
            }),
        ),
        (
            "advertisement-exchange-tuple-declared-twice",
            advertisement(RuleViolation::TupleDeclaredTwice {
                category: Category::Exchange,
                tuple: 1,
                first: 0,
            }),
        ),
        (
            "advertisement-reserve-declaration-of-another-maker",
            advertisement(RuleViolation::ReserveDeclarationOfAnotherMaker),
        ),
        (
            "advertisement-made-by-another-party",
            advertisement(RuleViolation::AdvertisementOfAnotherMaker),
        ),
        (
            "advertisement-signed-with-an-unpublished-key",
            advertisement(RuleViolation::SignedWithUnpublishedKey),
        ),
        (
            "advertisement-signed-with-its-own-statements-key",
            advertisement(RuleViolation::SignedWithStatementsKey),
        ),
        (
            "advertisement-listing-one-key-for-both-purposes",
            advertisement(RuleViolation::KeyListedTwice { key: 1, first: 0 }),
        ),
        (
            "successive-advertisements-signed-with-a-key-the-next-one-adds",
            advertisement(RuleViolation::SuccessorNotSignedWithPreviousAdvertisingKey),
        ),
        (
            "successive-advertisements-signed-with-the-previous-statements-key",
            // The one way through trips both rules: a statements key can sign `next` only once `next` lists it for
            // advertising, which promotes it.
            format!(
                "{:?}",
                vec![
                    RuleViolation::SuccessorNotSignedWithPreviousAdvertisingKey,
                    RuleViolation::StatementsKeyPromoted { key: 2 },
                ]
            ),
        ),
        (
            "successive-advertisements-promoting-the-previous-statements-key",
            advertisement(RuleViolation::StatementsKeyPromoted { key: 1 }),
        ),
        (
            "successive-advertisements-replaying-an-older-advertisement",
            advertisement(RuleViolation::SuccessorNotNewer),
        ),
        (
            "successive-advertisements-of-another-provider",
            advertisement(RuleViolation::SuccessorOfAnotherProvider),
        ),
        (
            "successive-advertisements-of-another-deployment",
            advertisement(RuleViolation::SuccessorOfAnotherDeployment),
        ),
        (
            "answered-operation-for-another-transaction",
            amounts(AmountMismatch::AnswersAnotherOperation),
        ),
        (
            "answered-operation-taking-its-fee-on-top-of-the-stated-amount",
            amounts(AmountMismatch::TookInMoreThanStated),
        ),
        (
            "answered-operation-charging-its-fee-in-another-asset",
            amounts(AmountMismatch::FeeInAnotherAsset),
        ),
        (
            "answered-operation-quoting-its-fee-in-another-asset",
            amounts(AmountMismatch::FeeInAnotherAsset),
        ),
        (
            "answered-operation-charging-more-than-it-took-in",
            amounts(AmountMismatch::FeeAboveAmountIn),
        ),
        (
            "answered-operation-leaving-an-undeclared-remainder",
            amounts(AmountMismatch::UndeclaredRemainder),
        ),
        (
            "answered-operation-leaving-a-whole-increment-behind",
            amounts(AmountMismatch::RemainderOfAWholeIncrement),
        ),
        (
            "operation-signed-with-another-key",
            "NotSignedByOperator".to_owned(),
        ),
        (
            "result-judgement-from-another-deployment",
            "Deployment".to_owned(),
        ),
        (
            "result-judgement-made-by-another-verifier",
            "NestedMaker".to_owned(),
        ),
        (
            "result-reversal-confirmation-from-another-deployment",
            "Deployment".to_owned(),
        ),
        (
            "result-reversal-confirmation-made-by-another-reserve",
            "NestedMaker".to_owned(),
        ),
    ]
}

/// The Rust rules of a statement's shape, run on a fixture that deserialises: an advertisement's
/// `validate_statement`, an operation's `check_signed_by_operator`, and a result's `check_context` in its own
/// deployment and protocol version, which reaches a nested judgement. The error is named as
/// `rust_rule_fixtures` names it.
fn rust_rules(shape: &str, value: &Value) -> Result<(), String> {
    match shape {
        "advertisement" => {
            let statement: Statement<Advertisement> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            provider_role::validate_statement(&statement).map_err(|v| format!("{v:?}"))
        }
        "operation" => {
            let statement: Statement<Operation> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            statement
                .check_signed_by_operator()
                .map_err(|_| "NotSignedByOperator".to_owned())
        }
        "result" => {
            let statement: Statement<ResultBody> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            statement
                .check_context(&statement.deployment, &statement.protocol_version)
                .map_err(|e| match e {
                    ContextMismatch::Deployment { .. } => "Deployment".to_owned(),
                    ContextMismatch::ProtocolVersion { .. } => "ProtocolVersion".to_owned(),
                    ContextMismatch::NestedMaker { .. } => "NestedMaker".to_owned(),
                })
        }
        "successive-advertisements" => {
            let bundle: SuccessiveAdvertisements =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            provider_role::validate_statement(&bundle.previous)
                .map_err(|v| format!("previous: {v:?}"))?;
            provider_role::validate_statement(&bundle.next).map_err(|v| format!("next: {v:?}"))?;
            provider_role::validate_successor(&bundle.previous, &bundle.next)
                .map_err(|v| format!("{v:?}"))
        }
        "answered-operation" => {
            let bundle: AnsweredOperation =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            bundle.check().map(|_| ()).map_err(|v| format!("{v:?}"))
        }
        _ => Ok(()),
    }
}

/// Schema-rejected fixtures that the Rust types still deserialise, each with the reason only the schema can
/// reject it. Every other schema-rejected fixture must fail to deserialise.
fn rust_accepts() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "statement-envelope-signature-in-a-non-canonical-encoding",
            "pattern: the signature value is a string in Rust",
        ),
        (
            "advertisement-without-offers",
            "minProperties: every offer is optional in Rust",
        ),
        (
            "advertisement-band-amount-with-a-fraction",
            "pattern: amounts are strings in Rust",
        ),
        (
            "advertisement-offer-without-tuples",
            "minItems: tuples are a Vec",
        ),
        (
            "advertisement-private-node-declared-twice",
            "uniqueItems: private nodes are a Vec",
        ),
        (
            "advertisement-provider-without-identifier-or-status-query",
            "minItems: tracking is a Vec",
        ),
        (
            "advertisement-provider-without-safe-retry-declaration",
            "if/then: safe_retry is optional for verifiers",
        ),
        (
            "advertisement-reserve-between-two-inner-nodes",
            "oneOf: either end of a reserve tuple is a NodeRef",
        ),
        (
            "advertisement-reserve-between-two-outer-nodes",
            "oneOf: either end of a reserve tuple is a NodeRef",
        ),
        (
            "advertisement-tuple-without-amount-bands",
            "minItems: bands are a Vec",
        ),
        (
            "advertisement-value-moving-provider-without-evidence-declaration",
            "if/then: evidence is optional for verifiers",
        ),
        (
            "advertisement-verifier-without-classes",
            "minItems: classes are a Vec",
        ),
        (
            "advertisement-verifier-without-identifier-or-status-query",
            "minItems: tracking is a Vec",
        ),
        (
            "advertisement-without-signing-keys",
            "minItems: keys are a Vec",
        ),
        (
            "advertisement-zero-fill-increment",
            "pattern: amounts are strings in Rust",
        ),
        (
            "statement-envelope-made-at-not-a-utc-time",
            "pattern: times are strings in Rust",
        ),
        (
            "statement-envelope-naming-no-shape",
            "pattern: shape is a string in Rust",
        ),
        (
            "statement-envelope-protocol-version-not-major-minor-patch",
            "pattern: the protocol version is a string in Rust",
        ),
        (
            "statement-envelope-signature-with-padding",
            "pattern: the signature value is a string in Rust",
        ),
        (
            "operation-withdrawal-dry-run-without-a-session",
            "if/then: a withdrawal's session is optional off a dry run",
        ),
        (
            "operation-with-a-fractional-amount",
            "pattern: amounts are strings in Rust",
        ),
        (
            "operation-with-an-input-of-a-bad-name",
            "propertyNames: inputs are a map of strings",
        ),
        (
            "result-success-with-no-evidence-listed",
            "minItems: evidence is a Vec",
        ),
        (
            "operation-reversal-of-a-zero-amount",
            "pattern: amounts are strings in Rust",
        ),
        (
            "result-reversal-succeeded-with-nothing-back",
            "pattern: amounts are strings in Rust",
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

/// A validator for `urn:rutent:provider-role:<shape>`, with every schema of the contract registered by its
/// `$id` and nothing fetched from elsewhere.
fn validator(shape: &str) -> Validator {
    validator_of(&format!("urn:rutent:provider-role:{shape}"))
}

/// A validator for the schema or definition at `reference`, with every schema of the contract registered.
fn validator_of(reference: &str) -> Validator {
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
    let root = serde_json::json!({ "$ref": reference });
    jsonschema::options()
        .offline()
        .with_registry(&registry)
        .build(&root)
        .unwrap_or_else(|e| panic!("{reference} schema builds: {e}"))
}

/// The schema errors of a fixture of `shape`, as sorted (instance location, keyword) pairs. A bundle's members
/// are each checked against their own schema or definition, with locations under the member's name, and a
/// missing or extra member is an error at the root.
fn shape_errors(shape: &str, instance: &Value) -> Vec<(String, String)> {
    let bundle: &[(&str, &str)] = match shape {
        "successive-advertisements" => &SUCCESSIVE_ADVERTISEMENTS_MEMBERS,
        "answered-operation" => &ANSWERED_OPERATION_MEMBERS,
        _ => return schema_errors(&validator(shape), instance),
    };
    let members = instance.as_object().cloned().unwrap_or_default();
    let mut errors = Vec::new();
    if members.len() != bundle.len() {
        errors.push((String::new(), "members".to_owned()));
    }
    for (member, reference) in bundle {
        match members.get(*member) {
            Some(value) => errors.extend(
                schema_errors(&validator_of(reference), value)
                    .into_iter()
                    .map(|(at, keyword)| (format!("/{member}{at}"), keyword)),
            ),
            None => errors.push((String::new(), "members".to_owned())),
        }
    }
    errors.sort();
    errors
}

fn shape_of(name: &str) -> &'static str {
    SHAPES
        .iter()
        .filter(|s| name.starts_with(&format!("{s}-")))
        .max_by_key(|s| s.len())
        .unwrap_or_else(|| panic!("fixture {name} names no shape"))
}

fn round_trip<T: Serialize + DeserializeOwned>(value: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

/// A statement deserialised, its shape checked as a reader would, and serialised back.
fn checked_round_trip<T>(value: &Value) -> Result<Value, String>
where
    T: Serialize + DeserializeOwned + StatementShape,
{
    let typed: Statement<T> = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    typed.check_shape().map_err(|e| format!("{e:?}"))?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

/// The fixture deserialised into its shape's Rust type and serialised back.
fn through_rust(shape: &str, value: &Value) -> Result<Value, String> {
    match shape {
        "advertisement" => checked_round_trip::<Advertisement>(value),
        "statement-envelope" => round_trip::<Statement<Value>>(value),
        "operation" => checked_round_trip::<Operation>(value),
        "result" => checked_round_trip::<ResultBody>(value),
        "successive-advertisements" => {
            let bundle: SuccessiveAdvertisements =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            bundle
                .previous
                .check_shape()
                .map_err(|e| format!("{e:?}"))?;
            bundle.next.check_shape().map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(&bundle).map_err(|e| e.to_string())
        }
        "answered-operation" => {
            let bundle: AnsweredOperation =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            bundle
                .operation
                .check_shape()
                .map_err(|e| format!("{e:?}"))?;
            bundle.result.check_shape().map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(&bundle).map_err(|e| e.to_string())
        }
        other => Err(format!("no Rust type for {other}")),
    }
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

/// Asserts the schemas reject `name` with exactly the errors its entry in `schema_rule_fixtures` names, and
/// that the Rust types reject it too unless `rust_accepts` lists it.
fn assert_schema_rejects(name: &str) {
    let table = schema_rule_fixtures();
    let (_, expected) = table
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is not a schema-rule fixture"));
    let mut expected: Vec<(String, String)> = expected
        .iter()
        .map(|(at, keyword)| ((*at).to_owned(), (*keyword).to_owned()))
        .collect();
    expected.sort();
    let shape = shape_of(name);
    let instance = fixture("invalid", name);
    let errors = shape_errors(shape, &instance);
    assert!(
        !errors.is_empty(),
        "invalid fixture {name} was accepted by the schemas"
    );
    assert_eq!(errors, expected, "schema errors of invalid fixture {name}");

    let allowed = rust_accepts().iter().any(|(n, _)| *n == name);
    match through_rust(shape, &instance) {
        Ok(_) => assert!(
            allowed,
            "invalid fixture {name} was accepted by the Rust types"
        ),
        Err(_) => assert!(
            !allowed,
            "invalid fixture {name} is listed in rust_accepts but the Rust types reject it"
        ),
    }
}

/// Asserts `name` passes the schemas, deserialises, and breaks exactly the Rust rule `expected`.
fn assert_rule_rejects(name: &str, expected: &str) {
    let shape = shape_of(name);
    let instance = fixture("invalid", name);
    let errors = shape_errors(shape, &instance);
    assert!(
        errors.is_empty(),
        "fixture {name} should pass the schemas, got {errors:?}"
    );
    through_rust(shape, &instance)
        .unwrap_or_else(|e| panic!("fixture {name} should deserialise: {e}"));
    match rust_rules(shape, &instance) {
        Ok(()) => panic!("invalid fixture {name} was accepted by the Rust rules"),
        Err(found) => assert_eq!(found, expected, "Rust rule broken by {name}"),
    }
}

// verifies: REQ-CAT-13
#[test]
fn exchange_spanning_two_nodes_is_invalid() {
    assert_schema_rejects("advertisement-exchange-spanning-two-nodes");
}

// verifies: REQ-CAT-13
#[test]
fn ramp_within_one_node_is_invalid() {
    assert_rule_rejects(
        "advertisement-ramp-within-one-node",
        &format!("{:?}", vec![RuleViolation::RampWithinOneNode { tuple: 0 }]),
    );
}

// verifies: REQ-CAT-13, DEC-16
#[test]
fn verifier_advertising_a_tuple_is_invalid() {
    assert_schema_rejects("advertisement-verifier-advertising-a-tuple");
}

// verifies: REQ-CAT-16
#[test]
fn provider_without_an_identifier_or_status_query_is_invalid() {
    assert_schema_rejects("advertisement-provider-without-identifier-or-status-query");
    assert_schema_rejects("advertisement-verifier-without-identifier-or-status-query");
}

// verifies: INV-9
#[test]
fn balance_answer_carrying_anything_but_yes_or_no_is_invalid() {
    assert_schema_rejects("result-balance-answer-carrying-an-amount");
    assert_schema_rejects("result-balance-answer-neither-yes-nor-no");
}

// verifies: REQ-CAT-18
#[test]
fn refusal_without_its_class_is_invalid() {
    assert_schema_rejects("result-refusal-without-its-class");
}

// verifies: REQ-ORD-1
#[test]
fn outcome_unknown_is_distinct_from_success_and_failure() {
    let unknown = fixture("valid", "result-transfer-outcome-unknown");
    assert_eq!(
        schema_errors(&validator("result"), &unknown),
        vec![],
        "outcome unknown is a valid result"
    );
    let statement: Statement<ResultBody> =
        serde_json::from_value(unknown).expect("outcome unknown deserialises");
    match statement.statement {
        ResultBody::Transfer(result) => assert!(
            matches!(result.outcome, TransferOutcome::Unknown(_)),
            "outcome unknown read as {:?}",
            result.outcome
        ),
        other => panic!("expected a transfer result, got {other:?}"),
    }
    // It carries nothing a success carries, and success and failure each need stated evidence.
    assert_schema_rejects("result-outcome-unknown-carrying-amounts");
    assert_schema_rejects("result-success-without-evidence");
    assert_schema_rejects("result-failure-without-evidence");
}

// verifies: REQ-PRT-1
#[test]
fn statement_without_its_deployment_and_protocol_version_is_invalid() {
    assert_schema_rejects("statement-envelope-without-deployment");
    assert_schema_rejects("statement-envelope-without-protocol-version");
    // Every kind of statement carries the envelope, a judgement nested in a result included.
    for (shape, name, envelope_at) in [
        (
            "advertisement",
            "advertisement-provider-under-several-categories",
            "",
        ),
        ("operation", "operation-transfer-between-two-nodes", ""),
        ("result", "result-transfer-failed", ""),
        (
            "result",
            "result-judgement-yes",
            "/statement/outcome/judgement",
        ),
    ] {
        for field in ["deployment", "protocol_version"] {
            let mut instance = fixture("valid", name);
            instance
                .pointer_mut(envelope_at)
                .and_then(Value::as_object_mut)
                .expect("an envelope")
                .remove(field);
            assert_eq!(
                shape_errors(shape, &instance),
                vec![(envelope_at.to_owned(), "required".to_owned())],
                "{name} without {field}"
            );
            assert!(
                through_rust(shape, &instance).is_err(),
                "{name} without {field} deserialises"
            );
        }
    }
    // A reader refuses a statement replayed from another deployment or protocol version.
    let statement: Statement<ResultBody> =
        serde_json::from_value(fixture("valid", "result-transfer-failed")).expect("deserialises");
    let here = statement.deployment.clone();
    let version = statement.protocol_version.clone();
    assert_eq!(statement.check_context(&here, &version), Ok(()));
    let elsewhere = Deployment {
        name: "rutent-other".into(),
        ..here.clone()
    };
    assert!(statement.check_context(&elsewhere, &version).is_err());
    let other_operator = Deployment {
        operator_key: provider_role::PublicKey::new("A".repeat(43)).expect("a canonical key"),
        ..here.clone()
    };
    assert!(statement.check_context(&other_operator, &version).is_err());
    assert!(statement.check_context(&here, "1.0.0").is_err());
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn a_statements_key_never_signs_an_advertisement_nor_grants_an_advertising_key() {
    for (name, rule) in rust_rule_fixtures() {
        if name.starts_with("successive-advertisements-")
            || name == "advertisement-signed-with-its-own-statements-key"
            || name == "advertisement-listing-one-key-for-both-purposes"
        {
            assert_rule_rejects(name, &rule);
        }
    }
    assert_schema_rejects("advertisement-key-without-a-purpose");
    // A new advertising key signed in by a key the accepted advertisement lists for advertising is accepted;
    // retiring the old key is a further advertisement, signed by either.
    let rotation = fixture(
        "valid",
        "successive-advertisements-adding-an-advertising-key-signed-by-the-previous-one",
    );
    assert_eq!(rust_rules("successive-advertisements", &rotation), Ok(()));
}

// verifies: THR-8
#[test]
fn a_non_canonical_alias_of_a_key_is_refused_in_both_layers() {
    assert_schema_rejects("operation-signed-with-a-non-canonical-alias-of-the-operator-key");
    let alias = provider_role::PublicKey::new("AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyB");
    assert!(
        alias.is_none(),
        "an alias with nonzero unused bits is no key"
    );
    let key = provider_role::PublicKey::new("AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA")
        .expect("a canonical key");
    assert_eq!(key.bytes()[..4], [1, 2, 3, 4]);
    assert_eq!(key.bytes()[31], 32);
}

/// The `answered-operation` fixtures of `kind`, asserting there is at least one, so a test over them can't pass
/// on nothing.
fn answered_operations(kind: &str) -> Vec<(String, Value)> {
    let found: Vec<(String, Value)> = fixtures(kind)
        .into_iter()
        .filter(|(name, _)| name.starts_with("answered-operation-"))
        .collect();
    assert!(!found.is_empty(), "no {kind} answered-operation fixtures");
    found
}

/// What `check_amounts` gives for a valid `answered-operation` fixture.
fn amounts_of(name: &str) -> Result<Option<Remainder>, Vec<AmountMismatch>> {
    let bundle: AnsweredOperation =
        serde_json::from_value(fixture("valid", name)).expect("the bundle deserialises");
    bundle.check()
}

/// Asserts every invalid `answered-operation` fixture whose rule is one of `rules` breaks exactly that rule, and
/// that there is at least one for each.
fn assert_amounts_reject(rules: &[AmountMismatch]) {
    let table = rust_rule_fixtures();
    let invalid = answered_operations("invalid");
    for rule in rules {
        let expected = format!("{:?}", vec![rule.clone()]);
        let names: Vec<&str> = table
            .iter()
            .filter(|(_, r)| *r == expected)
            .map(|(n, _)| *n)
            .collect();
        assert!(!names.is_empty(), "no invalid fixture breaks {rule:?}");
        for name in names {
            assert!(
                invalid.iter().any(|(n, _)| n == name),
                "missing invalid fixture {name}"
            );
            assert_rule_rejects(name, &expected);
        }
    }
}

// verifies: INV-1, REQ-CAT-11, THR-108, REQ-ORD-1
#[test]
fn a_result_takes_in_at_most_its_stated_amount_and_names_what_stays_at_the_origin() {
    for (name, _) in answered_operations("valid") {
        assert!(amounts_of(&name).is_ok(), "{name}: {:?}", amounts_of(&name));
    }
    // An exchange that fills in increments of 100000 leaves 50000 of the 25050000 asked unconverted, at its node.
    assert_eq!(
        amounts_of("answered-operation-conversion-filling-in-increments"),
        Ok(Some(Remainder {
            node: NodeRef::Inner("acme-ledger".into()),
            asset: "COP".into(),
            amount: "50000".into(),
        }))
    );
    // A withdrawal that takes in what it was asked leaves nothing at its outer node.
    assert_eq!(
        amounts_of("answered-operation-withdrawal-taking-in-the-stated-amount"),
        Ok(Some(Remainder {
            node: NodeRef::Outer(OuterNode {
                network: "co-bank-transfer".into(),
            }),
            asset: "COP".into(),
            amount: "0".into(),
        }))
    );
    // An `exact_out` operation states no amount in, so no remainder follows from it alone; nor from a quote.
    assert_eq!(
        amounts_of("answered-operation-transfer-fixing-what-it-delivers"),
        Ok(None)
    );
    assert_eq!(
        amounts_of("answered-operation-conversion-quote-charging-in-its-from-asset"),
        Ok(None)
    );
    assert_amounts_reject(&[
        AmountMismatch::AnswersAnotherOperation,
        AmountMismatch::TookInMoreThanStated,
        AmountMismatch::UndeclaredRemainder,
        AmountMismatch::RemainderOfAWholeIncrement,
    ]);
}

// verifies: REQ-QTE-5, REQ-CAT-9
#[test]
fn a_fee_is_charged_in_the_asset_the_segment_takes_in_and_within_it() {
    assert_amounts_reject(&[
        AmountMismatch::FeeInAnotherAsset,
        AmountMismatch::FeeAboveAmountIn,
    ]);
    // A quote is held to the same rule as the execution it prices.
    assert!(
        rust_rule_fixtures()
            .iter()
            .any(|(n, _)| *n == "answered-operation-quoting-its-fee-in-another-asset")
    );
}

// verifies: REQ-CAT-6, REQ-CAT-8, REQ-CAT-9, REQ-CAT-10, REQ-CAT-11, REQ-CAT-12, REQ-CAT-13, REQ-CAT-14, REQ-CAT-16, REQ-CAT-18, REQ-CAT-20, REQ-CAT-22, REQ-ORD-1, REQ-ORD-39, REQ-PRT-1, INV-9
#[test]
fn every_valid_fixture_validates_and_every_invalid_one_is_rejected() {
    let valid = fixtures("valid");
    assert!(!valid.is_empty(), "no valid fixtures");
    for (name, instance) in &valid {
        let shape = shape_of(name);
        let errors = shape_errors(shape, instance);
        assert!(
            errors.is_empty(),
            "valid fixture {name} was rejected by the schemas: {errors:?}"
        );
        let round_trip = through_rust(shape, instance)
            .unwrap_or_else(|e| panic!("valid fixture {name} doesn't deserialise: {e}"));
        assert_eq!(
            &round_trip, instance,
            "valid fixture {name} changes through the Rust types"
        );
        assert_eq!(
            rust_rules(shape, instance),
            Ok(()),
            "valid fixture {name} broke a Rust rule"
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
    for (name, _) in rust_accepts() {
        assert!(
            schema_rules.iter().any(|(n, _)| *n == name),
            "rust_accepts lists {name}, which is not a schema-rule fixture"
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
