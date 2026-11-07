//! The stand-ins themselves (`DEC-7`, `B-01`): one per provider category in each language, each admitted and
//! answering an operation of its category's role, as well as the suite's withdrawal.

mod common;

use common::{binding, declaration, install, languages};
use rutent_conformance::boundary::{Admitted, Handled, ModuleBinding, ModuleSpec, Output};
use rutent_contracts::provider_role::{Operation, Statement};

/// An operation of `kind` asked of `provider`, from the provider-role contract's valid fixtures.
fn operation(kind: &str, provider: &str) -> Statement<Operation> {
    let fixture = match kind {
        "deposit" => "operation-deposit-as-a-dry-run.json",
        "conversion" => "operation-conversion-as-a-dry-run.json",
        "transfer" => "operation-transfer-between-two-nodes.json",
        _ => "operation-judgement-question.json",
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../contracts/provider-role/fixtures/valid")
        .join(fixture);
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("the fixture")).expect("JSON");
    value["statement"]["provider"] = serde_json::Value::from(provider);
    serde_json::from_value(value).expect("a valid operation")
}

// verifies: AC-20, REQ-MDL-15, DEC-7
#[test]
fn each_categorys_stand_in_is_admitted_and_answers_an_operation_of_its_role() {
    for language in languages() {
        for (category, kind) in [
            ("reserve", "deposit"),
            ("exchange", "conversion"),
            ("ramp", "transfer"),
            ("verifier", "judgement"),
        ] {
            let mut binding = binding(category);
            let provider = format!("{category}-{language}");
            let spec = ModuleSpec {
                declaration: declaration(&provider, "1.0.0", &["withdrawal", kind]),
                steps: Vec::new(),
                output: Output::Honest,
                starts: true,
            };
            let admitted = install(&mut binding, &spec, &language, category, &[]);
            assert!(
                matches!(admitted, Admitted::Artefact(_)),
                "{provider} wasn't admitted"
            );
            let Handled::Answered(result) = binding.handle(&provider, &operation(kind, &provider))
            else {
                panic!("{provider} didn't answer a {kind}");
            };
            let value = serde_json::to_value(&result.statement).expect("JSON");
            assert_eq!(value["kind"], kind, "{provider} answered another kind");
            assert!(
                common::answers_as(&mut binding, &provider, "1.0.0", 1),
                "{provider} didn't answer a withdrawal"
            );
        }
    }
}
