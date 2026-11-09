//! Tests of cross-module calls on each module's own credentials (`T-014-04`, `REQ-MDL-21`, `REQ-MDL-9`,
//! `THR-86`): the call rule on its own ([`judge_call`]), checked against the contract's `validate_call_against`,
//! and the shared proxy serving a callee inside its caller's request. The platform's end-to-end tests are
//! `tests/proxy_calls.rs`.

use std::collections::BTreeMap;

use super::support::*;
use crate::proxy::{CallAsked, NamedCall, Refusal, RouteCalls, UseOf, judge_call, no_route};
use rutent_contracts::module_interface::{
    CallDeclaration, CallRefusalReason, CallRefused, CrossModuleCall, CrossModuleOutcome,
    Declaration, Nothing, OperationKind, RouteReference, validate_call, validate_call_against,
};
use rutent_contracts::provider_role::{Operation, Statement};
use serde_json::json;

const PROVIDERS: [&str; 4] = ["alpha", "beta", "gamma", "delta"];
const TRANSACTIONS: [&str; 3] = ["tx-1", "tx-2", "tx-3"];
const KINDS: [OperationKind; 8] = [
    OperationKind::Withdrawal,
    OperationKind::Deposit,
    OperationKind::Transfer,
    OperationKind::Conversion,
    OperationKind::BalanceVerification,
    OperationKind::Judgement,
    OperationKind::Reversal,
    OperationKind::ReversalConfirmation,
];

/// The provider-role fixture of an operation of `kind`, asked of `callee` for `transaction`.
fn operation(kind: OperationKind, callee: &str, transaction: &str) -> Operation {
    let name = match kind {
        OperationKind::Withdrawal => "operation-withdrawal-from-a-bank-reserve",
        OperationKind::Deposit => "operation-deposit-as-a-dry-run",
        OperationKind::Transfer => "operation-transfer-between-two-nodes",
        OperationKind::Conversion => "operation-conversion-as-a-dry-run",
        OperationKind::BalanceVerification => "operation-balance-verification",
        OperationKind::Judgement => "operation-judgement-question",
        OperationKind::Reversal => "operation-reversal-of-a-failed-transfer",
        OperationKind::ReversalConfirmation => "operation-reversal-confirmation-for-one-credit",
    };
    let mut value = fixture("provider-role", name);
    value["statement"]["provider"] = json!(callee);
    if kind != OperationKind::Judgement {
        value["statement"]["transaction"] = json!(transaction);
    }
    let statement: Statement<Operation> = serde_json::from_value(value).expect("an operation");
    assert_eq!(OperationKind::of(&statement.statement), kind);
    statement.statement
}

fn kinds(rng: &mut Rng) -> Vec<OperationKind> {
    let mut picked: Vec<OperationKind> = KINDS
        .iter()
        .copied()
        .filter(|_| rng.below(3) == 0)
        .collect();
    if picked.is_empty() {
        picked.push(*rng.pick(&KINDS));
    }
    picked
}

fn reference(n: u64) -> RouteReference {
    RouteReference {
        decision_record: id(&format!("decision-{}", n / 2)),
        segment: id(&format!("segment-{n}")),
    }
}

/// `caller`'s release, declaring calls to `calls`.
fn declaring(caller: &str, calls: Vec<CallDeclaration>) -> Declaration {
    let mut declaration = release(caller, "1.0.0", &[SECRET], KEY_B)
        .declaration()
        .clone();
    declaration.outside.calls = calls;
    declaration
}

// verifies: REQ-MDL-21, DEC-18
#[test]
fn a_call_is_allowed_exactly_when_declared_and_named_and_a_refusal_names_its_first_failing_check() {
    let mut allowed = 0;
    let mut by_reason: BTreeMap<String, usize> = BTreeMap::new();
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let caller = *rng.pick(&PROVIDERS);
        let mut calls = Vec::new();
        for callee in PROVIDERS {
            if callee != caller && rng.below(2) == 0 {
                calls.push(CallDeclaration {
                    callee: id(callee),
                    kinds: kinds(&mut rng),
                });
            }
        }
        let declaration = declaring(caller, calls);
        // What the routes name, kept as a model beside the platform's `RouteCalls`: named, then some ended.
        let mut routes = RouteCalls::default();
        let mut model: Vec<(String, NamedCall)> = Vec::new();
        for _ in 0..rng.below(8) {
            let transaction = *rng.pick(&TRANSACTIONS);
            // Most named calls are the caller's, so that named calls are common.
            let from = if rng.below(4) == 0 {
                *rng.pick(&PROVIDERS)
            } else {
                caller
            };
            let named = NamedCall {
                route: reference(rng.below(4)),
                caller: id(from),
                callee: id(rng.pick(&PROVIDERS)),
                kinds: kinds(&mut rng),
            };
            routes.name(&id(transaction), named.clone());
            model.push((transaction.to_owned(), named));
        }
        if rng.below(4) == 0 {
            let ended = *rng.pick(&TRANSACTIONS);
            routes.end(&id(ended));
            model.retain(|(t, _)| t != ended);
        }
        for _ in 0..6 {
            let transaction = *rng.pick(&TRANSACTIONS);
            let others: Vec<&str> = PROVIDERS.iter().copied().filter(|p| *p != caller).collect();
            let (mut callee, mut kind) = (*rng.pick(&others), *rng.pick(&KINDS));
            // Half the time, a call the caller declares, which the route mostly names, so that allowed calls are
            // common.
            if rng.below(2) == 0 && !declaration.outside.calls.is_empty() {
                let declared = rng.pick(&declaration.outside.calls).clone();
                callee = PROVIDERS
                    .iter()
                    .copied()
                    .find(|p| *p == declared.callee.as_str())
                    .expect("a provider");
                kind = *rng.pick(&declared.kinds);
                if rng.below(3) != 0 {
                    let named = NamedCall {
                        route: reference(rng.below(4)),
                        caller: id(caller),
                        callee: declared.callee.clone(),
                        kinds: vec![kind],
                    };
                    routes.name(&id(transaction), named.clone());
                    model.push((transaction.to_owned(), named));
                }
            }
            let claimed = match rng.below(6) {
                0 => None,
                1 => Some(*rng.pick(&KINDS)),
                _ => Some(kind),
            };
            let in_flight = rng.below(5) != 0;
            let asked = CallAsked {
                caller,
                transaction,
                callee,
                kind,
                claimed,
                in_flight,
            };
            let judged = judge_call(&declaration, &routes, &asked);

            // The model: the contract's order of checks.
            let naming = model
                .iter()
                .find(|(t, n)| {
                    t == transaction
                        && n.caller.as_str() == caller
                        && n.callee.as_str() == callee
                        && n.kinds.contains(&kind)
                })
                .map(|(_, n)| n.route.clone());
            let expected = match declaration
                .outside
                .calls
                .iter()
                .find(|c| c.callee.as_str() == callee)
            {
                None => Some(CallRefusalReason::UndeclaredCallee),
                Some(c) if !c.kinds.contains(&kind) || claimed != Some(kind) => {
                    Some(CallRefusalReason::UndeclaredKind)
                }
                Some(_) if !in_flight || naming.is_none() => {
                    Some(CallRefusalReason::NotNamedByRoute)
                }
                Some(_) => None,
            };
            assert_eq!(judged.refusal, expected, "seed {seed}: {asked:?}");
            match &naming {
                // A call a route names is recorded against that route, whatever else it fails.
                Some(route) => assert_eq!(&judged.route, route, "seed {seed}"),
                // Otherwise against the segment the caller is on in that transaction, or no route at all.
                None => assert!(
                    judged.route == no_route()
                        || model.iter().any(|(t, n)| t == transaction
                            && n.caller.as_str() == caller
                            && n.route == judged.route),
                    "seed {seed}: recorded on a route that isn't the caller's: {:?}",
                    judged.route
                ),
            }
            match expected {
                None => allowed += 1,
                Some(reason) => *by_reason.entry(format!("{reason:?}")).or_default() += 1,
            }

            // The record the platform writes from the judgement keeps to the contract.
            let record = CrossModuleCall {
                transaction: id(transaction),
                caller: id(caller),
                caller_version: declaration.version.clone(),
                route: judged.route.clone(),
                operation: operation(kind, callee, transaction),
                outcome: match judged.refusal {
                    Some(reason) => CrossModuleOutcome::Refused(CallRefused {
                        callee: id(callee),
                        kind,
                        reason,
                    }),
                    None => CrossModuleOutcome::Delivered(Nothing {}),
                },
            };
            assert_eq!(validate_call(&record), Ok(()), "seed {seed}: {record:?}");
            assert_eq!(
                validate_call_against(&declaration, &record),
                Ok(()),
                "seed {seed}: {record:?}"
            );
        }
    }
    // The generator reaches every outcome.
    assert!(allowed > 50, "only {allowed} calls allowed");
    for reason in ["UndeclaredCallee", "UndeclaredKind", "NotNamedByRoute"] {
        assert!(
            by_reason.get(reason).copied().unwrap_or(0) > 50,
            "too few {reason}: {by_reason:?}"
        );
    }
}

// verifies: REQ-MDL-21
#[test]
fn a_call_judged_against_another_providers_release_is_refused() {
    let routes = {
        let mut routes = RouteCalls::default();
        routes.name(
            &id("tx-1"),
            NamedCall {
                route: reference(1),
                caller: id("alpha"),
                callee: id("beta"),
                kinds: vec![OperationKind::Withdrawal],
            },
        );
        routes
    };
    let gamma = declaring(
        "gamma",
        vec![CallDeclaration {
            callee: id("beta"),
            kinds: vec![OperationKind::Withdrawal],
        }],
    );
    let asked = CallAsked {
        caller: "alpha",
        transaction: "tx-1",
        callee: "beta",
        kind: OperationKind::Withdrawal,
        claimed: Some(OperationKind::Withdrawal),
        in_flight: true,
    };
    // Gamma's release declares the call and the route names alpha's: neither makes alpha's call gamma's.
    assert_eq!(
        judge_call(&gamma, &routes, &asked).refusal,
        Some(CallRefusalReason::UndeclaredCallee)
    );
}

// verifies: REQ-MDL-21, REQ-MDL-9
#[test]
fn a_callee_resolves_only_its_own_secrets_while_its_callers_request_stays_in_flight() {
    let (mut proxy, stores) = proxy();
    let internal = &stores["internal"];
    internal.write(&secret_path("internal", "alpha", SECRET), "alpha-value");
    internal.write(
        &secret_path("internal", "alpha", "alpha-only"),
        "alpha-only",
    );
    internal.write(&secret_path("internal", "beta", SECRET), "beta-value");
    proxy
        .admit(
            &release("alpha", "1.0.0", &[SECRET, "alpha-only"], KEY_B),
            &advertisement("alpha", &[(KEY_A, "advertising"), (KEY_B, "statements")]),
        )
        .expect("alpha is admitted");
    admit(&mut proxy, "beta");

    // Alpha's outside part is serving a request when it calls beta: beta's module handles the call's operation, for
    // the same transaction, and serves a request of its own.
    in_flight(&mut proxy, "alpha", "tx-1");
    in_flight(&mut proxy, "beta", "tx-1");
    let seen_before = internal.seen().len();
    // The same name is each caller's own secret.
    assert_eq!(
        proxy
            .lookup(&caller("beta"), &lookup("tx-1", OPERATION, SECRET))
            .map(|v| v.as_str().to_owned()),
        Ok("beta-value".to_owned())
    );
    // Alpha's secret, by its name, is nothing beta declares, so the store isn't asked.
    assert_eq!(
        proxy.lookup(&caller("beta"), &lookup("tx-1", OPERATION, "alpha-only")),
        Err(Refusal::UndeclaredSecret)
    );
    assert_eq!(
        internal.seen()[seen_before..],
        [secret_path("internal", "beta", SECRET)],
        "beta's lookups reached another provider's path"
    );
    // The callee answered: its operation ends, and alpha's request is still in flight, on alpha's own secrets.
    proxy.end_request(&caller("beta"));
    proxy.end_operation(&caller("beta"));
    assert_eq!(
        proxy.lookup(&caller("beta"), &lookup("tx-1", OPERATION, SECRET)),
        Err(Refusal::NoRequestInFlight)
    );
    assert_eq!(
        proxy
            .lookup(&caller("alpha"), &lookup("tx-1", OPERATION, SECRET))
            .map(|v| v.as_str().to_owned()),
        Ok("alpha-value".to_owned())
    );
    // Every use is recorded with its own provider, and the call's transaction.
    let uses = proxy.take_uses();
    let by: Vec<(&str, &str, bool)> = uses
        .iter()
        .map(|u| {
            let UseOf::Secret(name) = &u.of else {
                panic!("a statement key use")
            };
            assert_eq!(u.transaction.as_str(), "tx-1");
            (u.provider.as_str(), name.as_str(), u.outcome.is_ok())
        })
        .collect();
    assert_eq!(
        by,
        [
            ("beta", SECRET, true),
            ("beta", "alpha-only", false),
            ("beta", SECRET, false),
            ("alpha", SECRET, true),
        ]
    );
}
