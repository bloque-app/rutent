//! Shared helpers for the snapshot's tests (`T-020-05`): a catalogue of reserves on `polygon` with signed
//! advertisements, Core's clock, the policy bounds, a log and an evidence store that keep what they are handed, and
//! the router-api schema every formed snapshot is checked against.

#![allow(dead_code)]

use std::cell::Cell;
use std::path::Path;

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::core_internal::calls::{
    AppendFailure, Evidence, Fetched, GeneralEntry, Kept, Log,
};
use rutent_contracts::core_internal::{Actor, ActorKind, Entry, ReadCheck, Receipt, Role};
use rutent_contracts::module_interface::AdmissionArtefact;
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Advertisement, Deployment, EvidenceGrade, KeyPurpose, PublicKey,
    SafeInteger, Signature, SignatureScheme, SigningKey as ListedKey, Statement, VERSION,
    signing_input,
};
use rutent_contracts::router_api::{
    CatalogueSnapshot, DecisionRecord, FeeFormula, Id, PolicyBounds, Request, Router, Timestamp,
    TupleKey, Version,
};
use rutent_core_orch::snapshot::{
    Ask, Clock, Exposures, Formed, Price, Reading, Received, Releases, Snapshots,
};
use rutent_prov::catalogue::{
    Bond, Catalogue, Category, Grant, GrantTerms, Instant, ReversalCapacity, tuples_of,
};
use rutent_router::RouteFinder;
use serde_json::{Value, json};

/// The reserve every test routes through: publisher `acme`.
pub const RESERVE: &str = "polygon-wallet-reserve";
/// A second reserve of the same publisher.
pub const SIBLING: &str = "polygon-vault-reserve";
/// When the catalogue joins the advertisements.
pub const JOINED_AT: u64 = 1_500;

/// Core's clock in a test: it reads what the test set, and only that.
#[derive(Debug)]
pub struct TestClock(pub Cell<Reading>);

impl TestClock {
    pub fn at(seconds: u64) -> Self {
        Self(Cell::new(Reading::at(seconds)))
    }

    /// Sets the clock to a whole second.
    pub fn set(&self, seconds: u64) {
        self.0.set(Reading::at(seconds));
    }

    /// Sets the clock inside a second.
    pub fn set_within(&self, seconds: u64, nanos: u32) {
        self.0.set(Reading { seconds, nanos });
    }
}

impl Clock for TestClock {
    fn now(&self) -> Reading {
        self.0.get()
    }
}

// Keys and signing --------------------------------------------------------------------------------------------

/// The deployment the tests' catalogues serve; its operator key is [`operator_key`]'s.
pub fn deployment() -> Deployment {
    Deployment {
        name: "rutent-test".into(),
        operator_key: public_key(&operator_key()),
    }
}

/// Another deployment, with the same operator key.
pub fn other_deployment() -> Deployment {
    Deployment {
        name: "rutent-other".into(),
        operator_key: public_key(&operator_key()),
    }
}

pub fn operator_key() -> SigningKey {
    signing_key("operator", 1)
}

pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// A key pair derived from a label and a number: the same on every run, and different for each.
pub fn signing_key(label: &str, n: u8) -> SigningKey {
    let mut seed = [7u8; 32];
    for (i, b) in label.bytes().enumerate() {
        seed[i % 31] = seed[i % 31].wrapping_mul(31).wrapping_add(b);
    }
    seed[31] = n;
    SigningKey::from_bytes(&seed)
}

pub fn public_key(key: &SigningKey) -> PublicKey {
    PublicKey::new(base64url(key.verifying_key().as_bytes())).expect("a canonical key")
}

/// A provider's advertising key.
pub fn advertising_key(provider: &str) -> SigningKey {
    signing_key(provider, 1)
}

/// A provider's `n`th statements key; its first advertisement lists the first.
pub fn statements_key(provider: &str, n: u8) -> SigningKey {
    signing_key(&format!("{provider}-statements"), n)
}

/// Keys listed by an advertisement: the provider's advertising key and the given statements keys.
pub fn keys_of(provider: &str, statements: &[&SigningKey]) -> Vec<ListedKey> {
    let listed = |key: &SigningKey, purpose| ListedKey {
        scheme: SignatureScheme::Ed25519,
        public_key: public_key(key),
        purpose,
    };
    std::iter::once(listed(&advertising_key(provider), KeyPurpose::Advertising))
        .chain(statements.iter().map(|k| listed(k, KeyPurpose::Statements)))
        .collect()
}

/// Signs a statement envelope written as JSON with `key`, and returns the bytes as sent.
pub fn sign(mut statement: Value, key: &SigningKey) -> Vec<u8> {
    sign_value(&mut statement, key);
    serde_json::to_vec(&statement).unwrap()
}

/// [`sign`], in place.
pub fn sign_value(statement: &mut Value, key: &SigningKey) {
    statement["signature"]["key"] = public_key(key).as_str().into();
    statement["signature"]["value"] = base64url(&[0u8; 64]).into();
    let input = signing_input(&serde_json::to_vec(&*statement).unwrap()).expect("a signing input");
    statement["signature"]["value"] = base64url(&key.sign(&input).to_bytes()).into();
}

/// An unsigned envelope around `body`, made by `maker` at `made_at`, in `deployment`.
pub fn envelope(
    deployment: &Deployment,
    shape: &str,
    maker: &str,
    made_at: u64,
    body: Value,
) -> Value {
    json!({
        "deployment": deployment,
        "protocol_version": VERSION,
        "shape": shape,
        "maker": maker,
        "made_at": Instant(made_at).rfc3339().unwrap(),
        "signature": { "scheme": "ed25519", "key": "", "value": "" },
        "statement": body,
    })
}

// The catalogue ---------------------------------------------------------------------------------------------------

/// The reserve fixture, as `provider` of `publisher`: a withdrawal from and a deposit to `polygon` in USDC.
pub fn reserve(provider: &str, publisher: &str) -> Advertisement {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../contracts/provider-role/fixtures/valid/advertisement-reserve-starting-routes-on-automatic-deposits.json",
    );
    let text = std::fs::read_to_string(path).unwrap();
    let mut ad: Statement<Advertisement> = serde_json::from_str(&text).unwrap();
    ad.statement.provider = provider.into();
    ad.statement.publisher = publisher.into();
    let mut value = serde_json::to_value(&ad.statement).unwrap();
    value["offers"]["reserve"]["declaration"]["maker"] = provider.into();
    serde_json::from_value(value).unwrap()
}

/// `ad` signed by its provider's advertising key, listing `keys`, dated `made_at`.
pub fn advertisement_bytes(ad: &Advertisement, keys: Vec<ListedKey>, made_at: u64) -> Vec<u8> {
    let mut body = ad.clone();
    body.keys = keys;
    let statement = Statement {
        deployment: deployment(),
        protocol_version: VERSION.into(),
        shape: ADVERTISEMENT_SHAPE.into(),
        maker: ad.provider.clone(),
        made_at: Instant(made_at).rfc3339().unwrap(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: public_key(&advertising_key(&ad.provider)),
            value: base64url(&[0u8; 64]),
        },
        statement: body,
    };
    sign(
        serde_json::to_value(&statement).unwrap(),
        &advertising_key(&ad.provider),
    )
}

/// A grant of `provider` for every tuple of `ad`, from 1 000 to `expires_at`.
pub fn grant(id: &str, ad: &Advertisement, expires_at: u64) -> Grant {
    Grant::new(GrantTerms {
        id: id.into(),
        provider: ad.provider.clone(),
        party: "Acme Payments Ltd".into(),
        granted_by: "operator-admin".into(),
        granted_at: Instant(1_000),
        expires_at: Instant(expires_at),
        categories: vec![Category::Reserve],
        tuples: tuples_of(ad),
        reversal_capacity: ReversalCapacity::check(
            Bond {
                asset: "USDC".into(),
                amount: 1_000,
                held_by: "escrow-co".into(),
            },
            500,
            "ops-lead",
            Instant(1_000),
        )
        .unwrap(),
    })
    .unwrap()
}

/// A catalogue of the given reserves `(provider, publisher)`, each granted until `expires_at`, joined at
/// [`JOINED_AT`] listing its first statements key, at module version `1.0.0`, with `polygon` public and held.
pub fn catalogue(reserves: &[(&str, &str)], expires_at: u64) -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue.set_context(deployment(), VERSION);
    catalogue.add_public_node("polygon").unwrap();
    catalogue.publish_operator_module("wallet-module").unwrap();
    catalogue
        .register_public_holding("polygon", "wallet-module")
        .unwrap();
    for (i, (provider, publisher)) in reserves.iter().enumerate() {
        let ad = reserve(provider, publisher);
        let id = format!("g-{i}");
        catalogue.grant(grant(&id, &ad, expires_at)).unwrap();
        catalogue
            .trust_advertising_key(&id, public_key(&advertising_key(provider)))
            .unwrap();
        let keys = keys_of(provider, &[&statements_key(provider, 1)]);
        catalogue
            .join(
                &advertisement_bytes(&ad, keys, JOINED_AT),
                Instant(JOINED_AT),
            )
            .unwrap();
        catalogue.set_module_version(provider, Version::new("1.0.0").unwrap());
        for key in tuples_of(&ad) {
            catalogue.set_evidence_floor(key, EvidenceGrade::Statement);
        }
    }
    catalogue
}

// Tuples, prices, bounds, requests ------------------------------------------------------------------------------

pub fn usdc() -> Value {
    json!({ "id": "USDC", "decimals": 6 })
}

pub fn outer() -> Value {
    json!({ "node": { "network": "polygon" }, "asset": usdc() })
}

pub fn inner() -> Value {
    json!({ "node": "polygon", "asset": usdc() })
}

/// A reserve's withdrawal tuple, from `polygon`'s outer node into its inner one.
pub fn withdrawal_tuple(provider: &str) -> TupleKey {
    serde_json::from_value(json!({ "provider": provider, "from": outer(), "to": inner() })).unwrap()
}

/// A reserve's deposit tuple, from `polygon`'s inner node out to its outer one.
pub fn deposit_tuple(provider: &str) -> TupleKey {
    serde_json::from_value(json!({ "provider": provider, "from": inner(), "to": outer() })).unwrap()
}

/// A price of no fee and a minute's settlement for `tuple`, stated at 1 500.
pub fn free_price(tuple: TupleKey) -> Price {
    Price {
        tuple,
        stated_at: Timestamp::new(Instant(JOINED_AT).rfc3339().unwrap()).unwrap(),
        fee: serde_json::from_value::<FeeFormula>(json!({
            "flat": "0", "proportional": { "numerator": "0", "denominator": "1" }
        }))
        .unwrap(),
        rate: None,
        settlement_seconds: SafeInteger::try_from(60).unwrap(),
    }
}

/// A free price for both tuples of each reserve.
pub fn prices(providers: &[&str]) -> Vec<Price> {
    providers
        .iter()
        .flat_map(|p| {
            [
                free_price(withdrawal_tuple(p)),
                free_price(deposit_tuple(p)),
            ]
        })
        .collect()
}

/// Bounds of the tests' deployment with the given ceilings and no operator's fee.
pub fn bounds(provider_ceilings: Value, publisher_ceilings: Value) -> PolicyBounds {
    serde_json::from_value(json!({
        "deployment": deployment(),
        "protocol_version": VERSION,
        "policy_version": "policy-1",
        "max_hops": 4,
        "max_parts": 1,
        "excluded_providers": [],
        "excluded_tuples": [],
        "provider_ceilings": provider_ceilings,
        "publisher_ceilings": publisher_ceilings,
        "operator_fee": { "flat": "0", "proportional": { "numerator": "0", "denominator": "1" } },
        "slippage_limits": [],
    }))
    .unwrap()
}

pub fn no_ceilings() -> PolicyBounds {
    bounds(json!([]), json!([]))
}

/// A shortlist request moving `amount` USDC units from `polygon`'s outer node to its outer node.
pub fn shortlist_request(amount: &str) -> Request {
    serde_json::from_value(json!({
        "request_id": "req-1",
        "round": { "kind": "shortlist" },
        "from": outer(),
        "to": outer(),
        "amount": amount,
        "objective": { "kind": "lowest_total_cost" },
    }))
    .unwrap()
}

/// A withdrawal of `amount` at `provider`, as a route's operation names it.
pub fn withdrawal(provider: &str, amount: &str) -> rutent_contracts::router_api::Operation {
    serde_json::from_value(json!({
        "kind": "withdrawal",
        "provider": provider,
        "from": outer(),
        "to": inner(),
        "amount_in": amount,
        "fee": { "asset": "USDC", "amount": "0" },
        "amount_out": amount,
        "figures": ["fig-0001"],
    }))
    .unwrap()
}

pub fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

// Forming, checked -------------------------------------------------------------------------------------------------

/// Forms a snapshot and checks it against router-api's schema and its own contract: every snapshot a test forms
/// passes both.
pub fn form(
    catalogue: &Catalogue,
    clock: &TestClock,
    exposures: &Exposures,
    releases: &Releases,
    bounds: &PolicyBounds,
    prices: &[Price],
    statements: &[Received],
) -> Formed {
    let formed = Snapshots::new(catalogue, clock, exposures, releases)
        .form(&Ask {
            snapshot_id: "snap-1",
            bounds,
            prices,
            statements,
        })
        .expect("a snapshot is formed");
    assert_passes_the_schema(&formed.snapshot);
    formed
}

/// Routes `request` over `snapshot` with the router.
pub fn route(
    request: &Request,
    snapshot: &CatalogueSnapshot,
    bounds: &PolicyBounds,
) -> DecisionRecord {
    let record = RouteFinder::route(request, snapshot, bounds);
    record
        .check()
        .expect("the router's record passes its contract");
    record
}

/// Whether any kept candidate of a shortlist record runs through `provider`.
pub fn offered_through(record: &DecisionRecord, provider: &str) -> bool {
    record.shortlist().iter().any(|c| {
        c.parts
            .iter()
            .any(|p| p.operations.iter().any(|o| o.provider.as_str() == provider))
    })
}

pub fn assert_passes_the_schema(snapshot: &CatalogueSnapshot) {
    let value = serde_json::to_value(snapshot).unwrap();
    let errors: Vec<String> = validator()
        .iter_errors(&value)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let back: CatalogueSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(&back, snapshot);
}

fn validator() -> jsonschema::Validator {
    let contracts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
    let read =
        |p: &Path| -> Value { serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap() };
    let mut registry = jsonschema::Registry::new();
    for contract in ["provider-role", "router-api"] {
        for entry in std::fs::read_dir(contracts.join(contract).join("schemas")).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().ends_with(".schema.json") {
                let schema = read(&path);
                let id = schema["$id"].as_str().unwrap().to_owned();
                registry = registry.add(id, schema).unwrap();
            }
        }
    }
    let registry = registry.prepare().unwrap();
    let schema = read(&contracts.join("router-api/schemas/catalogue-snapshot.schema.json"));
    jsonschema::options()
        .offline()
        .with_registry(&registry)
        .build(&schema)
        .unwrap()
}

// Releases --------------------------------------------------------------------------------------------------------

/// An accepted release of `provider` at `1.0.0`, from module-interface's fixture, declaring `calls`.
pub fn accepted_release(provider: &str, calls: Value) -> AdmissionArtefact {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../contracts/module-interface/fixtures/valid/admission-artefact-of-a-first-release-freshly-accepted.json",
    );
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let declaration = &mut value["assertion"]["statement"]["declaration"];
    declaration["provider"] = provider.into();
    declaration["version"] = "1.0.0".into();
    declaration["outside"]["calls"] = calls;
    value["assertion"]["maker"] = provider.into();
    serde_json::from_value(value).unwrap()
}

// The log and the evidence store ----------------------------------------------------------------------------------

/// A log that keeps every entry it is handed, after its check.
#[derive(Debug, Default)]
pub struct KeptLog {
    pub entries: Vec<Entry>,
}

fn digest(byte: u8) -> rutent_contracts::module_interface::Digest {
    serde_json::from_value(
        json!({ "algorithm": "sha-256", "value": format!("{byte:02x}").repeat(32) }),
    )
    .unwrap()
}

impl Log for KeptLog {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        let entry = entry.entry();
        entry.check().map_err(AppendFailure::Refused)?;
        self.entries.push(entry.clone());
        Ok(Receipt {
            log: id("log-1"),
            index: SafeInteger::try_from(self.entries.len() as u64 - 1).unwrap(),
            entry: digest(0x72),
        })
    }
}

/// An evidence store that keeps every statement it is handed.
#[derive(Debug, Default)]
pub struct KeptEvidence {
    pub kept: Vec<Vec<u8>>,
}

impl Evidence for KeptEvidence {
    fn keep(&mut self, received: &[u8]) -> Result<Kept, AppendFailure> {
        self.kept.push(received.to_vec());
        Ok(Kept {
            commitment: digest(self.kept.len() as u8),
            restates: None,
        })
    }

    fn fetch(&mut self, _check: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Ok(Fetched::Erased)
    }
}

/// The actor a provider's statement claims made it.
pub fn provider_actor(provider: &str) -> Actor {
    Actor {
        party: id("acme-payments"),
        acts: id(provider),
        role: Role::Provider,
        kind: ActorKind::Component,
    }
}

/// Core acting on its own account.
pub fn core_actor() -> Actor {
    Actor {
        party: id("rutent"),
        acts: id("rutent-core"),
        role: Role::Core,
        kind: ActorKind::Component,
    }
}
