//! Property tests of the shared credential proxy's invariants (`DEC-18`, `T-014-02`): seeded, so a failure names
//! the seed that replays it.

use std::collections::BTreeMap;

use super::support::*;
use crate::proxy::Refusal;

const PROVIDERS: [&str; 4] = ["alpha", "beta", "gamma", "delta"];
const TRANSACTIONS: [&str; 3] = ["tx-1", "tx-2", "tx-3"];
const OPERATIONS: [&str; 2] = [OPERATION, "other.call"];
const NAMES: [&str; 2] = [SECRET, "other-secret"];

fn role_of(provider: &str) -> Option<&'static str> {
    match provider {
        "alpha" | "beta" => Some("internal"),
        "gamma" => Some("integrations"),
        _ => None,
    }
}

/// A proxy with `alpha`, `beta` and `gamma` admitted, each holding its own [`SECRET`] in its role's store, and
/// `delta`, which has no role, holding one in every store.
fn seeded() -> (Proxy, BTreeMap<String, std::sync::Arc<Store>>) {
    let (mut proxy, stores) = proxy();
    for provider in PROVIDERS {
        match role_of(provider) {
            Some(role) => {
                stores[role].write(
                    &secret_path(role, provider, SECRET),
                    &format!("{provider}-value"),
                );
                admit(&mut proxy, provider);
            }
            None => {
                for role in ROLES {
                    stores[role].write(&secret_path(role, provider, SECRET), "delta-value");
                }
            }
        }
    }
    (proxy, stores)
}

// verifies: DEC-9, REQ-MDL-9, REQ-MDL-6
#[test]
fn a_lookup_resolves_only_the_callers_own_declared_secret_for_its_request_in_flight() {
    let mut resolved = 0;
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let (mut proxy, stores) = seeded();
        // What the platform has in flight per provider: the transaction handled, and the operation served.
        let mut handling: BTreeMap<&str, (&str, Option<&str>)> = BTreeMap::new();
        for step in 0..80 {
            let provider = *rng.pick(&PROVIDERS);
            let (mut transaction, mut operation) =
                (*rng.pick(&TRANSACTIONS), *rng.pick(&OPERATIONS));
            // Half the time, name what the provider has in flight, so lookups that should resolve are common.
            if rng.below(2) == 0
                && let Some((t, serving)) = handling.get(provider)
            {
                transaction = t;
                operation = serving.unwrap_or(OPERATION);
            }
            let admitted = role_of(provider).is_some();
            let who = caller(provider);
            // Weighted: starts and lookups often, ends seldom, so requests stay in flight long enough to use.
            let action = match rng.below(20) {
                0..=2 => 0,
                3..=7 => 1,
                8 => 2,
                9 => 3,
                _ => 4,
            };
            match action {
                0 => {
                    let started = proxy.begin_operation(&who, &id(transaction));
                    assert_eq!(started.is_ok(), admitted, "seed {seed} step {step}");
                    if admitted {
                        handling.insert(provider, (transaction, None));
                    }
                }
                1 => {
                    let expected = admitted
                        && handling
                            .get(provider)
                            .is_some_and(|(t, _)| *t == transaction)
                        && operation == OPERATION;
                    let started = proxy.begin_request(&who, &id(transaction), &name(operation));
                    assert_eq!(started.is_ok(), expected, "seed {seed} step {step}");
                    if expected {
                        handling.insert(provider, (transaction, Some(operation)));
                    }
                }
                2 => {
                    proxy.end_request(&who);
                    if let Some((_, serving)) = handling.get_mut(provider) {
                        *serving = None;
                    }
                }
                3 => {
                    proxy.end_operation(&who);
                    handling.remove(provider);
                }
                _ => {
                    let secret = *rng.pick(&NAMES);
                    let before = all_seen(&stores).len();
                    let expected = admitted
                        && handling.get(provider) == Some(&(transaction, Some(operation)))
                        && secret == SECRET;
                    let outcome = proxy.lookup(&who, &lookup(transaction, operation, secret));
                    let seen = all_seen(&stores);
                    if expected {
                        resolved += 1;
                        assert_eq!(
                            outcome.map(|v| v.as_str().to_owned()),
                            Ok(format!("{provider}-value")),
                            "seed {seed} step {step}"
                        );
                        let role = role_of(provider).unwrap();
                        assert_eq!(seen.len(), before + 1, "seed {seed} step {step}");
                        assert_eq!(
                            stores[role].seen().last(),
                            Some(&secret_path(role, provider, SECRET)),
                            "seed {seed} step {step}: another role's bucket was asked"
                        );
                    } else {
                        assert!(outcome.is_err(), "seed {seed} step {step}: resolved");
                        assert_eq!(seen.len(), before, "seed {seed} step {step}: store asked");
                    }
                }
            }
        }
        // No lookup ever reached another provider's keys than its caller's, in any bucket.
        for (role, store) in &stores {
            for path in store.seen() {
                assert!(
                    PROVIDERS.iter().any(|p| role_of(p) == Some(role.as_str())
                        && path == secret_path(role, p, SECRET)),
                    "seed {seed}: {path} was asked of {role}"
                );
            }
        }
    }
    assert!(
        resolved > CASES / 2,
        "too few lookups resolved to test anything: {resolved}"
    );
}

// verifies: REQ-MDL-6, THR-4
#[test]
fn only_the_declared_statement_key_listed_for_statements_is_ever_resolved() {
    let (mut resolved, mut refused) = (0, 0);
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let (mut proxy, stores) = proxy();
        let internal = &stores["internal"];
        let mut listed: Vec<(String, &str)> = (0..1 + rng.below(4))
            .map(|_| {
                let purpose = *rng.pick(&["advertising", "statements"]);
                (rng.key(), purpose)
            })
            .collect();
        if rng.below(3) == 0 {
            listed.push((
                OPERATOR_KEY.to_owned(),
                *rng.pick(&["advertising", "statements"]),
            ));
        }
        let unlisted = rng.key();
        let candidates: Vec<&str> = listed
            .iter()
            .map(|(k, _)| k.as_str())
            .chain([unlisted.as_str(), OPERATOR_KEY])
            .collect();
        let declared = *rng.pick(&candidates);
        for key in &candidates {
            internal.write(&key_path("internal", "alpha", key), "private-half");
        }
        let keys: Vec<(&str, &str)> = listed.iter().map(|(k, p)| (k.as_str(), *p)).collect();
        proxy
            .admit(
                &release("alpha", "1.0.0", &[SECRET], declared),
                &advertisement("alpha", &keys),
            )
            .unwrap();
        proxy
            .begin_operation(&caller("alpha"), &id("tx-1"))
            .unwrap();

        let outcome = proxy.statement_key(&caller("alpha"), &id("tx-1"));

        let purpose = keys.iter().find(|(k, _)| *k == declared).map(|(_, p)| *p);
        let expected = purpose == Some("statements") && declared != OPERATOR_KEY;
        assert_eq!(
            outcome.is_ok(),
            expected,
            "seed {seed}: {declared} {purpose:?}"
        );
        let asked = internal.seen();
        if expected {
            resolved += 1;
            assert_eq!(
                asked,
                [key_path("internal", "alpha", declared)],
                "seed {seed}"
            );
        } else {
            refused += 1;
            assert!(asked.is_empty(), "seed {seed}: {asked:?}");
        }
        for (key, purpose) in &keys {
            if *purpose == "advertising" {
                assert!(
                    !asked.contains(&key_path("internal", "alpha", key)),
                    "seed {seed}: an advertising key was asked for"
                );
            }
        }
    }
    assert!(
        resolved > CASES / 5 && refused > CASES / 5,
        "too one-sided to test anything: {resolved} resolved, {refused} refused"
    );
}

// verifies: REQ-MDL-6, THR-50
#[test]
fn every_lookup_leaves_one_record_with_its_callers_transaction_and_no_value() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let (mut proxy, stores) = seeded();
        stores["internal"].write(&key_path("internal", "alpha", KEY_B), "alpha-private");
        let mut made = Vec::new();
        for _ in 0..20 {
            let provider = *rng.pick(&PROVIDERS);
            let transaction = *rng.pick(&TRANSACTIONS);
            match rng.below(4) {
                0 => {
                    let _ = proxy.begin_operation(&caller(provider), &id(transaction));
                    let _ =
                        proxy.begin_request(&caller(provider), &id(transaction), &name(OPERATION));
                }
                1 => {
                    let _ = proxy.statement_key(&caller(provider), &id(transaction));
                    made.push((provider, transaction, None));
                }
                _ => {
                    let operation = *rng.pick(&OPERATIONS);
                    let _ = proxy.lookup(
                        &caller(provider),
                        &lookup(transaction, operation, rng.pick(&NAMES)),
                    );
                    made.push((provider, transaction, Some(operation)));
                }
            }
        }

        let uses = proxy.take_uses();
        let recorded: Vec<_> = uses
            .iter()
            .map(|u| {
                (
                    u.provider.as_str(),
                    u.transaction.as_str(),
                    u.operation.as_ref().map(|o| o.as_str()),
                )
            })
            .collect();
        assert_eq!(recorded, made, "seed {seed}");
        let printed = format!("{uses:?}");
        assert!(
            !printed.contains("-value") && !printed.contains("-private"),
            "seed {seed}: a record prints a value"
        );
    }
}

/// Whether `text` is one plain path segment of at most 255 bytes, as the store's paths take it.
fn addressable(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 255
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

// verifies: DEC-9, REQ-MDL-9
#[test]
fn a_declared_name_reaches_the_store_only_as_one_plain_segment_of_at_most_255_bytes() {
    const CHARACTERS: [&str; 10] = ["a", "Z", "7", "-", "_", ".", "/", "%", " ", "é"];
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let length = match rng.below(3) {
            0 => 1 + rng.below(4),
            1 => 250 + rng.below(10),
            _ => 1 + rng.below(300),
        };
        let plain = rng.below(2) == 0;
        let secret: String = (0..length)
            .map(|_| {
                if plain {
                    *rng.pick(&CHARACTERS[..6])
                } else {
                    *rng.pick(&CHARACTERS)
                }
            })
            .collect();
        let (mut proxy, stores) = proxy();

        let admitted = proxy.admit(
            &release("alpha", "1.0.0", &[secret.as_str()], KEY_B),
            &advertisement("alpha", &[(KEY_A, "advertising"), (KEY_B, "statements")]),
        );

        if !addressable(&secret) {
            assert_eq!(
                admitted,
                Err(Refusal::UnaddressableName),
                "seed {seed}: {secret:?}"
            );
            continue;
        }
        assert_eq!(admitted, Ok(()), "seed {seed}: {secret:?}");
        in_flight(&mut proxy, "alpha", "tx-1");
        let _ = proxy.lookup(&caller("alpha"), &lookup("tx-1", OPERATION, &secret));
        assert_eq!(
            stores["internal"].seen(),
            [secret_path("internal", "alpha", &secret)],
            "seed {seed}"
        );
    }
}
