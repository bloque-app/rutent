//! The module-interface contract's tests (`T-003-01`, `T-002-03`): its schemas against its fixtures, its Rust types against
//! its schemas, and its Rust rules.

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::module_interface::{
    self, Address, AddressBlock, AdmissionArtefact, AdmissionRefusal, CallOutcome,
    CallRefusalReason, CrossModuleCall, CrossModuleOutcome, Declaration, DeclaredList, Nothing,
    OutboundCallRecord, OutsideRequest, OutsideResponse, RefusalReason, ReleaseAssertion,
    RuleViolation, SecretAnswer, SecretLookup, SecretOutcome, SecretRefusalReason, Widening,
};
use rutent_contracts::provider_role::{self, Advertisement, Deployment, ResultBody, Statement};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Every schema the contract's shapes reach, by contract and name; each is registered by its `$id`,
/// `urn:rutent:<contract>:<name>`. The provider-role schemas are referred to, never copied.
const SCHEMAS: [(&str, &str); 19] = [
    ("module-interface", "common"),
    ("module-interface", "declaration"),
    ("module-interface", "outside-request"),
    ("module-interface", "outside-response"),
    ("module-interface", "secret-lookup"),
    ("module-interface", "secret-answer"),
    ("module-interface", "outbound-call-record"),
    ("module-interface", "cross-module-call"),
    ("module-interface", "release-assertion"),
    ("module-interface", "admission-artefact"),
    ("provider-role", "common"),
    ("provider-role", "declarations"),
    ("provider-role", "reserve-declaration"),
    ("provider-role", "advertisement"),
    ("provider-role", "statement-envelope"),
    ("provider-role", "operation"),
    ("provider-role", "judgement"),
    ("provider-role", "result"),
    ("provider-role", "reversal-confirmation"),
];

/// The shapes fixtures are written in. A fixture's shape is the longest of these its file name starts with,
/// followed by `-`. `signed-result`, `declared-secret-lookup` and `declared-outbound-call-record` are bundles of
/// values, not schemas of their own: see [`SignedResult`], [`DeclaredSecretLookup`] and
/// [`DeclaredOutboundCallRecord`].
const SHAPES: [&str; 16] = [
    "declaration",
    "outside-request",
    "outside-response",
    "secret-lookup",
    "secret-answer",
    "outbound-call-record",
    "cross-module-call",
    "signed-result",
    "declared-secret-lookup",
    "declared-outbound-call-record",
    "declared-cross-module-call",
    "release-assertion",
    "admission-artefact",
    "advertised-artefact",
    "operator-artefact",
    "successive-releases",
];

/// The members of an `advertised-artefact` bundle: an admission artefact and its provider's enveloped
/// advertisement, for the release-assertion key rules (`validate_assertion_key`).
const ADVERTISED_ARTEFACT_MEMBERS: [(&str, &str); 2] = [
    ("advertisement", "urn:rutent:provider-role:advertisement"),
    ("artefact", "urn:rutent:module-interface:admission-artefact"),
];

/// The members of an `operator-artefact` bundle: the deployment the platform serves, from its settings, and the
/// admission artefact of a module the operator publishes, for the operator's release rules
/// (`validate_operator_assertion`).
const OPERATOR_ARTEFACT_MEMBERS: [(&str, &str); 2] = [
    (
        "deployment",
        "urn:rutent:provider-role:statement-envelope#/properties/deployment",
    ),
    ("artefact", "urn:rutent:module-interface:admission-artefact"),
];

/// The members of a `successive-releases` bundle: an admitted release and the release that follows it, for the
/// rules that relate them (`validate_release`).
const SUCCESSIVE_RELEASES_MEMBERS: [(&str, &str); 2] = [
    ("previous", "urn:rutent:module-interface:admission-artefact"),
    ("next", "urn:rutent:module-interface:admission-artefact"),
];

/// An `advertised-artefact` fixture.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvertisedArtefact {
    advertisement: Statement<Advertisement>,
    artefact: AdmissionArtefact,
}

/// An `operator-artefact` fixture.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperatorArtefact {
    deployment: Deployment,
    artefact: AdmissionArtefact,
}

/// A `successive-releases` fixture.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuccessiveReleases {
    previous: AdmissionArtefact,
    next: AdmissionArtefact,
}

/// The members of a `signed-result` bundle, each with the `$id` of the schema it follows.
const SIGNED_RESULT_MEMBERS: [(&str, &str); 3] = [
    ("declaration", "urn:rutent:module-interface:declaration"),
    ("advertisement", "urn:rutent:provider-role:advertisement"),
    ("result", "urn:rutent:provider-role:result"),
];

/// The members of a `declared-secret-lookup` bundle, each with the `$id` of the schema it follows.
const DECLARED_SECRET_LOOKUP_MEMBERS: [(&str, &str); 2] = [
    ("declaration", "urn:rutent:module-interface:declaration"),
    ("lookup", "urn:rutent:module-interface:secret-lookup"),
];

/// A `declared-secret-lookup` fixture: a module's declaration and a secret lookup its outside part makes, for the
/// rule that relates the two (`validate_lookup`).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredSecretLookup {
    declaration: Declaration,
    lookup: SecretLookup,
}

/// The members of a `declared-cross-module-call` bundle, each with the `$id` of the schema it follows.
const DECLARED_CROSS_MODULE_CALL_MEMBERS: [(&str, &str); 2] = [
    ("declaration", "urn:rutent:module-interface:declaration"),
    ("call", "urn:rutent:module-interface:cross-module-call"),
];

/// A `declared-cross-module-call` fixture: a caller's declaration and a cross-module call it made, for the rules
/// that relate the call's outcome to what the caller declares (`validate_call_against`).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredCrossModuleCall {
    declaration: Declaration,
    call: CrossModuleCall,
}

/// The members of a `declared-outbound-call-record` bundle, each with the `$id` of the schema it follows.
const DECLARED_OUTBOUND_CALL_RECORD_MEMBERS: [(&str, &str); 2] = [
    ("declaration", "urn:rutent:module-interface:declaration"),
    ("record", "urn:rutent:module-interface:outbound-call-record"),
];

/// A `declared-outbound-call-record` fixture: a module's declaration and an outbound call record of it, for the
/// rule that relates the two (`validate_record_secrets`).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredOutboundCallRecord {
    declaration: Declaration,
    record: OutboundCallRecord,
}

/// A `signed-result` fixture: a module's declaration, its provider's enveloped advertisement and a result the
/// module signed, for the statement-signing key rule, which relates the three (`validate_statement_key`).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedResult {
    declaration: Declaration,
    advertisement: Statement<Advertisement>,
    result: Statement<ResultBody>,
}

/// Invalid fixtures the schemas must reject, each with the exact (instance location, keyword) errors of the one
/// rule it breaks.
fn schema_rule_fixtures() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            "advertised-artefact-asserted-with-a-non-canonical-alias-of-the-statement-key",
            vec![("/artefact/assertion/signature/key", "pattern")],
        ),
        (
            "declaration-sandboxed-part-declaring-a-secret-lookup",
            vec![("/sandboxed", "additionalProperties")],
        ),
        (
            "declaration-sandboxed-part-declaring-a-destination",
            vec![("/sandboxed", "additionalProperties")],
        ),
        (
            "declaration-sandboxed-part-handling-an-unknown-kind",
            vec![("/sandboxed/handles/0", "enum")],
        ),
        (
            "declaration-sandboxed-part-handling-nothing",
            vec![("/sandboxed/handles", "minItems")],
        ),
        (
            "declaration-destination-with-a-wildcard-host",
            vec![("/outside/destinations/0/host", "pattern")],
        ),
        (
            "declaration-destination-at-an-address-literal",
            vec![("/outside/destinations/0/host", "pattern")],
        ),
        (
            "declaration-destination-over-plain-http",
            vec![("/outside/destinations/0/scheme", "enum")],
        ),
        (
            "declaration-destination-without-addresses",
            vec![("/outside/destinations/0", "required")],
        ),
        (
            "declaration-destination-pinned-to-a-host-name",
            vec![("/outside/destinations/0/addresses", "oneOf")],
        ),
        (
            "declaration-secret-declared-twice",
            vec![("/outside/secrets", "uniqueItems")],
        ),
        (
            "declaration-operation-reaching-no-destination",
            vec![("/outside/operations/0/destinations", "minItems")],
        ),
        (
            "declaration-call-without-kinds",
            vec![("/outside/calls/0/kinds", "minItems")],
        ),
        (
            "declaration-call-carrying-credentials",
            vec![("/outside/calls/0", "additionalProperties")],
        ),
        ("declaration-without-calls", vec![("/outside", "required")]),
        (
            "declaration-without-its-statement-key",
            vec![("/outside", "required")],
        ),
        (
            "declaration-without-its-interface-version",
            vec![("", "required")],
        ),
        (
            "outside-request-naming-a-secret",
            vec![("", "additionalProperties")],
        ),
        (
            "outside-request-naming-a-destination",
            vec![("", "additionalProperties")],
        ),
        (
            "outside-request-without-a-transaction",
            vec![("", "required")],
        ),
        (
            "outside-response-refused-without-a-reason",
            vec![("/outcome", "required")],
        ),
        (
            "outside-response-unknown-carrying-a-body",
            vec![("/outcome", "additionalProperties")],
        ),
        (
            "secret-lookup-naming-a-provider",
            vec![("", "additionalProperties")],
        ),
        (
            "secret-lookup-without-a-transaction",
            vec![("", "required")],
        ),
        ("secret-lookup-without-an-operation", vec![("", "required")]),
        (
            "secret-answer-resolved-without-a-value",
            vec![("/outcome", "required")],
        ),
        (
            "secret-answer-resolved-to-an-empty-value",
            vec![("/outcome/value", "minLength")],
        ),
        (
            "outbound-call-record-without-a-transaction",
            vec![("", "required")],
        ),
        (
            "outbound-call-record-without-a-module-version",
            vec![("", "required")],
        ),
        (
            "outbound-call-record-without-hops",
            vec![("/hops", "minItems")],
        ),
        (
            "outbound-call-record-carrying-a-secret-value",
            vec![("/secrets/0", "type")],
        ),
        (
            "outbound-call-record-connected-to-a-host-name-as-its-address",
            vec![("/hops/0/address", "anyOf")],
        ),
        (
            "outbound-call-record-connected-to-a-malformed-ipv6-address",
            vec![("/hops/0/address", "anyOf")],
        ),
        (
            "outbound-call-record-hop-with-a-null-address",
            vec![("/hops/0/address", "type")],
        ),
        (
            "outbound-call-record-hop-with-a-null-destination",
            vec![("/hops/0/destination", "type")],
        ),
        (
            "declaration-statement-key-malformed",
            vec![("/outside/statement_key", "pattern")],
        ),
        (
            "outbound-call-record-refused-without-a-reason",
            vec![("/outcome", "required")],
        ),
        (
            "outbound-call-record-completed-carrying-a-reason",
            vec![("/outcome", "additionalProperties")],
        ),
        ("cross-module-call-without-a-route", vec![("", "required")]),
        (
            "cross-module-call-without-a-transaction",
            vec![("", "required")],
        ),
        (
            "cross-module-call-carrying-credentials",
            vec![("", "additionalProperties")],
        ),
        (
            "cross-module-call-route-without-its-segment",
            vec![("/route", "required")],
        ),
        (
            "cross-module-call-without-its-outcome",
            vec![("", "required")],
        ),
        (
            "cross-module-call-refused-with-a-null-reason",
            vec![("/outcome/reason", "enum")],
        ),
        (
            "cross-module-call-refused-for-an-unknown-reason",
            vec![("/outcome/reason", "enum")],
        ),
        (
            "cross-module-call-refused-without-its-reason",
            vec![("/outcome", "required")],
        ),
        (
            "outbound-call-record-refused-for-an-unknown-reason",
            vec![("/outcome/reason", "enum")],
        ),
        (
            "secret-answer-refused-for-an-unknown-reason",
            vec![("/outcome/reason", "enum")],
        ),
        (
            "cross-module-call-unanswered-carrying-a-reason",
            vec![("/outcome", "additionalProperties")],
        ),
        (
            "admission-artefact-without-a-signed-assertion",
            vec![("", "required")],
        ),
        (
            "admission-artefact-with-an-unsigned-assertion",
            vec![("/assertion", "required")],
        ),
        (
            "admission-artefact-without-its-platform-build-version",
            vec![("/build", "required")],
        ),
        (
            "admission-artefact-without-its-history",
            vec![("", "required")],
        ),
        (
            "admission-artefact-without-its-machine-code",
            vec![("/build", "required")],
        ),
        (
            "admission-artefact-widening-but-accepted-automatically",
            vec![("/acceptance/required", "const")],
        ),
        (
            "admission-artefact-of-a-first-release-accepted-automatically",
            vec![("/acceptance/required", "const")],
        ),
        (
            "admission-artefact-of-a-first-release-listing-widenings",
            vec![("/capabilities/widenings", "maxItems")],
        ),
        (
            "admission-artefact-freshly-accepted-by-no-one",
            vec![("/acceptance/accepted", "required")],
        ),
        (
            "admission-artefact-with-a-malformed-digest",
            vec![("/received/outside/value", "pattern")],
        ),
        (
            "admission-artefact-with-a-widening-of-an-unknown-kind",
            vec![("/capabilities/widenings/0/kind", "enum")],
        ),
        (
            "release-assertion-under-another-shape",
            vec![("/shape", "const")],
        ),
        (
            "admission-artefact-of-a-first-release-comparing-with-a-version",
            vec![("/capabilities", "not")],
        ),
        (
            "admission-artefact-with-a-history-comparing-with-nothing",
            vec![("/capabilities", "required")],
        ),
    ]
}

/// Invalid fixtures that pass the schemas and must be rejected by a Rust rule, with the rule each breaks, as
/// `rust_rules` names it.
fn rust_rule_fixtures() -> Vec<(&'static str, String)> {
    let one = |v: RuleViolation| format!("{:?}", vec![v]);
    vec![
        (
            "declaration-operation-naming-an-undeclared-destination",
            one(RuleViolation::UndeclaredDestination {
                operation: 0,
                destination: "bank_files".to_owned(),
            }),
        ),
        (
            "declaration-operation-using-an-undeclared-secret",
            one(RuleViolation::UndeclaredSecret {
                operation: 1,
                secret: "bridge.webhook_secret".to_owned(),
            }),
        ),
        (
            "declaration-destination-declared-twice",
            one(RuleViolation::DeclaredTwice {
                list: DeclaredList::Destinations,
                index: 1,
                first: 0,
            }),
        ),
        (
            "declaration-operation-declared-twice",
            one(RuleViolation::DeclaredTwice {
                list: DeclaredList::Operations,
                index: 1,
                first: 0,
            }),
        ),
        (
            "declaration-call-to-its-own-provider",
            one(RuleViolation::DeclaredCallToOwnProvider { call: 0 }),
        ),
        (
            "declaration-call-of-holding-balance",
            one(RuleViolation::DeclaredCallOfHoldingBalance { call: 0 }),
        ),
        (
            "declaration-call-declared-twice",
            one(RuleViolation::DeclaredTwice {
                list: DeclaredList::Calls,
                index: 1,
                first: 0,
            }),
        ),
        (
            "declaration-destination-pinned-to-special-purpose-blocks",
            format!(
                "{:?}",
                (0..4)
                    .map(|block| RuleViolation::SpecialPurposeAddressBlock {
                        destination: 0,
                        block,
                    })
                    .collect::<Vec<_>>()
            ),
        ),
        (
            "declared-secret-lookup-of-a-secret-its-operation-does-not-declare",
            one(RuleViolation::SecretUndeclaredForOperation),
        ),
        (
            "declared-secret-lookup-for-an-undeclared-operation",
            one(RuleViolation::LookupForUndeclaredOperation),
        ),
        (
            "declared-outbound-call-record-using-a-secret-its-operation-does-not-declare",
            one(RuleViolation::RecordedSecretUndeclaredForOperation { secret: 2 }),
        ),
        (
            "declared-outbound-call-record-of-another-module-version",
            one(RuleViolation::RecordOfAnotherModule),
        ),
        (
            "outbound-call-record-completed-through-an-unconnected-hop",
            one(RuleViolation::HopNotConnected { hop: 1 }),
        ),
        (
            "outbound-call-record-unknown-through-an-unconnected-hop-before-the-last",
            one(RuleViolation::HopNotConnected { hop: 0 }),
        ),
        (
            "outbound-call-record-unknown-ending-at-a-hop-that-matched-no-destination",
            one(RuleViolation::HopNotConnected { hop: 0 }),
        ),
        (
            "cross-module-call-to-its-own-provider",
            one(RuleViolation::CallToOwnProvider),
        ),
        (
            "cross-module-call-for-another-transaction",
            one(RuleViolation::TransactionMismatch),
        ),
        (
            "cross-module-call-refusal-naming-another-callee",
            one(RuleViolation::RefusalNamesAnotherCall),
        ),
        (
            "declared-cross-module-call-delivered-to-an-undeclared-callee",
            one(RuleViolation::UndeclaredCallDelivered),
        ),
        (
            "declared-cross-module-call-refused-as-undeclared-though-declared",
            one(RuleViolation::RefusalReasonUntrue),
        ),
        (
            "declared-cross-module-call-of-another-module-version",
            one(RuleViolation::CallOfAnotherModule),
        ),
        (
            "declared-cross-module-call-delivered-of-an-undeclared-kind",
            one(RuleViolation::UndeclaredCallDelivered),
        ),
        (
            "declared-cross-module-call-refused-as-an-undeclared-kind-to-an-undeclared-callee",
            one(RuleViolation::RefusalReasonUntrue),
        ),
        (
            "declared-cross-module-call-unanswered-of-an-undeclared-kind",
            one(RuleViolation::UndeclaredCallDelivered),
        ),
        (
            "declared-cross-module-call-refused-as-a-limit-to-an-undeclared-callee",
            one(RuleViolation::RefusalReasonUntrue),
        ),
        (
            "signed-result-signed-with-another-advertised-key",
            one(RuleViolation::ResultNotSignedWithStatementKey),
        ),
        (
            "signed-result-statement-key-not-advertised",
            one(RuleViolation::StatementKeyNotAdvertised),
        ),
        (
            "signed-result-statement-key-is-the-operator-key",
            one(RuleViolation::StatementKeyIsOperatorKey),
        ),
        (
            "signed-result-statement-key-listed-for-advertising",
            one(RuleViolation::StatementKeyNotForStatements),
        ),
        (
            "advertised-artefact-asserted-with-an-advertised-statements-key",
            one(RuleViolation::AssertionKeyNotForAdvertising),
        ),
        (
            "declaration-destination-pinned-to-the-6to4-relay-and-srv6-blocks",
            format!(
                "{:?}",
                (0..2)
                    .map(|block| RuleViolation::SpecialPurposeAddressBlock {
                        destination: 0,
                        block,
                    })
                    .collect::<Vec<_>>()
            ),
        ),
        (
            "successive-releases-handling-a-new-kind-with-no-widening-recorded",
            one(RuleViolation::WideningsMisrecorded),
        ),
        (
            "signed-result-checked-against-another-providers-advertisement",
            one(RuleViolation::AdvertisementOfAnotherProvider),
        ),
        (
            "signed-result-from-another-deployment",
            one(RuleViolation::ResultOfAnotherDeployment),
        ),
        (
            "signed-result-made-by-another-provider",
            one(RuleViolation::ResultOfAnotherMaker),
        ),
        (
            "admission-artefact-of-an-altered-outside-part",
            one(RuleViolation::PartAltered {
                part: module_interface::PartName::Outside,
            }),
        ),
        (
            "admission-artefact-whose-history-names-its-own-version",
            one(RuleViolation::VersionAlreadyAdmitted { entry: 0 }),
        ),
        (
            "advertised-artefact-asserted-by-another-maker",
            one(RuleViolation::AssertionOfAnotherMaker),
        ),
        (
            "advertised-artefact-asserted-with-an-unadvertised-key",
            one(RuleViolation::AssertionKeyNotAdvertised),
        ),
        (
            "advertised-artefact-asserted-with-the-operator-key",
            one(RuleViolation::AssertionKeyIsOperatorKey),
        ),
        (
            "advertised-artefact-asserted-with-the-statement-key",
            // Both rules catch it: the statement key is listed for statements, and it is the declared one.
            format!(
                "{:?}",
                vec![
                    RuleViolation::AssertionKeyNotForAdvertising,
                    RuleViolation::AssertionSignedWithStatementKey,
                ]
            ),
        ),
        (
            "advertised-artefact-declaring-an-advertising-key-as-its-statement-key",
            one(RuleViolation::StatementKeyNotForStatements),
        ),
        (
            "advertised-artefact-declaring-an-unadvertised-statement-key",
            one(RuleViolation::StatementKeyNotAdvertised),
        ),
        (
            "advertised-artefact-declaring-the-operator-key-as-its-statement-key",
            one(RuleViolation::StatementKeyIsOperatorKey),
        ),
        (
            "advertised-artefact-checked-against-another-providers-advertisement",
            one(RuleViolation::AdvertisementOfAnotherProvider),
        ),
        (
            "advertised-artefact-from-another-deployment",
            one(RuleViolation::AssertionOfAnotherDeployment),
        ),
        (
            "operator-artefact-asserted-with-a-providers-key",
            one(RuleViolation::OperatorAssertionNotSignedWithOperatorKey),
        ),
        (
            "operator-artefact-from-another-deployment",
            one(RuleViolation::AssertionOfAnotherDeployment),
        ),
        (
            "operator-artefact-declaring-the-operator-key-as-its-statement-key",
            one(RuleViolation::StatementKeyIsOperatorKey),
        ),
        (
            "admission-artefact-comparing-with-a-version-its-history-lacks",
            one(RuleViolation::ComparedWithNotLastAdmitted),
        ),
        (
            "successive-releases-widening-with-no-widening-recorded",
            one(RuleViolation::WideningsMisrecorded),
        ),
        (
            "successive-releases-of-another-providers-module",
            one(RuleViolation::ReleaseOfAnotherModule),
        ),
        (
            "successive-releases-asserted-with-the-previous-statement-key",
            one(RuleViolation::AssertionSignedWithPreviousStatementKey),
        ),
        (
            "successive-releases-of-another-deployment",
            one(RuleViolation::ReleaseOfAnotherDeployment),
        ),
        (
            "successive-releases-under-another-protocol-version",
            one(RuleViolation::ReleaseUnderAnotherProtocolVersion),
        ),
        (
            "admission-artefact-whose-history-lists-an-earlier-version-twice",
            one(RuleViolation::VersionAlreadyAdmitted { entry: 1 }),
        ),
        (
            "successive-releases-with-the-history-not-extended",
            one(RuleViolation::HistoryNotExtended),
        ),
        (
            "successive-releases-after-a-release-not-yet-accepted",
            one(RuleViolation::PreviousNotAdmitted),
        ),
    ]
}

/// The Rust rules of a shape, run on a fixture that deserialises. The error is named as `rust_rule_fixtures`
/// names it.
fn rust_rules(shape: &str, value: &Value) -> Result<(), String> {
    fn run<T: DeserializeOwned>(
        value: &Value,
        rule: impl Fn(&T) -> Result<(), Vec<RuleViolation>>,
    ) -> Result<(), String> {
        let typed: T = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        rule(&typed).map_err(|v| format!("{v:?}"))
    }
    match shape {
        "declaration" => run(value, module_interface::validate_declaration),
        "outbound-call-record" => run(value, module_interface::validate_record),
        "cross-module-call" => run(value, module_interface::validate_call),
        "signed-result" => {
            let bundle: SignedResult =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            signed_result_members_hold(&bundle)?;
            module_interface::validate_statement_key(
                &bundle.declaration,
                &bundle.advertisement,
                &bundle.result,
            )
            .map_err(|v| format!("{v:?}"))
        }
        "declared-secret-lookup" => {
            let bundle: DeclaredSecretLookup =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            module_interface::validate_declaration(&bundle.declaration)
                .map_err(|v| format!("declaration: {v:?}"))?;
            module_interface::validate_lookup(&bundle.declaration, &bundle.lookup)
                .map_err(|v| format!("{v:?}"))
        }
        "declared-outbound-call-record" => {
            let bundle: DeclaredOutboundCallRecord =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            module_interface::validate_declaration(&bundle.declaration)
                .map_err(|v| format!("declaration: {v:?}"))?;
            module_interface::validate_record(&bundle.record)
                .map_err(|v| format!("record: {v:?}"))?;
            module_interface::validate_record_secrets(&bundle.declaration, &bundle.record)
                .map_err(|v| format!("{v:?}"))
        }
        "declared-cross-module-call" => {
            let bundle: DeclaredCrossModuleCall =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            module_interface::validate_declaration(&bundle.declaration)
                .map_err(|v| format!("declaration: {v:?}"))?;
            module_interface::validate_call(&bundle.call).map_err(|v| format!("call: {v:?}"))?;
            module_interface::validate_call_against(&bundle.declaration, &bundle.call)
                .map_err(|v| format!("{v:?}"))
        }
        "release-assertion" => {
            let statement: Statement<ReleaseAssertion> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            statement
                .check_shape()
                .map_err(|_| "ShapeMismatch".to_owned())
        }
        "admission-artefact" => run(value, module_interface::validate_artefact),
        "advertised-artefact" => {
            let bundle: AdvertisedArtefact =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            provider_role::validate_statement(&bundle.advertisement)
                .map_err(|v| format!("advertisement: {v:?}"))?;
            module_interface::validate_artefact(&bundle.artefact)
                .map_err(|v| format!("artefact: {v:?}"))?;
            module_interface::validate_assertion_key(&bundle.artefact, &bundle.advertisement)
                .map_err(|v| format!("{v:?}"))
        }
        "operator-artefact" => {
            let bundle: OperatorArtefact =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            module_interface::validate_artefact(&bundle.artefact)
                .map_err(|v| format!("artefact: {v:?}"))?;
            module_interface::validate_operator_assertion(&bundle.artefact, &bundle.deployment)
                .map_err(|v| format!("{v:?}"))
        }
        "successive-releases" => {
            let bundle: SuccessiveReleases =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            module_interface::validate_artefact(&bundle.previous)
                .map_err(|v| format!("previous: {v:?}"))?;
            module_interface::validate_artefact(&bundle.next)
                .map_err(|v| format!("next: {v:?}"))?;
            module_interface::validate_release(&bundle.previous, &bundle.next)
                .map_err(|v| format!("{v:?}"))
        }
        _ => Ok(()),
    }
}

/// The rules of a `signed-result` bundle's members, each in its own contract, so a bundle breaks only the
/// statement-signing key rule it is about.
fn signed_result_members_hold(bundle: &SignedResult) -> Result<(), String> {
    module_interface::validate_declaration(&bundle.declaration)
        .map_err(|v| format!("declaration: {v:?}"))?;
    provider_role::validate_statement(&bundle.advertisement)
        .map_err(|v| format!("advertisement: {v:?}"))?;
    // In its own context only: relating the result's deployment to the advertisement's is
    // `validate_statement_key`'s, so a caller can't skip it.
    bundle
        .result
        .check_context(&bundle.result.deployment, &bundle.result.protocol_version)
        .map_err(|e| format!("result: {e:?}"))
}

/// Schema-rejected fixtures that the Rust types still deserialise, each with the reason only the schema can
/// reject it. Every other schema-rejected fixture must fail to deserialise.
fn rust_accepts() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "admission-artefact-widening-but-accepted-automatically",
            "if/then across fields: validate_artefact refuses it too",
        ),
        (
            "admission-artefact-of-a-first-release-accepted-automatically",
            "if/then across fields: validate_artefact refuses it too",
        ),
        (
            "admission-artefact-of-a-first-release-listing-widenings",
            "if/then across fields: validate_artefact refuses it too",
        ),
        (
            "admission-artefact-freshly-accepted-by-no-one",
            "if/then across fields: validate_artefact refuses it too",
        ),
        (
            "release-assertion-under-another-shape",
            "const: check_shape refuses it, as for every provider-role statement",
        ),
        (
            "admission-artefact-of-a-first-release-comparing-with-a-version",
            "if/then across fields: validate_artefact refuses it too",
        ),
        (
            "admission-artefact-with-a-history-comparing-with-nothing",
            "if/then across fields: validate_artefact refuses it too",
        ),
        (
            "declaration-sandboxed-part-handling-nothing",
            "minItems: handles are a Vec",
        ),
        (
            "declaration-destination-with-a-wildcard-host",
            "pattern: a host is non-empty text in Rust",
        ),
        (
            "declaration-destination-at-an-address-literal",
            "pattern: a host is non-empty text in Rust",
        ),
        (
            "declaration-secret-declared-twice",
            "uniqueItems: secrets are a Vec",
        ),
        (
            "declaration-operation-reaching-no-destination",
            "minItems: an operation's destinations are a Vec",
        ),
        (
            "declaration-call-without-kinds",
            "minItems: a call's kinds are a Vec",
        ),
        (
            "outbound-call-record-without-hops",
            "minItems: hops are a Vec",
        ),
    ]
}

/// Schema-rejected fixtures in `rust_accepts` that a Rust rule still refuses once deserialised, with that rule as
/// `rust_rules` names it. Both layers then hold the rule.
fn rust_also_rejects() -> Vec<(&'static str, String)> {
    let one = |v: RuleViolation| format!("{:?}", vec![v]);
    vec![
        (
            "admission-artefact-widening-but-accepted-automatically",
            one(RuleViolation::WidenedReleaseNotFresh),
        ),
        (
            "admission-artefact-of-a-first-release-accepted-automatically",
            one(RuleViolation::FirstReleaseNotFresh),
        ),
        (
            "admission-artefact-of-a-first-release-listing-widenings",
            one(RuleViolation::FirstReleaseWithWidenings),
        ),
        (
            "admission-artefact-freshly-accepted-by-no-one",
            one(RuleViolation::FreshAcceptanceByNoOne),
        ),
        (
            "release-assertion-under-another-shape",
            "ShapeMismatch".to_owned(),
        ),
        (
            "admission-artefact-of-a-first-release-comparing-with-a-version",
            one(RuleViolation::ComparedWithNotLastAdmitted),
        ),
        (
            "admission-artefact-with-a-history-comparing-with-nothing",
            one(RuleViolation::ComparedWithNotLastAdmitted),
        ),
    ]
}

fn contracts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn contract_dir() -> PathBuf {
    contracts_dir().join("module-interface")
}

fn read_json(path: &Path) -> Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

/// A validator for the schema `id`, with every schema it reaches registered by its `$id` and nothing fetched
/// from elsewhere.
fn validator(id: &str) -> Validator {
    let schemas: Vec<(String, Value)> = SCHEMAS
        .iter()
        .map(|(contract, name)| {
            let schema = read_json(
                &contracts_dir()
                    .join(contract)
                    .join("schemas")
                    .join(format!("{name}.schema.json")),
            );
            let id = schema["$id"]
                .as_str()
                .expect("schema has an $id")
                .to_owned();
            assert_eq!(id, format!("urn:rutent:{contract}:{name}"), "$id of {name}");
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
    let root = serde_json::json!({ "$ref": id });
    jsonschema::options()
        .offline()
        .with_registry(&registry)
        .build(&root)
        .unwrap_or_else(|e| panic!("{id} schema builds: {e}"))
}

/// The schema errors of a fixture of `shape`, as sorted (instance location, keyword) pairs. A bundle's members
/// are checked against their own schemas, with locations under the member's name, and a missing or extra member
/// is an error at the root.
fn shape_errors(shape: &str, instance: &Value) -> Vec<(String, String)> {
    let bundle: &[(&str, &str)] = match shape {
        "signed-result" => &SIGNED_RESULT_MEMBERS,
        "declared-secret-lookup" => &DECLARED_SECRET_LOOKUP_MEMBERS,
        "declared-outbound-call-record" => &DECLARED_OUTBOUND_CALL_RECORD_MEMBERS,
        "declared-cross-module-call" => &DECLARED_CROSS_MODULE_CALL_MEMBERS,
        "advertised-artefact" => &ADVERTISED_ARTEFACT_MEMBERS,
        "operator-artefact" => &OPERATOR_ARTEFACT_MEMBERS,
        "successive-releases" => &SUCCESSIVE_RELEASES_MEMBERS,
        _ => {
            return schema_errors(
                &validator(&format!("urn:rutent:module-interface:{shape}")),
                instance,
            );
        }
    };
    let mut errors = Vec::new();
    let members = instance.as_object().cloned().unwrap_or_default();
    if members.len() != bundle.len() {
        errors.push((String::new(), "members".to_owned()));
    }
    for &(member, id) in bundle {
        match members.get(member) {
            Some(value) => errors.extend(
                schema_errors(&validator(id), value)
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

/// The fixture deserialised into its shape's Rust type and serialised back.
fn through_rust(shape: &str, value: &Value) -> Result<Value, String> {
    match shape {
        "declaration" => round_trip::<Declaration>(value),
        "outside-request" => round_trip::<OutsideRequest>(value),
        "outside-response" => round_trip::<OutsideResponse>(value),
        "secret-lookup" => round_trip::<SecretLookup>(value),
        "secret-answer" => round_trip::<SecretAnswer>(value),
        "outbound-call-record" => round_trip::<OutboundCallRecord>(value),
        "cross-module-call" => round_trip::<CrossModuleCall>(value),
        "signed-result" => round_trip::<SignedResult>(value),
        "declared-secret-lookup" => round_trip::<DeclaredSecretLookup>(value),
        "declared-outbound-call-record" => round_trip::<DeclaredOutboundCallRecord>(value),
        "declared-cross-module-call" => round_trip::<DeclaredCrossModuleCall>(value),
        "release-assertion" => round_trip::<Statement<ReleaseAssertion>>(value),
        "admission-artefact" => round_trip::<AdmissionArtefact>(value),
        "advertised-artefact" => round_trip::<AdvertisedArtefact>(value),
        "operator-artefact" => round_trip::<OperatorArtefact>(value),
        "successive-releases" => round_trip::<SuccessiveReleases>(value),
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
    if let Some((_, rule)) = rust_also_rejects().into_iter().find(|(n, _)| *n == name) {
        assert!(
            allowed,
            "{name} is in rust_also_rejects but not rust_accepts"
        );
        assert_eq!(
            rust_rules(shape, &instance),
            Err(rule),
            "Rust rule refusing schema-rejected fixture {name}"
        );
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

// verifies: REQ-MDL-6
#[test]
fn sandboxed_part_declaring_a_secret_lookup_is_invalid() {
    assert_schema_rejects("declaration-sandboxed-part-declaring-a-secret-lookup");
    // Nor can the sandboxed part ask for one across the split: its request to the outside part names none.
    assert_schema_rejects("outside-request-naming-a-secret");
}

// verifies: REQ-MDL-8
#[test]
fn outbound_call_record_without_a_transaction_is_invalid() {
    assert_schema_rejects("outbound-call-record-without-a-transaction");
}

// verifies: REQ-MDL-3
#[test]
fn result_signed_with_another_key_than_the_statement_key_is_invalid() {
    assert_schema_rejects("declaration-without-its-statement-key");
    let table = rust_rule_fixtures();
    for name in [
        "signed-result-signed-with-another-advertised-key",
        "signed-result-statement-key-not-advertised",
        "signed-result-statement-key-is-the-operator-key",
        "signed-result-checked-against-another-providers-advertisement",
        "signed-result-from-another-deployment",
        "signed-result-made-by-another-provider",
        "signed-result-statement-key-listed-for-advertising",
    ] {
        let (_, rule) = table
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not a Rust-rule fixture"));
        assert_rule_rejects(name, rule);
    }
}

// verifies: REQ-MDL-6, REQ-MDL-7, REQ-MDL-8, REQ-MDL-16, REQ-MDL-17, REQ-MDL-19, REQ-MDL-21
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

// verifies: REQ-MDL-3, REQ-MDL-6
#[test]
fn secret_lookup_of_a_secret_its_operation_does_not_declare_is_invalid() {
    assert_schema_rejects("secret-lookup-without-an-operation");
    let table = rust_rule_fixtures();
    for name in [
        "declared-secret-lookup-of-a-secret-its-operation-does-not-declare",
        "declared-secret-lookup-for-an-undeclared-operation",
    ] {
        let (_, rule) = table
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not a Rust-rule fixture"));
        assert_rule_rejects(name, rule);
    }
}

// verifies: REQ-MDL-6, REQ-MDL-8
#[test]
fn outbound_call_record_naming_a_secret_its_operation_does_not_declare_is_invalid() {
    let table = rust_rule_fixtures();
    for name in [
        "declared-outbound-call-record-using-a-secret-its-operation-does-not-declare",
        "declared-outbound-call-record-of-another-module-version",
    ] {
        let (_, rule) = table
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not a Rust-rule fixture"));
        assert_rule_rejects(name, rule);
    }
}

// verifies: REQ-MDL-6
#[test]
fn a_resolved_secret_never_prints_its_value() {
    // THR-50: a secret answer that reaches a log through `Debug` must not carry the plaintext.
    let answer: SecretAnswer =
        serde_json::from_value(fixture("valid", "secret-answer-resolved")).expect("deserialises");
    let SecretOutcome::Resolved(resolved) = &answer.outcome else {
        panic!("the fixture resolves its secret");
    };
    let plaintext = resolved.value.as_str().to_owned();
    assert!(!plaintext.is_empty());
    let printed = [
        format!("{answer:?}"),
        format!("{answer:#?}"),
        format!("{:?}", answer.outcome),
        format!("{:#?}", answer.outcome),
        format!("{resolved:?}"),
        format!("{resolved:#?}"),
        format!("{:?}", resolved.value),
        format!("{:#?}", resolved.value),
    ];
    for text in &printed {
        assert!(
            !text.contains(&plaintext),
            "Debug printed the secret: {text}"
        );
        assert!(text.contains("<redacted>"), "Debug didn't redact: {text}");
    }
}

/// Address forms, each with the schema's verdict and Rust's. The schema checks the shape of an address; Rust
/// also requires its one text form and refuses IPv4-mapped addresses. A form only Rust refuses is marked so.
const ADDRESS_FORMS: [(&str, bool, bool); 15] = [
    // (text, the schema accepts, Rust accepts)
    ("192.0.2.10", true, true),
    ("2001:db8::1", true, true),
    ("::", true, true),
    ("::1", true, true),
    ("1::", true, true),
    (":::", false, false),
    ("1:::2", false, false),
    ("1::2::3", false, false),
    ("2001:DB8::1", false, false),
    ("192.0.2.010", false, false),
    ("::ffff:192.0.2.10", false, false),
    // Only Rust refuses these: leading zeros, a single zero group compressed, an uncompressed run of zero
    // groups, an IPv4-mapped address in hexadecimal.
    ("2001:0db8::1", true, false),
    ("2001:db8::1:0:1:1:1", true, false),
    ("2001:db8:0:0:0:0:0:1", true, false),
    ("::ffff:c000:20a", true, false),
];

/// Address-block forms, as [`ADDRESS_FORMS`]. A block's special-purpose overlap is a declaration rule, checked
/// by `validate_declaration`, not here.
const ADDRESS_BLOCK_FORMS: [(&str, bool, bool); 9] = [
    ("185.12.64.0/24", true, true),
    ("2a0a:4cc0::/32", true, true),
    ("0.0.0.0/0", true, true),
    ("::/0", true, true),
    ("185.12.64.0/33", false, false),
    ("185.12.64.0/024", false, false),
    (":::/32", false, false),
    ("2a0a:04cc0::/32", false, false),
    ("2a0a:4cc0:0:0:0:0:0:0/32", true, false),
];

// verifies: REQ-MDL-7
#[test]
fn address_forms_agree_between_the_schema_and_rust() {
    let address = validator("urn:rutent:module-interface:common#/$defs/address");
    for (text, schema, rust) in ADDRESS_FORMS {
        assert_eq!(
            address.is_valid(&Value::from(text)),
            schema,
            "schema verdict on address {text}"
        );
        assert_eq!(
            Address::new(text).is_some(),
            rust,
            "Rust verdict on address {text}"
        );
        assert!(
            !rust || schema,
            "Rust accepts {text}, which the schema refuses"
        );
    }
    let block = validator("urn:rutent:module-interface:common#/$defs/address_block");
    for (text, schema, rust) in ADDRESS_BLOCK_FORMS {
        assert_eq!(
            block.is_valid(&Value::from(text)),
            schema,
            "schema verdict on block {text}"
        );
        assert_eq!(
            AddressBlock::new(text).is_some(),
            rust,
            "Rust verdict on block {text}"
        );
        assert!(
            !rust || schema,
            "Rust accepts {text}, which the schema refuses"
        );
    }
}

/// Asserts each named fixture breaks exactly the Rust rule `rust_rule_fixtures` gives it.
fn assert_rules_reject(names: &[&str]) {
    let table = rust_rule_fixtures();
    for name in names {
        let (_, rule) = table
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} is not a Rust-rule fixture"));
        assert_rule_rejects(name, rule);
    }
}

// verifies: REQ-MDL-1
#[test]
fn artefact_without_a_signed_assertion_is_invalid() {
    assert_schema_rejects("admission-artefact-without-a-signed-assertion");
    assert_schema_rejects("admission-artefact-with-an-unsigned-assertion");
}

// verifies: REQ-MDL-1, REQ-CAT-23
#[test]
fn operator_release_is_signed_with_the_operator_key_of_the_platforms_deployment() {
    let valid = fixture("valid", "operator-artefact-asserted-with-the-operator-key");
    assert_eq!(rust_rules("operator-artefact", &valid), Ok(()));
    assert_rules_reject(&[
        "operator-artefact-asserted-with-a-providers-key",
        "operator-artefact-from-another-deployment",
        "operator-artefact-declaring-the-operator-key-as-its-statement-key",
    ]);
}

// verifies: REQ-MDL-1
#[test]
fn altered_or_wrongly_signed_release_is_refused() {
    assert_rules_reject(&[
        "admission-artefact-of-an-altered-outside-part",
        "advertised-artefact-asserted-by-another-maker",
        "advertised-artefact-asserted-with-an-unadvertised-key",
        "advertised-artefact-asserted-with-the-operator-key",
        "advertised-artefact-asserted-with-the-statement-key",
        "advertised-artefact-checked-against-another-providers-advertisement",
        "advertised-artefact-from-another-deployment",
        "successive-releases-of-another-providers-module",
        "successive-releases-asserted-with-the-previous-statement-key",
        "successive-releases-of-another-deployment",
        "successive-releases-under-another-protocol-version",
        "advertised-artefact-asserted-with-an-advertised-statements-key",
        "advertised-artefact-declaring-an-advertising-key-as-its-statement-key",
        "advertised-artefact-declaring-an-unadvertised-statement-key",
        "advertised-artefact-declaring-the-operator-key-as-its-statement-key",
    ]);
    assert_schema_rejects("release-assertion-under-another-shape");
    // An alias of the statement key, differing only in its unused bits, is no key in either layer, so it can't
    // slip past the "isn't the statement key" check.
    assert_schema_rejects(
        "advertised-artefact-asserted-with-a-non-canonical-alias-of-the-statement-key",
    );
}

// verifies: REQ-MDL-2, REQ-MDL-15
#[test]
fn artefact_records_the_build_that_made_its_machine_code() {
    // Both layers refuse an artefact that doesn't say which platform build made its machine code, or what that
    // machine code was: without them, what runs can't be traced or re-derived.
    assert_schema_rejects("admission-artefact-without-its-platform-build-version");
    assert_schema_rejects("admission-artefact-without-its-machine-code");
}

// verifies: REQ-MDL-3
#[test]
fn release_widening_capabilities_is_marked_for_fresh_acceptance() {
    assert_schema_rejects("admission-artefact-widening-but-accepted-automatically");
    assert_schema_rejects("admission-artefact-of-a-first-release-accepted-automatically");
    // A first release can't pass as a later one by naming a predecessor its history lacks.
    assert_schema_rejects("admission-artefact-of-a-first-release-comparing-with-a-version");
    assert_rules_reject(&[
        "successive-releases-widening-with-no-widening-recorded",
        "successive-releases-handling-a-new-kind-with-no-widening-recorded",
        "admission-artefact-comparing-with-a-version-its-history-lacks",
    ]);
    let pair: SuccessiveReleases = serde_json::from_value(fixture(
        "valid",
        "successive-releases-widening-and-marked-for-fresh-acceptance",
    ))
    .expect("deserialises");
    let computed = module_interface::widening(pair.previous.declaration(), pair.next.declaration());
    assert!(!computed.is_empty(), "the release widens");
    assert_eq!(pair.next.capabilities.widenings, computed);
    assert_eq!(
        pair.next.acceptance.required,
        module_interface::AcceptanceMode::Fresh
    );
}

/// A declaration from the pinned-blocks fixture, changed by `change`.
fn declaration_with(change: impl Fn(&mut Value)) -> Declaration {
    let mut value = fixture("valid", "declaration-destinations-pinned-to-address-blocks");
    change(&mut value);
    let declaration: Declaration = serde_json::from_value(value).expect("deserialises");
    module_interface::validate_declaration(&declaration).expect("a valid declaration");
    declaration
}

fn unchanged(_: &mut Value) {}

fn with_call(kinds: &'static [&'static str]) -> impl Fn(&mut Value) {
    move |d: &mut Value| {
        d["outside"]["calls"] = serde_json::json!([{ "callee": "bridge-pay", "kinds": kinds }]);
    }
}

fn with_blocks(destination: usize, blocks: &'static [&'static str]) -> impl Fn(&mut Value) {
    move |d: &mut Value| {
        d["outside"]["destinations"][destination]["addresses"] = serde_json::json!(blocks)
    }
}

// verifies: REQ-MDL-3
#[test]
fn widening_lists_every_widening_and_no_narrowing() {
    type Change = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Change, Change, Value)> = vec![
        (
            "a kind the sandboxed part newly handles",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["sandboxed"]["handles"] = serde_json::json!([
                    "withdrawal",
                    "deposit",
                    "balance_verification",
                    "reversal_confirmation"
                ])
            }),
            serde_json::json!([{ "kind": "handles", "operation_kind": "reversal_confirmation" }]),
        ),
        (
            "a kind no longer handled",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["sandboxed"]["handles"] = serde_json::json!(["withdrawal", "deposit"])
            }),
            serde_json::json!([]),
        ),
        (
            "a new operation",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["outside"]["operations"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!(
                        { "name": "statements.list", "destinations": ["bank_api"], "secrets": [] }
                    ))
            }),
            serde_json::json!([{ "kind": "operation", "operation": "statements.list" }]),
        ),
        (
            "a destination an operation gains",
            Box::new(|d: &mut Value| {
                d["outside"]["operations"][0]["destinations"] = serde_json::json!(["bank_api"])
            }),
            Box::new(unchanged),
            serde_json::json!([{ "kind": "operation_destination", "operation": "statements.fetch", "destination": "bank_files" }]),
        ),
        (
            "a secret an operation gains",
            Box::new(|d: &mut Value| {
                d["outside"]["operations"][0]["secrets"] =
                    serde_json::json!(["bank.client_certificate"])
            }),
            Box::new(unchanged),
            serde_json::json!([{ "kind": "operation_secret", "operation": "statements.fetch", "secret": "bank.client_key" }]),
        ),
        (
            "a new secret",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["outside"]["secrets"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!("bank.archive_key"))
            }),
            serde_json::json!([{ "kind": "secret", "secret": "bank.archive_key" }]),
        ),
        (
            "a destination of the same name at another host",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["outside"]["destinations"][0]["host"] = serde_json::json!("elsewhere.example")
            }),
            serde_json::json!([{ "kind": "destination", "destination": "bank_api" }]),
        ),
        (
            "blocks opened to public",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["outside"]["destinations"][1]["addresses"] = serde_json::json!("public")
            }),
            serde_json::json!([{ "kind": "addresses", "destination": "bank_files" }]),
        ),
        (
            "a block no old block contains",
            Box::new(unchanged),
            Box::new(with_blocks(1, &["185.12.65.16/28", "185.12.70.0/24"])),
            serde_json::json!([{ "kind": "addresses", "destination": "bank_files" }]),
        ),
        (
            "a block only the union of two old blocks covers",
            Box::new(with_blocks(1, &["185.12.65.0/25", "185.12.65.128/25"])),
            Box::new(with_blocks(1, &["185.12.65.0/24"])),
            serde_json::json!([{ "kind": "addresses", "destination": "bank_files" }]),
        ),
        (
            "a new callee",
            Box::new(unchanged),
            Box::new(with_call(&["transfer"])),
            serde_json::json!([{ "kind": "call", "callee": "bridge-pay" }]),
        ),
        (
            "a new kind asked of an existing callee",
            Box::new(with_call(&["transfer"])),
            Box::new(with_call(&["transfer", "conversion"])),
            serde_json::json!([{ "kind": "call_kind", "callee": "bridge-pay", "operation_kind": "conversion" }]),
        ),
        (
            "a changed statement-signing key",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["outside"]["statement_key"] =
                    serde_json::json!("CAkKCwwNDg8QERITFBUWFxgZGhscHR4fICEiIyQlJic")
            }),
            serde_json::json!([{ "kind": "statement_key" }]),
        ),
        (
            "nothing changed",
            Box::new(unchanged),
            Box::new(unchanged),
            serde_json::json!([]),
        ),
        (
            "an operation removed",
            Box::new(unchanged),
            Box::new(|d: &mut Value| d["outside"]["operations"] = serde_json::json!([])),
            serde_json::json!([]),
        ),
        (
            "a destination an operation loses",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["outside"]["operations"][0]["destinations"] = serde_json::json!(["bank_api"])
            }),
            serde_json::json!([]),
        ),
        (
            "a block narrowed",
            Box::new(unchanged),
            Box::new(with_blocks(0, &["185.12.64.0/25"])),
            serde_json::json!([]),
        ),
        (
            "public narrowed to blocks",
            Box::new(|d: &mut Value| {
                d["outside"]["destinations"][1]["addresses"] = serde_json::json!("public")
            }),
            Box::new(unchanged),
            serde_json::json!([]),
        ),
        (
            "a kind no longer asked of a callee",
            Box::new(with_call(&["transfer", "conversion"])),
            Box::new(with_call(&["transfer"])),
            serde_json::json!([]),
        ),
        (
            "a callee no longer called",
            Box::new(with_call(&["transfer"])),
            Box::new(unchanged),
            serde_json::json!([]),
        ),
        (
            "a secret moved from one operation to another",
            Box::new(|d: &mut Value| {
                d["outside"]["operations"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!(
                        { "name": "statements.list", "destinations": ["bank_api"], "secrets": [] }
                    ))
            }),
            Box::new(|d: &mut Value| {
                d["outside"]["operations"][0]["secrets"] =
                    serde_json::json!(["bank.client_certificate"]);
                d["outside"]["operations"].as_array_mut().unwrap().push(serde_json::json!(
                    { "name": "statements.list", "destinations": ["bank_api"], "secrets": ["bank.client_key"] }
                ))
            }),
            serde_json::json!([{ "kind": "operation_secret", "operation": "statements.list", "secret": "bank.client_key" }]),
        ),
        (
            "an operation renamed",
            Box::new(unchanged),
            Box::new(|d: &mut Value| {
                d["outside"]["operations"][0]["name"] = serde_json::json!("statements.download")
            }),
            serde_json::json!([{ "kind": "operation", "operation": "statements.download" }]),
        ),
        (
            "a call to another callee",
            Box::new(with_call(&["transfer"])),
            Box::new(|d: &mut Value| {
                d["outside"]["calls"] =
                    serde_json::json!([{ "callee": "chain-bridge", "kinds": ["transfer"] }])
            }),
            serde_json::json!([{ "kind": "call", "callee": "chain-bridge" }]),
        ),
        (
            "a reversal now asked of an existing callee",
            Box::new(with_call(&["transfer"])),
            Box::new(with_call(&["transfer", "reversal"])),
            serde_json::json!([{ "kind": "call_kind", "callee": "bridge-pay", "operation_kind": "reversal" }]),
        ),
        (
            "a new callee asked only for reversal confirmations",
            Box::new(unchanged),
            Box::new(
                |d: &mut Value| d["outside"]["calls"] = serde_json::json!([{ "callee": "polygon-wallet-reserve", "kinds": ["reversal_confirmation"] }]),
            ),
            serde_json::json!([{ "kind": "call", "callee": "polygon-wallet-reserve" }]),
        ),
        (
            "a reversal confirmation now asked of an existing callee",
            Box::new(with_call(&["transfer"])),
            Box::new(with_call(&["transfer", "reversal_confirmation"])),
            serde_json::json!([{ "kind": "call_kind", "callee": "bridge-pay", "operation_kind": "reversal_confirmation" }]),
        ),
        (
            "reversals no longer asked of a callee",
            Box::new(with_call(&[
                "transfer",
                "reversal",
                "reversal_confirmation",
            ])),
            Box::new(with_call(&["transfer"])),
            serde_json::json!([]),
        ),
    ];
    for (case, old, new, expected) in cases {
        let found = module_interface::widening(&declaration_with(old), &declaration_with(new));
        let expected: Vec<Widening> = serde_json::from_value(expected).expect("expected widenings");
        assert_eq!(found, expected, "widenings for {case}");
    }
}

// verifies: REQ-MDL-21, REQ-MDL-9
#[test]
fn refused_cross_module_call_is_recorded_with_its_reason() {
    assert_schema_rejects("cross-module-call-without-its-outcome");
    assert_schema_rejects("cross-module-call-refused-with-a-null-reason");
    assert_schema_rejects("cross-module-call-refused-for-an-unknown-reason");
    assert_rules_reject(&[
        "cross-module-call-refusal-naming-another-callee",
        "declared-cross-module-call-delivered-to-an-undeclared-callee",
        "declared-cross-module-call-refused-as-undeclared-though-declared",
    ]);
}

/// A `declared-cross-module-call` fixture, read into the contract's types.
fn declared_call(kind: &str, name: &str) -> (Declaration, CrossModuleCall) {
    let bundle: DeclaredCrossModuleCall =
        serde_json::from_value(fixture(kind, name)).expect("a declared cross-module call");
    (bundle.declaration, bundle.call)
}

// verifies: REQ-MDL-21
#[test]
fn call_refused_as_an_undeclared_kind_passes_validate_call_against() {
    // The forwarded kind is declared, but the caller's code named another: the platform refuses it as
    // `undeclared_kind`, and the record names the kind forwarded.
    let name = "declared-cross-module-call-refused-as-an-undeclared-kind-its-caller-named";
    let (declaration, call) = declared_call("valid", name);
    let CrossModuleOutcome::Refused(refused) = &call.outcome else {
        panic!("{name} is a refused call");
    };
    assert_eq!(refused.reason, CallRefusalReason::UndeclaredKind);
    let declared = declaration
        .outside
        .calls
        .iter()
        .find(|c| c.callee.as_str() == call.callee())
        .expect("the callee is declared");
    assert!(
        declared.kinds.contains(&refused.kind),
        "the forwarded kind is declared"
    );
    assert_eq!(module_interface::validate_call(&call), Ok(()));
    assert_eq!(
        module_interface::validate_call_against(&declaration, &call),
        Ok(())
    );
    // A declared callee refused as undeclared is still untrue, and so is an undeclared kind of an undeclared
    // callee.
    assert_rules_reject(&[
        "declared-cross-module-call-refused-as-undeclared-though-declared",
        "declared-cross-module-call-refused-as-an-undeclared-kind-to-an-undeclared-callee",
    ]);
}

// verifies: REQ-MDL-21
#[test]
fn call_its_callee_never_answered_is_not_recorded_as_delivered() {
    let name = "declared-cross-module-call-its-callee-never-answered";
    let (declaration, call) = declared_call("valid", name);
    assert_eq!(call.outcome, CrossModuleOutcome::Unanswered(Nothing {}));
    assert_ne!(
        serde_json::to_value(&call.outcome).expect("json")["status"],
        "delivered"
    );
    assert_eq!(module_interface::validate_call(&call), Ok(()));
    assert_eq!(
        module_interface::validate_call_against(&declaration, &call),
        Ok(())
    );
    // Passed on unanswered is still passed on: its callee and kind must be declared, and it carries nothing.
    assert_rules_reject(&["declared-cross-module-call-unanswered-of-an-undeclared-kind"]);
    assert_schema_rejects("cross-module-call-unanswered-carrying-a-reason");
}

/// The reason a fixture's refused outcome gives on the wire.
fn wire_reason(name: &str, at: &str) -> Value {
    fixture("valid", name)
        .pointer(at)
        .unwrap_or_else(|| panic!("{name} has {at}"))
        .clone()
}

// verifies: DEC-4, REQ-MDL-9, REQ-MDL-11
#[test]
fn cross_module_call_refused_by_a_limit_or_a_near_deadline_names_its_own_reason() {
    for (name, reason, wire) in [
        (
            "declared-cross-module-call-refused-as-a-limit-was-reached",
            CallRefusalReason::LimitReached,
            "limit_reached",
        ),
        (
            "declared-cross-module-call-refused-as-too-little-of-the-deadline-was-left",
            CallRefusalReason::DeadlineTooNear,
            "deadline_too_near",
        ),
    ] {
        let (declaration, call) = declared_call("valid", name);
        let CrossModuleOutcome::Refused(refused) = &call.outcome else {
            panic!("{name} is a refused call");
        };
        assert_eq!(refused.reason, reason, "{name}");
        assert_ne!(refused.reason, CallRefusalReason::CalleeUnavailable);
        assert_eq!(wire_reason(name, "/call/outcome/reason"), wire);
        assert_eq!(module_interface::validate_call(&call), Ok(()));
        assert_eq!(
            module_interface::validate_call_against(&declaration, &call),
            Ok(())
        );
    }
    // A limit doesn't excuse an undeclared callee: the first check the call fails still names the reason.
    assert_rules_reject(&["declared-cross-module-call-refused-as-a-limit-to-an-undeclared-callee"]);
}

// verifies: DEC-4, REQ-MDL-6, REQ-MDL-7
#[test]
fn outbound_call_record_refused_for_a_failed_tls_check_has_its_own_reason() {
    let name = "outbound-call-record-refused-as-the-peer-was-not-verified";
    let record: OutboundCallRecord =
        serde_json::from_value(fixture("valid", name)).expect("a record");
    assert_eq!(
        record.outcome,
        CallOutcome::Refused(module_interface::Refused {
            reason: RefusalReason::PeerNotVerified
        })
    );
    assert_eq!(wire_reason(name, "/outcome/reason"), "peer_not_verified");
    // The hop matched the declared destination by name, and no verified connection was made.
    assert!(record.hops[0].destination.is_some() && record.hops[0].address.is_none());
    assert_eq!(module_interface::validate_record(&record), Ok(()));
    assert_schema_rejects("outbound-call-record-refused-for-an-unknown-reason");
}

// verifies: DEC-4, REQ-MDL-6
#[test]
fn secret_answer_for_a_store_that_gave_no_answer_has_its_own_reason() {
    let name = "secret-answer-refused-as-the-store-gave-no-answer";
    let answer: SecretAnswer = serde_json::from_value(fixture("valid", name)).expect("an answer");
    let SecretOutcome::Refused(refused) = &answer.outcome else {
        panic!("{name} is refused");
    };
    assert_eq!(refused.reason, SecretRefusalReason::StoreUnavailable);
    assert_ne!(refused.reason, SecretRefusalReason::NotFound);
    assert_eq!(wire_reason(name, "/outcome/reason"), "store_unavailable");
    assert_schema_rejects("secret-answer-refused-for-an-unknown-reason");
}

// verifies: DEC-4, REQ-AUD-1, REQ-MDL-8
#[test]
fn unknown_call_whose_last_hop_connected_to_nothing_is_a_valid_record() {
    let name = "outbound-call-record-unknown-as-nothing-connected";
    let record: OutboundCallRecord =
        serde_json::from_value(fixture("valid", name)).expect("a record");
    assert_eq!(record.outcome, CallOutcome::Unknown(Nothing {}));
    let last = record.hops.last().expect("a hop");
    assert!(last.destination.is_some() && last.address.is_none());
    assert_eq!(module_interface::validate_record(&record), Ok(()));
    // Only the last hop may have connected to nothing, and only one that matched a declared destination.
    assert_rules_reject(&[
        "outbound-call-record-unknown-through-an-unconnected-hop-before-the-last",
        "outbound-call-record-unknown-ending-at-a-hop-that-matched-no-destination",
        "outbound-call-record-completed-through-an-unconnected-hop",
    ]);
}

// verifies: DEC-4
#[test]
fn admission_pending_and_assertion_unverified_are_distinct_reasons_that_print_their_names() {
    let reasons = [
        AdmissionRefusal::AdmissionPending,
        AdmissionRefusal::AssertionUnverified,
        AdmissionRefusal::VersionAlreadyAdmitted,
    ];
    for (i, a) in reasons.iter().enumerate() {
        for b in &reasons[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(
        format!("{:?}", AdmissionRefusal::AdmissionPending),
        "AdmissionPending"
    );
    assert_eq!(
        format!("{:?}", AdmissionRefusal::AssertionUnverified),
        "AssertionUnverified"
    );
}

// verifies: DEC-4
#[test]
fn version_matches_the_changelog() {
    let changelog = fs::read_to_string(contract_dir().join("CHANGELOG.md")).expect("changelog");
    let top = changelog
        .lines()
        .find_map(|line| line.strip_prefix("## "))
        .expect("changelog has a version heading");
    assert_eq!(top.trim(), module_interface::VERSION);
}
