//! What the credential proxy's tests share (`T-014-02`): a fake store per bucket, deployment settings with two
//! process roles, admitted releases and advertisements written from the contract's fixtures, and a seeded
//! generator for the property tests.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::proxy::{
    Caller, CredentialProxy, InvalidSettings, KvResolver, ProxySettings, Request, Response, Token,
    Transport, TransportError, kv_bucket,
};
use rutent_contracts::module_interface::{AdmissionArtefact, Identifier, Name, SecretLookup};
use rutent_contracts::provider_role::{Advertisement, Statement};
use serde_json::{Value, json};

/// The proxy over fake stores, as the tests build it.
pub type Proxy = CredentialProxy<KvResolver<Arc<Store>>>;

/// A fake KV version 2 store: answers each path written to it, 404 otherwise, and keeps every request's path. Set
/// failing, it answers every request with that status instead.
#[derive(Default)]
pub struct Store {
    values: Mutex<HashMap<String, String>>,
    seen: Mutex<Vec<String>>,
    failing: Mutex<Option<u16>>,
}

impl Store {
    /// Writes `value` at `path`, as the store's current version.
    pub fn write(&self, path: &str, value: &str) {
        self.values
            .lock()
            .unwrap()
            .insert(path.to_owned(), value.to_owned());
    }

    /// The paths asked for, in order.
    pub fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    /// Answers every later request with `status` and no secret, as a sealed, overloaded or misconfigured store
    /// does; `None` answers as before.
    pub fn fail_with(&self, status: Option<u16>) {
        *self.failing.lock().unwrap() = status;
    }
}

impl Transport for Store {
    fn get(&self, request: &Request) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push(request.path().to_owned());
        if let Some(status) = *self.failing.lock().unwrap() {
            return Ok(Response {
                status,
                body: b"{\"errors\":[]}".to_vec(),
            });
        }
        Ok(match self.values.lock().unwrap().get(request.path()) {
            Some(value) => Response {
                status: 200,
                body:
                    json!({ "data": { "data": { "value": value }, "metadata": { "version": 1 } } })
                        .to_string()
                        .into_bytes(),
            },
            None => Response {
                status: 404,
                body: b"{\"errors\":[]}".to_vec(),
            },
        })
    }
}

/// The two process roles of the tests' deployment.
pub const ROLES: [&str; 2] = ["internal", "integrations"];

/// The deployment settings: two roles on one store and mount, with their own secrets and keys prefixes, and the
/// providers `alpha` and `beta` given `internal`, `gamma` given `integrations`. `delta` has no role.
pub fn settings() -> ProxySettings {
    ProxySettings::from_json(
        &json!({
            "deployment": deployment(),
            "roles": {
                "internal": {
                    "server": "openbao",
                    "store": { "scheme": "https", "host": "bao.internal", "port": 8200 },
                    "ca_file": "/etc/rutent/bao-ca.pem",
                    "mount": "secret",
                    "secrets": "rutent/internal/secrets",
                    "keys": "rutent/internal/keys"
                },
                "integrations": {
                    "server": "vault",
                    "store": { "scheme": "https", "host": "bao.internal", "port": 8200 },
                    "ca_file": "/etc/rutent/bao-ca.pem",
                    "mount": "secret",
                    "secrets": "rutent/integrations/secrets",
                    "keys": "rutent/integrations/keys",
                    "namespace": "integrations"
                }
            },
            "providers": { "alpha": "internal", "beta": "internal", "gamma": "integrations" }
        })
        .to_string(),
    )
    .expect("the tests' settings are valid")
}

/// The deployment the tests' settings name: the contract fixtures' deployment and operator key.
pub fn deployment() -> Value {
    json!({ "name": "rutent-test", "operator_key": OPERATOR_KEY })
}

/// Where provider `provider` of role `role` keeps its secret `name`.
pub fn secret_path(role: &str, provider: &str, name: &str) -> String {
    format!("/v1/secret/data/rutent/{role}/secrets/{provider}/{name}")
}

/// Where provider `provider` of role `role` keeps the private half of statement key `key`.
pub fn key_path(role: &str, provider: &str, key: &str) -> String {
    format!("/v1/secret/data/rutent/{role}/keys/{provider}/{key}")
}

/// A proxy over one fake store per role, and those stores by role.
pub fn proxy() -> (Proxy, BTreeMap<String, Arc<Store>>) {
    let stores: BTreeMap<String, Arc<Store>> = ROLES
        .iter()
        .map(|r| ((*r).to_owned(), Arc::new(Store::default())))
        .collect();
    let proxy = CredentialProxy::new(&settings(), |role, settings| {
        let token = Token::new(format!("{role}-token-not-a-secret")).unwrap();
        kv_bucket::<Arc<Store>>(settings, token, Arc::clone(&stores[role]))
    })
    .map_err(|e: InvalidSettings| e)
    .expect("the proxy builds");
    (proxy, stores)
}

/// Every path any store was asked for.
pub fn all_seen(stores: &BTreeMap<String, Arc<Store>>) -> Vec<String> {
    stores.values().flat_map(|s| s.seen()).collect()
}

/// The one outside operation the tests' releases declare, and the secret it declares.
pub const OPERATION: &str = "bank.call";
pub const SECRET: &str = "api-key";

/// Three valid Ed25519 public keys, from the contract's fixtures, and the deployment's operator key.
pub const KEY_A: &str = "jE6S_8w7b1zcqEMqDpW7jzZ3WLzylA7ZWZgKEj-_OHg";
pub const KEY_B: &str = "PT4_QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1w";
pub const KEY_C: &str = "CAkKCwwNDg8QERITFBUWFxgZGhscHR4fICEiIyQlJic";
pub const OPERATOR_KEY: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA";

fn fixture(contract: &str, name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts")
        .join(contract)
        .join("fixtures/valid")
        .join(format!("{name}.json"));
    serde_json::from_slice(&std::fs::read(&path).expect("fixture is readable"))
        .expect("fixture is JSON")
}

/// An accepted release of `provider` at `version`, declaring one outside operation, [`OPERATION`], with
/// `secrets`, and `statement_key` as its statement key.
pub fn release(
    provider: &str,
    version: &str,
    secrets: &[&str],
    statement_key: &str,
) -> AdmissionArtefact {
    let mut artefact = fixture(
        "module-interface",
        "admission-artefact-of-a-release-accepted-automatically",
    );
    artefact["assertion"]["maker"] = json!(provider);
    let declaration = &mut artefact["assertion"]["statement"]["declaration"];
    declaration["provider"] = json!(provider);
    declaration["version"] = json!(version);
    declaration["outside"]["operations"] = json!([{
        "name": OPERATION,
        "destinations": ["bank_api"],
        "secrets": secrets,
    }]);
    declaration["outside"]["secrets"] = json!(secrets);
    declaration["outside"]["statement_key"] = json!(statement_key);
    serde_json::from_value(artefact).expect("an admission artefact")
}

/// The same release, still awaiting acceptance.
pub fn unaccepted(mut artefact: AdmissionArtefact) -> AdmissionArtefact {
    artefact.acceptance.accepted = None;
    artefact
}

/// `provider`'s accepted advertisement, listing `keys` with their purposes (`"advertising"` or `"statements"`).
pub fn advertisement(provider: &str, keys: &[(&str, &str)]) -> Statement<Advertisement> {
    let mut statement = fixture("provider-role", "advertisement-exchange-within-one-node");
    statement["maker"] = json!(provider);
    statement["statement"]["provider"] = json!(provider);
    statement["statement"]["keys"] = keys
        .iter()
        .map(|(key, purpose)| json!({ "scheme": "ed25519", "public_key": key, "purpose": purpose }))
        .collect();
    if let Some((key, _)) = keys.iter().find(|(_, purpose)| *purpose == "advertising") {
        statement["signature"]["key"] = json!(key);
    }
    serde_json::from_value(statement).expect("an advertisement")
}

/// The caller whose process is `provider`'s.
pub fn caller(provider: &str) -> Caller {
    Caller::process_of(Identifier::new(provider).unwrap())
}

pub fn id(text: &str) -> Identifier {
    Identifier::new(text).unwrap()
}

pub fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

/// A lookup of `secret` for `transaction` and `operation`.
pub fn lookup(transaction: &str, operation: &str, secret: &str) -> SecretLookup {
    SecretLookup {
        transaction: id(transaction),
        operation: name(operation),
        name: name(secret),
    }
}

/// Admits `provider`'s release declaring [`SECRET`] for [`OPERATION`], with [`KEY_B`] as its statement key and
/// an advertisement listing [`KEY_A`] for advertising and [`KEY_B`] for statements.
pub fn admit(proxy: &mut Proxy, provider: &str) {
    proxy
        .admit(
            &release(provider, "1.0.0", &[SECRET], KEY_B),
            &advertisement(provider, &[(KEY_A, "advertising"), (KEY_B, "statements")]),
        )
        .expect("the release is admitted");
}

/// Starts handling `transaction` for `provider` and puts an outside request for [`OPERATION`] in flight.
pub fn in_flight(proxy: &mut Proxy, provider: &str, transaction: &str) {
    proxy
        .begin_operation(&caller(provider), &id(transaction))
        .expect("the operation starts");
    proxy
        .begin_request(&caller(provider), &id(transaction), &name(OPERATION))
        .expect("the request is in flight");
}

/// A seeded generator, so a property test's failure names the seed that replays it (`DEC-18`).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    /// A public key from 32 generated bytes, in its canonical base64url text.
    pub fn key(&mut self) -> String {
        let bytes: Vec<u8> = (0..32).map(|_| self.next() as u8).collect();
        base64url(&bytes)
    }
}

/// The property tests' number of cases.
pub const CASES: u64 = 400;

/// `bytes` in base64url without padding.
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}
