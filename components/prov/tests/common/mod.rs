//! Shared helpers for `CMP-PROV`'s tests: the contract's valid advertisements as the catalogue meets them, and a
//! small seeded generator for the property tests (`DEC-18`), so a failing case replays from its seed.

#![allow(dead_code)]

use std::path::Path;

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Advertisement, Deployment, KeyPurpose, PublicKey, Signature,
    SignatureScheme, SigningKey as ListedKey, Statement, VERSION, signing_input,
};
use rutent_prov::catalogue::{
    Bond, Category, Grant, GrantTerms, Instant, ReversalCapacity, TupleKey,
};

/// The statement inside one of provider-role's valid advertisement fixtures.
pub fn advertisement(fixture: &str) -> Advertisement {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/provider-role/fixtures/valid")
        .join(format!("{fixture}.json"));
    let text = std::fs::read_to_string(&path).expect("fixture is readable");
    let statement: Statement<Advertisement> = serde_json::from_str(&text).expect("fixture parses");
    statement.statement
}

/// The ramp provider with a public and a private node: publisher `acme`, provider `bridge-pay`.
pub fn ramp_advertisement() -> Advertisement {
    advertisement("advertisement-ramp-between-a-public-and-a-private-node")
}

/// The reserve provider that starts routes on automatic deposits.
pub fn reserve_advertisement() -> Advertisement {
    advertisement("advertisement-reserve-starting-routes-on-automatic-deposits")
}

/// A bond in `USDC` held by a third party.
pub fn bond(amount: u64) -> Bond {
    Bond {
        asset: "USDC".into(),
        amount,
        held_by: "escrow-co".into(),
    }
}

/// A recorded reversal-capacity check, for a ceiling at most the bond.
pub fn capacity(bond_amount: u64, ceiling: u64) -> ReversalCapacity {
    ReversalCapacity::check(bond(bond_amount), ceiling, "ops-lead", Instant(1_000))
        .expect("ceiling within bond")
}

/// A grant of `provider` to `party` for `tuples`, in force from 1 000 to `expires_at`.
pub fn grant(id: &str, provider: &str, tuples: Vec<TupleKey>, expires_at: u64) -> Grant {
    let mut categories: Vec<Category> = Vec::new();
    for t in &tuples {
        if !categories.contains(&t.category) {
            categories.push(t.category);
        }
    }
    Grant::new(GrantTerms {
        id: id.into(),
        provider: provider.into(),
        party: "Acme Payments Ltd".into(),
        granted_by: "operator-admin".into(),
        granted_at: Instant(1_000),
        expires_at: Instant(expires_at),
        categories,
        tuples,
        reversal_capacity: capacity(1_000, 500),
    })
    .expect("grant is well formed")
}

/// A deterministic generator (xorshift64*), seeded per case.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value below `n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

/// How many seeded cases each property runs.
pub const CASES: u64 = 400;

use rutent_contracts::provider_role::EvidenceGrade;
use rutent_contracts::router_api::Version;
use rutent_prov::catalogue::{Catalogue, tuples_of};

/// The exchange provider `dex-pool`, publisher `acme`.
pub fn exchange_advertisement() -> Advertisement {
    advertisement("advertisement-exchange-within-one-node")
}

/// The ramp between two private nodes of `acme`.
pub fn internal_rail_advertisement() -> Advertisement {
    advertisement("advertisement-ramp-between-two-private-nodes-of-one-publisher")
}

/// When the fixtures' catalogues join their advertisements.
pub const JOINED_AT: Instant = Instant(1_500);

/// A catalogue of `acme`'s advertisements, each with a grant, a module version, an evidence floor of grade 4 (the
/// weakest) on every tuple, `polygon` on the public list with a holding account, and `acme-ledger` with the
/// publisher's. Only what the caller leaves out of this is missing.
pub fn acme_catalogue(ads: &[Advertisement]) -> Catalogue {
    acme_catalogue_made_at(ads, JOINED_AT)
}

/// [`acme_catalogue`], with each advertisement dated `made_at` and joined at [`JOINED_AT`].
pub fn acme_catalogue_made_at(ads: &[Advertisement], made_at: Instant) -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue.add_public_node("polygon").unwrap();
    catalogue.publish_operator_module("wallet-module").unwrap();
    catalogue
        .register_public_holding("polygon", "wallet-module")
        .unwrap();
    for (i, ad) in ads.iter().enumerate() {
        let keys = tuples_of(ad);
        catalogue
            .grant(grant(
                &format!("g-{i}"),
                &ad.provider,
                keys.clone(),
                100_000,
            ))
            .unwrap();
        join_made_at(&mut catalogue, ad, made_at, JOINED_AT).unwrap();
        catalogue.set_module_version(&ad.provider, Version::new("1.0.0").unwrap());
        for key in keys {
            catalogue.set_evidence_floor(key, EvidenceGrade::Statement);
        }
    }
    for node in ["acme-ledger", "acme-card-book"] {
        if catalogue.publishers().publisher_of_node(node).is_some() {
            catalogue
                .register_private_holding(node, "acme", "acme-record-interface")
                .unwrap();
        }
    }
    catalogue
}

/// The snapshot's figure origins as (provider, publisher).
pub fn origins(
    snapshot: &rutent_contracts::router_api::CatalogueSnapshot,
) -> Vec<(String, String)> {
    use rutent_contracts::router_api::Origin;
    snapshot
        .figures
        .iter()
        .filter_map(|f| match &f.origin {
            Origin::Provider {
                provider,
                publisher,
                ..
            } => Some((provider.to_string(), publisher.to_string())),
            Origin::Operator { .. } => None,
        })
        .collect()
}

// Signing ------------------------------------------------------------------------------------------------------

/// The deployment the tests' catalogues serve. Its operator key is [`operator_key`]'s, so the tests can sign as
/// the operator (`T-016-04`).
pub fn deployment() -> Deployment {
    Deployment {
        name: "rutent-test".into(),
        operator_key: public_key(&operator_key()),
    }
}

/// The operator's own signing key, whose public half is the tests' deployment's `operator_key`: the deployment
/// settings' key, never one an advertisement lists.
pub fn operator_key() -> SigningKey {
    signing_key("operator", 1)
}

/// The base64url text of bytes, unpadded.
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

/// The public key of a signing key.
pub fn public_key(key: &SigningKey) -> PublicKey {
    PublicKey::new(base64url(key.verifying_key().as_bytes())).expect("a canonical key")
}

/// A provider's advertising key.
pub fn advertising_key(provider: &str) -> SigningKey {
    signing_key(provider, 1)
}

/// A provider's statements key.
pub fn statements_key(provider: &str) -> SigningKey {
    signing_key(provider, 2)
}

/// An advertisement's keys: the provider's advertising key and statements key.
pub fn keys_of(provider: &str) -> Vec<ListedKey> {
    vec![
        ListedKey {
            scheme: SignatureScheme::Ed25519,
            public_key: public_key(&advertising_key(provider)),
            purpose: KeyPurpose::Advertising,
        },
        ListedKey {
            scheme: SignatureScheme::Ed25519,
            public_key: public_key(&statements_key(provider)),
            purpose: KeyPurpose::Statements,
        },
    ]
}

/// Signs a statement envelope, written as JSON, with `key`: puts the key in, signs the signing input, puts the
/// signature in. Returns the bytes as sent.
pub fn sign_json(mut statement: serde_json::Value, key: &SigningKey) -> Vec<u8> {
    sign_value(&mut statement, key);
    serde_json::to_vec(&statement).unwrap()
}

/// [`sign_json`], in place.
pub fn sign_value(statement: &mut serde_json::Value, key: &SigningKey) {
    statement["signature"]["key"] = public_key(key).as_str().into();
    statement["signature"]["value"] = base64url(&[0u8; 64]).into();
    let input = signing_input(&serde_json::to_vec(&*statement).unwrap()).expect("a signing input");
    statement["signature"]["value"] = base64url(&key.sign(&input).to_bytes()).into();
}

/// `ad`'s statement, made at `made_at` by its provider and signed with `key`, as the bytes a provider sends. Its
/// keys are the provider's own ([`keys_of`]), unless `keys` says otherwise.
pub fn advertisement_bytes(
    ad: &Advertisement,
    keys: Option<Vec<ListedKey>>,
    made_at: Instant,
    key: &SigningKey,
) -> Vec<u8> {
    advertisement_bytes_at(ad, keys, &made_at.rfc3339().expect("a time"), key)
}

/// [`advertisement_bytes`], dated by its text.
pub fn advertisement_bytes_at(
    ad: &Advertisement,
    keys: Option<Vec<ListedKey>>,
    made_at: &str,
    key: &SigningKey,
) -> Vec<u8> {
    let mut statement_body = ad.clone();
    statement_body.keys = keys.unwrap_or_else(|| keys_of(&ad.provider));
    let statement = Statement {
        deployment: deployment(),
        protocol_version: VERSION.into(),
        shape: ADVERTISEMENT_SHAPE.into(),
        maker: ad.provider.clone(),
        made_at: made_at.to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: public_key(key),
            value: base64url(&[0u8; 64]),
        },
        statement: statement_body,
    };
    sign_json(serde_json::to_value(&statement).unwrap(), key)
}

/// Joins `ad` the way a provider would send it: signed with its advertising key, dated `at` (a later one dated a
/// second later for each it has joined before), and the key trusted by the provider's first grant if it has one.
/// The catalogue serves the tests' deployment.
pub fn join(
    catalogue: &mut Catalogue,
    ad: &Advertisement,
    at: Instant,
) -> Result<(), rutent_prov::catalogue::JoinRefusal> {
    let made_at = Instant(at.0 + catalogue.advertisement_history(&ad.provider).len() as u64);
    join_made_at(catalogue, ad, made_at, at)
}

/// [`join`], dated `made_at`.
pub fn join_made_at(
    catalogue: &mut Catalogue,
    ad: &Advertisement,
    made_at: Instant,
    at: Instant,
) -> Result<(), rutent_prov::catalogue::JoinRefusal> {
    catalogue.set_context(deployment(), VERSION);
    let first_grant = catalogue
        .grants_of_provider(&ad.provider)
        .first()
        .map(|g| g.id().to_owned());
    if let Some(id) = first_grant {
        catalogue
            .trust_advertising_key(&id, public_key(&advertising_key(&ad.provider)))
            .expect("the key is trusted");
    }
    let bytes = advertisement_bytes(ad, None, made_at, &advertising_key(&ad.provider));
    catalogue.join(&bytes, at)
}
