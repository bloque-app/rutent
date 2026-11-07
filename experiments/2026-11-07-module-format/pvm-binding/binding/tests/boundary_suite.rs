//! The module-boundary suite (`T-003-03`) on the PolkaVM binding: every category's stand-ins, in Rust and in C.
//! It needs the stand-ins `build.sh` makes, so it is ignored by default; `run.sh` runs it with
//! `RUTENT_PVM_GUESTS` set, before the suite's full report.

use std::path::PathBuf;

use rutent_conformance::boundary::{
    Admitted, BoundaryHarness, Endpoint, Handled, LookupOutcome, ModuleBinding, ModuleSpec, Output,
    RunConfig, Step, run_boundary_suite,
};
use rutent_conformance::role::{SuiteContext, placeholder_key, placeholder_signature};
use rutent_contracts::module_interface::{
    CallOutcome, CallRefusalReason, CrossModuleOutcome, Declaration, OperationKind, PartName,
    Parts, RefusalReason, Refused, SecretRefusalReason, validate_call, validate_call_against,
    validate_record,
};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_pvm_binding::binding::{CATEGORIES, LANGUAGES, PvmBinding, Setup, assertion};
use rutent_pvm_binding::isolation::Isolation;
use rutent_pvm_binding::spec;
use serde_json::json;

fn setup() -> Setup {
    Setup {
        worker: PathBuf::from(env!("CARGO_BIN_EXE_rutent-pvm-worker")),
        admit: PathBuf::from(env!("CARGO_BIN_EXE_rutent-pvm-admit")),
        guests: PathBuf::from(
            std::env::var("RUTENT_PVM_GUESTS")
                .expect("RUTENT_PVM_GUESTS: the stand-ins build.sh makes"),
        ),
        isolation: Isolation::Required,
    }
}

// verifies: AC-20, REQ-MDL-15, REQ-MDL-18, REQ-MDL-13, REQ-MDL-9
#[test]
#[ignore = "needs the stand-ins build.sh makes; run.sh runs it with RUTENT_PVM_GUESTS set"]
fn every_categorys_stand_ins_pass_the_module_boundary_suite_in_rust_and_in_c() {
    for category in CATEGORIES {
        let mut binding = PvmBinding::new(setup(), category);
        let report = run_boundary_suite(&mut binding, &RunConfig::default());
        assert_eq!(report.languages, ["rs", "c"], "{category}");
        assert!(
            report.passed(),
            "{category}: findings {:?}, uncovered {:?}",
            report.findings,
            report.uncovered
        );
    }
}

fn two_operations(provider: &str) -> Declaration {
    declaration(provider, json!([]))
}

fn declaration(provider: &str, calls: serde_json::Value) -> Declaration {
    serde_json::from_value(json!({
        "provider": provider,
        "version": "1.0.0",
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": ["withdrawal"] },
        "outside": {
            "operations": [
                { "name": "op.a", "destinations": ["a"], "secrets": [] },
                { "name": "op.b", "destinations": ["b"], "secrets": ["b.token"] },
            ],
            "destinations": [
                { "name": "a", "scheme": "https", "host": "a.twofaced.example", "port": 443, "addresses": "public" },
                { "name": "b", "scheme": "https", "host": "b.twofaced.example", "port": 443, "addresses": "public" },
            ],
            "secrets": ["b.token"],
            "calls": calls,
            "statement_key": placeholder_key(1),
        },
    }))
    .expect("a valid declaration")
}

fn operation(provider: &str, transaction: &str) -> Statement<Operation> {
    let context = SuiteContext::default();
    serde_json::from_value(json!({
        "deployment": context.deployment,
        "protocol_version": context.protocol_version,
        "shape": rutent_contracts::provider_role::OPERATION_SHAPE,
        "maker": context.operator,
        "made_at": "2026-11-07T00:00:00Z",
        "signature": { "scheme": "ed25519", "key": context.deployment.operator_key, "value": placeholder_signature() },
        "statement": {
            "kind": "withdrawal",
            "transaction": transaction,
            "provider": provider,
            "origin": { "network": "conformance-network" },
            "destination": "conformance-ledger",
            "asset": "COP",
            "amount": "1000",
            "dry_run": false,
            "inputs": {},
        },
    }))
    .expect("a valid operation")
}

// verifies: REQ-MDL-6, REQ-MDL-7, REQ-MDL-8
#[test]
#[ignore = "needs the stand-ins build.sh makes; run.sh runs it with RUTENT_PVM_GUESTS set"]
fn an_outside_part_naming_another_operation_than_the_request_in_flight_gets_neither_its_secret_nor_its_destination()
 {
    let b = Endpoint::https("b.twofaced.example");
    for language in LANGUAGES {
        for hostile in [false, true] {
            let mut binding = PvmBinding::new(setup(), "reserve");
            binding.set_output_limit(4096);
            let provider = format!("twofaced-{language}-{hostile}");
            binding.seed_secret(&provider, "b.token", "canary-b-token");
            binding.serve(
                &Endpoint::https("a.twofaced.example"),
                "185.12.64.10".parse().unwrap(),
                None,
            );
            binding.serve(&b, "185.12.64.11".parse().unwrap(), None);
            let spec_ = ModuleSpec {
                declaration: two_operations(&provider),
                steps: vec![
                    Step::LookUpSecret {
                        part: PartName::Outside,
                        operation: "op.b".to_owned(),
                        name: "b.token".to_owned(),
                    },
                    Step::Call {
                        operation: "op.b".to_owned(),
                        endpoint: b.clone(),
                    },
                ],
                output: Output::Honest,
                starts: true,
            };
            // The hostile module's sandboxed part sends every request as `op.a`; its outside part still looks up
            // `op.b`'s secret and calls `op.b`'s destination.
            let mut text = spec::encode(&spec_, "reserve");
            if hostile {
                text.push_str("x op.a\n");
            }
            let part = |p| {
                spec::patch(binding.template(language, p).expect("built"), &text).expect("fits")
            };
            let binaries = Parts {
                sandboxed: part(PartName::Sandboxed),
                outside: part(PartName::Outside),
            };
            let admitted = binding.admit(&assertion(&spec_.declaration, &binaries), &binaries);
            assert!(
                matches!(admitted, Admitted::Artefact(_)),
                "{language}: admitted"
            );
            assert!(binding.accept(&provider, "1.0.0", "reviewer").is_some());
            binding.take_events();
            let handled = binding.handle(
                &provider,
                &operation(&provider, &format!("tx-{language}-1")),
            );
            let events = binding.take_events();
            let lookup = events.lookups.first().expect("a lookup");
            let record = events.records.first().expect("a record");
            if hostile {
                assert_eq!(lookup.lookup.operation.as_str(), "op.a", "{language}");
                assert_eq!(
                    lookup.outcome,
                    Some(LookupOutcome::Refused(
                        SecretRefusalReason::UndeclaredSecret
                    )),
                    "{language}"
                );
                assert!(
                    binding.delivered_secrets(&provider).is_empty(),
                    "{language}"
                );
                assert_eq!(record.operation.as_str(), "op.a", "{language}");
                assert_eq!(
                    record.outcome,
                    CallOutcome::Refused(Refused {
                        reason: RefusalReason::UndeclaredOperation
                    }),
                    "{language}"
                );
                assert!(
                    binding.received(&b).is_empty(),
                    "{language}: b received the call"
                );
                assert_eq!(
                    validate_record(record),
                    Ok(()),
                    "{language}: the refused record"
                );
            } else {
                assert_eq!(lookup.outcome, Some(LookupOutcome::Resolved), "{language}");
                assert_eq!(record.operation.as_str(), "op.b", "{language}");
                assert!(
                    matches!(record.outcome, CallOutcome::Completed(_)),
                    "{language}"
                );
                assert_eq!(binding.received(&b).len(), 1, "{language}");
                assert!(matches!(handled, Handled::Answered(_)), "{language}");
            }
        }
    }
}

/// Builds and installs a release of `spec`, admitted and accepted.
fn install(binding: &mut PvmBinding, language: &str, spec: &ModuleSpec) {
    let text = spec::encode(spec, "reserve");
    let part = |p| spec::patch(binding.template(language, p).expect("built"), &text).expect("fits");
    let binaries = Parts {
        sandboxed: part(PartName::Sandboxed),
        outside: part(PartName::Outside),
    };
    let admitted = binding.admit(&assertion(&spec.declaration, &binaries), &binaries);
    assert!(
        matches!(admitted, Admitted::Artefact(_)),
        "{language}: admitted"
    );
    let provider = spec.declaration.provider.as_str().to_owned();
    assert!(binding.accept(&provider, "1.0.0", "reviewer").is_some());
}

// verifies: REQ-MDL-21, REQ-MDL-9
#[test]
#[ignore = "needs the stand-ins build.sh makes; run.sh runs it with RUTENT_PVM_GUESTS set"]
fn a_cross_module_call_is_judged_by_the_kind_of_the_operation_forwarded_not_the_kind_the_guest_names()
 {
    for language in LANGUAGES {
        let mut binding = PvmBinding::new(setup(), "reserve");
        binding.set_output_limit(4096);
        let callee = format!("callee-{language}");
        let caller = format!("caller-{language}");
        install(
            &mut binding,
            language,
            &ModuleSpec {
                declaration: two_operations(&callee),
                steps: Vec::new(),
                output: Output::Honest,
                starts: true,
            },
        );
        // The caller declares only `deposit` of the callee, and its guest names `deposit`; the operation the
        // platform would forward is the withdrawal it is serving.
        let spec_ = ModuleSpec {
            declaration: declaration(&caller, json!([{ "callee": callee, "kinds": ["deposit"] }])),
            steps: vec![Step::CallModule {
                callee: callee.clone(),
                kind: OperationKind::Deposit,
            }],
            output: Output::Honest,
            starts: true,
        };
        install(&mut binding, language, &spec_);
        let before = binding.operations_received(&callee);
        binding.take_events();
        binding.handle(&caller, &operation(&caller, &format!("tx-{language}-2")));
        let events = binding.take_events();
        let call = events.calls.first().expect("a cross-module call record");
        match &call.outcome {
            CrossModuleOutcome::Refused(refused) => {
                assert_eq!(
                    refused.reason,
                    CallRefusalReason::UndeclaredKind,
                    "{language}"
                );
                assert_eq!(refused.kind, OperationKind::Withdrawal, "{language}");
            }
            other => panic!("{language}: {other:?}"),
        }
        assert_eq!(
            binding.operations_received(&callee),
            before,
            "{language}: delivered"
        );
        assert_eq!(validate_call(call), Ok(()), "{language}");
        assert_eq!(
            validate_call_against(&spec_.declaration, call),
            Ok(()),
            "{language}"
        );
    }
}
