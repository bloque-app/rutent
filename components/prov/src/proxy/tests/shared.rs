//! Tests of the shared credential proxy (`T-014-02`, `DEC-9`, ADR-0004): it answers each lookup only for the
//! calling process's own provider, only for the transaction and operation of the outside request in flight for
//! that provider's module, only a secret the admitted release declares for that operation, and the statement key
//! only as the release declares it, never a key listed for `advertising`. Every use is recorded with its
//! transaction, and each provider's lookups reach only its process role's bucket.
//!
//! The stores are fakes, one per role (`support`). The resolvers' own behaviour against OpenBao and Vault is
//! `tests/proxy.rs`'s.

use super::support::*;
use crate::proxy::{
    InFlight, ProxySettings, Refusal, ResolveError, Token, UseOf, answer, judge, kv_bucket,
};
use rutent_contracts::module_interface::{SecretOutcome, SecretRefusalReason};
use serde_json::json;

// verifies: REQ-MDL-9, DEC-9
#[test]
fn lookup_for_another_providers_secret_is_refused() {
    let (mut proxy, stores) = proxy();
    let internal = &stores["internal"];
    internal.write(&secret_path("internal", "beta", SECRET), "beta-canary");
    admit(&mut proxy, "alpha");
    admit(&mut proxy, "beta");
    in_flight(&mut proxy, "alpha", "tx-alpha");
    in_flight(&mut proxy, "beta", "tx-beta");

    // Alpha declares the same name; the lookup names no provider, so it is alpha's own, which it doesn't hold.
    assert_eq!(
        proxy.lookup(&caller("alpha"), &lookup("tx-alpha", OPERATION, SECRET)),
        Err(Refusal::NotFound)
    );
    // Claiming beta's transaction in flight gets alpha nothing: no request of alpha's is for it.
    assert_eq!(
        proxy.lookup(&caller("alpha"), &lookup("tx-beta", OPERATION, SECRET)),
        Err(Refusal::NotTheRequestInFlight)
    );
    // A process with no role in the deployment, and one with no admitted release, get nothing either.
    assert_eq!(
        proxy.lookup(&caller("delta"), &lookup("tx-beta", OPERATION, SECRET)),
        Err(Refusal::NoRole)
    );
    assert_eq!(
        proxy.lookup(&caller("gamma"), &lookup("tx-beta", OPERATION, SECRET)),
        Err(Refusal::NoRelease)
    );
    assert!(
        all_seen(&stores).iter().all(|p| !p.contains("/beta/")),
        "a lookup reached beta's keys: {:?}",
        all_seen(&stores)
    );

    // The control: beta's own lookup resolves.
    assert_eq!(
        proxy
            .lookup(&caller("beta"), &lookup("tx-beta", OPERATION, SECRET))
            .unwrap()
            .as_str(),
        "beta-canary"
    );
}

// verifies: REQ-MDL-6
#[test]
fn lookup_for_an_operation_with_no_request_in_flight_is_refused() {
    let (mut proxy, stores) = proxy();
    stores["internal"].write(&secret_path("internal", "alpha", SECRET), "alpha-value");
    admit(&mut proxy, "alpha");
    let (alpha, wanted) = (caller("alpha"), lookup("tx-1", OPERATION, SECRET));

    // Nothing handled yet.
    assert_eq!(
        proxy.lookup(&alpha, &wanted),
        Err(Refusal::NoRequestInFlight)
    );
    // An operation handled, but no outside request in flight.
    proxy.begin_operation(&alpha, &id("tx-1")).unwrap();
    assert_eq!(
        proxy.lookup(&alpha, &wanted),
        Err(Refusal::NoRequestInFlight)
    );
    // An outside request for an operation the release doesn't declare isn't put in flight.
    assert_eq!(
        proxy.begin_request(&alpha, &id("tx-1"), &name("other.call")),
        Err(Refusal::UndeclaredOperation)
    );
    // Nor one for another transaction than the one handled.
    assert_eq!(
        proxy.begin_request(&alpha, &id("tx-2"), &name(OPERATION)),
        Err(Refusal::NotTheRequestInFlight)
    );
    // In flight: a lookup naming another operation or transaction is refused.
    proxy
        .begin_request(&alpha, &id("tx-1"), &name(OPERATION))
        .unwrap();
    assert_eq!(
        proxy.lookup(&alpha, &lookup("tx-1", "other.call", SECRET)),
        Err(Refusal::NotTheRequestInFlight)
    );
    assert_eq!(
        proxy.lookup(&alpha, &lookup("tx-2", OPERATION, SECRET)),
        Err(Refusal::NotTheRequestInFlight)
    );
    assert!(
        stores["internal"].seen().is_empty(),
        "a refused lookup reached the store"
    );

    // The control: the lookup for the request in flight resolves.
    assert_eq!(
        proxy.lookup(&alpha, &wanted).unwrap().as_str(),
        "alpha-value"
    );

    // Once the request is answered, nothing is in flight again.
    proxy.end_request(&alpha);
    assert_eq!(
        proxy.lookup(&alpha, &wanted),
        Err(Refusal::NoRequestInFlight)
    );
    proxy.end_operation(&alpha);
    assert_eq!(
        proxy.begin_request(&alpha, &id("tx-1"), &name(OPERATION)),
        Err(Refusal::NoRequestInFlight)
    );
    assert_eq!(stores["internal"].seen().len(), 1);
}

// verifies: REQ-CAT-23, REQ-MDL-1, THR-4
#[test]
fn operators_own_release_is_admitted_with_no_advertisement_and_never_resolves_the_operator_key() {
    let (mut proxy, stores) = proxy();
    let internal = &stores["internal"];
    for key in [KEY_B, OPERATOR_KEY] {
        internal.write(&key_path("internal", "alpha", key), "private-half");
    }
    let alpha = caller("alpha");
    let operator_key =
        rutent_contracts::provider_role::PublicKey::new(OPERATOR_KEY).expect("a key");
    let operators = |statement_key: &str| {
        let mut artefact = release("alpha", "1.0.0", &[], statement_key);
        artefact.assertion.signature.key = operator_key.clone();
        artefact
    };

    // Not the operator's: signed with a provider's key, or of another deployment. Nor accepted yet.
    let mut elsewhere = operators(KEY_B);
    elsewhere.assertion.deployment.name = "rutent-elsewhere".into();
    for (artefact, refusal) in [
        (
            release("alpha", "1.0.0", &[], KEY_B),
            Refusal::NotTheOperatorsRelease,
        ),
        (elsewhere, Refusal::NotTheOperatorsRelease),
        (unaccepted(operators(KEY_B)), Refusal::NotAccepted),
    ] {
        assert_eq!(proxy.admit_operator(&artefact), Err(refusal));
    }
    assert_eq!(
        proxy.begin_operation(&alpha, &id("tx-1")),
        Err(Refusal::NoRelease)
    );

    // The operator's release declaring the operator key as its statement key: it is never resolved.
    proxy.admit_operator(&operators(OPERATOR_KEY)).unwrap();
    proxy.begin_operation(&alpha, &id("tx-1")).unwrap();
    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-1")),
        Err(Refusal::StatementKeyIsOperatorKey)
    );
    assert!(
        internal.seen().is_empty(),
        "a refused key reached the store"
    );

    // The control: the key it declares resolves, with no advertisement listing it.
    proxy.admit_operator(&operators(KEY_B)).unwrap();
    proxy.begin_operation(&alpha, &id("tx-2")).unwrap();
    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-2")).unwrap().as_str(),
        "private-half"
    );
    // An advertisement naming the operator's module is refused and changes nothing: no advertisement vouches for
    // its keys.
    assert_eq!(
        proxy.advertise(&advertisement("alpha", &[(KEY_B, "advertising")])),
        Err(Refusal::AdvertisementOfAnOperatorsModule)
    );
    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-2")).unwrap().as_str(),
        "private-half"
    );
}

// verifies: REQ-MDL-6, THR-4
#[test]
fn advertising_key_is_never_resolved() {
    let (mut proxy, stores) = proxy();
    let internal = &stores["internal"];
    for key in [KEY_A, KEY_B, KEY_C, OPERATOR_KEY] {
        internal.write(&key_path("internal", "alpha", key), "private-half");
        internal.write(
            &secret_path("internal", "alpha", key),
            "a-secret-by-that-name",
        );
    }
    let alpha = caller("alpha");
    let advertised = [
        (KEY_A, "advertising"),
        (KEY_B, "statements"),
        (OPERATOR_KEY, "statements"),
    ];

    // A release declaring the advertising key as its statement key: the key is never resolved.
    proxy
        .admit(
            &release("alpha", "1.0.0", &[SECRET], KEY_A),
            &advertisement("alpha", &advertised),
        )
        .unwrap();
    proxy.begin_operation(&alpha, &id("tx-1")).unwrap();
    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-1")),
        Err(Refusal::StatementKeyForAdvertising)
    );
    // Nor one the advertisement doesn't list, nor the deployment's operator key.
    for (key, refusal) in [
        (KEY_C, Refusal::StatementKeyNotAdvertised),
        (OPERATOR_KEY, Refusal::StatementKeyIsOperatorKey),
    ] {
        proxy
            .admit(
                &release("alpha", "1.0.1", &[SECRET], key),
                &advertisement("alpha", &advertised),
            )
            .unwrap();
        proxy.begin_operation(&alpha, &id("tx-1")).unwrap();
        assert_eq!(proxy.statement_key(&alpha, &id("tx-1")), Err(refusal));
    }
    assert!(
        internal.seen().is_empty(),
        "a refused key reached the store"
    );

    // A declared secret named like the advertising key is read from the secrets, never from the keys.
    proxy
        .admit(
            &release("alpha", "1.0.2", &[KEY_A], KEY_B),
            &advertisement("alpha", &advertised),
        )
        .unwrap();
    in_flight(&mut proxy, "alpha", "tx-2");
    assert_eq!(
        proxy
            .lookup(&alpha, &lookup("tx-2", OPERATION, KEY_A))
            .unwrap()
            .as_str(),
        "a-secret-by-that-name"
    );

    // The control: the declared key, listed for statements, resolves from the keys.
    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-2")).unwrap().as_str(),
        "private-half"
    );
    assert_eq!(
        internal.seen(),
        vec![
            secret_path("internal", "alpha", KEY_A),
            key_path("internal", "alpha", KEY_B)
        ]
    );
}

// verifies: REQ-MDL-6
#[test]
fn every_use_is_recorded_with_its_transaction() {
    let (mut proxy, stores) = proxy();
    stores["internal"].write(&secret_path("internal", "alpha", SECRET), "alpha-value");
    stores["internal"].write(&key_path("internal", "alpha", KEY_B), "alpha-private");
    admit(&mut proxy, "alpha");
    let alpha = caller("alpha");

    // Refused: no request in flight.
    let _ = proxy.lookup(&alpha, &lookup("tx-0", OPERATION, SECRET));
    in_flight(&mut proxy, "alpha", "tx-1");
    let _ = proxy.lookup(&alpha, &lookup("tx-1", OPERATION, SECRET));
    let _ = proxy.lookup(&alpha, &lookup("tx-1", OPERATION, "undeclared"));
    let _ = proxy.statement_key(&alpha, &id("tx-1"));

    let uses = proxy.take_uses();
    let seen: Vec<_> = uses
        .iter()
        .map(|u| {
            (
                u.provider.as_str(),
                u.module_version.as_ref().map(|v| v.as_str()),
                u.transaction.as_str(),
                u.operation.as_ref().map(|o| o.as_str()),
                u.of.clone(),
                u.outcome.clone(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            (
                "alpha",
                Some("1.0.0"),
                "tx-0",
                Some(OPERATION),
                UseOf::Secret(name(SECRET)),
                Err(Refusal::NoRequestInFlight)
            ),
            (
                "alpha",
                Some("1.0.0"),
                "tx-1",
                Some(OPERATION),
                UseOf::Secret(name(SECRET)),
                Ok(())
            ),
            (
                "alpha",
                Some("1.0.0"),
                "tx-1",
                Some(OPERATION),
                UseOf::Secret(name("undeclared")),
                Err(Refusal::UndeclaredSecret)
            ),
            (
                "alpha",
                Some("1.0.0"),
                "tx-1",
                None,
                UseOf::StatementKey(Some(
                    rutent_contracts::provider_role::PublicKey::new(KEY_B).unwrap()
                )),
                Ok(())
            ),
        ]
    );
    let printed = format!("{uses:?}");
    assert!(
        !printed.contains("alpha-value") && !printed.contains("alpha-private"),
        "a record prints a value: {printed}"
    );
    assert!(proxy.take_uses().is_empty(), "records are taken once");
}

// verifies: DEC-9
#[test]
fn each_providers_lookups_reach_only_its_process_roles_bucket() {
    let (mut proxy, stores) = proxy();
    stores["internal"].write(&secret_path("internal", "alpha", SECRET), "internal-value");
    stores["integrations"].write(
        &secret_path("integrations", "gamma", SECRET),
        "integrations-value",
    );
    // The same name in the other role's bucket is never read.
    stores["integrations"].write(&secret_path("integrations", "alpha", SECRET), "wrong");
    stores["internal"].write(&secret_path("internal", "gamma", SECRET), "wrong");
    for provider in ["alpha", "gamma"] {
        admit(&mut proxy, provider);
        in_flight(&mut proxy, provider, "tx-1");
    }

    let alpha = proxy
        .lookup(&caller("alpha"), &lookup("tx-1", OPERATION, SECRET))
        .unwrap();
    let gamma = proxy
        .lookup(&caller("gamma"), &lookup("tx-1", OPERATION, SECRET))
        .unwrap();

    assert_eq!(
        (alpha.as_str(), gamma.as_str()),
        ("internal-value", "integrations-value")
    );
    assert_eq!(
        stores["internal"].seen(),
        vec![secret_path("internal", "alpha", SECRET)]
    );
    assert_eq!(
        stores["integrations"].seen(),
        vec![secret_path("integrations", "gamma", SECRET)]
    );
}

// verifies: DEC-9, REQ-MDL-6
#[test]
fn statement_key_is_resolved_only_while_its_transaction_is_handled() {
    let (mut proxy, stores) = proxy();
    stores["internal"].write(&key_path("internal", "alpha", KEY_B), "alpha-private");
    admit(&mut proxy, "alpha");
    let alpha = caller("alpha");

    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-1")),
        Err(Refusal::NoRequestInFlight)
    );
    proxy.begin_operation(&alpha, &id("tx-1")).unwrap();
    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-2")),
        Err(Refusal::NotTheRequestInFlight)
    );
    assert_eq!(
        proxy.statement_key(&caller("beta"), &id("tx-1")),
        Err(Refusal::NoRelease)
    );
    assert!(stores["internal"].seen().is_empty());
    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-1")).unwrap().as_str(),
        "alpha-private"
    );
    proxy.end_operation(&alpha);
    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-1")),
        Err(Refusal::NoRequestInFlight)
    );
}

// verifies: REQ-MDL-6, DEC-9
#[test]
fn release_not_accepted_or_not_the_callers_is_not_admitted() {
    let (mut proxy, _) = proxy();
    let keys = [(KEY_A, "advertising"), (KEY_B, "statements")];

    assert_eq!(
        proxy.admit(
            &unaccepted(release("alpha", "1.0.0", &[SECRET], KEY_B)),
            &advertisement("alpha", &keys)
        ),
        Err(Refusal::NotAccepted)
    );
    assert_eq!(
        proxy.admit(
            &release("alpha", "1.0.0", &[SECRET], KEY_B),
            &advertisement("beta", &keys)
        ),
        Err(Refusal::AdvertisementOfAnotherProvider)
    );
    assert_eq!(
        proxy.admit(
            &release("delta", "1.0.0", &[SECRET], KEY_B),
            &advertisement("delta", &keys)
        ),
        Err(Refusal::NoRole)
    );
    // Nothing was admitted, so nothing can start.
    for provider in ["alpha", "delta"] {
        assert_eq!(
            proxy.begin_operation(&caller(provider), &id("tx-1")),
            Err(Refusal::NoRelease)
        );
    }
}

// verifies: DEC-9, REQ-MDL-9
#[test]
fn release_declaring_a_name_the_store_cannot_address_is_not_admitted() {
    let (mut proxy, _) = proxy();
    let keys = [(KEY_A, "advertising"), (KEY_B, "statements")];
    let longest = "a".repeat(255);
    let too_long = "a".repeat(256);

    for unaddressable in [
        too_long.as_str(),
        "../beta/api-key",
        "a/b",
        "..",
        "with space",
    ] {
        assert_eq!(
            proxy.admit(
                &release("alpha", "1.0.0", &[unaddressable], KEY_B),
                &advertisement("alpha", &keys)
            ),
            Err(Refusal::UnaddressableName),
            "{unaddressable:?}"
        );
    }
    proxy
        .admit(
            &release("alpha", "1.0.0", &[longest.as_str()], KEY_B),
            &advertisement("alpha", &keys),
        )
        .expect("255 bytes is addressable");
}

// verifies: REQ-MDL-6, REQ-MDL-9
#[test]
fn a_new_admission_replaces_the_running_release() {
    let (mut proxy, stores) = proxy();
    stores["internal"].write(&secret_path("internal", "alpha", SECRET), "alpha-value");
    admit(&mut proxy, "alpha");
    proxy
        .admit(
            &release("alpha", "1.0.1", &["other"], KEY_B),
            &advertisement("alpha", &[(KEY_A, "advertising"), (KEY_B, "statements")]),
        )
        .unwrap();
    in_flight(&mut proxy, "alpha", "tx-1");

    assert_eq!(
        proxy.lookup(&caller("alpha"), &lookup("tx-1", OPERATION, SECRET)),
        Err(Refusal::UndeclaredSecret)
    );
    assert!(stores["internal"].seen().is_empty());
    assert_eq!(
        proxy.take_uses()[0]
            .module_version
            .as_ref()
            .map(|v| v.as_str()),
        Some("1.0.1")
    );
}

// verifies: DEC-4, REQ-MDL-6
#[test]
fn secret_store_that_gave_no_answer_is_not_reported_as_not_found() {
    let (mut proxy, stores) = proxy();
    let internal = &stores["internal"];
    internal.write(&secret_path("internal", "alpha", SECRET), "alpha-value");
    admit(&mut proxy, "alpha");
    in_flight(&mut proxy, "alpha", "tx-alpha");
    let wanted = lookup("tx-alpha", OPERATION, SECRET);

    // Sealed, overloaded, refusing the token, or answering what the resolver doesn't expect: the secret is there,
    // and the store didn't say so.
    for status in [503, 429, 403, 418] {
        internal.fail_with(Some(status));
        let result = proxy.lookup(&caller("alpha"), &wanted);
        assert!(
            matches!(result, Err(Refusal::Store(_))),
            "{status}: {result:?}"
        );
        let answered = answer(&wanted, &result);
        let refused = serde_json::to_value(&answered).unwrap()["outcome"].clone();
        assert_eq!(refused["status"], "refused", "{status}");
        assert_ne!(refused["reason"], "not_found", "{status}");
        assert_eq!(refused["reason"], "store_unavailable", "{status}");
    }

    // The control: the same lookup resolves once the store answers.
    internal.fail_with(None);
    assert_eq!(
        proxy.lookup(&caller("alpha"), &wanted).unwrap().as_str(),
        "alpha-value"
    );
}

// verifies: REQ-MDL-6, DEC-9, DEC-4
#[test]
fn refusals_map_onto_the_contracts_three_reasons() {
    let wanted = lookup("tx-1", OPERATION, SECRET);
    let undeclared = [
        Refusal::NoRole,
        Refusal::NoRelease,
        Refusal::NoRequestInFlight,
        Refusal::RequestOfAnotherProvider,
        Refusal::NotTheRequestInFlight,
        Refusal::UndeclaredSecret,
        Refusal::UndeclaredOperation,
    ];
    for refusal in undeclared {
        assert_eq!(
            refusal.wire_reason(),
            SecretRefusalReason::UndeclaredSecret,
            "{refusal:?}"
        );
    }
    assert_eq!(
        Refusal::NotFound.wire_reason(),
        SecretRefusalReason::NotFound
    );
    // Every store failure is one reason on the wire, so the module learns nothing about the store.
    for failed in [
        ResolveError::InvalidName,
        ResolveError::Denied,
        ResolveError::Unavailable { status: 503 },
        ResolveError::Unexpected { status: 418 },
        ResolveError::Malformed,
    ] {
        assert_eq!(
            Refusal::Store(failed.clone()).wire_reason(),
            SecretRefusalReason::StoreUnavailable,
            "{failed:?}"
        );
    }
    assert!(matches!(
        answer(&wanted, &Err(Refusal::Store(ResolveError::Denied))).outcome,
        SecretOutcome::Refused(r) if r.reason == SecretRefusalReason::StoreUnavailable
    ));

    let refused = answer(&wanted, &Err(Refusal::NotTheRequestInFlight));
    assert_eq!(
        (refused.transaction, refused.operation, refused.name),
        (
            wanted.transaction.clone(),
            wanted.operation.clone(),
            wanted.name.clone()
        )
    );
    assert!(matches!(
        refused.outcome,
        SecretOutcome::Refused(r) if r.reason == SecretRefusalReason::UndeclaredSecret
    ));
    let value = rutent_contracts::module_interface::SecretValue::new("v").unwrap();
    assert!(matches!(
        answer(&wanted, &Ok(value)).outcome,
        SecretOutcome::Resolved(_)
    ));
}

// verifies: DEC-9, REQ-MDL-9
#[test]
fn judgement_refuses_a_request_in_flight_for_another_provider() {
    let declaration = release("alpha", "1.0.0", &[SECRET], KEY_B)
        .declaration()
        .clone();
    let wanted = lookup("tx-1", OPERATION, SECRET);
    let flight = |provider: &str, transaction: &str, operation: &str| InFlight {
        provider: id(provider),
        transaction: id(transaction),
        operation: name(operation),
    };

    assert_eq!(
        judge(&caller("alpha"), &declaration, None, &wanted),
        Err(Refusal::NoRequestInFlight)
    );
    assert_eq!(
        judge(
            &caller("alpha"),
            &declaration,
            Some(&flight("beta", "tx-1", OPERATION)),
            &wanted
        ),
        Err(Refusal::RequestOfAnotherProvider)
    );
    assert_eq!(
        judge(
            &caller("beta"),
            &declaration,
            Some(&flight("beta", "tx-1", OPERATION)),
            &wanted
        ),
        Err(Refusal::ReleaseOfAnotherProvider)
    );
    assert_eq!(
        judge(
            &caller("alpha"),
            &declaration,
            Some(&flight("alpha", "tx-1", OPERATION)),
            &lookup("tx-1", OPERATION, "undeclared")
        ),
        Err(Refusal::UndeclaredSecret)
    );
    assert_eq!(
        judge(
            &caller("alpha"),
            &declaration,
            Some(&flight("alpha", "tx-1", OPERATION)),
            &wanted
        ),
        Ok(())
    );
}

// verifies: DEC-9
#[test]
fn settings_whose_buckets_nest_or_name_no_role_are_refused() {
    let role = |secrets: &str, keys: &str| {
        json!({
            "server": "openbao",
            "store": { "scheme": "https", "host": "bao.internal", "port": 8200 },
            "mount": "secret",
            "secrets": secrets,
            "keys": keys
        })
    };
    let settings = |roles: serde_json::Value, providers: serde_json::Value| {
        ProxySettings::from_json(
            &json!({ "deployment": deployment(), "roles": roles, "providers": providers })
                .to_string(),
        )
    };

    // The control.
    settings(
        json!({ "internal": role("r/internal/s", "r/internal/k"), "integrations": role("r/integrations/s", "r/integrations/k") }),
        json!({ "alpha": "internal" }),
    )
    .expect("disjoint buckets are valid");

    for (roles, providers) in [
        // A role's keys inside its own secrets.
        (
            json!({ "internal": role("r/internal", "r/internal/keys") }),
            json!({}),
        ),
        // Two roles' secrets sharing a prefix.
        (
            json!({ "internal": role("r/s", "r/internal/k"), "integrations": role("r/s", "r/integrations/k") }),
            json!({}),
        ),
        // One role's bucket inside another's.
        (
            json!({ "internal": role("r", "k/internal"), "integrations": role("r/integrations", "k/integrations") }),
            json!({}),
        ),
        // A provider given a role the settings don't define.
        (
            json!({ "internal": role("r/s", "r/k") }),
            json!({ "alpha": "external" }),
        ),
        // A provider, a role or a prefix that isn't plain path segments.
        (
            json!({ "internal": role("r/s", "r/k") }),
            json!({ "../alpha": "internal" }),
        ),
        (json!({ "in/ternal": role("r/s", "r/k") }), json!({})),
        (json!({ "internal": role("r/../s", "r/k") }), json!({})),
    ] {
        assert!(
            settings(roles.clone(), providers.clone()).is_err(),
            "accepted: {roles} {providers}"
        );
    }

    // Buckets that alias one store path across a mount, a namespace or a host's case.
    let on = |mount: &str, namespace: Option<&str>, host: &str, secrets: &str, keys: &str| {
        let mut role = role(secrets, keys);
        role["mount"] = json!(mount);
        role["store"]["host"] = json!(host);
        if let Some(namespace) = namespace {
            role["namespace"] = json!(namespace);
        }
        role
    };
    for (internal, integrations) in [
        // `secret` + `data/x` and `secret/data` + `x` both read `secret/data/data/x`.
        (
            on("secret", None, "bao.internal", "data/x", "k/internal"),
            on("secret/data", None, "bao.internal", "x", "k/integrations"),
        ),
        // Namespace `ns1` with mount `secret`, and mount `ns1/secret` with none, are one path.
        (
            on("secret", Some("ns1"), "bao.internal", "r/s", "r/internal/k"),
            on(
                "ns1/secret",
                None,
                "bao.internal",
                "r/s",
                "r/integrations/k",
            ),
        ),
        // A host name in another case is the same store.
        (
            on("secret", None, "bao.internal", "r/s", "r/internal/k"),
            on("secret", None, "BAO.Internal", "r/s", "r/integrations/k"),
        ),
        // So is a host name with its root's trailing dot.
        (
            on("secret", None, "bao.internal", "r/s", "r/internal/k"),
            on("secret", None, "bao.internal.", "r/s", "r/integrations/k"),
        ),
    ] {
        assert!(
            settings(
                json!({ "internal": internal.clone(), "integrations": integrations.clone() }),
                json!({})
            )
            .is_err(),
            "accepted aliasing buckets: {internal} {integrations}"
        );
    }

    // The same prefixes on another mount, or in another namespace, don't nest.
    let mut other_mount = role("r/s", "r/k2");
    other_mount["mount"] = json!("kv-integrations");
    settings(
        json!({ "internal": role("r/s", "r/k"), "integrations": other_mount }),
        json!({}),
    )
    .expect("another mount is another bucket");
}

// verifies: DEC-9
#[test]
fn proxy_over_settings_that_were_never_checked_is_refused() {
    // Settings made without `from_json`, whose role's keys sit inside its secrets.
    let nesting: ProxySettings = serde_json::from_value(json!({
        "deployment": deployment(),
        "roles": { "internal": {
            "server": "openbao",
            "store": { "scheme": "https", "host": "bao.internal", "port": 8200 },
            "mount": "secret",
            "secrets": "r/internal",
            "keys": "r/internal/keys"
        } },
        "providers": { "alpha": "internal" }
    }))
    .unwrap();
    let mut built = 0;

    let proxy = Proxy::new(&nesting, |role, settings| {
        built += 1;
        kv_bucket(
            settings,
            Token::new(format!("{role}-token")).unwrap(),
            std::sync::Arc::new(Store::default()),
        )
    });

    assert!(proxy.is_err(), "a proxy was built over nesting buckets");
    assert_eq!(
        built, 0,
        "a bucket was built before the settings were checked"
    );
}

// verifies: REQ-MDL-6, THR-4
#[test]
fn statement_key_listed_twice_once_for_advertising_is_never_resolved() {
    let (mut proxy, stores) = proxy();
    stores["internal"].write(&key_path("internal", "alpha", KEY_B), "private-half");
    let alpha = caller("alpha");
    proxy
        .admit(
            &release("alpha", "1.0.0", &[SECRET], KEY_B),
            &advertisement(
                "alpha",
                &[
                    (KEY_A, "advertising"),
                    (KEY_B, "statements"),
                    (KEY_B, "advertising"),
                ],
            ),
        )
        .unwrap();
    proxy.begin_operation(&alpha, &id("tx-1")).unwrap();

    assert_eq!(
        proxy.statement_key(&alpha, &id("tx-1")),
        Err(Refusal::StatementKeyForAdvertising)
    );
    assert!(stores["internal"].seen().is_empty());
}

// verifies: REQ-MDL-6, THR-4, DEC-9
#[test]
fn advertisement_of_another_deployment_is_not_admitted() {
    let (mut proxy, _) = proxy();
    let keys = [(KEY_A, "advertising"), (KEY_B, "statements")];
    // An advertisement naming another operator key: the proxy's operator key is the deployment's, from its
    // settings, so this one can't move it.
    let mut elsewhere = advertisement("alpha", &keys);
    elsewhere.deployment.operator_key =
        rutent_contracts::provider_role::PublicKey::new(KEY_C).unwrap();
    let mut renamed = advertisement("alpha", &keys);
    renamed.deployment.name = "another-deployment".to_owned();

    for advertisement in [elsewhere, renamed] {
        assert_eq!(
            proxy.admit(&release("alpha", "1.0.0", &[SECRET], KEY_C), &advertisement),
            Err(Refusal::AdvertisementOfAnotherDeployment)
        );
    }
}
