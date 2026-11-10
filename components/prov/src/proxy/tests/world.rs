//! Property tests of the production world (`DEC-18`, `T-014-05`): the platform's hooks drive the credential
//! proxy, and a lookup is answered only inside the request in flight the hooks describe, only when the platform's
//! own verdict passed, and every lookup leaves one use. Seeded, so a failure names the seed that replays it. No
//! process runs: the world is driven as the platform drives it.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::num::NonZeroU16;

use rutent_contracts::module_interface::{Host, SecretRefusalReason};

use super::support::*;
use crate::egress::{ConnectError, Connector, Dial, NameResolver, Request, Response};
use crate::module::platform::World;
use crate::proxy::{KvResolver, ProductionWorld, Refusal};

/// The world's network, which these tests never reach.
struct Nowhere;

impl NameResolver for Nowhere {
    fn resolve(&self, _: &Host, _: NonZeroU16) -> Vec<IpAddr> {
        Vec::new()
    }
}

impl Connector for Nowhere {
    fn exchange(&self, _: &Dial<'_>, _: &Request) -> Result<Response, ConnectError> {
        Err(ConnectError::Unreachable)
    }
}

type Tested = ProductionWorld<KvResolver<std::sync::Arc<Store>>, Nowhere, Nowhere>;

/// `alpha` and `beta` have a role and an advertisement, `gamma` a role and no advertisement, `delta` neither.
const PROVIDERS: [&str; 4] = ["alpha", "beta", "gamma", "delta"];
const TRANSACTIONS: [&str; 2] = ["tx-1", "tx-2"];
const OPERATIONS: [&str; 2] = [OPERATION, "other.call"];
const NAMES: [&str; 2] = [SECRET, "other-secret"];

fn role_of(provider: &str) -> Option<&'static str> {
    match provider {
        "alpha" | "beta" => Some("internal"),
        "gamma" => Some("integrations"),
        _ => None,
    }
}

fn world() -> (Tested, BTreeMap<String, std::sync::Arc<Store>>) {
    let (proxy, stores) = proxy();
    for provider in PROVIDERS {
        for role in ROLES {
            stores[role].write(
                &secret_path(role, provider, SECRET),
                &format!("{provider}-value"),
            );
        }
    }
    let mut world = ProductionWorld::new(proxy, Nowhere, Nowhere);
    for provider in ["alpha", "beta"] {
        world.advertise(advertisement(
            provider,
            &[(KEY_A, "advertising"), (KEY_B, "statements")],
        ));
    }
    (world, stores)
}

/// What the platform told the world about one provider: whether its running release was admitted, and the
/// transaction it handles with the operation in flight.
#[derive(Default, Clone)]
struct Model {
    admitted: bool,
    handling: Option<(&'static str, Option<&'static str>)>,
}

// verifies: REQ-MDL-17, REQ-MDL-9, REQ-MDL-6, DEC-9
#[test]
fn the_world_answers_a_lookup_only_inside_the_request_its_hooks_put_in_flight() {
    let (mut answered, mut refused_by_verdict) = (0, 0);
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let (mut world, stores) = world();
        let mut model: BTreeMap<&str, Model> = BTreeMap::new();
        // Every release starts first, as the platform starts each admitted one; later starts replace it.
        for provider in PROVIDERS {
            world.running(&release(provider, "1.0.0", &[SECRET], KEY_B));
            model.insert(
                provider,
                Model {
                    admitted: matches!(provider, "alpha" | "beta"),
                    handling: None,
                },
            );
        }
        assert_eq!(world.take_unadmitted().len(), 2, "seed {seed}");
        for step in 0..60 {
            // The providers that can resolve are picked most often, so that resolved lookups are common.
            let provider = if rng.below(3) == 0 {
                *rng.pick(&PROVIDERS)
            } else {
                *rng.pick(&["alpha", "beta"])
            };
            let who = caller(provider);
            let state = model.entry(provider).or_default();
            let (mut transaction, mut operation) =
                (*rng.pick(&TRANSACTIONS), *rng.pick(&OPERATIONS));
            if rng.below(4) != 0
                && let Some((t, serving)) = state.handling
            {
                transaction = t;
                operation = serving.unwrap_or(OPERATION);
            }
            let at = format!("seed {seed} step {step}");
            match rng.below(12) {
                0 => {
                    world.running(&release(provider, "1.0.0", &[SECRET], KEY_B));
                    let admitted = matches!(provider, "alpha" | "beta");
                    *state = Model {
                        admitted,
                        handling: None,
                    };
                    assert_eq!(world.take_unadmitted().is_empty(), admitted, "{at}");
                }
                1 | 2 => {
                    world.begin_operation(&who, &id(transaction));
                    if state.admitted {
                        state.handling = Some((transaction, None));
                    }
                }
                3 | 4 => {
                    world.begin_request(&who, &id(transaction), &name(operation));
                    if let Some((t, serving)) = &mut state.handling
                        && *t == transaction
                        && operation == OPERATION
                    {
                        *serving = Some(operation);
                    }
                }
                5 => {
                    world.end_request(&who);
                    if let Some((_, serving)) = &mut state.handling {
                        *serving = None;
                    }
                }
                6 => {
                    world.end_operation(&who);
                    state.handling = None;
                    assert_eq!(world.in_flight(&id(provider)), None, "{at}");
                }
                _ => {
                    let secret = *rng.pick(&NAMES);
                    let verdict = if rng.below(4) == 0 {
                        Err(Refusal::NotTheRequestInFlight)
                    } else {
                        Ok(())
                    };
                    let expected = verdict.is_ok()
                        && state.admitted
                        && state.handling == Some((transaction, Some(operation)))
                        && secret == SECRET;
                    let asked_before = all_seen(&stores).len();
                    let uses_before = world.proxy().take_uses();
                    assert!(uses_before.is_empty(), "{at}");
                    let result = world.lookup(
                        &who,
                        &lookup(transaction, operation, secret),
                        verdict.clone(),
                    );
                    let asked = all_seen(&stores).len() - asked_before;
                    assert_eq!(result.is_ok(), expected, "{at}");
                    if let Ok(value) = &result {
                        assert_eq!(value.as_str(), format!("{provider}-value"), "{at}");
                        answered += 1;
                    }
                    if verdict.is_err() {
                        assert_eq!(result, Err(SecretRefusalReason::UndeclaredSecret), "{at}");
                        refused_by_verdict += 1;
                    }
                    // The store is asked exactly for the lookups the world answers, and only from the caller's
                    // own path.
                    assert_eq!(asked, usize::from(expected), "{at}");
                    if expected {
                        let path = all_seen(&stores).pop().expect("a path");
                        assert_eq!(
                            path,
                            secret_path(role_of(provider).expect("a role"), provider, SECRET),
                            "{at}"
                        );
                    }
                    let uses = world.proxy().take_uses();
                    assert_eq!(uses.len(), 1, "{at}: one use per lookup");
                    assert_eq!(uses[0].provider.as_str(), provider, "{at}");
                    assert_eq!(uses[0].transaction.as_str(), transaction, "{at}");
                    assert_eq!(uses[0].outcome.is_ok(), expected, "{at}");
                }
            }
            let flying = world.in_flight(&id(provider));
            let modelled = model[provider]
                .handling
                .and_then(|(t, serving)| serving.map(|op| (t, op)));
            assert_eq!(
                flying.map(|f| (
                    f.transaction.as_str().to_owned(),
                    f.operation.as_str().to_owned()
                )),
                modelled.map(|(t, op)| (t.to_owned(), op.to_owned())),
                "seed {seed} step {step}"
            );
        }
    }
    // Neither side of the property holds vacuously.
    assert!(answered > 200, "{answered}");
    assert!(refused_by_verdict > 200, "{refused_by_verdict}");
}
