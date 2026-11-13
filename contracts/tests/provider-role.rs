//! The provider-role contract's tests (`T-002-01`, `T-002-02`, `T-002-03`): its schemas against its fixtures, its Rust types
//! against its schemas, and its Rust validation.

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::provider_role::{
    self, Advertisement, AmountMismatch, AnswerMismatch, ArrivalAnswer, ArrivalNotice,
    ArrivalOutcome, ArrivalQuestion, Category, ContextMismatch, CreditRestated, Credits,
    Deployment, Hold, NodeRef, Operation, OuterNode, RedemptionRefused, Remainder, ResultBody,
    RuleViolation, Statement, StatementShape, Terms, TermsViolation, TransferOutcome, TupleTerms,
};
use rutent_contracts::provider_role::{InnerSegments, InnerSegmentsViolation, Standing};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Every schema of the contract, registered by `$id` as `urn:rutent:provider-role:<name>`.
const SCHEMAS: [&str; 15] = [
    "common",
    "declarations",
    "reserve-declaration",
    "advertisement",
    "statement-envelope",
    "operation",
    "result",
    "judgement",
    "reversal-confirmation",
    "terms",
    "hold",
    "arrival-notice",
    "arrival-question",
    "arrival-answer",
    "inner-segments",
];

/// The shapes fixtures are written in. A fixture's shape is the longest of these its file name starts with,
/// followed by `-`. `successive-advertisements`, `answered-operation`, `arrival-notices` and
/// `answered-arrival-question` are bundles of values, not schemas of their own: see [`SuccessiveAdvertisements`],
/// [`AnsweredOperation`], [`ArrivalNotices`] and [`AnsweredArrivalQuestion`].
const SHAPES: [&str; 15] = [
    "advertisement",
    "statement-envelope",
    "operation",
    "result",
    "terms",
    "hold",
    "arrival-notice",
    "arrival-question",
    "arrival-answer",
    "inner-segments",
    "successive-advertisements",
    "answered-operation",
    "arrival-notices",
    "answered-arrival-question",
    "published-inner-segments",
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

/// The members of an `arrival-notices` bundle: two of a reserve's enveloped arrival notices, for the rule that
/// relates them (`check_one_credit`).
const ARRIVAL_NOTICES_MEMBERS: [(&str, &str); 2] = [
    ("earlier", "urn:rutent:provider-role:arrival-notice"),
    ("later", "urn:rutent:provider-role:arrival-notice"),
];

/// An `arrival-notices` fixture.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArrivalNotices {
    earlier: Statement<ArrivalNotice>,
    later: Statement<ArrivalNotice>,
}

/// The members of an `answered-arrival-question` bundle: the operator's enveloped question of what arrived and the
/// reserve's enveloped answer to it, for the rules that relate them (`check_answer`).
const ANSWERED_ARRIVAL_QUESTION_MEMBERS: [(&str, &str); 2] = [
    ("question", "urn:rutent:provider-role:arrival-question"),
    ("answer", "urn:rutent:provider-role:arrival-answer"),
];

/// The members of a `published-inner-segments` bundle: the advertisement of a record's maker and the enveloped
/// record, for the rules that relate them (`check_inner_segments`).
const PUBLISHED_INNER_SEGMENTS_MEMBERS: [(&str, &str); 2] = [
    ("advertisement", "urn:rutent:provider-role:advertisement"),
    ("record", "urn:rutent:provider-role:inner-segments"),
];

/// A `published-inner-segments` fixture.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedInnerSegments {
    advertisement: Statement<Advertisement>,
    record: Statement<InnerSegments>,
}

/// An `answered-arrival-question` fixture.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnsweredArrivalQuestion {
    question: Statement<ArrivalQuestion>,
    answer: Statement<ArrivalAnswer>,
}

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
            "arrival-notice-without-its-transaction",
            vec![("/statement", "required")],
        ),
        (
            "arrival-notice-without-its-outer-node",
            vec![("/statement", "required")],
        ),
        (
            "arrival-notice-without-its-asset",
            vec![("/statement", "required")],
        ),
        (
            "arrival-notice-without-its-amount",
            vec![("/statement", "required")],
        ),
        (
            "arrival-notice-without-its-account",
            vec![("/statement", "required")],
        ),
        (
            "arrival-notice-without-its-holder",
            vec![("/statement", "required")],
        ),
        (
            "arrival-notice-at-an-inner-node",
            vec![("/statement/node", "type")],
        ),
        (
            "arrival-notice-of-a-zero-amount",
            vec![("/statement/amount", "pattern")],
        ),
        (
            "arrival-notice-asset-without-its-unit",
            vec![("/statement/asset", "required")],
        ),
        (
            "arrival-notice-under-another-shape",
            vec![("/shape", "const")],
        ),
        (
            "arrival-question-without-its-transaction",
            vec![("/statement", "required")],
        ),
        (
            "arrival-question-without-a-session",
            vec![("/statement", "required")],
        ),
        (
            "arrival-question-under-another-shape",
            vec![("/shape", "const")],
        ),
        (
            "arrival-question-at-an-inner-node",
            vec![("/statement/node", "type")],
        ),
        (
            "arrival-answer-without-its-transaction",
            vec![("/statement", "required")],
        ),
        (
            "arrival-answer-arrived-with-no-notices",
            vec![("/statement/outcome/notices", "minItems")],
        ),
        (
            "arrival-answer-that-nothing-arrived-carrying-a-notice",
            vec![("/statement/outcome", "additionalProperties")],
        ),
        (
            "arrival-answer-refusal-saying-when-it-lifts",
            vec![("/statement/outcome/refusal/class", "enum")],
        ),
        (
            "arrival-answer-carrying-another-shape-as-its-notice",
            vec![("/statement/outcome/notices/0/shape", "const")],
        ),
        (
            "arrival-answer-under-another-shape",
            vec![("/shape", "const")],
        ),
        (
            "inner-segments-segment-without-its-amount-in",
            vec![("/statement/segments/0", "required")],
        ),
        (
            "inner-segments-segment-without-its-amount-out",
            vec![("/statement/segments/0", "required")],
        ),
        (
            "inner-segments-segment-without-its-fee",
            vec![("/statement/segments/0", "required")],
        ),
        (
            "inner-segments-segment-without-its-asset-in",
            vec![("/statement/segments/0", "required")],
        ),
        (
            "inner-segments-segment-without-its-transaction",
            vec![("/statement/segments/0", "required")],
        ),
        (
            "inner-segments-fee-with-a-fraction",
            vec![("/statement/segments/0/fee", "pattern")],
        ),
        (
            "inner-segments-without-segments",
            vec![("/statement/segments", "minItems")],
        ),
        (
            "inner-segments-without-its-label",
            vec![("/statement", "required")],
        ),
        (
            "inner-segments-labelled-as-proof",
            vec![("/statement/standing", "const")],
        ),
        (
            "inner-segments-without-its-publisher",
            vec![("/statement", "required")],
        ),
        (
            "inner-segments-under-another-shape",
            vec![("/shape", "const")],
        ),
        (
            "terms-with-a-zero-denominator",
            vec![("/statement/fee/proportional/denominator", "pattern")],
        ),
        ("terms-under-another-shape", vec![("/shape", "const")]),
        (
            "hold-for-no-order-or-session",
            vec![("/statement/for", "oneOf")],
        ),
        (
            "hold-without-its-transaction",
            vec![("/statement", "required")],
        ),
        (
            "hold-of-a-record-without-its-digest",
            vec![("/statement/record", "required")],
        ),
        (
            "result-withdrawal-billing-its-fee-separately-as-text",
            vec![("/statement/outcome/fee/billed_separately", "type")],
        ),
        (
            "result-transfer-failed-showing-the-providers-own-fault",
            vec![("/statement/outcome/fault", "enum")],
        ),
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
            "operation-holding-balance-naming-a-session",
            vec![("/statement", "additionalProperties")],
        ),
        (
            "operation-holding-balance-without-a-node",
            vec![("/statement", "required")],
        ),
        (
            "result-holding-balance-with-a-negative-balance",
            vec![("/statement/outcome/balance", "pattern")],
        ),
        (
            "result-holding-balance-with-a-leading-zero",
            vec![("/statement/outcome/balance", "pattern")],
        ),
        (
            "result-holding-balance-without-a-balance",
            vec![("/statement/outcome", "required")],
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
    let terms = |v: TermsViolation| format!("{:?}", vec![v]);
    let answer = |v: AnswerMismatch| format!("{:?}", vec![v]);
    let inner = |v: InnerSegmentsViolation| format!("{:?}", vec![v]);
    vec![
        (
            "arrival-notices-of-one-credit-with-another-amount",
            format!("{:?}", CreditRestated::OtherFigures),
        ),
        (
            "published-inner-segments-against-another-providers-advertisement",
            inner(InnerSegmentsViolation::NotItsMakersAdvertisement),
        ),
        (
            "published-inner-segments-for-another-publisher",
            inner(InnerSegmentsViolation::OfAnotherPublisher),
        ),
        (
            "published-inner-segments-at-a-node-its-maker-does-not-declare",
            inner(InnerSegmentsViolation::NodeNotDeclared),
        ),
        (
            "arrival-question-signed-with-another-key",
            "NotSignedByOperator".to_owned(),
        ),
        (
            "arrival-answer-carrying-another-reserves-notice",
            "NestedMaker".to_owned(),
        ),
        (
            "answered-arrival-question-under-another-transaction",
            answer(AnswerMismatch::AnswersAnotherQuestion),
        ),
        (
            "answered-arrival-question-by-another-reserve",
            answer(AnswerMismatch::OfAnotherReserve),
        ),
        (
            "answered-arrival-question-at-another-node",
            answer(AnswerMismatch::AtAnotherNode { notice: 0 }),
        ),
        (
            "answered-arrival-question-in-another-asset",
            answer(AnswerMismatch::InAnotherAsset { notice: 0 }),
        ),
        (
            "answered-arrival-question-in-another-unit",
            answer(AnswerMismatch::InAnotherAsset { notice: 0 }),
        ),
        (
            "answered-arrival-question-about-another-account",
            answer(AnswerMismatch::ToAnotherAccount { notice: 0 }),
        ),
        (
            "terms-made-by-another-party",
            terms(TermsViolation::OfAnotherMaker),
        ),
        (
            "terms-of-an-exchange-without-a-slippage-regime",
            terms(TermsViolation::NoSlippageWhereARate),
        ),
        (
            "terms-of-a-reserve-with-a-slippage-regime",
            terms(TermsViolation::SlippageWhereNoRate),
        ),
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
        "terms" => {
            let statement: Statement<TupleTerms> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            provider_role::check_terms(&statement).map_err(|v| format!("{v:?}"))
        }
        "arrival-notices" => {
            let bundle: ArrivalNotices =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            provider_role::check_one_credit(&bundle.earlier, &bundle.later)
                .map(|_| ())
                .map_err(|v| format!("{v:?}"))
        }
        "arrival-question" => {
            let statement: Statement<ArrivalQuestion> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            statement
                .check_signed_by_operator()
                .map_err(|_| "NotSignedByOperator".to_owned())
        }
        "published-inner-segments" => {
            let bundle: PublishedInnerSegments =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            provider_role::validate_statement(&bundle.advertisement)
                .map_err(|v| format!("advertisement: {v:?}"))?;
            provider_role::check_inner_segments(&bundle.record, &bundle.advertisement)
                .map_err(|v| format!("{v:?}"))
        }
        "arrival-answer" => {
            let statement: Statement<ArrivalAnswer> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            statement
                .check_context(&statement.deployment, &statement.protocol_version)
                .map_err(|e| match e {
                    ContextMismatch::Deployment { .. } => "Deployment".to_owned(),
                    ContextMismatch::ProtocolVersion { .. } => "ProtocolVersion".to_owned(),
                    ContextMismatch::NestedMaker { .. } => "NestedMaker".to_owned(),
                })
        }
        "answered-arrival-question" => {
            let bundle: AnsweredArrivalQuestion =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            bundle
                .question
                .check_signed_by_operator()
                .map_err(|_| "NotSignedByOperator".to_owned())?;
            bundle
                .answer
                .check_context(&bundle.answer.deployment, &bundle.answer.protocol_version)
                .map_err(|e| format!("{e:?}"))?;
            provider_role::check_answer(&bundle.question, &bundle.answer)
                .map_err(|v| format!("{v:?}"))
        }
        _ => Ok(()),
    }
}

/// Schema-rejected fixtures that the Rust types still deserialise, each with the reason only the schema can
/// reject it. Every other schema-rejected fixture must fail to deserialise.
fn rust_accepts() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "arrival-notice-of-a-zero-amount",
            "pattern: amounts are strings in Rust",
        ),
        (
            "arrival-answer-arrived-with-no-notices",
            "minItems: notices are a Vec",
        ),
        (
            "inner-segments-fee-with-a-fraction",
            "pattern: amounts are strings in Rust",
        ),
        (
            "inner-segments-without-segments",
            "minItems: segments are a Vec",
        ),
        (
            "terms-with-a-zero-denominator",
            "pattern: amounts are strings in Rust",
        ),
        (
            "statement-envelope-signature-in-a-non-canonical-encoding",
            "pattern: the signature value is a string in Rust",
        ),
        (
            "result-holding-balance-with-a-negative-balance",
            "pattern: amounts are strings in Rust",
        ),
        (
            "result-holding-balance-with-a-leading-zero",
            "pattern: amounts are strings in Rust",
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
        "arrival-notices" => &ARRIVAL_NOTICES_MEMBERS,
        "answered-arrival-question" => &ANSWERED_ARRIVAL_QUESTION_MEMBERS,
        "published-inner-segments" => &PUBLISHED_INNER_SEGMENTS_MEMBERS,
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
        "terms" => checked_round_trip::<TupleTerms>(value),
        "hold" => checked_round_trip::<Hold>(value),
        "arrival-notice" => checked_round_trip::<ArrivalNotice>(value),
        "arrival-question" => checked_round_trip::<ArrivalQuestion>(value),
        "arrival-answer" => checked_round_trip::<ArrivalAnswer>(value),
        "inner-segments" => checked_round_trip::<InnerSegments>(value),
        "published-inner-segments" => {
            let bundle: PublishedInnerSegments =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            bundle
                .advertisement
                .check_shape()
                .map_err(|e| format!("{e:?}"))?;
            bundle.record.check_shape().map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(&bundle).map_err(|e| e.to_string())
        }
        "answered-arrival-question" => {
            let bundle: AnsweredArrivalQuestion =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            bundle
                .question
                .check_shape()
                .map_err(|e| format!("{e:?}"))?;
            bundle.answer.check_shape().map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(&bundle).map_err(|e| e.to_string())
        }
        "arrival-notices" => {
            let bundle: ArrivalNotices =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            bundle.earlier.check_shape().map_err(|e| format!("{e:?}"))?;
            bundle.later.check_shape().map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(&bundle).map_err(|e| e.to_string())
        }
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

// verifies: REQ-AUD-15, INV-1
#[test]
fn holding_balance_is_the_operators_question_and_a_whole_figure() {
    for name in [
        "operation-holding-balance-naming-a-session",
        "operation-holding-balance-without-a-node",
        "result-holding-balance-with-a-negative-balance",
        "result-holding-balance-with-a-leading-zero",
        "result-holding-balance-without-a-balance",
    ] {
        assert_schema_rejects(name);
    }
}

// verifies: REQ-AUD-15
#[test]
fn holding_balance_statements_read_into_their_rust_types() {
    use rutent_contracts::provider_role::{
        HoldingBalanceOutcome, Operation, ResultBody, Statement,
    };
    for (name, balance) in [
        ("result-holding-balance-answered", Some("150")),
        (
            "result-holding-balance-answered-with-nothing-held",
            Some("0"),
        ),
        ("result-holding-balance-refused-finally", None),
    ] {
        let value = fixture("valid", name);
        let result: Statement<ResultBody> = serde_json::from_value(value).expect("a result");
        let ResultBody::HoldingBalance(r) = result.statement else {
            panic!("{name} isn't a holding balance result");
        };
        match (r.outcome, balance) {
            (HoldingBalanceOutcome::Answered(a), Some(b)) => assert_eq!(a.balance, b),
            (HoldingBalanceOutcome::Refused(_), None) => {}
            _ => panic!("{name}"),
        }
    }
    let value = fixture("valid", "operation-holding-balance");
    let operation: Statement<Operation> = serde_json::from_value(value).expect("an operation");
    assert!(matches!(operation.statement, Operation::HoldingBalance(_)));
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

/// A provider's terms are its own, with a slippage regime exactly where its tuple sets a rate; a reserve's hold is
/// redeemed only for the order it names, or an order accepted in the session it names (round 3 of `T-020-01`).
// verifies: REQ-QTE-5, REQ-QTE-6, REQ-QTE-15
#[test]
fn terms_are_their_providers_own_and_a_hold_is_redeemed_only_for_its_order() {
    let terms: Statement<TupleTerms> = serde_json::from_value(fixture(
        "valid",
        "terms-of-an-exchange-with-its-slippage-regime",
    ))
    .unwrap();
    assert!(terms.statement.tuple.sets_a_rate());
    assert_eq!(provider_role::check_terms(&terms), Ok(()));
    let mut other = terms.clone();
    other.maker = "us-bank".to_owned();
    other.statement.slippage = None;
    assert_eq!(
        provider_role::check_terms(&other),
        Err(vec![
            TermsViolation::OfAnotherMaker,
            TermsViolation::NoSlippageWhereARate
        ])
    );
    let for_order: Statement<Hold> =
        serde_json::from_value(fixture("valid", "hold-for-an-order")).unwrap();
    assert_eq!(
        provider_role::check_redemption(&for_order, "ord-1", None),
        Ok(())
    );
    assert_eq!(
        provider_role::check_redemption(&for_order, "ord-2", Some("sess-1")),
        Err(RedemptionRefused::ForAnotherOrder)
    );
    let for_session: Statement<Hold> =
        serde_json::from_value(fixture("valid", "hold-for-a-session")).unwrap();
    assert_eq!(
        provider_role::check_redemption(&for_session, "ord-2", Some("sess-1")),
        Ok(())
    );
    assert_eq!(
        provider_role::check_redemption(&for_session, "ord-2", Some("sess-2")),
        Err(RedemptionRefused::ForAnotherSession)
    );
    assert_eq!(
        provider_role::check_redemption(&for_session, "ord-2", None),
        Err(RedemptionRefused::ForAnotherSession)
    );
}

/// A reserve's notice of what arrived names the transaction it gives the credit, its per-credit reference: without
/// it two notices of one credit, signed at different times, couldn't be known as one, nor the deposit keyed on it
/// (`T-020-18`).
// verifies: REQ-ORD-23
#[test]
fn arrival_notice_without_its_per_credit_reference_is_invalid() {
    assert_schema_rejects("arrival-notice-without-its-transaction");
    let notice: Statement<ArrivalNotice> =
        serde_json::from_value(fixture("valid", "arrival-notice-of-a-credit")).unwrap();
    assert_eq!(notice.statement.transaction, "tx-credit-0001");
}

/// A reserve's notice names what arrived: its outer node, the asset with its unit, the amount and the account,
/// with the account's holder, so Core reads the figures from the signed bytes (`T-020-18`).
// verifies: REQ-ORD-14
#[test]
fn arrival_notice_without_its_outer_node_asset_amount_or_account_is_invalid() {
    for name in [
        "arrival-notice-without-its-outer-node",
        "arrival-notice-without-its-asset",
        "arrival-notice-without-its-amount",
        "arrival-notice-without-its-account",
        "arrival-notice-without-its-holder",
        "arrival-notice-at-an-inner-node",
        "arrival-notice-asset-without-its-unit",
        "arrival-notice-of-a-zero-amount",
    ] {
        assert_schema_rejects(name);
    }
    let notice: Statement<ArrivalNotice> =
        serde_json::from_value(fixture("valid", "arrival-notice-of-a-credit")).unwrap();
    assert_eq!(notice.check_shape(), Ok(()));
    assert_eq!(notice.statement.node.network, "co-ach");
    assert_eq!(
        (
            notice.statement.asset.id.as_str(),
            notice.statement.asset.decimals
        ),
        ("COP", 2)
    );
    assert_eq!(notice.statement.amount, "150000");
    assert_eq!(notice.statement.account, "acct-77");
    assert_eq!(notice.statement.holder, "party-ana");
}

/// Two notices of one credit, signed at different times, are one credit; the same transaction at another reserve is
/// another credit, and the same transaction with other figures is refused (`T-020-18`).
// verifies: REQ-ORD-23
#[test]
fn two_notices_of_one_credit_signed_at_different_times_are_one_credit() {
    let one: ArrivalNotices = serde_json::from_value(fixture(
        "valid",
        "arrival-notices-of-one-credit-signed-at-different-times",
    ))
    .unwrap();
    assert_ne!(one.earlier.made_at, one.later.made_at);
    assert_eq!(
        provider_role::check_one_credit(&one.earlier, &one.later),
        Ok(Credits::One)
    );
    let two: ArrivalNotices = serde_json::from_value(fixture(
        "valid",
        "arrival-notices-of-one-reference-at-two-reserves",
    ))
    .unwrap();
    assert_eq!(
        provider_role::check_one_credit(&two.earlier, &two.later),
        Ok(Credits::Two)
    );
    let mut another = one.later.clone();
    another.statement.transaction = "tx-credit-0002".to_owned();
    another.statement.amount = "1".to_owned();
    assert_eq!(
        provider_role::check_one_credit(&one.earlier, &another),
        Ok(Credits::Two)
    );
    for change in [
        |n: &mut ArrivalNotice| n.node.network = "co-pse".to_owned(),
        |n: &mut ArrivalNotice| n.asset.decimals = 0,
        |n: &mut ArrivalNotice| n.account = "acct-78".to_owned(),
        |n: &mut ArrivalNotice| n.holder = "party-bo".to_owned(),
    ] {
        let mut restated = one.later.clone();
        change(&mut restated.statement);
        assert_eq!(
            provider_role::check_one_credit(&one.earlier, &restated),
            Err(CreditRestated::OtherFigures)
        );
    }
}

/// The operator's question of what arrived names its own transaction, fresh for the order it is asked for, so the
/// reserve's answer names it and counts only as that transaction's (`T-020-18`).
// verifies: REQ-ORD-1
#[test]
fn question_of_what_arrived_without_the_orders_transaction_is_invalid() {
    assert_schema_rejects("arrival-question-without-its-transaction");
    assert_schema_rejects("arrival-answer-without-its-transaction");
    let question: Statement<ArrivalQuestion> = serde_json::from_value(fixture(
        "valid",
        "arrival-question-of-what-arrived-at-an-account",
    ))
    .unwrap();
    assert_eq!(question.check_shape(), Ok(()));
    assert_eq!(question.check_signed_by_operator(), Ok(()));
    assert_eq!(question.statement.transaction, "tx-arrival-1");
}

/// The reserve answers the question of what arrived under the question's transaction, carrying its notice of the
/// credit whole, the same notice it sends of its own accord; an answer under another transaction, another
/// reserve's, or carrying a notice of another node, asset or account doesn't answer it, and a notice another
/// reserve made isn't the answering reserve's to carry (`T-020-18`).
// verifies: REQ-ORD-1, REQ-ORD-23
#[test]
fn what_arrived_is_answered_by_the_notice_under_the_questions_transaction() {
    let answered: AnsweredArrivalQuestion = serde_json::from_value(fixture(
        "valid",
        "answered-arrival-question-by-the-notice-of-its-credit",
    ))
    .unwrap();
    assert_eq!(
        provider_role::check_answer(&answered.question, &answered.answer),
        Ok(())
    );
    let unsolicited: Statement<ArrivalNotice> =
        serde_json::from_value(fixture("valid", "arrival-notice-of-a-credit")).unwrap();
    let ArrivalOutcome::Arrived(arrived) = &answered.answer.statement.outcome else {
        panic!("the answer carries what arrived");
    };
    assert_eq!(arrived.notices, vec![unsolicited.clone()]);
    assert_eq!(
        provider_role::check_one_credit(&unsolicited, &arrived.notices[0]),
        Ok(Credits::One)
    );
    let mut stray = answered.answer.clone();
    stray.maker = "eu-bank".to_owned();
    stray.statement.transaction = "tx-arrival-2".to_owned();
    if let ArrivalOutcome::Arrived(arrived) = &mut stray.statement.outcome {
        arrived.notices[0].statement.account = "acct-78".to_owned();
    }
    assert_eq!(
        provider_role::check_answer(&answered.question, &stray),
        Err(vec![
            AnswerMismatch::AnswersAnotherQuestion,
            AnswerMismatch::OfAnotherReserve,
            AnswerMismatch::ToAnotherAccount { notice: 0 },
        ])
    );
    assert!(matches!(
        stray.check_context(&stray.deployment, &stray.protocol_version),
        Err(ContextMismatch::NestedMaker { .. })
    ));
    // The same asset counted in another unit is another asset: an amount is read in its unit.
    let mut other_unit = answered.answer.clone();
    if let ArrivalOutcome::Arrived(arrived) = &mut other_unit.statement.outcome {
        arrived.notices[0].statement.asset.decimals = 0;
    }
    assert_eq!(
        provider_role::check_answer(&answered.question, &other_unit),
        Err(vec![AnswerMismatch::InAnotherAsset { notice: 0 }])
    );
    let nothing: AnsweredArrivalQuestion = serde_json::from_value(fixture(
        "valid",
        "answered-arrival-question-that-nothing-arrived",
    ))
    .unwrap();
    assert_eq!(
        provider_role::check_answer(&nothing.question, &nothing.answer),
        Ok(())
    );
}

/// A publisher's record of the segments inside its private node gives, for each, the asset and amount in and out
/// and the fee, which the replay compares with the public movement out (`T-020-18`).
// verifies: REQ-ORD-41
#[test]
fn inner_segment_record_without_its_amounts_in_and_out_or_its_fee_is_invalid() {
    for name in [
        "inner-segments-segment-without-its-amount-in",
        "inner-segments-segment-without-its-amount-out",
        "inner-segments-segment-without-its-fee",
        "inner-segments-segment-without-its-asset-in",
        "inner-segments-fee-with-a-fraction",
    ] {
        assert_schema_rejects(name);
    }
    let record: Statement<InnerSegments> = serde_json::from_value(fixture(
        "valid",
        "inner-segments-of-a-conversion-and-a-transfer",
    ))
    .unwrap();
    let first = &record.statement.segments[0];
    assert_eq!(
        (
            first.asset_in.as_str(),
            first.amount_in.as_str(),
            first.asset_out.as_str(),
            first.amount_out.as_str(),
            first.fee.as_str()
        ),
        ("COP", "150000", "USD", "3600", "1500")
    );
    // A record that doesn't add up is still the publisher's record: the contract refuses no figure, so the replay
    // sees it and records the mismatch.
    let off: Statement<InnerSegments> =
        serde_json::from_value(fixture("valid", "inner-segments-that-do-not-add-up")).unwrap();
    let published: PublishedInnerSegments = serde_json::from_value(fixture(
        "valid",
        "published-inner-segments-of-a-node-its-maker-declares",
    ))
    .unwrap();
    assert_eq!(
        provider_role::check_inner_segments(&off, &published.advertisement),
        Ok(())
    );
}

/// A publisher's record of inner segments carries its label, the publisher's word, and is made by a provider of
/// that publisher's which declares the node, stating for it, since a publisher holds no key: inside a private node
/// there is no other witness (`T-020-18`, `AC-35`).
// verifies: REQ-ORD-3
#[test]
fn publishers_record_of_inner_segments_is_labelled_the_publishers_word() {
    let record: Statement<InnerSegments> = serde_json::from_value(fixture(
        "valid",
        "inner-segments-of-a-conversion-and-a-transfer",
    ))
    .unwrap();
    assert_eq!(record.statement.standing, Standing::PublishersWord);
    assert_eq!(record.check_shape(), Ok(()));
    let published: PublishedInnerSegments = serde_json::from_value(fixture(
        "valid",
        "published-inner-segments-of-a-node-its-maker-declares",
    ))
    .unwrap();
    assert_eq!(published.record, record);
    assert_ne!(record.maker, record.statement.publisher);
    assert_eq!(
        provider_role::check_inner_segments(&record, &published.advertisement),
        Ok(())
    );
    assert_eq!(
        serde_json::to_value(&record).unwrap()["statement"]["standing"],
        "publishers_word"
    );
    assert_schema_rejects("inner-segments-without-its-label");
    assert_schema_rejects("inner-segments-labelled-as-proof");
    assert_schema_rejects("inner-segments-without-its-publisher");
    for (name, rule) in [
        (
            "published-inner-segments-against-another-providers-advertisement",
            InnerSegmentsViolation::NotItsMakersAdvertisement,
        ),
        (
            "published-inner-segments-for-another-publisher",
            InnerSegmentsViolation::OfAnotherPublisher,
        ),
        (
            "published-inner-segments-at-a-node-its-maker-does-not-declare",
            InnerSegmentsViolation::NodeNotDeclared,
        ),
    ] {
        assert_rule_rejects(name, &format!("{:?}", vec![rule]));
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

/// RFC 8785 §3.2.2's sample, as the RFC writes it, parsed and then canonicalised as §3.2.3 shows.
const RFC_8785_SAMPLE: &str = r#"{
       "numbers": [333333333.33333329, 1E30, 4.50,
                   2e-3, 0.000000000000000000000000001],
       "string": "\u20ac$\u000F\u000aA'\u0042\u0022\u005c\\\"\/",
       "literals": [null, true, false]
     }"#;

/// RFC 8785 §3.2.3's canonical form of the sample, with its display line wraps removed.
const RFC_8785_SAMPLE_CANONICAL: &str = "{\"literals\":[null,true,false],\"numbers\":[333333333.3333333,\
1e+30,4.5,0.002,1e-27],\"string\":\"€$\\u000f\\nA'B\\\"\\\\\\\\\\\"/\"}";

/// RFC 8785 §3.2.3's sorting test data.
const RFC_8785_SORTING: &str = r#"{
       "\u20ac": "Euro Sign",
       "\r": "Carriage Return",
       "\ufb33": "Hebrew Letter Dalet With Dagesh",
       "1": "One",
       "\ud83d\ude00": "Emoji: Grinning Face",
       "\u0080": "Control",
       "\u00f6": "Latin Small Letter O With Diaeresis"
     }"#;

/// RFC 8785 Appendix B, Table 1: each finite double, by its IEEE 754 bits, and its JSON representation.
const RFC_8785_NUMBERS: [(u64, &str); 24] = [
    (0x0000000000000000, "0"),
    (0x8000000000000000, "0"),
    (0x0000000000000001, "5e-324"),
    (0x8000000000000001, "-5e-324"),
    (0x7fefffffffffffff, "1.7976931348623157e+308"),
    (0xffefffffffffffff, "-1.7976931348623157e+308"),
    (0x4340000000000000, "9007199254740992"),
    (0xc340000000000000, "-9007199254740992"),
    (0x4430000000000000, "295147905179352830000"),
    (0x44b52d02c7e14af5, "9.999999999999997e+22"),
    (0x44b52d02c7e14af6, "1e+23"),
    (0x44b52d02c7e14af7, "1.0000000000000001e+23"),
    (0x444b1ae4d6e2ef4e, "999999999999999700000"),
    (0x444b1ae4d6e2ef4f, "999999999999999900000"),
    (0x444b1ae4d6e2ef50, "1e+21"),
    (0x3eb0c6f7a0b5ed8c, "9.999999999999997e-7"),
    (0x3eb0c6f7a0b5ed8d, "0.000001"),
    (0x41b3de4355555553, "333333333.3333332"),
    (0x41b3de4355555554, "333333333.33333325"),
    (0x41b3de4355555555, "333333333.3333333"),
    (0x41b3de4355555556, "333333333.3333334"),
    (0x41b3de4355555557, "333333333.33333343"),
    (0xbecbf647612f3696, "-0.0000033333333333333333"),
    (0x43143ff3c1cb0959, "1424953923781206.2"),
];

fn canonical_text(json: &str) -> String {
    String::from_utf8(provider_role::canonicalize(json.as_bytes()).expect("canonical"))
        .expect("UTF-8")
}

// verifies: REQ-STM-4, INV-8, REQ-PRT-1
#[test]
fn canonical_encoding_matches_the_rfc_8785_test_vectors() {
    // §3.2.2 and §3.2.3: primitives and the members sorted.
    assert_eq!(canonical_text(RFC_8785_SAMPLE), RFC_8785_SAMPLE_CANONICAL);

    // §3.2.3: members sorted by their names' UTF-16 code units, in the order the RFC expects.
    let sorted = canonical_text(RFC_8785_SORTING);
    let values: Vec<&str> = sorted
        .split('"')
        .filter(|part| part.contains(' ') || *part == "One" || *part == "Control")
        .collect();
    assert_eq!(
        values,
        [
            "Carriage Return",
            "One",
            "Control",
            "Latin Small Letter O With Diaeresis",
            "Euro Sign",
            "Emoji: Grinning Face",
            "Hebrew Letter Dalet With Dagesh",
        ]
    );
    assert_eq!(
        sorted,
        "{\"\\r\":\"Carriage Return\",\"1\":\"One\",\"\u{80}\":\"Control\",\
         \"\u{f6}\":\"Latin Small Letter O With Diaeresis\",\"\u{20ac}\":\"Euro Sign\",\
         \"\u{1f600}\":\"Emoji: Grinning Face\",\"\u{fb33}\":\"Hebrew Letter Dalet With Dagesh\"}"
    );

    // Appendix B: every finite sample, written in any exact form and read back as its double.
    for (bits, expected) in RFC_8785_NUMBERS {
        let value = f64::from_bits(bits);
        for written in [
            format!("{value:e}"),
            format!("{value:.780e}"),
            format!("[{value:?}]"),
        ] {
            let canonical = canonical_text(&written);
            let canonical = canonical.trim_start_matches('[').trim_end_matches(']');
            assert_eq!(canonical, expected, "{bits:016x} written as {written:.40}");
        }
        assert_eq!(
            provider_role::to_canonical(&value).map(|b| String::from_utf8(b).expect("UTF-8")),
            Ok(expected.to_owned()),
            "{bits:016x} as a value"
        );
    }
    // Appendix B's NaN and Infinity rows: no JSON form, so an error (§3.2.2.3), never `null`.
    for bits in [
        0x7fffffffffffffff_u64,
        0x7ff0000000000000,
        0xfff0000000000000,
    ] {
        let value = f64::from_bits(bits);
        assert_eq!(
            provider_role::to_canonical(&value),
            Err(provider_role::CanonicalError::NonFiniteNumber),
            "{bits:016x}"
        );
        assert_eq!(
            provider_role::to_canonical(&[(String::from("x"), vec![value])]),
            Err(provider_role::CanonicalError::NonFiniteNumber),
            "{bits:016x} nested in a value"
        );
    }
}

// verifies: REQ-PRT-1
#[test]
fn text_with_no_canonical_form_is_refused() {
    use provider_role::CanonicalError;
    let refused = |text: &str| provider_role::canonicalize(text.as_bytes()).unwrap_err();
    assert_eq!(
        refused(r#"{"a":1,"a":2}"#),
        CanonicalError::DuplicateMember { at: 7 }
    );
    assert_eq!(
        refused(r#"{"a":{"b":1,"\u0062":2}}"#),
        CanonicalError::DuplicateMember { at: 12 }
    );
    assert_eq!(
        refused(r#"["\udead"]"#),
        CanonicalError::LoneSurrogate { at: 2 }
    );
    assert_eq!(
        refused(r#"["\ud83d x"]"#),
        CanonicalError::LoneSurrogate { at: 2 }
    );
    assert_eq!(
        refused("[1e400]"),
        CanonicalError::NumberOutOfRange { at: 1 }
    );
    // RFC 7493 §2.1: no noncharacter, written or escaped.
    assert_eq!(
        refused("[\"a\u{ffff}\"]"),
        CanonicalError::Noncharacter { at: 3 }
    );
    assert_eq!(
        refused(r#"["ab\ufdd0"]"#),
        CanonicalError::Noncharacter { at: 4 }
    );
    assert_eq!(
        refused(r#"{"\ud83f\udffe":1}"#),
        CanonicalError::Noncharacter { at: 2 }
    );
    assert!(provider_role::canonicalize("[\"\u{fffd}\u{fdcf}\u{fdf0}\"]".as_bytes()).is_ok());
    assert_eq!(refused("[01]"), CanonicalError::Syntax { at: 2 });
    assert_eq!(refused("[1,]"), CanonicalError::Syntax { at: 3 });
    assert_eq!(refused("{} {}"), CanonicalError::Syntax { at: 3 });
    assert_eq!(refused("[\"a\tb\"]"), CanonicalError::Syntax { at: 3 });
    assert_eq!(
        provider_role::canonicalize(b"[\"\xff\"]").unwrap_err(),
        CanonicalError::NotUtf8
    );
    let deep = format!("{}{}", "[".repeat(129), "]".repeat(129));
    assert_eq!(refused(&deep), CanonicalError::TooDeep { at: 128 });
    let deepest = format!("{}{}", "[".repeat(128), "]".repeat(128));
    assert!(provider_role::canonicalize(deepest.as_bytes()).is_ok());
}

// verifies: REQ-PRT-1, REQ-STM-4
#[test]
fn a_value_and_its_text_have_one_canonical_form() {
    let statement: Statement<Advertisement> = serde_json::from_value(fixture(
        "valid",
        "advertisement-provider-under-several-categories",
    ))
    .expect("an advertisement");
    let from_value = provider_role::to_canonical(&statement).expect("canonical");
    let pretty = serde_json::to_vec_pretty(&statement).expect("json");
    assert_eq!(provider_role::canonicalize(&pretty), Ok(from_value.clone()));
    assert_eq!(provider_role::canonicalize(&from_value), Ok(from_value));
}
