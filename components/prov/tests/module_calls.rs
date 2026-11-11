//! A cross-module call is judged by the kind of the operation the platform forwards, never by the kind the guest
//! names (`REQ-MDL-21`, `REQ-MDL-9`).

#[path = "module_support/mod.rs"]
mod common;

use std::time::Duration;

use common::{Probe, answers_as, binding, declaration, install, languages, spec, withdrawal};
use rutent_conformance::boundary::{
    BoundaryHarness, Handled, ModuleBinding, ModuleSpec, Output, PlatformRefusal, Step,
};
use rutent_contracts::module_interface::{
    CallRefusalReason, CrossModuleOutcome, Declaration, OperationKind, validate_call,
    validate_call_against,
};
use rutent_prov::module::platform::OPERATION_DEADLINE;
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

/// A caller that may call `callee` for withdrawals, and a spec whose one step does.
fn caller_of(caller: &str, callee: &str) -> ModuleSpec {
    ModuleSpec {
        declaration: calling(caller, callee, &["withdrawal"]),
        steps: vec![Step::CallModule {
            callee: callee.to_owned(),
            kind: OperationKind::Withdrawal,
        }],
        output: Output::Honest,
        starts: true,
    }
}

// verifies: REQ-MDL-11
#[test]
fn call_with_too_little_deadline_left_is_refused() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (caller, callee) = (format!("caller-{language}"), format!("callee-{language}"));
        install(
            &mut binding,
            &spec(&callee, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        let module = caller_of(&caller, &callee);
        install(&mut binding, &module, &language, "reserve", &[]);
        // The caller's whole deadline, 1.5 seconds, is less than the 3 seconds a callee needs: whatever has
        // passed since the operation began, less than that is left.
        binding.set_deadline(Duration::from_millis(1500));
        binding.set_call_budget(Duration::from_secs(3));
        binding.take_events();
        let handled = binding.handle(&caller, &withdrawal(&caller, 1));
        assert!(
            matches!(handled, Handled::Answered(_)),
            "{language}: a refused call ended the caller's own operation: {handled:?}"
        );
        let events = binding.take_events();
        let call = events.calls.first().expect("a record of the call");
        assert!(
            matches!(
                &call.outcome,
                CrossModuleOutcome::Refused(r) if r.reason == CallRefusalReason::DeadlineTooNear
            ),
            "{language}: the call wasn't refused: {:?}",
            call.outcome
        );
        assert_eq!(validate_call(call), Ok(()));
        assert_eq!(validate_call_against(&module.declaration, call), Ok(()));
        assert_eq!(
            binding.operations_received(&callee),
            0,
            "{language}: the callee was handed an operation"
        );
        assert!(
            binding.take_limits().is_empty(),
            "a refused call is no limit hit of the caller"
        );
        // The control: with the whole deadline and little needed, the same call is delivered.
        binding.set_deadline(OPERATION_DEADLINE);
        binding.set_call_budget(Duration::from_millis(1));
        binding.handle(&caller, &withdrawal(&caller, 2));
        let events = binding.take_events();
        assert!(
            matches!(
                events.calls.first().map(|c| &c.outcome),
                Some(CrossModuleOutcome::Delivered(_))
            ),
            "{language}: the control's call wasn't delivered"
        );
        assert_eq!(binding.operations_received(&callee), 1);
    }
}

// verifies: REQ-MDL-21
#[test]
fn binding_records_a_call_its_callee_never_answered_as_undelivered() {
    for language in languages() {
        let mut binding = binding("reserve");
        let callees: [(String, Vec<Probe>, Output); 2] = [
            (
                format!("trapping-{language}"),
                vec![Probe::Trap],
                Output::Honest,
            ),
            (
                format!("garbling-{language}"),
                Vec::new(),
                Output::Unparseable,
            ),
        ];
        for (callee, probes, output) in callees {
            let module = ModuleSpec {
                declaration: declaration(&callee, "1.0.0", &["withdrawal"]),
                steps: Vec::new(),
                output,
                starts: true,
            };
            install(&mut binding, &module, &language, "reserve", &probes);
            let caller = format!("caller-{callee}");
            let caller_module = caller_of(&caller, &callee);
            install(&mut binding, &caller_module, &language, "reserve", &[]);
            binding.take_events();
            let handled = binding.handle(&caller, &withdrawal(&caller, 1));
            assert!(
                matches!(handled, Handled::Answered(_)),
                "{callee}: the callee's silence ended the caller's operation: {handled:?}"
            );
            let events = binding.take_events();
            let call = events.calls.first().expect("a record of the call");
            assert!(
                matches!(call.outcome, CrossModuleOutcome::Unanswered(_)),
                "{callee}: a call its callee never answered wasn't recorded as unanswered: {:?}",
                call.outcome
            );
            assert_eq!(validate_call(call), Ok(()), "{callee}");
            assert_eq!(
                validate_call_against(&caller_module.declaration, call),
                Ok(()),
                "{callee}"
            );
            // The callee was handed the operation: only an answer would have made it delivered.
            assert_eq!(binding.operations_received(&callee), 1, "{callee}");
            // And it did not answer.
            let direct = binding.handle(&callee, &withdrawal(&callee, 2));
            assert!(
                matches!(
                    direct,
                    Handled::Refused(
                        PlatformRefusal::Unavailable | PlatformRefusal::MalformedOutput
                    )
                ),
                "{callee}: it answered when asked directly: {direct:?}"
            );
        }
        // The control: a callee that answers is delivered.
        let callee = format!("answering-{language}");
        install(
            &mut binding,
            &spec(&callee, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        let caller = format!("caller-{callee}");
        install(
            &mut binding,
            &caller_of(&caller, &callee),
            &language,
            "reserve",
            &[],
        );
        binding.take_events();
        binding.handle(&caller, &withdrawal(&caller, 1));
        let events = binding.take_events();
        assert!(matches!(
            events.calls.first().map(|c| &c.outcome),
            Some(CrossModuleOutcome::Delivered(_))
        ));
        assert!(answers_as(&mut binding, &callee, "1.0.0", 3));
    }
}

/// The evidence reference of the caller's answer, which names each value its outside part obtained.
fn reference(handled: &Handled) -> String {
    let Handled::Answered(result) = handled else {
        panic!("the caller didn't answer: {handled:?}")
    };
    serde_json::to_value(&result.statement).expect("JSON")["outcome"]["evidence"][0]["reference"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// The calling module is told each outcome as it is recorded: a callee that answered is `delivered`, one the
/// caller doesn't declare is `refused`, and one that was handed the operation and gave no answer is `unanswered`,
/// never `refused`, since it may have acted on it.
// verifies: REQ-ORD-1, REQ-MDL-21
#[test]
fn caller_is_told_each_cross_module_outcome_as_it_is_recorded() {
    for language in languages() {
        let mut binding = binding("reserve");
        let answering = format!("answering-{language}");
        let trapping = format!("trapping-{language}");
        install(
            &mut binding,
            &spec(&answering, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &spec(&trapping, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Trap],
        );
        // The caller declares calls to the answering and the trapping callee, and to no other.
        let undeclared = format!("undeclared-{language}");
        for (callee, told, recorded) in [
            (&answering, "delivered", "delivered"),
            (&trapping, "unanswered", "unanswered"),
            (&undeclared, "refused", "refused"),
        ] {
            let caller = format!("caller-{told}-{language}");
            let declared = if callee == &undeclared {
                &answering
            } else {
                callee
            };
            let module = ModuleSpec {
                declaration: calling(&caller, declared, &["withdrawal"]),
                steps: Vec::new(),
                output: Output::Honest,
                starts: true,
            };
            install(
                &mut binding,
                &module,
                &language,
                "reserve",
                &[Probe::Ask {
                    callee: callee.clone(),
                    kind: OperationKind::Withdrawal,
                }],
            );
            binding.take_events();
            let handled = binding.handle(&caller, &withdrawal(&caller, 1));
            assert_eq!(
                reference(&handled),
                format!("{caller}-1.0.0-{told}"),
                "{caller}: the guest was told another outcome"
            );
            let events = binding.take_events();
            let call = events.calls.first().expect("a record of the call");
            let status = serde_json::to_value(&call.outcome).expect("JSON")["status"].clone();
            assert_eq!(status, recorded, "{caller}: {:?}", call.outcome);
        }
    }
}

/// A caller whose cross-module call was delivered and that then traps may have set its callee acting: its
/// operation is an outcome unknown, never `Unavailable`, and what ended it is recorded beside it.
// verifies: REQ-ORD-1, REQ-MDL-21
#[test]
fn caller_that_traps_after_a_cross_module_call_has_an_unknown_outcome() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (caller, callee) = (format!("caller-{language}"), format!("callee-{language}"));
        install(
            &mut binding,
            &spec(&callee, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &caller_of(&caller, &callee),
            &language,
            "reserve",
            &[Probe::Trap],
        );
        binding.take_events();
        binding.take_unknown();
        let handled = binding.handle(&caller, &withdrawal(&caller, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::OutcomeUnknown),
            "{language}: a trap after a delivered call was given another reason"
        );
        let events = binding.take_events();
        assert!(
            matches!(
                events.calls.first().map(|c| &c.outcome),
                Some(CrossModuleOutcome::Delivered(_))
            ),
            "{language}: {:?}",
            events.calls
        );
        let unknown = binding.take_unknown();
        assert_eq!(unknown.len(), 1, "{language}: {unknown:?}");
        assert_eq!(unknown[0].provider, caller);
        assert_eq!(unknown[0].ended_by, PlatformRefusal::Unavailable);
    }
}

/// A call is made only when the callee's whole budget and the caller's reserve after it are both left of the
/// caller's deadline: with the budget left but not the reserve, it is refused as `deadline_too_near`, and the
/// callee is handed nothing.
// verifies: REQ-MDL-11, REQ-ORD-1
#[test]
fn call_needs_the_callees_budget_and_the_callers_reserve_left() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (caller, callee) = (format!("caller-{language}"), format!("callee-{language}"));
        install(
            &mut binding,
            &spec(&callee, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &caller_of(&caller, &callee),
            &language,
            "reserve",
            &[],
        );
        // A budget of one second leaves a reserve of half a second: the call needs one and a half left. A
        // deadline of 1.4 seconds leaves the budget but not the reserve.
        binding.set_call_budget(Duration::from_secs(1));
        binding.set_deadline(Duration::from_millis(1400));
        binding.take_events();
        let handled = binding.handle(&caller, &withdrawal(&caller, 1));
        assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
        let events = binding.take_events();
        assert!(
            matches!(
                events.calls.first().map(|c| &c.outcome),
                Some(CrossModuleOutcome::Refused(r)) if r.reason == CallRefusalReason::DeadlineTooNear
            ),
            "{language}: {:?}",
            events.calls
        );
        assert_eq!(binding.operations_received(&callee), 0, "{language}");

        // The control: with both left, the same call is delivered.
        binding.set_deadline(Duration::from_secs(3));
        binding.handle(&caller, &withdrawal(&caller, 2));
        let events = binding.take_events();
        assert!(
            matches!(
                events.calls.first().map(|c| &c.outcome),
                Some(CrossModuleOutcome::Delivered(_))
            ),
            "{language}: {:?}",
            events.calls
        );
        binding.set_deadline(OPERATION_DEADLINE);
    }
}
