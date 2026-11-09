//! Cross-module calls on each module's own credentials, on the running platform (`T-014-04`, `REQ-MDL-21`,
//! `REQ-MDL-9`, `THR-86`): a call is made only where the caller declares it and the route names it, and the callee
//! resolves only its own secrets.
//!
//! These tests hand operations to the platform itself (`Platform::handle`), not through the suite's binding, whose
//! route names every call a release declares: here each test names the route's calls itself.

#[path = "module_support/mod.rs"]
mod common;

use common::{OPERATION, WasmBinding, binding, declaration, install, languages, withdrawal};
use rutent_conformance::boundary::{BoundaryHarness, ModuleSpec, Output, Step};
use rutent_contracts::module_interface::{
    CallRefusalReason, CrossModuleCall, CrossModuleOutcome, Declaration, Identifier, OperationKind,
    PartName, RouteReference, SecretRefusalReason, validate_call, validate_call_against,
};
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::{Events, Handled, LookupOutcome};
use rutent_prov::proxy::NamedCall;
use serde_json::json;

fn id(text: &str) -> Identifier {
    Identifier::new(text).expect("an identifier")
}

fn route(record: &str, segment: &str) -> RouteReference {
    RouteReference {
        decision_record: id(record),
        segment: id(segment),
    }
}

/// `provider`'s declaration, handling withdrawals, that may call each of `callees` for withdrawals, and declares
/// `extra` secrets for its outside operation beside `own.key`.
fn calling(provider: &str, callees: &[&str], extra: &[&str]) -> Declaration {
    let mut value =
        serde_json::to_value(declaration(provider, "1.0.0", &["withdrawal"])).expect("JSON");
    value["outside"]["calls"] = callees
        .iter()
        .map(|c| json!({ "callee": c, "kinds": ["withdrawal"] }))
        .collect();
    for secret in extra {
        for pointer in ["/outside/secrets", "/outside/operations/0/secrets"] {
            value
                .pointer_mut(pointer)
                .and_then(|l| l.as_array_mut())
                .expect("a list")
                .push(json!(secret));
        }
    }
    serde_json::from_value(value).expect("a valid declaration")
}

/// The outside part looking up `name` for the one outside operation.
fn look_up(name: &str) -> Step {
    Step::LookUpSecret {
        part: PartName::Outside,
        operation: OPERATION.to_owned(),
        name: name.to_owned(),
    }
}

fn call(callee: &str) -> Step {
    Step::CallModule {
        callee: callee.to_owned(),
        kind: OperationKind::Withdrawal,
    }
}

/// A module of `declaration`'s provider taking `steps`.
fn module(declaration: Declaration, steps: Vec<Step>) -> ModuleSpec {
    ModuleSpec {
        declaration,
        steps,
        output: Output::Honest,
        starts: true,
    }
}

/// The route segment naming `caller`'s call to `callee` for `kinds`.
fn named(route: RouteReference, caller: &str, callee: &str, kinds: &[OperationKind]) -> NamedCall {
    NamedCall {
        route,
        caller: id(caller),
        callee: id(callee),
        kinds: kinds.to_vec(),
    }
}

/// Hands `provider` its withdrawal `n` on the platform itself, and returns how it ended and what it set off.
fn run(binding: &mut WasmBinding, provider: &str, n: usize) -> (Handled, Events) {
    let platform: &mut Platform<_> = binding;
    platform.take_events();
    let handled = platform.handle(provider, &withdrawal(provider, n));
    (handled, platform.take_events())
}

/// The one cross-module call record of `events`, checked against the contract and the caller's declaration.
fn the_call(events: &Events, caller: &Declaration, label: &str) -> CrossModuleCall {
    assert_eq!(events.calls.len(), 1, "{label}: not one call record");
    let call = events.calls[0].clone();
    assert_eq!(
        validate_call(&call),
        Ok(()),
        "{label}: the record is invalid"
    );
    assert_eq!(
        validate_call_against(caller, &call),
        Ok(()),
        "{label}: the record contradicts the caller's declaration"
    );
    call
}

// verifies: REQ-MDL-21
#[test]
fn undeclared_cross_module_call_is_refused() {
    for language in languages() {
        let mut binding = binding("reserve");
        let callee = format!("callee-{language}");
        let caller = format!("caller-{language}");
        let stranger = format!("stranger-{language}");
        install(
            &mut binding,
            &module(declaration(&callee, "1.0.0", &["withdrawal"]), Vec::new()),
            &language,
            "reserve",
            &[],
        );
        // The caller declares its call to the callee; the stranger declares none, and makes the same call.
        let declared = calling(&caller, &[&callee], &[]);
        let undeclared = calling(&stranger, &[], &[]);
        for spec in [
            module(declared.clone(), vec![call(&callee)]),
            module(undeclared.clone(), vec![call(&callee)]),
        ] {
            install(&mut binding, &spec, &language, "reserve", &[]);
        }
        let segment = route("decision-1", "segment-2");
        // Each operation handed has a transaction of its own (`REQ-MDL-14`).
        let caller_tx = id(&format!("tx-{caller}-10"));
        // Each case: who calls, what the route names in its transaction, and the reason expected.
        let cases: Vec<(&str, &str, &Declaration, Vec<NamedCall>, CallRefusalReason)> = vec![
            (
                "a call the caller doesn't declare, though the route names it",
                &stranger,
                &undeclared,
                vec![named(
                    segment.clone(),
                    &stranger,
                    &callee,
                    &[OperationKind::Withdrawal],
                )],
                CallRefusalReason::UndeclaredCallee,
            ),
            (
                "a declared call no route names",
                &caller,
                &declared,
                Vec::new(),
                CallRefusalReason::NotNamedByRoute,
            ),
            (
                "a declared call the route names for another caller",
                &caller,
                &declared,
                vec![named(
                    segment.clone(),
                    &stranger,
                    &callee,
                    &[OperationKind::Withdrawal],
                )],
                CallRefusalReason::NotNamedByRoute,
            ),
            (
                "a declared call the route names for another kind",
                &caller,
                &declared,
                vec![named(
                    segment.clone(),
                    &caller,
                    &callee,
                    &[OperationKind::Deposit],
                )],
                CallRefusalReason::NotNamedByRoute,
            ),
            (
                "a declared call the route names for another callee",
                &caller,
                &declared,
                vec![named(
                    segment.clone(),
                    &caller,
                    &stranger,
                    &[OperationKind::Withdrawal],
                )],
                CallRefusalReason::NotNamedByRoute,
            ),
        ];
        for (n, (label, who, declaration, names, reason)) in (1..).zip(cases) {
            let transaction = id(&format!("tx-{who}-{n}"));
            for named in names {
                binding.routes.name(&transaction, named);
            }
            // The same call, named in another transaction, names nothing in this one.
            binding.routes.name(
                &id("tx-elsewhere"),
                named(segment.clone(), who, &callee, &[OperationKind::Withdrawal]),
            );
            let before = binding.operations_received(&callee);
            let (_, events) = run(&mut binding, who, n);
            let record = the_call(&events, declaration, label);
            match &record.outcome {
                CrossModuleOutcome::Refused(refused) => {
                    assert_eq!(
                        refused.reason, reason,
                        "{language}: {label}: another reason"
                    );
                }
                other => panic!("{language}: {label}: the call wasn't refused: {other:?}"),
            }
            assert_eq!(
                binding.operations_received(&callee),
                before,
                "{language}: {label}: the callee was handed the operation"
            );
            binding.routes.end(&transaction);
            binding.routes.end(&id("tx-elsewhere"));
        }
        // The control: the route names the declared call, and it is delivered, recorded against that segment.
        binding.routes.name(
            &caller_tx,
            named(
                segment.clone(),
                &caller,
                &callee,
                &[OperationKind::Withdrawal],
            ),
        );
        let before = binding.operations_received(&callee);
        let (_, events) = run(&mut binding, &caller, 10);
        let record = the_call(&events, &declared, "the control");
        assert!(
            matches!(record.outcome, CrossModuleOutcome::Delivered(_)),
            "{language}: the named call wasn't delivered: {:?}",
            record.outcome
        );
        assert_eq!(
            record.route, segment,
            "{language}: recorded on another route"
        );
        assert_eq!(binding.operations_received(&callee), before + 1);
        // Once its route ends, the same call is refused.
        binding.routes.end(&caller_tx);
        let ended = id(&format!("tx-{caller}-11"));
        binding.routes.name(
            &ended,
            named(
                segment.clone(),
                &caller,
                &callee,
                &[OperationKind::Withdrawal],
            ),
        );
        binding.routes.end(&ended);
        let (_, events) = run(&mut binding, &caller, 11);
        let record = the_call(&events, &declared, "after the route ended");
        assert!(
            matches!(
                &record.outcome,
                CrossModuleOutcome::Refused(r) if r.reason == CallRefusalReason::NotNamedByRoute
            ),
            "{language}: a call of an ended route wasn't refused: {:?}",
            record.outcome
        );
        assert_eq!(binding.operations_received(&callee), before + 1);
    }
}

// verifies: REQ-MDL-21, REQ-MDL-9
#[test]
fn declared_call_runs_on_the_callees_own_credentials() {
    for language in languages() {
        let mut binding = binding("reserve");
        let callee = format!("callee-{language}");
        let caller = format!("caller-{language}");
        // Hostile callee: it resolves its own key, and then asks for the caller's own secret by its name, which
        // only the caller declares and holds.
        install(
            &mut binding,
            &module(
                declaration(&callee, "1.0.0", &["withdrawal"]),
                vec![look_up("own.key"), look_up("caller.only")],
            ),
            &language,
            "reserve",
            &[],
        );
        let declared = calling(&caller, &[&callee], &["caller.only"]);
        install(
            &mut binding,
            &module(declared.clone(), vec![look_up("own.key"), call(&callee)]),
            &language,
            "reserve",
            &[],
        );
        // Both hold a secret of the same name, each its own value; only the caller holds `caller.only`.
        binding.seed_secret(&caller, "own.key", "caller-own-value");
        binding.seed_secret(&caller, "caller.only", "caller-only-value");
        binding.seed_secret(&callee, "own.key", "callee-own-value");
        let transaction = id(&format!("tx-{caller}-1"));
        let segment = route("decision-1", "segment-1");
        binding.routes.name(
            &transaction,
            named(
                segment.clone(),
                &caller,
                &callee,
                &[OperationKind::Withdrawal],
            ),
        );
        let before = binding.operations_received(&callee);
        let (handled, events) = run(&mut binding, &caller, 1);
        assert!(
            matches!(handled, Handled::Answered(_)),
            "{language}: the caller's operation wasn't answered: {handled:?}"
        );
        let record = the_call(&events, &declared, "the declared call");
        assert!(
            matches!(record.outcome, CrossModuleOutcome::Delivered(_)),
            "{language}: the declared call wasn't delivered: {:?}",
            record.outcome
        );
        assert_eq!(
            record.route, segment,
            "{language}: recorded on another route"
        );
        assert_eq!(binding.operations_received(&callee), before + 1);
        // Each was handed only its own value: the callee its own key, never the caller's of the same name, and
        // never the caller's other secret.
        assert_eq!(
            binding.delivered_secrets(&caller),
            vec!["caller-own-value".to_owned()],
            "{language}: the caller was handed another value"
        );
        assert_eq!(
            binding.delivered_secrets(&callee),
            vec!["callee-own-value".to_owned()],
            "{language}: the callee was handed another provider's value"
        );
        // The callee's lookups are its own, for the call's transaction: its key resolved, the caller's secret
        // refused as undeclared.
        let callee_lookups: Vec<_> = events
            .lookups
            .iter()
            .filter(|l| l.provider == callee)
            .collect();
        assert_eq!(callee_lookups.len(), 2, "{language}: {callee_lookups:?}");
        for lookup in &callee_lookups {
            assert_eq!(lookup.part, PartName::Outside);
            assert_eq!(lookup.lookup.transaction, transaction);
        }
        assert_eq!(callee_lookups[0].lookup.name.as_str(), "own.key");
        assert_eq!(callee_lookups[0].outcome, Some(LookupOutcome::Resolved));
        assert_eq!(callee_lookups[1].lookup.name.as_str(), "caller.only");
        assert_eq!(
            callee_lookups[1].outcome,
            Some(LookupOutcome::Refused(
                SecretRefusalReason::UndeclaredSecret
            )),
            "{language}: the callee resolved the caller's secret"
        );
        // And the caller's own lookups are the caller's.
        assert!(
            events
                .lookups
                .iter()
                .all(|l| l.provider == caller || l.provider == callee),
            "{language}: a lookup of a third provider"
        );
        binding.routes.end(&transaction);
    }
}
