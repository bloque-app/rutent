//! A cross-module call is judged by the kind of the operation the platform forwards, never by the kind the guest
//! names (`REQ-MDL-21`, `REQ-MDL-9`).

mod common;

use common::{binding, declaration, install, languages, withdrawal};
use rutent_conformance::boundary::{BoundaryHarness, ModuleBinding, ModuleSpec, Output, Step};
use rutent_contracts::module_interface::{
    CallRefusalReason, CrossModuleOutcome, Declaration, OperationKind, validate_call,
    validate_call_against,
};
use serde_json::json;

/// `provider`'s declaration, handling withdrawals, that may call `callee` for `kinds`.
fn calling(provider: &str, callee: &str, kinds: &[&str]) -> Declaration {
    let mut value =
        serde_json::to_value(declaration(provider, "1.0.0", &["withdrawal"])).expect("JSON");
    value["outside"]["calls"] = json!([{ "callee": callee, "kinds": kinds }]);
    serde_json::from_value(value).expect("a valid declaration")
}

// verifies: REQ-MDL-21, REQ-MDL-9
#[test]
fn a_cross_module_call_is_judged_by_the_kind_of_the_operation_forwarded_not_the_kind_the_guest_names()
 {
    for language in languages() {
        let mut binding = binding("reserve");
        let callee = format!("callee-{language}");
        // The callee handles withdrawals, so a withdrawal forwarded to it would be delivered and handled.
        install(
            &mut binding,
            &common::spec(&callee, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        for (caller, declared, named, delivered) in [
            // Hostile: the caller may call the callee only for conversions (the contract's quote-bearing kind),
            // and its guest names a conversion while the operation it is handling, and the platform would
            // forward, is a withdrawal.
            (
                format!("forger-{language}"),
                "conversion",
                OperationKind::Conversion,
                false,
            ),
            // Hostile: the forwarded kind is declared, but the guest names another. Refused as pvm-binding refuses
            // it; the contract's `validate_call_against` reads no refusal reason for this case (see the run note).
            (
                format!("misnamed-{language}"),
                "withdrawal",
                OperationKind::Deposit,
                false,
            ),
            // The control: the caller declares withdrawals and names one.
            (
                format!("honest-{language}"),
                "withdrawal",
                OperationKind::Withdrawal,
                true,
            ),
        ] {
            let spec = ModuleSpec {
                declaration: calling(&caller, &callee, &[declared]),
                steps: vec![Step::CallModule {
                    callee: callee.clone(),
                    kind: named,
                }],
                output: Output::Honest,
                starts: true,
            };
            install(&mut binding, &spec, &language, "reserve", &[]);
            let before = binding.operations_received(&callee);
            binding.take_events();
            binding.handle(&caller, &withdrawal(&caller, 1));
            let events = binding.take_events();
            let call = events.calls.first().expect("a cross-module call record");
            assert_eq!(
                validate_call(call),
                Ok(()),
                "{caller}: the record is invalid"
            );
            if delivered {
                assert!(
                    matches!(call.outcome, CrossModuleOutcome::Delivered(_)),
                    "{caller}: the control's call wasn't delivered: {:?}",
                    call.outcome
                );
                assert_eq!(
                    binding.operations_received(&callee),
                    before + 1,
                    "{caller}: the control's callee received nothing"
                );
                assert_eq!(
                    validate_call_against(&spec.declaration, call),
                    Ok(()),
                    "{caller}: the record breaks its declaration"
                );
                continue;
            }
            match &call.outcome {
                CrossModuleOutcome::Refused(refused) => {
                    assert_eq!(
                        refused.reason,
                        CallRefusalReason::UndeclaredKind,
                        "{caller}: refused for another reason"
                    );
                    assert_eq!(
                        refused.kind,
                        OperationKind::Withdrawal,
                        "{caller}: the record names the guest's kind, not the forwarded one"
                    );
                }
                other => panic!("{caller}: the call wasn't refused: {other:?}"),
            }
            assert_eq!(
                binding.operations_received(&callee),
                before,
                "{caller}: the callee received the forwarded operation"
            );
            if declared != "withdrawal" {
                assert_eq!(
                    validate_call_against(&spec.declaration, call),
                    Ok(()),
                    "{caller}: the record breaks its declaration"
                );
            }
        }
    }
}
