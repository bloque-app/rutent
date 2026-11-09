//! Real signatures for the suites (`T-010-10`; `DEC-4`, `REQ-MDL-12`, `REQ-MDL-18`).
//!
//! The suites' statements carry placeholder keys and signatures (`role::placeholder_key`), which are well formed
//! but no point of the curve. A platform that verifies a statement, as the catalogue does, can't take them. This
//! module gives the suites what they lack, built here and not imported from a platform (`DEC-2`):
//!
//! - **Keys.** Ed25519 keys derived from a label and a number, so a run is the same on every machine and no
//!   random source is needed. A provider has an advertising key and a statements key ([`advertising_key`],
//!   [`statements_key`]); [`another_key`] is one no advertisement lists.
//! - **Signing.** [`sign_value`] puts the key in and signs the statement's signing input (the contract's
//!   `provider_role::signing_input`: the RFC 8785 canonical form without `signature.value`).
//! - **Verifying.** [`verify`] checks a statement's signature against the key it names, strictly;
//!   [`Ed25519Verifier`] is that as the role suite's `SignatureVerifier`. Whether the key is one the maker
//!   publishes is another check, which the suites and the platform's catalogue make.
//! - **Advertisements.** [`advertisement`] is a provider's signed advertisement, listing its two keys.
//! - **Stand-ins that sign.** [`Signing`] runs a role provider, whose statements carry placeholders, so that
//!   every statement it sends is signed by the key its placeholder stands for.

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Advertisement, KeyPurpose, Operation, PublicKey, ResultBody, Signature,
    SignatureScheme, SigningKey as ListedKey, Statement, signing_input,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::boundary::sha256::digest;
use crate::role::stand_in::Reserve;
use crate::role::{
    Received, RoleHarness, RoleProvider, SignatureVerifier, SuiteContext, Verification,
};

pub use ed25519_dalek::SigningKey as Ed25519Key;

/// When the providers' advertisements are made: a day before the suites' statements (`2026-11-07`), so the key
/// they list is in force when those are made.
pub const ADVERTISED_AT: &str = "2026-11-06T00:00:00Z";

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The base64url text of bytes, unpadded.
pub fn base64url(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(BASE64URL[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// The 64 bytes a canonical signature value stands for, if it is one.
fn signature_bytes(text: &str) -> Option<[u8; 64]> {
    if !crate::role::is_canonical_signature(text) {
        return None;
    }
    let mut bytes = Vec::with_capacity(66);
    for chunk in text.as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = BASE64URL.iter().position(|a| a == c)? as u32;
            n |= v << (18 - 6 * i);
        }
        let count = chunk.len() - 1;
        bytes.extend_from_slice(&n.to_be_bytes()[1..=count]);
    }
    bytes.try_into().ok()
}

/// A key derived from a label and a number: the same on every run, and different for each.
pub fn signing_key(label: &str, n: u8) -> SigningKey {
    let mut input = label.as_bytes().to_vec();
    input.push(n);
    SigningKey::from_bytes(&digest(&input))
}

/// A provider's advertising key, which signs its advertisements and its release assertions.
pub fn advertising_key(provider: &str) -> SigningKey {
    signing_key(provider, 1)
}

/// A provider's statements key, which a release declares as the key its module's statements are signed with.
pub fn statements_key(provider: &str) -> SigningKey {
    signing_key(provider, 2)
}

/// A key no provider publishes: whoever signs with it is signing as no one.
pub fn another_key() -> SigningKey {
    signing_key("a key no advertisement lists", 0)
}

/// The public key of a signing key.
pub fn public_key(key: &SigningKey) -> PublicKey {
    PublicKey::new(base64url(key.verifying_key().as_bytes())).expect("a canonical key")
}

/// Signs a statement envelope, written as JSON, in place: puts `key` in, signs the signing input, puts the
/// signature in.
pub fn sign_value(statement: &mut Value, key: &SigningKey) {
    statement["signature"]["key"] = public_key(key).as_str().into();
    statement["signature"]["value"] = base64url(&[0u8; 64]).into();
    let input = signing_input(&serde_json::to_vec(&*statement).expect("JSON"))
        .expect("a statement with a signing input");
    statement["signature"]["value"] = base64url(&key.sign(&input).to_bytes()).into();
}

/// `statement`, signed anew with `key`.
pub fn sign_statement<T: Serialize + DeserializeOwned>(
    statement: &Statement<T>,
    key: &SigningKey,
) -> Statement<T> {
    let mut value = serde_json::to_value(statement).expect("a statement serialises");
    sign_value(&mut value, key);
    serde_json::from_value(value).expect("a signed statement reads back")
}

/// Why a statement's signature isn't accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// The bytes have no signing input, or no signature in its place.
    NoSignature,
    /// The key it names is no point of the curve.
    NoSuchKey,
    /// The signature is not the key's over the signing input.
    DoesNotVerify,
}

/// Checks the signature of a statement, given as the bytes it arrived in, against the key it names, strictly.
pub fn verify(received: &[u8]) -> Result<(), Rejected> {
    let value: Value = serde_json::from_slice(received).map_err(|_| Rejected::NoSignature)?;
    let key = value["signature"]["key"]
        .as_str()
        .and_then(PublicKey::new)
        .ok_or(Rejected::NoSignature)?;
    let signature = value["signature"]["value"]
        .as_str()
        .and_then(signature_bytes)
        .ok_or(Rejected::NoSignature)?;
    let input = signing_input(received).map_err(|_| Rejected::NoSignature)?;
    let key = VerifyingKey::from_bytes(&key.bytes()).map_err(|_| Rejected::NoSuchKey)?;
    key.verify_strict(&input, &ed25519_dalek::Signature::from_bytes(&signature))
        .map_err(|_| Rejected::DoesNotVerify)
}

/// The role suite's verifier of signatures: each must verify against the key it names. It is the suite's own, built
/// on the contract's signing input; a platform plugs in its own, as the catalogue is.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ed25519Verifier;

impl SignatureVerifier for Ed25519Verifier {
    fn verify(&self, _maker: &str, _signature: &Signature, received: &[u8]) -> Verification {
        match verify(received) {
            Ok(()) => Verification::Verified,
            Err(why) => Verification::Failed(format!("{why:?}")),
        }
    }
}

/// The keys `provider` publishes, as an advertisement lists them.
pub fn listed_keys(provider: &str) -> Vec<ListedKey> {
    [
        (advertising_key(provider), KeyPurpose::Advertising),
        (statements_key(provider), KeyPurpose::Statements),
    ]
    .into_iter()
    .map(|(key, purpose)| ListedKey {
        scheme: SignatureScheme::Ed25519,
        public_key: public_key(&key),
        purpose,
    })
    .collect()
}

/// The signed advertisement of `provider`, as the bytes it sends: a reserve over a bank network and its ledger,
/// listing the provider's advertising key and statements key, made at [`ADVERTISED_AT`] and signed with the
/// advertising key.
pub fn advertisement(provider: &str, context: &SuiteContext) -> Vec<u8> {
    let mut statement = Reserve::new(context).advertisement().into_statement();
    statement.maker = provider.to_owned();
    statement.made_at = ADVERTISED_AT.to_owned();
    statement.statement.provider = provider.to_owned();
    statement.statement.keys = listed_keys(provider);
    if let Some(reserve) = statement.statement.offers.reserve.as_mut() {
        reserve.declaration.maker = provider.to_owned();
    }
    let mut value = serde_json::to_value(&statement).expect("an advertisement serialises");
    sign_value(&mut value, &advertising_key(provider));
    serde_json::to_vec(&value).expect("JSON")
}

/// The signing key a placeholder key stands for: its bytes are the seed.
pub fn real(placeholder: &str) -> SigningKey {
    SigningKey::from_bytes(
        &PublicKey::new(placeholder)
            .expect("a placeholder key")
            .bytes(),
    )
}

/// Signs `value` and every statement inside it, innermost first, with the keys its placeholders stand for.
fn resign(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(resign),
        Value::Object(map) => {
            map.values_mut().for_each(resign);
            let is_statement = map.get("signature").is_some_and(|s| s["key"].is_string())
                && map.contains_key("maker")
                && map.contains_key("shape");
            if !is_statement {
                return;
            }
            let key = real(map["signature"]["key"].as_str().expect("a key"));
            if map["shape"] == ADVERTISEMENT_SHAPE {
                for entry in map["statement"]["keys"].as_array_mut().expect("keys") {
                    let own = real(entry["public_key"].as_str().expect("a key"));
                    entry["public_key"] = public_key(&own).as_str().into();
                }
            }
            sign_value(value, &key);
        }
        _ => {}
    }
}

/// The bytes of a statement as sent, signed anew: it and every statement inside it, by the keys their placeholders
/// stand for, and an advertisement's listed keys the real ones.
pub fn signed(sent: &[u8]) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(sent).expect("a statement");
    resign(&mut value);
    serde_json::to_vec(&value).expect("JSON")
}

/// The bytes of a statement as sent, with the statements nested in it signed anew as [`signed`] does, and the
/// statement itself left as it is: for a platform that signs its own statements, and passes on those a module
/// nested in its answer.
pub fn signed_inside(sent: &[u8]) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(sent).expect("a statement");
    if let Value::Object(map) = &mut value {
        map.values_mut().for_each(resign);
    }
    serde_json::to_vec(&value).expect("JSON")
}

/// A role provider whose statements carry real signatures: it runs `P`, whose statements carry placeholders, and
/// signs each anew, and each statement nested in it, with the key its placeholder stands for.
#[derive(Debug, Clone)]
pub struct Signing<P>(pub P);

impl<P: RoleProvider> RoleProvider for Signing<P> {
    fn advertisement(&self) -> Received<Advertisement> {
        Received::from_bytes(signed(self.0.advertisement().bytes())).expect("reads back")
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        Received::from_bytes(signed(self.0.handle(operation).bytes())).expect("reads back")
    }

    fn state(&self) -> String {
        self.0.state()
    }

    fn status(&mut self, transaction: &str) -> Option<Received<ResultBody>> {
        self.0
            .status(transaction)
            .map(|r| Received::from_bytes(signed(r.bytes())).expect("reads back"))
    }
}

impl<P: RoleHarness> RoleHarness for Signing<P> {
    fn force_outcome_unknown(&mut self) -> bool {
        self.0.force_outcome_unknown()
    }

    fn force_failure(&mut self) -> bool {
        self.0.force_failure()
    }

    fn credit_reversal(&mut self, segment: &str, node: &str, asset: &str, amount: &str) -> bool {
        self.0.credit_reversal(segment, node, asset, amount)
    }
}
