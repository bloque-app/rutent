//! The answers' signing (`T-015-04` part 3, `REQ-MDL-6` applied to readers): the host signs each answer with the
//! reader's statement key, which it resolves by name when it signs and never hands to the reader's process.
//!
//! The private half is held as the base64url text, unpadded, of its 32-byte Ed25519 seed, as the provider system's
//! store holds a provider's. It is refused unless its public half is the reader's listed statement key, so a store
//! holding another key can't sign in the reader's name. The key is used for one answer and dropped.

use std::collections::BTreeMap;
use std::sync::Mutex;

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::module_interface::SecretValue;
use rutent_contracts::provider_role::{
    Deployment, PublicKey, Signature, SignatureScheme, Statement, signing_input,
};
use rutent_contracts::reader::{ANSWER_SHAPE, ReaderAnswer};
use rutent_contracts::router_api::{Id, ProtocolVersion};

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Resolves a reader's statement key by name, when the host signs (`REQ-MDL-17`, `DEC-9` applied to readers): keyed by
/// reader, so the same name under two readers is two keys. An implementation asks its store on each call and keeps no
/// copy, so a rotated key is used from the next answer on.
pub trait StatementKeys: Send + Sync {
    /// The private half of `reader`'s key `name`, or `None` if the store holds none.
    fn resolve(&self, reader: &Id, name: &str) -> Option<SecretValue>;
}

/// Keys the host was handed at start in its environment, read on each call: `RUTENT_READER_KEY_<READER>_<NAME>`, each
/// name upper-cased with every character but a letter or digit made `_`. A deployment's secrets reach an ECS task's
/// environment this way; the readers' processes start with an empty one. A rotated key is read at the next start.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvKeys;

impl EnvKeys {
    /// The variable `reader`'s key `name` is read from.
    pub fn variable(reader: &Id, name: &str) -> String {
        let part = |text: &str| -> String {
            text.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect()
        };
        format!("RUTENT_READER_KEY_{}_{}", part(reader.as_str()), part(name))
    }
}

impl StatementKeys for EnvKeys {
    fn resolve(&self, reader: &Id, name: &str) -> Option<SecretValue> {
        std::env::var(Self::variable(reader, name))
            .ok()
            .and_then(SecretValue::new)
    }
}

/// Keys held in memory, for a test or an embedding that resolves them itself.
#[derive(Debug, Default)]
pub struct MemoryKeys {
    keys: Mutex<BTreeMap<(String, String), SecretValue>>,
}

impl MemoryKeys {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts `value` as `reader`'s key `name`, in place of any before it.
    pub fn put(&self, reader: &Id, name: &str, value: SecretValue) {
        if let Ok(mut keys) = self.keys.lock() {
            keys.insert((reader.as_str().to_owned(), name.to_owned()), value);
        }
    }
}

impl StatementKeys for MemoryKeys {
    fn resolve(&self, reader: &Id, name: &str) -> Option<SecretValue> {
        self.keys
            .lock()
            .ok()?
            .get(&(reader.as_str().to_owned(), name.to_owned()))
            .cloned()
    }
}

/// Why an answer wasn't signed. No variant carries the key (`THR-50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningError {
    /// The store holds no key by that name for the reader.
    NoKey,
    /// The key resolved isn't the base64url text of a 32-byte seed.
    MalformedKey,
    /// The key resolved isn't the private half of the reader's listed statement key.
    NotTheListedKey,
    /// The answer has no signing input, or doesn't serialise.
    NoSigningInput,
}

/// The envelope an answer is signed in: the deployment and protocol version it names, the reader as its maker, and
/// the reader's listed key.
#[derive(Debug, Clone, Copy)]
pub struct Envelope<'a> {
    /// Core's deployment.
    pub deployment: &'a Deployment,
    /// The protocol version.
    pub protocol_version: &'a ProtocolVersion,
    /// The reader, the answer's maker.
    pub maker: &'a Id,
    /// The reader's listed statement key.
    pub key: &'a PublicKey,
    /// When the host made the statement, by its wall clock: never read for the deadline.
    pub made_at: &'a str,
}

/// Signs `answer` in `envelope` with `secret`, the private half of the reader's listed key, and gives the statement's
/// bytes: the reader's answer as Core reads it. The key is dropped when it returns.
pub fn sign_answer(
    envelope: &Envelope<'_>,
    answer: ReaderAnswer,
    secret: &SecretValue,
) -> Result<Vec<u8>, SigningError> {
    let seed = decode_seed(secret.as_str()).ok_or(SigningError::MalformedKey)?;
    let key = SigningKey::from_bytes(&seed);
    if key.verifying_key().to_bytes() != envelope.key.bytes() {
        return Err(SigningError::NotTheListedKey);
    }
    let mut statement = Statement {
        deployment: envelope.deployment.clone(),
        protocol_version: envelope.protocol_version.as_str().to_owned(),
        shape: ANSWER_SHAPE.to_owned(),
        maker: envelope.maker.as_str().to_owned(),
        made_at: envelope.made_at.to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: envelope.key.clone(),
            value: String::new(),
        },
        statement: answer,
    };
    let unsigned = serde_json::to_vec(&statement).map_err(|_| SigningError::NoSigningInput)?;
    let input = signing_input(&unsigned).map_err(|_| SigningError::NoSigningInput)?;
    statement.signature.value = base64url(&key.sign(&input).to_bytes());
    serde_json::to_vec(&statement).map_err(|_| SigningError::NoSigningInput)
}

/// The base64url text of bytes, unpadded.
fn base64url(bytes: &[u8]) -> String {
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

/// The 32 bytes of a seed written as base64url text, unpadded, in its one canonical spelling.
fn decode_seed(text: &str) -> Option<[u8; 32]> {
    if text.len() != 43 {
        return None;
    }
    let mut bits = 0u32;
    let mut count = 0u32;
    let mut out = Vec::with_capacity(33);
    for c in text.bytes() {
        let value = BASE64URL.iter().position(|b| *b == c)? as u32;
        bits = bits << 6 | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    // 43 characters are 258 bits: the last two must be zero, so the text has one spelling.
    if bits != 0 {
        return None;
    }
    out.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::{base64url, decode_seed};

    // verifies: REQ-MDL-6
    #[test]
    fn statement_key_seed_reads_back_in_its_one_spelling() {
        let seed: [u8; 32] = core::array::from_fn(|i| i as u8 * 7);
        let text = base64url(&seed);
        assert_eq!(decode_seed(&text), Some(seed));
        // The same bytes with a trailing bit set: another spelling, refused.
        let mut other = text.clone().into_bytes();
        let last = super::BASE64URL
            .iter()
            .position(|b| *b == other[42])
            .unwrap();
        other[42] = super::BASE64URL[(last & !3) | 1];
        assert_eq!(decode_seed(core::str::from_utf8(&other).unwrap()), None);
        assert_eq!(decode_seed(&text[..42]), None);
    }

    // verifies: REQ-MDL-6, DEC-28
    #[test]
    fn answer_is_signed_only_with_the_readers_listed_key() {
        use super::{Envelope, SigningError, sign_answer};
        use ed25519_dalek::SigningKey;
        use rutent_contracts::module_interface::SecretValue;
        use rutent_contracts::provider_role::{Deployment, NoDetail, PublicKey};
        use rutent_contracts::reader::{Answered, Asked, ReaderAnswer, Reason, RequestId};
        use rutent_contracts::router_api::{Id, ProtocolVersion, Version};

        let listed = SigningKey::from_bytes(&[3u8; 32]);
        let public = PublicKey::new(base64url(&listed.verifying_key().to_bytes())).unwrap();
        let deployment = Deployment {
            name: "rutent-test".to_owned(),
            operator_key: public.clone(),
        };
        let (maker, version) = (
            Id::new("base").unwrap(),
            ProtocolVersion::new("4.0.0").unwrap(),
        );
        let envelope = Envelope {
            deployment: &deployment,
            protocol_version: &version,
            maker: &maker,
            key: &public,
            made_at: "2026-11-27T10:00:00Z",
        };
        let answer = ReaderAnswer {
            request: RequestId::from_random([9; 16]).unwrap(),
            network: Id::new("base").unwrap(),
            version: Version::new("0.1.0").unwrap(),
            asked: Asked::NetworkRules(NoDetail {}),
            answer: Answered::Unreadable {
                why: Reason::new("checks_not_passed").unwrap(),
            },
        };
        let secret = |key: &SigningKey| SecretValue::new(base64url(&key.to_bytes())).unwrap();
        assert!(sign_answer(&envelope, answer.clone(), &secret(&listed)).is_ok());
        let other = SigningKey::from_bytes(&[4u8; 32]);
        assert_eq!(
            sign_answer(&envelope, answer.clone(), &secret(&other)),
            Err(SigningError::NotTheListedKey)
        );
        assert_eq!(
            sign_answer(&envelope, answer, &SecretValue::new("not-a-seed").unwrap()),
            Err(SigningError::MalformedKey)
        );
    }
}
